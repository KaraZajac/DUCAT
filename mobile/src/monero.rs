//! Talking to a Monero daemon (§17).
//!
//! # On Dandelion++
//!
//! **Dandelion++ is the daemon's, not the wallet's.** It runs in `monerod`'s
//! transaction relay: a node holds a new transaction through a stem phase
//! before fluffing it to its peers. A wallet has no peer connections of its
//! own, so it cannot stem anything — it calls `send_raw_transaction` and the
//! node it called does the rest.
//!
//! It has also been in every release since 0.15, in 2019, so "supports D++" is
//! not a property that distinguishes one live stagenet node from another. Any
//! node serving this network today does it.
//!
//! What a wallet actually chooses, and what these functions expose:
//!
//! - **Which node** it hands the transaction to. That node learns the link
//!   between your address for it and the transaction, which is the exposure
//!   D++ exists to prevent *between* nodes and cannot prevent at the one you
//!   talked to. Your own node removes it entirely.
//! - **What carries the request.** An onion endpoint hides the link from the
//!   network path even when the node itself is a stranger's.
//!
//! So the ranking is by **trust and transport**, not by protocol support, and
//! the UI says which one is in use rather than claiming a property it cannot
//! check.

use std::io::Read;
use std::str::FromStr;
use std::time::{Duration, Instant};

/// How much the node we are using can learn about us.
#[derive(uniffi::Enum, Clone, Copy, PartialEq, Eq, Debug)]
pub enum NodeTrust {
    /// Ours. It learns nothing we did not already know.
    Own,
    /// Reached through Tor. A stranger's node, but not one that also sees where
    /// the request came from.
    Onion,
    /// A stranger's node on the open network. It sees our address and our
    /// transactions together.
    PublicClearnet,
}

/// A node worth trying, and what using it costs.
#[derive(uniffi::Record, Clone)]
pub struct NodeCandidate {
    pub url: String,
    pub trust: NodeTrust,
    pub label: String,
}

/// The list, in the order it should be tried.
///
/// Own first, then onion, then clearnet — the order in which they give away
/// less. A user pointing at their own node should never be silently demoted to
/// a public one, so a configured node is returned alone rather than at the head
/// of a fallback list.
#[uniffi::export]
pub fn monero_default_nodes(own_url: Option<String>) -> Vec<NodeCandidate> {
    if let Some(u) = own_url.filter(|u| !u.trim().is_empty()) {
        return vec![NodeCandidate {
            url: u.trim().to_string(),
            trust: NodeTrust::Own,
            label: "your node".into(),
        }];
    }
    let mut nodes = vec![
        NodeCandidate {
            url: "http://node.monerodevs.org:38089".into(),
            trust: NodeTrust::PublicClearnet,
            label: "monerodevs (stagenet)".into(),
        },
        NodeCandidate {
            url: "http://node2.monerodevs.org:38089".into(),
            trust: NodeTrust::PublicClearnet,
            label: "monerodevs 2 (stagenet)".into(),
        },
        NodeCandidate {
            url: "http://node3.monerodevs.org:38089".into(),
            trust: NodeTrust::PublicClearnet,
            label: "monerodevs 3 (stagenet)".into(),
        },
        NodeCandidate {
            url: "http://xmr-lux.boldsuck.org:38081".into(),
            trust: NodeTrust::PublicClearnet,
            label: "boldsuck (stagenet)".into(),
        },
        NodeCandidate {
            url: "http://stagenet.xmr-tw.org:38081".into(),
            trust: NodeTrust::PublicClearnet,
            label: "xmr-tw (stagenet)".into(),
        },
    ];
    // A random starting point, so every phone does not probe — and then
    // settle on — the same first entry. The picker takes the first usable
    // candidate; without rotation, one flaky-but-answering node at the head
    // of a fixed list becomes everyone's node (observed in the field: a
    // whole day fed to boldsuck while two healthy nodes sat unprobed).
    let mut b = [0u8; 1];
    rand_core::RngCore::fill_bytes(&mut rand_core::OsRng, &mut b);
    let n = nodes.len();
    nodes.rotate_left((b[0] as usize) % n);
    nodes
}

/// What a probe found. Named apart from Veilid's `NodeStatus`: two things
/// called the same in one bridge is a footgun for every caller downstream.
#[derive(uniffi::Record, Clone)]
pub struct MoneroNodeStatus {
    pub url: String,
    pub reachable: bool,
    pub height: u64,
    /// The daemon's own view of whether it has caught up. A node still syncing
    /// will happily answer and report a height that is behind, which would show
    /// a wallet a balance that is merely old.
    pub synced: bool,
    /// `stagenet`, `mainnet` or `testnet`, straight from the daemon. Checked
    /// rather than assumed: pointing a stagenet wallet at mainnet is the kind
    /// of mistake that is only funny until it involves real money.
    pub nettype: String,
    pub rtt_ms: u64,
    pub error: Option<String>,
}

#[derive(uniffi::Error, Debug, thiserror::Error)]
pub enum MoneroError {
    #[error("{0}")]
    Failed(String),
}

/// Ask a daemon where it is. Blocking; call it off the main thread.
#[uniffi::export]
pub fn monero_probe(url: String, timeout_ms: u32) -> MoneroNodeStatus {
    let t0 = Instant::now();
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_millis(timeout_ms as u64))
        .build();

    let mut out = MoneroNodeStatus {
        url: url.clone(),
        reachable: false,
        height: 0,
        synced: false,
        nettype: String::new(),
        rtt_ms: 0,
        error: None,
    };

    let res = agent
        .post(&format!("{}/json_rpc", url.trim_end_matches('/')))
        .set("Content-Type", "application/json")
        .send_string(r#"{"jsonrpc":"2.0","id":"0","method":"get_info"}"#);

    out.rtt_ms = t0.elapsed().as_millis() as u64;
    match res {
        Err(e) => {
            out.error = Some(short_error(&e.to_string()));
            out
        }
        Ok(resp) => match resp
            .into_string()
            .map_err(|e| e.to_string())
            .and_then(|b| serde_json::from_str::<serde_json::Value>(&b).map_err(|e| e.to_string()))
        {
            Err(e) => {
                out.error = Some(format!("bad response: {e}"));
                out
            }
            Ok(v) => {
                let r = &v["result"];
                out.reachable = true;
                out.height = r["height"].as_u64().unwrap_or(0);
                out.synced = r["synchronized"].as_bool().unwrap_or(false);
                out.nettype = r["nettype"].as_str().unwrap_or("").to_string();
                out
            }
        },
    }
}

/// Try candidates in order and keep the first that is usable on this network.
///
/// "Usable" means reachable **and** synced **and** on the network we asked for.
/// A node that answers is not the same as a node worth trusting a balance to,
/// and taking the first that merely responds is how a wallet ends up reading a
/// chain that stopped hours ago.
#[uniffi::export]
pub fn monero_pick_node(
    candidates: Vec<NodeCandidate>,
    want_nettype: String,
    timeout_ms: u32,
) -> Result<MoneroNodeStatus, MoneroError> {
    let mut last: Option<MoneroNodeStatus> = None;
    for c in &candidates {
        let s = monero_probe(c.url.clone(), timeout_ms);
        if s.reachable && s.synced && s.nettype == want_nettype {
            return Ok(s);
        }
        last = Some(s);
    }
    Err(MoneroError::Failed(match last {
        Some(s) if s.reachable && s.nettype != want_nettype => {
            format!("that node is on {}, not {want_nettype}", s.nettype)
        }
        Some(s) if s.reachable => "reachable but still syncing".into(),
        Some(s) => s.error.unwrap_or_else(|| "unreachable".into()),
        None => "no nodes configured".into(),
    }))
}

/// ureq's errors carry the whole URL and a stack of causes. A person
/// troubleshooting needs the reason, not the transcript.
fn short_error(e: &str) -> String {
    let lower = e.to_lowercase();
    if lower.contains("timed out") || lower.contains("timeout") {
        "timed out".into()
    } else if lower.contains("dns") {
        "cannot resolve that host".into()
    } else if lower.contains("connection refused") {
        "connection refused".into()
    } else {
        e.chars().take(90).collect()
    }
}


// ---------------------------------------------------------------------------
// Scanning (§17.2)
// ---------------------------------------------------------------------------
//
// There is no `monero-wallet-rpc` on a phone, so the wallet scans the chain
// itself: fetch each block, test its outputs against the view key, keep what is
// ours. One request per block, which is why every call here takes a bound and
// reports how far it actually got instead of running until it finishes.

/// One output we own.
#[derive(uniffi::Record, Clone)]
pub struct OwnedOutput {
    pub amount_pxmr: u64,
    pub height: u64,
    /// Which subaddress received it: 0 is the primary, anything else is a
    /// per-contact address (§15.10) — the only thing that ties an arriving
    /// output to a counterparty without believing a note.
    pub minor: u32,
    /// Hex key image, so spentness can be checked against the daemon. Derived
    /// from the spend secret, which is why scanning for a *balance* needs more
    /// than the view key that scanning for *receipts* does.
    pub key_image_hex: String,
    /// The output itself, serialized.
    ///
    /// Spending needs the whole thing — key offset, commitment, position on the
    /// chain — and none of that survives a summary. Keeping the bytes means a
    /// send does not have to rescan to find what the wallet already found.
    pub blob: Vec<u8>,
    /// The transaction this output was created by.
    ///
    /// Without it an output list cannot be turned back into a list of payments.
    /// Two outputs from one transaction look like two receipts, and *change* —
    /// which is the wallet paying itself the remainder — looks like income. A
    /// screen built on outputs alone told someone they had received twice and
    /// spent nothing.
    pub tx_hash_hex: String,
    /// The block's own timestamp, in seconds. Zero if unknown.
    ///
    /// A height is not a time. Nobody reconciling a payment against a receipt
    /// knows what block 2184652 means.
    pub timestamp: u64,
}

/// How far a scan got, and what it found.
#[derive(uniffi::Record, Clone)]
pub struct ScanResult {
    /// The height scanning reached. Persist this: rescanning from the restore
    /// height every time is the difference between a wallet that opens and one
    /// that appears to hang.
    pub scanned_to: u64,
    /// The chain tip when we started, so a caller can show progress honestly
    /// rather than implying it is finished.
    pub tip: u64,
    pub outputs: Vec<OwnedOutput>,
    /// Blocks actually fetched and examined.
    ///
    /// Reported because the loop skips a block it cannot read and carries on,
    /// which makes "scanned a window, found nothing" indistinguishable from
    /// "read nothing at all". Without this a completely broken transport looks
    /// exactly like an empty wallet — which is what it did look like.
    pub blocks_read: u32,
    /// Blocks that could not be fetched or expanded.
    pub blocks_failed: u32,
}

/// Scan a bounded window for outputs belonging to this wallet.
///
/// `spend_key_hex` is the **secret** spend key: key images cannot be derived
/// without it, and without key images an output cannot be told from one already
/// spent. A view-only scan shows every receipt and would call money that is
/// already gone a balance.
#[uniffi::export]
pub fn monero_scan(
    node_url: String,
    spend_key_hex: String,
    from_height: u64,
    max_blocks: u32,
    // How many per-contact subaddresses to watch (minors 1..=n of account 0).
    // A payment to an unregistered subaddress is invisible, so the caller
    // passes its high-water allocation, not a guess.
    subaddress_minors: u32,
) -> Result<ScanResult, MoneroError> {
    use curve25519_dalek::constants::ED25519_BASEPOINT_TABLE;
    use monero_daemon_rpc::prelude::*;
    use monero_wallet::ed25519::{Point, Scalar};
    use monero_wallet::{Scanner, ViewPair};
    use zeroize::Zeroizing;

    let raw = hex_to_bytes(&spend_key_hex)
        .ok_or_else(|| MoneroError::Failed("spend key is not hex".into()))?;
    let spend = Scalar::read(&mut raw.as_slice())
        .map_err(|_| MoneroError::Failed("spend key is not a valid scalar".into()))?;
    // §4.3's derivation: view = H(spend), so one secret restores both. Hashed
    // over the scalar's canonical encoding, matching create_wallet — hashing the
    // input hex instead would derive a different, silently wrong view key.
    let mut sb = Vec::new();
    spend.write(&mut sb).map_err(|e| MoneroError::Failed(format!("scalar: {e}")))?;
    let view = Zeroizing::new(Scalar::hash(&sb));
    let spend_pub = Point::from(&spend.into() * ED25519_BASEPOINT_TABLE);

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| MoneroError::Failed(format!("runtime: {e}")))?;

    rt.block_on(async {
        let rpc = monero_daemon_rpc::MoneroDaemon::new(UreqTransport::new(node_url.clone()))
            .await
            .map_err(|e| MoneroError::Failed(format!("connect: {e:?}")))?;
        let tip = rpc
            .latest_block_number()
            .await
            .map_err(|e| MoneroError::Failed(format!("height: {e:?}")))? as u64;

        let vp = ViewPair::new(spend_pub, view)
            .map_err(|e| MoneroError::Failed(format!("view pair: {e:?}")))?;
        let mut scanner = Scanner::new(vp);
        for m in 1..=subaddress_minors {
            if let Some(idx) = monero_wallet::address::SubaddressIndex::new(0, m) {
                scanner.register_subaddress(idx);
            }
        }
        let mut outputs = Vec::new();
        let (mut read, mut failed) = (0u32, 0u32);

        // Bounded, and the bound is not politeness: each block is a round trip
        // to someone else's node, so an unbounded loop on a phone is a scan
        // that never returns and a screen that never updates.
        let last = tip.min(from_height + max_blocks as u64);
        let mut h = from_height;
        let mut first_error: Option<String> = None;
        while h <= last {
            let block = match rpc.block_by_number(h as usize).await {
                Ok(b) => b,
                // A gap is skipped rather than fatal: one flaky block should not
                // discard the window's progress. Counted, though — see
                // `blocks_failed`.
                Err(e) => {
                    if first_error.is_none() { first_error = Some(format!("{e:?}")); }
                    failed += 1; h += 1; continue
                }
            };
            let sb = match rpc.expand_to_scannable_block(block).await {
                Ok(b) => b,
                Err(e) => {
                    if first_error.is_none() { first_error = Some(format!("{e:?}")); }
                    failed += 1; h += 1; continue
                }
            };
            read += 1;
            // Read before `scan` consumes the block.
            let block_time = sb.block.header.timestamp;
            if let Ok(found) = scanner.scan(sb) {
                for o in found.not_additionally_locked() {
                    // KI = (spend + key_offset) · H_p(output key).
                    //
                    // `Point::key_image()` is **not** the hash: it validates a
                    // point and hands it straight back. Using it produced x·P
                    // instead of x·H_p(P), so every key image was wrong, the
                    // daemon answered "not spent" for outputs that were, and the
                    // wallet counted spent money twice. This is the derivation
                    // monero-wallet's own signing path uses.
                    let x: curve25519_dalek::Scalar = spend.into() + o.key_offset().into();
                    let hp: curve25519_dalek::EdwardsPoint =
                        Point::biased_hash(o.key().compress().to_bytes()).into();
                    let ki = Some(x * hp);
                    outputs.push(OwnedOutput {
                        amount_pxmr: o.commitment().amount,
                        height: h,
                        minor: o.subaddress().map(|s| s.address()).unwrap_or(0),
                        key_image_hex: ki
                            .map(|k| hex_of(k.compress().to_bytes().as_slice()))
                            .unwrap_or_default(),
                        blob: o.serialize(),
                        tx_hash_hex: hex_of(&o.transaction()),
                        timestamp: block_time,
                    });
                }
            }
            h += 1;
        }

        // Every block in the window failing is a broken connection, not an
        // empty wallet, and must not be reported as progress.
        if read == 0 && failed > 0 {
            return Err(MoneroError::Failed(format!(
                "could not read any of {failed} blocks: {}",
                first_error.unwrap_or_else(|| "unknown".into())
            )));
        }
        Ok(ScanResult { scanned_to: last, tip, outputs, blocks_read: read, blocks_failed: failed })
    })
}

/// §15.10's per-contact address: subaddress (0, minor) of this wallet.
///
/// One counterparty, one address — two people comparing notes hold two
/// strings nothing links. Minor 0 is refused because it *is* the primary:
/// handing it out as "a subaddress" would be the reuse this exists to end.
#[uniffi::export]
pub fn monero_subaddress(
    spend_key_hex: String,
    minor: u32,
    stagenet: bool,
) -> Result<String, MoneroError> {
    use curve25519_dalek::constants::ED25519_BASEPOINT_TABLE;
    use monero_wallet::address::{Network, SubaddressIndex};
    use monero_wallet::ed25519::{Point, Scalar};
    use monero_wallet::ViewPair;
    use zeroize::Zeroizing;

    let idx = SubaddressIndex::new(0, minor)
        .ok_or_else(|| MoneroError::Failed("minor 0 is the primary address".into()))?;
    let raw = hex_to_bytes(&spend_key_hex)
        .ok_or_else(|| MoneroError::Failed("spend key is not hex".into()))?;
    let spend = Scalar::read(&mut raw.as_slice())
        .map_err(|_| MoneroError::Failed("spend key is not a valid scalar".into()))?;
    let mut sb = Vec::new();
    spend.write(&mut sb).map_err(|e| MoneroError::Failed(format!("scalar: {e}")))?;
    let view = Zeroizing::new(Scalar::hash(&sb));
    let spend_pub = Point::from(&spend.into() * ED25519_BASEPOINT_TABLE);
    let vp = ViewPair::new(spend_pub, view)
        .map_err(|e| MoneroError::Failed(format!("view pair: {e:?}")))?;
    let network = if stagenet { Network::Stagenet } else { Network::Mainnet };
    Ok(vp.subaddress(network, idx).to_string())
}

/// Scan with a view key alone, for an address we cannot spend from.
///
/// Finds every receipt and **cannot tell whether any of it is still there**,
/// because key images need the spend secret. That is exactly what a watch-only
/// wallet is, and the distinction has to reach the screen: this total is what
/// arrived, not what is available.
#[uniffi::export]
pub fn monero_scan_view_only(
    node_url: String,
    address: String,
    view_key_hex: String,
    from_height: u64,
    max_blocks: u32,
    subaddress_minors: u32,
) -> Result<ScanResult, MoneroError> {
    use monero_daemon_rpc::prelude::*;
    use monero_wallet::address::{MoneroAddress, Network};
    use monero_wallet::ed25519::Scalar;
    use monero_wallet::{Scanner, ViewPair};
    use zeroize::Zeroizing;

    let raw = hex_to_bytes(&view_key_hex)
        .ok_or_else(|| MoneroError::Failed("view key is not hex".into()))?;
    let view = Zeroizing::new(
        Scalar::read(&mut raw.as_slice())
            .map_err(|_| MoneroError::Failed("view key is not a valid scalar".into()))?,
    );
    let addr = MoneroAddress::from_str(Network::Stagenet, &address)
        .map_err(|e| MoneroError::Failed(format!("address: {e:?}")))?;

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| MoneroError::Failed(format!("runtime: {e}")))?;

    rt.block_on(async {
        let rpc = monero_daemon_rpc::MoneroDaemon::new(UreqTransport::new(node_url))
            .await
            .map_err(|e| MoneroError::Failed(format!("connect: {e:?}")))?;
        let tip = rpc
            .latest_block_number()
            .await
            .map_err(|e| MoneroError::Failed(format!("height: {e:?}")))? as u64;

        let vp = ViewPair::new(addr.spend(), view)
            .map_err(|e| MoneroError::Failed(format!("view pair: {e:?}")))?;
        let mut scanner = Scanner::new(vp);
        for m in 1..=subaddress_minors {
            if let Some(idx) = monero_wallet::address::SubaddressIndex::new(0, m) {
                scanner.register_subaddress(idx);
            }
        }
        let mut outputs = Vec::new();

        let last = tip.min(from_height + max_blocks as u64);
        let mut h = from_height;
        let (mut read, mut failed) = (0u32, 0u32);
        while h <= last {
            let Ok(block) = rpc.block_by_number(h as usize).await else { failed += 1; h += 1; continue };
            let Ok(sb) = rpc.expand_to_scannable_block(block).await else { failed += 1; h += 1; continue };
            read += 1;
            // Read before `scan` consumes the block.
            let block_time = sb.block.header.timestamp;
            if let Ok(found) = scanner.scan(sb) {
                for o in found.not_additionally_locked() {
                    outputs.push(OwnedOutput {
                        amount_pxmr: o.commitment().amount,
                        height: h,
                        minor: o.subaddress().map(|s| s.address()).unwrap_or(0),
                        // Deliberately blank: with no spend secret there is no
                        // key image, and inventing a placeholder would let a
                        // caller ask about spentness and believe the answer.
                        key_image_hex: String::new(),
                        blob: o.serialize(),
                        tx_hash_hex: hex_of(&o.transaction()),
                        timestamp: block_time,
                    });
                }
            }
            h += 1;
        }
        if read == 0 && failed > 0 {
            return Err(MoneroError::Failed(format!("could not read any of {failed} blocks")));
        }
        Ok(ScanResult { scanned_to: last, tip, outputs, blocks_read: read, blocks_failed: failed })
    })
}

/// What a stored output says about itself.
///
/// Exists so a wallet that scanned before these fields were recorded does not
/// have to read the chain again to get them: everything here was already inside
/// the blob it kept in order to be able to spend. Re-scanning to recover data
/// you already have on disk is a half-hour of someone's afternoon.
#[derive(uniffi::Record, Clone)]
pub struct OutputMeta {
    pub tx_hash_hex: String,
    /// Which output of that transaction this is.
    pub index_in_transaction: u64,
    /// The one-time key this output was paid to — its address on the chain.
    /// Not the wallet's address: every output gets its own, which is what makes
    /// two payments to the same person unlinkable.
    pub stealth_key_hex: String,
    pub amount_pxmr: u64,
}

/// Read an output's own record of itself, without touching the network.
#[uniffi::export]
pub fn monero_output_meta(blob: Vec<u8>) -> Result<OutputMeta, MoneroError> {
    use monero_wallet::WalletOutput;

    let o = WalletOutput::read(&mut blob.as_slice())
        .map_err(|e| MoneroError::Failed(format!("output: {e}")))?;
    Ok(OutputMeta {
        tx_hash_hex: hex_of(&o.transaction()),
        index_in_transaction: o.index_in_transaction(),
        stealth_key_hex: hex_of(o.key().compress().to_bytes().as_slice()),
        amount_pxmr: o.commitment().amount,
    })
}

/// A transaction as the chain records it.
///
/// The point of `key_images` is not display: it is how a wallet works out that
/// *it* sent a transaction. A spend leaves no receipt on the sender's side —
/// the only local trace is that some of your outputs stop being unspent. Match
/// this transaction's inputs against your own key images and the answer is
/// exact, with no local record needed, which means a send made before the app
/// recorded sends is still recoverable from the chain.
#[derive(uniffi::Record, Clone)]
pub struct TxDetails {
    pub tx_hash_hex: String,
    pub version: u32,
    /// Fee paid, in piconero. Zero for a miner transaction.
    pub fee_pxmr: u64,
    /// Key images consumed. Empty for a miner transaction.
    pub key_images_hex: Vec<String>,
    pub input_count: u32,
    pub output_count: u32,
    /// Decoys plus the real spend, per input. Monero's anonymity set.
    pub ring_size: u32,
    /// A lock beyond the standard ten blocks, or zero.
    pub additional_timelock: u64,
    /// Bytes in `tx_extra` — where the transaction public key and any payment
    /// ID live.
    pub extra_len: u32,
    /// Whether this is a coinbase transaction.
    pub coinbase: bool,
}

/// Fetch one transaction from the daemon.
#[uniffi::export]
pub fn monero_tx_details(node_url: String, tx_hash_hex: String) -> Result<TxDetails, MoneroError> {
    use monero_daemon_rpc::prelude::*;
    use monero_wallet::transaction::{Input, Timelock, Transaction};

    let raw = hex_to_bytes(&tx_hash_hex)
        .filter(|b| b.len() == 32)
        .ok_or_else(|| MoneroError::Failed("transaction id is not 32 bytes of hex".into()))?;
    let mut hash = [0u8; 32];
    hash.copy_from_slice(&raw);

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| MoneroError::Failed(format!("runtime: {e}")))?;

    rt.block_on(async {
        let rpc = monero_daemon_rpc::MoneroDaemon::new(UreqTransport::new(node_url))
            .await
            .map_err(|e| MoneroError::Failed(format!("connect: {e:?}")))?;
        let tx: Transaction = rpc
            .transaction(hash)
            .await
            .map_err(|e| MoneroError::Failed(format!("transaction: {e:?}")))?;

        let prefix = tx.prefix();
        let mut kis = Vec::new();
        let mut ring = 0u32;
        let mut coinbase = false;
        for i in &prefix.inputs {
            match i {
                Input::Gen(_) => coinbase = true,
                Input::ToKey { key_image, key_offsets, .. } => {
                    kis.push(hex_of(key_image.to_bytes().as_slice()));
                    ring = ring.max(key_offsets.len() as u32);
                }
            }
        }

        // The fee lives in the RingCT base, not the prefix, and a pruned or v1
        // transaction has none to report rather than a fee of zero.
        let fee = match &tx {
            Transaction::V2 { proofs: Some(p), .. } => p.base.fee,
            _ => 0,
        };

        Ok(TxDetails {
            tx_hash_hex,
            version: tx.version() as u32,
            fee_pxmr: fee,
            key_images_hex: kis,
            input_count: prefix.inputs.len() as u32,
            output_count: prefix.outputs.len() as u32,
            ring_size: ring,
            additional_timelock: match prefix.additional_timelock {
                Timelock::None => 0,
                Timelock::Block(b) => b as u64,
                Timelock::Time(t) => t,
            },
            extra_len: prefix.extra.len() as u32,
            coinbase,
        })
    })
}

/// When a block was mined, in seconds.
///
/// For filling in times on outputs found before the scanner recorded them. One
/// request per height, so callers ask about the few they are missing rather
/// than the range.
#[uniffi::export]
pub fn monero_block_time(node_url: String, height: u64) -> Result<u64, MoneroError> {
    use monero_daemon_rpc::prelude::*;

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| MoneroError::Failed(format!("runtime: {e}")))?;

    rt.block_on(async {
        let rpc = monero_daemon_rpc::MoneroDaemon::new(UreqTransport::new(node_url))
            .await
            .map_err(|e| MoneroError::Failed(format!("connect: {e:?}")))?;
        let block = rpc
            .block_by_number(height as usize)
            .await
            .map_err(|e| MoneroError::Failed(format!("block: {e:?}")))?;
        Ok(block.header.timestamp)
    })
}

/// A payment sighted in the mempool: real bytes, zero confirmations.
#[derive(uniffi::Record, Clone)]
pub struct PoolHit {
    pub tx_hash_hex: String,
    pub amount_pxmr: u64,
}

/// Scan the mempool for outputs to this wallet (§17.5's *seen*, not settled).
///
/// The library keeps `scan_transaction` private, so each pool transaction is
/// wrapped in a **synthetic block** — a minimal miner transaction, one hash,
/// a dummy RingCT index — and fed to the ordinary scanner, the construction
/// §O14 recorded as viable. The dummy index poisons nothing because nothing
/// from here is ever spent: a pool hit exists to answer "has the customer's
/// payment left their phone", and the real output arrives through the block
/// scanner with a real index when it is mined.
///
/// Bounded: the pool is listed first (hashes only), and at most `max`
/// transactions are fetched and scanned per call.
#[uniffi::export]
pub fn monero_scan_pool(
    node_url: String,
    spend_key_hex: String,
    max: u32,
    subaddress_minors: u32,
) -> Result<Vec<PoolHit>, MoneroError> {
    use curve25519_dalek::constants::ED25519_BASEPOINT_TABLE;
    use monero_daemon_rpc::prelude::*;
    use monero_wallet::block::{Block, BlockHeader};
    use monero_wallet::ed25519::{Point, Scalar};
    use monero_wallet::interface::ScannableBlock;
    use monero_wallet::transaction::{Input, NotPruned, Pruned, Timelock, Transaction, TransactionPrefix};
    use monero_wallet::{Scanner, ViewPair};
    use zeroize::Zeroizing;

    let raw = hex_to_bytes(&spend_key_hex)
        .ok_or_else(|| MoneroError::Failed("spend key is not hex".into()))?;
    let spend = Scalar::read(&mut raw.as_slice())
        .map_err(|_| MoneroError::Failed("spend key is not a valid scalar".into()))?;
    let mut sb = Vec::new();
    spend.write(&mut sb).map_err(|e| MoneroError::Failed(format!("scalar: {e}")))?;
    let view = Zeroizing::new(Scalar::hash(&sb));
    let spend_pub = Point::from(&spend.into() * ED25519_BASEPOINT_TABLE);

    // The pool listing, hashes only. `get_transaction_pool` carries whole
    // blobs in an escaping scheme not worth trusting; the ids are enough, and
    // the transactions come back clean through the ordinary fetch.
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(10))
        .build();
    let body = agent
        .post(&format!("{}/get_transaction_pool", node_url.trim_end_matches('/')))
        .call()
        .map_err(|e| MoneroError::Failed(short_error(&e.to_string())))?
        .into_string()
        .map_err(|e| MoneroError::Failed(format!("pool read: {e}")))?;
    let mut hashes: Vec<[u8; 32]> = Vec::new();
    // Pulled by key rather than by full deserialize: the entries also carry
    // `tx_blob` fields full of escaped binary that serde_json will choke on
    // long before it reaches the ids.
    for part in body.split("\"id_hash\": \"").skip(1) {
        if let Some(h) = part.get(..64).and_then(hex_to_bytes) {
            if h.len() == 32 {
                let mut a = [0u8; 32];
                a.copy_from_slice(&h);
                hashes.push(a);
            }
        }
        if hashes.len() >= max as usize {
            break;
        }
    }
    if hashes.is_empty() {
        return Ok(Vec::new());
    }

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| MoneroError::Failed(format!("runtime: {e}")))?;

    rt.block_on(async {
        let rpc = monero_daemon_rpc::MoneroDaemon::new(UreqTransport::new(node_url.clone()))
            .await
            .map_err(|e| MoneroError::Failed(format!("connect: {e:?}")))?;

        let vp = ViewPair::new(spend_pub, view)
            .map_err(|e| MoneroError::Failed(format!("view pair: {e:?}")))?;
        let mut scanner = Scanner::new(vp);
        for m in 1..=subaddress_minors {
            if let Some(idx) = monero_wallet::address::SubaddressIndex::new(0, m) {
                scanner.register_subaddress(idx);
            }
        }
        let mut hits = Vec::new();

        for hash in hashes {
            // One at a time, tolerantly: a transaction can leave the pool
            // between the listing and the fetch, and that is churn, not error.
            let Ok(tx) = rpc.transaction(hash).await else { continue };
            let tx: Transaction<NotPruned> = tx;

            let miner = Transaction::<NotPruned>::V2 {
                prefix: TransactionPrefix {
                    additional_timelock: Timelock::None,
                    inputs: vec![Input::Gen(1)],
                    outputs: vec![],
                    extra: vec![],
                },
                proofs: None,
            };
            let Some(block) = Block::new(
                BlockHeader {
                    hardfork_version: 16,
                    hardfork_signal: 16,
                    timestamp: 0,
                    previous: [0u8; 32],
                    nonce: 0,
                },
                miner,
                vec![hash],
            ) else {
                continue;
            };
            let synthetic = ScannableBlock {
                block,
                transactions: vec![Transaction::<Pruned>::from(tx)],
                output_index_for_first_ringct_output: Some(0),
            };
            if let Ok(found) = scanner.scan(synthetic) {
                for o in found.not_additionally_locked() {
                    hits.push(PoolHit {
                        tx_hash_hex: hex_of(&hash),
                        amount_pxmr: o.commitment().amount,
                    });
                }
            }
        }
        Ok(hits)
    })
}

/// Ask the daemon which of these key images are already spent.
///
/// Separate from scanning because it is one request for the whole set rather
/// than one per block, and because a wallet that has not asked cannot tell a
/// balance from a history.
#[uniffi::export]
pub fn monero_spent(node_url: String, key_images_hex: Vec<String>) -> Result<Vec<bool>, MoneroError> {
    if key_images_hex.is_empty() {
        return Ok(Vec::new());
    }
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(20))
        .build();
    let body = serde_json::json!({ "key_images": key_images_hex });
    let resp = agent
        .post(&format!("{}/is_key_image_spent", node_url.trim_end_matches('/')))
        .set("Content-Type", "application/json")
        .send_string(&body.to_string())
        .map_err(|e| MoneroError::Failed(short_error(&e.to_string())))?;
    let v: serde_json::Value = resp
        .into_string()
        .map_err(|e| MoneroError::Failed(e.to_string()))
        .and_then(|b| serde_json::from_str(&b).map_err(|e| MoneroError::Failed(e.to_string())))?;
    // 0 = unspent, 1 = spent in the chain, 2 = spent in the pool. Anything not
    // zero means the money is gone or going, and both must count as spent —
    // showing pool-spent output as available is how a wallet double-spends.
    Ok(v["spent_status"]
        .as_array()
        .map(|a| a.iter().map(|x| x.as_u64().unwrap_or(0) != 0).collect())
        .unwrap_or_default())
}

fn hex_of(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn hex_to_bytes(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
        .collect()
}

// ---------------------------------------------------------------------------
// What it is worth, and the two ways that question misleads
// ---------------------------------------------------------------------------
//
// **Asking leaks.** A price lookup tells whoever answers that this device cares
// about Monero's price, at a time, from an IP. That is a smaller disclosure than
// the wallet itself makes to a public node, but it is a disclosure the user did
// not ask for, so it is cached hard and can be turned off entirely.
//
// **A stagenet balance is worth nothing.** Converting test coins to a currency
// figure would put a number on the screen that a person could act on, and there
// is no world in which that number is true. The rate is still shown, because it
// is real, but the caller is told which network it is pricing so it can say so.

/// The daemon connection, over the same HTTP client as everything else here.
///
/// `monero-simple-request-rpc` exists and works on a desktop. It pulls in hyper
/// and its own async machinery, and on the phone the scan failed while the
/// plain-`ureq` probe beside it succeeded — two HTTP stacks in one binary, only
/// one of them proven on the device.
///
/// `HttpTransport` is a single method, so the fix is to have one stack rather
/// than diagnose the second. The blocking call inside an async fn is
/// deliberate: the runtime driving it exists only for this chain and has
/// nothing else to run.
#[derive(Clone)]
pub(crate) struct UreqTransport {
    url: String,
    agent: ureq::Agent,
}

impl UreqTransport {
    pub(crate) fn new(url: String) -> Self {
        UreqTransport {
            url: url.trim_end_matches('/').to_string(),
            agent: ureq::AgentBuilder::new()
                // Generous: a scan asks for blocks one at a time and a slow node
                // is better waited on than abandoned mid-window.
                .timeout(Duration::from_secs(30))
                .build(),
        }
    }
}

impl monero_daemon_rpc::HttpTransport for UreqTransport {
    fn post(
        &self,
        route: &str,
        body: Vec<u8>,
        response_size_limit: Option<usize>,
    ) -> impl Send + std::future::Future<Output = Result<Vec<u8>, monero_daemon_rpc::prelude::InterfaceError>>
    {
        let url = format!("{}/{}", self.url, route.trim_start_matches('/'));
        let agent = self.agent.clone();
        async move {
            // `InterfaceError` is the transport-failed variant; `InvalidInterface`
            // would tell the caller to stop using this node, which a timeout does
            // not justify.
            let err = monero_daemon_rpc::prelude::InterfaceError::InterfaceError;
            let resp = agent
                .post(&url)
                .set("Content-Type", "application/octet-stream")
                .send_bytes(&body)
                .map_err(|e| err(short_error(&e.to_string())))?;

            // **Bounded.** `read_to_end` on a stranger's socket is an
            // out-of-memory bug with a network trigger: the node chooses how
            // much to send and the phone has to hold all of it. The caller's
            // limit is honoured, and a default applies where it gives none —
            // ignoring the argument was silently opting out of the protection
            // the trait was offering.
            const FALLBACK_LIMIT: usize = 32 * 1024 * 1024;
            let limit = response_size_limit.unwrap_or(FALLBACK_LIMIT);
            let mut out = Vec::new();
            // One byte over, so hitting the cap is distinguishable from a
            // response that happens to be exactly that size.
            resp.into_reader()
                .take(limit as u64 + 1)
                .read_to_end(&mut out)
                .map_err(|e| err(e.to_string()))?;
            if out.len() > limit {
                return Err(err(format!("response exceeded {limit} bytes")));
            }
            Ok(out)
        }
    }
}

/// A quote, for display only.
#[derive(uniffi::Record, Clone)]
pub struct Rate {
    pub currency: String,
    /// Units of `currency` per XMR. A float because this never touches a spend
    /// path — §18.2 keeps money in integer piconero, and this is a caption.
    pub per_xmr: f64,
    pub source: String,
    pub fetched_at: u64,
}

/// Fetch a quote. Two sources, because one going down should not blank the
/// screen; both are public and need no account.
#[uniffi::export]
pub fn monero_rate(currency: String, timeout_ms: u32) -> Result<Rate, MoneroError> {
    let cur = currency.to_lowercase();
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_millis(timeout_ms as u64))
        .build();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let done = |p: f64, source: &str| Rate {
        currency: cur.to_uppercase(),
        per_xmr: p,
        source: source.into(),
        fetched_at: now,
    };

    // Four independent sources, tried in order; the common case is still one
    // request. A field phone once printed "no price source answered" because
    // the chain was two entries long and one of them only quoted three
    // currencies — losing the price is losing the fiat display everywhere, so
    // the chain is as deep as the free, keyless APIs allow (verified live
    // 2026-08-17: CryptoCompare now demands a key and is deliberately absent).

    // CoinGecko: every currency the app offers.
    let cg = format!(
        "https://api.coingecko.com/api/v3/simple/price?ids=monero&vs_currencies={cur}"
    );
    if let Ok(r) = agent.get(&cg).call() {
        if let Ok(txt) = r.into_string() {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&txt) {
                if let Some(p) = v["monero"][&cur].as_f64() {
                    return Ok(done(p, "CoinGecko"));
                }
            }
        }
    }

    // CoinPaprika: most currencies; a miss falls through rather than failing.
    let pk = format!(
        "https://api.coinpaprika.com/v1/tickers/xmr-monero?quotes={}",
        cur.to_uppercase()
    );
    if let Ok(r) = agent.get(&pk).call() {
        if let Ok(txt) = r.into_string() {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&txt) {
                if let Some(p) = v["quotes"][cur.to_uppercase()]["price"].as_f64() {
                    return Ok(done(p, "CoinPaprika"));
                }
            }
        }
    }

    // Kraken quotes a handful of pairs directly.
    if matches!(cur.as_str(), "usd" | "eur" | "gbp") {
        let pair = format!("XMR{}", cur.to_uppercase());
        let url = format!("https://api.kraken.com/0/public/Ticker?pair={pair}");
        if let Ok(r) = agent.get(&url).call() {
            if let Ok(txt) = r.into_string() {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&txt) {
                    if let Some(obj) = v["result"].as_object() {
                        if let Some(first) = obj.values().next() {
                            if let Some(last) = first["c"][0].as_str() {
                                if let Ok(p) = last.parse::<f64>() {
                                    return Ok(done(p, "Kraken"));
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // Bitfinex still lists XMR/USD; index 6 of the ticker array is the last
    // trade. USD only, and last on purpose: by the time four sources have
    // failed, the phone is offline and no fifth would answer either.
    if cur == "usd" {
        if let Ok(r) = agent.get("https://api-pub.bitfinex.com/v2/ticker/tXMRUSD").call() {
            if let Ok(txt) = r.into_string() {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&txt) {
                    if let Some(p) = v.get(6).and_then(|x| x.as_f64()) {
                        return Ok(done(p, "Bitfinex"));
                    }
                }
            }
        }
    }

    Err(MoneroError::Failed("no price source answered".into()))
}


// ---------------------------------------------------------------------------
// Spending (§17)
// ---------------------------------------------------------------------------

/// What a send would cost, or what one did.
#[derive(uniffi::Record, Clone)]
pub struct SendResult {
    pub txid_hex: String,
    pub fee_pxmr: u64,
    /// How many nodes took it. **One is not the network.** §8.7.2 was learned
    /// twice in this project: a relay returned success and propagated nothing.
    pub accepted_by: u32,
}

/// Build, sign and broadcast a transaction.
///
/// `input_blobs` are outputs from a previous scan. The caller chooses which to
/// spend, because that choice is §17.2's whole subject — one output pays one
/// person at a time, and a wallet that silently consolidates has decided
/// something about the user's privacy on their behalf.
#[uniffi::export]
pub fn monero_send(
    node_url: String,
    spend_key_hex: String,
    input_blobs: Vec<Vec<u8>>,
    to_address: String,
    amount_pxmr: u64,
    // 0 slow, 1 normal, 2 fast, 3 fastest — the same tiers the estimate uses.
    // This was hardcoded to the cheapest, so the speed a user picked changed
    // the number they were shown and nothing about the transaction.
    priority: u32,
) -> Result<SendResult, MoneroError> {
    use monero_daemon_rpc::prelude::*;
    use monero_wallet::address::MoneroAddress;
    use monero_wallet::ed25519::Scalar;
    use monero_wallet::send::{Change, SignableTransaction};
    use monero_wallet::{OutputWithDecoys, ViewPair, WalletOutput};
    use monero_wallet::ringct::RctType;
    use rand_core::{OsRng, RngCore};
    use zeroize::Zeroizing;

    if input_blobs.is_empty() {
        return Err(MoneroError::Failed("nothing to spend".into()));
    }
    let raw = hex_to_bytes(&spend_key_hex)
        .ok_or_else(|| MoneroError::Failed("spend key is not hex".into()))?;
    let spend = Zeroizing::new(
        Scalar::read(&mut raw.as_slice())
            .map_err(|_| MoneroError::Failed("spend key is not a valid scalar".into()))?,
    );
    let mut sb = Vec::new();
    spend.write(&mut sb).map_err(|e| MoneroError::Failed(format!("scalar: {e}")))?;
    let view = Zeroizing::new(Scalar::hash(&sb));
    let spend_pub = monero_wallet::ed25519::Point::from(
        &(*spend).into() * curve25519_dalek::constants::ED25519_BASEPOINT_TABLE,
    );
    let vp = ViewPair::new(spend_pub, view)
        .map_err(|e| MoneroError::Failed(format!("view pair: {e:?}")))?;

    let dest = MoneroAddress::from_str_with_unchecked_network(&to_address)
        .map_err(|e| MoneroError::Failed(format!("address: {e:?}")))?;

    let outputs: Vec<WalletOutput> = input_blobs
        .iter()
        .map(|b| WalletOutput::read(&mut b.as_slice()))
        .collect::<Result<_, _>>()
        .map_err(|e| MoneroError::Failed(format!("stored output: {e}")))?;

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| MoneroError::Failed(format!("runtime: {e}")))?;

    rt.block_on(async {
        let rpc = monero_daemon_rpc::MoneroDaemon::new(UreqTransport::new(node_url.clone()))
            .await
            .map_err(|e| MoneroError::Failed(format!("connect: {e:?}")))?;
        let tip = rpc
            .latest_block_number()
            .await
            .map_err(|e| MoneroError::Failed(format!("height: {e:?}")))?;

        // Decoys are what make the spend indistinguishable from fifteen others.
        // Selected per input and per send, never reused.
        let mut decoyed = Vec::new();
        for o in outputs {
            decoyed.push(
                OutputWithDecoys::new(&mut OsRng, &rpc, 16, tip, o)
                    .await
                    .map_err(|e| MoneroError::Failed(format!("decoys: {e:?}")))?,
            );
        }

        let fee_rate = rpc
            .fee_rate(
                match priority {
                    0 => FeePriority::Unimportant,
                    1 => FeePriority::Normal,
                    2 => FeePriority::Elevated,
                    _ => FeePriority::Priority,
                },
                u64::MAX,
            )
            .await
            .map_err(|e| MoneroError::Failed(format!("fee rate: {e:?}")))?;

        let mut outgoing = Zeroizing::new([0u8; 32]);
        OsRng.fill_bytes(outgoing.as_mut());

        let tx = SignableTransaction::new(
            RctType::ClsagBulletproofPlus,
            outgoing,
            decoyed,
            vec![(dest, amount_pxmr)],
            // Change back to ourselves. Omitting it does not save a fee, it
            // donates the remainder to the miners.
            Change::new(vp.clone(), None),
            vec![],
            fee_rate,
        )
        .map_err(|e| MoneroError::Failed(describe_send_error(&format!("{e:?}"))))?;

        let fee = tx.necessary_fee();
        let signed = tx
            .sign(&mut OsRng, &spend)
            .map_err(|e| MoneroError::Failed(format!("signing: {e:?}")))?;
        let txid = hex_of(&signed.hash());

        // §8.7.2, learned twice here: one relay accepted a transaction, returned
        // success, and propagated nothing. Ok from a single node means that node
        // took it, not that the network has it. Nodes deduplicate, so submitting
        // everywhere is free.
        let mut accepted = 0u32;
        for r in [
            node_url.as_str(),
            "http://xmr-lux.boldsuck.org:38081",
            "http://node.monerodevs.org:38089",
            "http://stagenet.xmr-tw.org:38081",
        ] {
            let Ok(t) = monero_daemon_rpc::MoneroDaemon::new(UreqTransport::new(r.to_string())).await
            else { continue };
            if t.publish_transaction(&signed).await.is_ok() {
                accepted += 1;
            }
        }
        if accepted == 0 {
            return Err(MoneroError::Failed(
                "signed, but no node accepted it — it has not been sent".into(),
            ));
        }

        Ok(SendResult { txid_hex: txid, fee_pxmr: fee, accepted_by: accepted })
    })
}

/// Turn the library's error into something a person can act on.
fn describe_send_error(raw: &str) -> String {
    if raw.contains("NotEnoughFunds") || raw.contains("NotEnoughCoins") {
        "not enough in the notes you picked, once the fee is counted".into()
    } else if raw.contains("TooManyInputs") {
        "too many notes at once — send in smaller batches".into()
    } else if raw.contains("NoInputs") {
        "no notes selected".into()
    } else if raw.contains("NoOutputs") {
        "no destination".into()
    } else {
        format!("could not build the transaction: {raw}")
    }
}

// ---------------------------------------------------------------------------
// What a payment will cost (§17)
// ---------------------------------------------------------------------------

/// A fee estimate, with everything needed to show why it is that number.
#[derive(uniffi::Record, Clone)]
pub struct FeeEstimate {
    pub fee_pxmr: u64,
    /// Piconero per byte, straight from the daemon.
    pub per_byte: u64,
    /// The transaction size this assumed.
    pub estimated_bytes: u64,
    /// Roughly how long until the first confirmation, at this priority.
    pub minutes_to_confirm: u32,
    /// The four tiers the daemon offered, cheapest first, as whole fees for
    /// this transaction shape — so a caller can show the trade rather than a
    /// number nobody can compare against anything.
    pub tier_fees_pxmr: Vec<u64>,
}

/// Estimate the fee for a transaction of this shape.
///
/// **An estimate, and labelled one everywhere it is shown.** The exact fee is
/// only known once decoys are chosen and the transaction is built, which costs
/// network round trips and cannot happen on every keystroke. It is close: the
/// weight model below is the actual structure of a CLSAG/Bulletproof+
/// transaction, not a guess at an average.
#[uniffi::export]
pub fn monero_fee_estimate(
    node_url: String,
    inputs: u32,
    outputs: u32,
    priority: u32,
) -> Result<FeeEstimate, MoneroError> {
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(15))
        .build();
    let resp = agent
        .post(&format!("{}/json_rpc", node_url.trim_end_matches('/')))
        .set("Content-Type", "application/json")
        .send_string(r#"{"jsonrpc":"2.0","id":"0","method":"get_fee_estimate"}"#)
        .map_err(|e| MoneroError::Failed(short_error(&e.to_string())))?;
    let v: serde_json::Value = resp
        .into_string()
        .map_err(|e| MoneroError::Failed(e.to_string()))
        .and_then(|b| serde_json::from_str(&b).map_err(|e| MoneroError::Failed(e.to_string())))?;
    let r = &v["result"];

    let tiers: Vec<u64> = r["fees"]
        .as_array()
        .map(|a| a.iter().filter_map(|x| x.as_u64()).collect())
        .filter(|v: &Vec<u64>| !v.is_empty())
        .unwrap_or_else(|| vec![r["fee"].as_u64().unwrap_or(20_000)]);
    let mask = r["quantization_mask"].as_u64().unwrap_or(10_000).max(1);

    let bytes = estimated_weight(inputs.max(1), outputs.max(2));
    let pick = |per_byte: u64| quantize(per_byte.saturating_mul(bytes), mask);

    let idx = (priority as usize).min(tiers.len() - 1);
    Ok(FeeEstimate {
        fee_pxmr: pick(tiers[idx]),
        per_byte: tiers[idx],
        estimated_bytes: bytes,
        // Monero targets two-minute blocks. The cheapest tier can sit through a
        // few before it is included; the dearest is normally the next block.
        minutes_to_confirm: match idx {
            0 => 20,
            1 => 6,
            2 => 4,
            _ => 2,
        },
        tier_fees_pxmr: tiers.iter().map(|t| pick(*t)).collect(),
    })
}

/// Monero rounds a fee up to a multiple of the quantisation mask, so an
/// estimate that does not is always slightly under — and "slightly under" on a
/// fee is a transaction that does not relay.
fn quantize(fee: u64, mask: u64) -> u64 {
    fee.div_ceil(mask).saturating_mul(mask)
}

/// Size of a ring-size-16 CLSAG transaction with Bulletproofs+.
///
/// Built from the parts rather than fitted to an average:
///
/// - **~650 bytes per input** — a CLSAG signature is `32·ring + 64` = 576 bytes,
///   plus the key image and the ring's key offsets.
/// - **~72 bytes per output** — one-time key, encrypted amount, ECDH tag.
/// - **~576 bytes of range proof** for up to two outputs, growing by a step each
///   time the output count crosses a power of two, because Bulletproofs+ are
///   logarithmic in the padded count.
/// - **~100 bytes** of prefix, version, unlock time and extra.
fn estimated_weight(inputs: u32, outputs: u32) -> u64 {
    let padded = outputs.next_power_of_two().max(2);
    let range_proof = 576 + 32 * (padded.trailing_zeros() as u64).saturating_sub(1) * 2;
    100 + (inputs as u64) * 650 + (outputs as u64) * 72 + range_proof
}
