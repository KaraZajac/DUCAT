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
    vec![
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
        NodeCandidate {
            url: "http://node.monerodevs.org:38089".into(),
            trust: NodeTrust::PublicClearnet,
            label: "monerodevs (stagenet)".into(),
        },
    ]
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
    /// Hex key image, so spentness can be checked against the daemon. Derived
    /// from the spend secret, which is why scanning for a *balance* needs more
    /// than the view key that scanning for *receipts* does.
    pub key_image_hex: String,
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
) -> Result<ScanResult, MoneroError> {
    use curve25519_dalek::constants::ED25519_BASEPOINT_TABLE;
    use monero_simple_request_rpc::prelude::*;
    use monero_simple_request_rpc::SimpleRequestTransport;
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
        let rpc = SimpleRequestTransport::new(node_url.clone())
            .await
            .map_err(|e| MoneroError::Failed(format!("connect: {e:?}")))?;
        let tip = rpc
            .latest_block_number()
            .await
            .map_err(|e| MoneroError::Failed(format!("height: {e:?}")))? as u64;

        let vp = ViewPair::new(spend_pub, view)
            .map_err(|e| MoneroError::Failed(format!("view pair: {e:?}")))?;
        let mut scanner = Scanner::new(vp);
        let mut outputs = Vec::new();

        // Bounded, and the bound is not politeness: each block is a round trip
        // to someone else's node, so an unbounded loop on a phone is a scan
        // that never returns and a screen that never updates.
        let last = tip.min(from_height + max_blocks as u64);
        let mut h = from_height;
        while h <= last {
            let block = match rpc.block_by_number(h as usize).await {
                Ok(b) => b,
                // A gap is skipped rather than fatal: one flaky block should not
                // discard the window's progress.
                Err(_) => { h += 1; continue }
            };
            let sb = match rpc.expand_to_scannable_block(block).await {
                Ok(b) => b,
                Err(_) => { h += 1; continue }
            };
            if let Ok(found) = scanner.scan(sb) {
                for o in found.not_additionally_locked() {
                    // KI = (spend + key_offset) * Hp(output key). Derivable only
                    // with the spend secret, which is what separates knowing a
                    // payment arrived from knowing it is still there.
                    let x: curve25519_dalek::Scalar = spend.into() + o.key_offset().into();
                    let ki = o.key().key_image().map(|gen| gen * x);
                    outputs.push(OwnedOutput {
                        amount_pxmr: o.commitment().amount,
                        height: h,
                        key_image_hex: ki
                            .map(|k| hex_of(k.compress().to_bytes().as_slice()))
                            .unwrap_or_default(),
                    });
                }
            }
            h += 1;
        }

        Ok(ScanResult { scanned_to: last, tip, outputs })
    })
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
) -> Result<ScanResult, MoneroError> {
    use monero_simple_request_rpc::prelude::*;
    use monero_simple_request_rpc::SimpleRequestTransport;
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
        let rpc = SimpleRequestTransport::new(node_url)
            .await
            .map_err(|e| MoneroError::Failed(format!("connect: {e:?}")))?;
        let tip = rpc
            .latest_block_number()
            .await
            .map_err(|e| MoneroError::Failed(format!("height: {e:?}")))? as u64;

        let vp = ViewPair::new(addr.spend(), view)
            .map_err(|e| MoneroError::Failed(format!("view pair: {e:?}")))?;
        let mut scanner = Scanner::new(vp);
        let mut outputs = Vec::new();

        let last = tip.min(from_height + max_blocks as u64);
        let mut h = from_height;
        while h <= last {
            let Ok(block) = rpc.block_by_number(h as usize).await else { h += 1; continue };
            let Ok(sb) = rpc.expand_to_scannable_block(block).await else { h += 1; continue };
            if let Ok(found) = scanner.scan(sb) {
                for o in found.not_additionally_locked() {
                    outputs.push(OwnedOutput {
                        amount_pxmr: o.commitment().amount,
                        height: h,
                        // Deliberately blank: with no spend secret there is no
                        // key image, and inventing a placeholder would let a
                        // caller ask about spentness and believe the answer.
                        key_image_hex: String::new(),
                    });
                }
            }
            h += 1;
        }
        Ok(ScanResult { scanned_to: last, tip, outputs })
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
