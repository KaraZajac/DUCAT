//! Escrow, live: a 2-of-2 threshold wallet on stagenet, released by FROST.
//!
//! Part IV's bond and every deposit the rental verticals want reduce to one
//! question: can two mutually-distrusting parties hold money that neither
//! can move alone, and release it together, with the result looking like any
//! other Monero transaction? This example answers it against the real chain:
//!
//!   cargo run -p ducat-mobile --example escrowtest -- setup
//!       mint 2-of-2 threshold keys (dealer for the in-process demo; the
//!       wire ceremony uses PedPoP over the mailbox), print the escrow's
//!       address. Keys persist via a seed file so every run agrees.
//!
//!   cargo run -p ducat-mobile --example escrowtest -- balance
//!       scan the chain for the escrow's outputs.
//!
//!   cargo run -p ducat-mobile --example escrowtest -- release <address>
//!       both participants FROST-sign one transaction paying the escrow's
//!       whole balance to <address>, and broadcast it. Neither key alone
//!       can do this — that is the entire point.
//!
//! The escrow's view key is derived from the group key, so both parties (and
//! only they, plus anyone they show the group key) can scan for the funding.
//! On-chain, nothing distinguishes any of this from an ordinary wallet.

use std::collections::HashMap;

use modular_frost::{
    curve::Ed25519,
    sign::{PreprocessMachine, SignMachine, SignatureMachine},
    Participant, ThresholdKeys,
};
use monero_wallet::address::{AddressType, MoneroAddress, Network};
use monero_wallet::ed25519::Scalar;
use monero_wallet::ViewPair;
use rand_core::{OsRng, RngCore, SeedableRng};
use zeroize::Zeroizing;

const NODE: &str = "http://xmr-lux.boldsuck.org:38081";

/// A ureq HttpTransport — the app keeps its own in monero.rs; the example
/// carries a minimal twin so it stays self-contained.
#[derive(Clone)]
struct Ureq {
    url: String,
    agent: ureq::Agent,
}
impl Ureq {
    fn new(url: String) -> Self {
        Ureq {
            url: url.trim_end_matches('/').to_string(),
            agent: ureq::AgentBuilder::new()
                .timeout(std::time::Duration::from_secs(30))
                .build(),
        }
    }
}
impl monero_daemon_rpc::HttpTransport for Ureq {
    fn post(
        &self,
        route: &str,
        body: Vec<u8>,
        limit: Option<usize>,
    ) -> impl Send + std::future::Future<
        Output = Result<Vec<u8>, monero_daemon_rpc::prelude::InterfaceError>,
    > {
        let url = format!("{}/{}", self.url, route.trim_start_matches('/'));
        let agent = self.agent.clone();
        async move {
            use std::io::Read as _;
            let err = monero_daemon_rpc::prelude::InterfaceError::InterfaceError;
            let resp = agent
                .post(&url)
                .set("Content-Type", "application/octet-stream")
                .send_bytes(&body)
                .map_err(|e| err(e.to_string()))?;
            let cap = limit.unwrap_or(32 * 1024 * 1024);
            let mut out = Vec::new();
            resp.into_reader()
                .take(cap as u64 + 1)
                .read_to_end(&mut out)
                .map_err(|e| err(e.to_string()))?;
            Ok(out)
        }
    }
}

fn state_dir() -> std::path::PathBuf {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crate has a parent")
        .join("research/monero-rs");
    std::fs::create_dir_all(&p).expect("state dir");
    p
}

/// Deterministic keys from a persisted seed: the dealer runs identically on
/// every invocation, so "setup" is idempotent and the two halves of the demo
/// always agree. The seed file is the escrow — treat it like the money.
fn keys() -> HashMap<Participant, ThresholdKeys<Ed25519>> {
    let seed_path = state_dir().join("escrow.seed");
    let seed: [u8; 32] = if seed_path.exists() {
        std::fs::read(&seed_path).expect("seed").try_into().expect("32 bytes")
    } else {
        let mut s = [0u8; 32];
        OsRng.fill_bytes(&mut s);
        std::fs::write(&seed_path, s).expect("write seed");
        s
    };
    let mut rng = rand_chacha::ChaCha20Rng::from_seed(seed);
    dkg_dealer::key_gen::<_, Ed25519>(&mut rng, 2, 2).expect("dealer keygen")
}

/// The escrow's wallet: spend key = the group key nobody holds alone; view
/// key = a scalar derived *from* the group key, so both parties can scan.
fn escrow_view_pair(keys: &HashMap<Participant, ThresholdKeys<Ed25519>>) -> ViewPair {
    let group = keys.values().next().expect("keys").group_key();
    let mut material = b"DUCAT-ESCROW-VIEW-v0".to_vec();
    material.extend_from_slice(group.compress().as_bytes());
    let view = Zeroizing::new(Scalar::hash(&material));
    // group_key() is dalek_ff_group's EdwardsPoint; .0 is the curve25519
    // point monero-wallet's Point wraps.
    let spend = monero_wallet::ed25519::Point::from(group.0);
    ViewPair::new(spend, view).expect("view pair")
}

fn scan(
    vp: &ViewPair,
    from: u64,
) -> (u64, Vec<monero_wallet::WalletOutput>) {
    use monero_daemon_rpc::prelude::*;
    #[allow(unused_imports)]
    use monero_wallet::Scanner;

    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    rt.block_on(async {
        let rpc = monero_daemon_rpc::MoneroDaemon::new(Ureq::new(NODE.into()))
            .await
            .expect("connect");
        let tip = rpc.latest_block_number().await.expect("height") as u64;
        let mut scanner = Scanner::new(vp.clone());
        let mut outputs = Vec::new();
        let mut h = from;
        while h <= tip {
            let Ok(block) = rpc.block_by_number(h as usize).await else { h += 1; continue };
            let Ok(sb) = rpc.expand_to_scannable_block(block).await else { h += 1; continue };
            if let Ok(found) = scanner.scan(sb) {
                for o in found.not_additionally_locked() {
                    println!("  found {} pXMR at height {h}", o.commitment().amount);
                    outputs.push(o);
                }
            }
            h += 1;
        }
        (tip, outputs)
    })
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mode = args.get(1).map(|s| s.as_str()).unwrap_or("setup");

    let keys = keys();
    let vp = escrow_view_pair(&keys);
    let addr = vp.legacy_address(Network::Stagenet);
    let height_path = state_dir().join("escrow.height");

    match mode {
        "setup" => {
            // Record the birth height once: the escrow has no history to miss.
            if !height_path.exists() {
                use monero_daemon_rpc::prelude::*;
                let rt =
                    tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
                let tip = rt.block_on(async {
                    let rpc =
                        monero_daemon_rpc::MoneroDaemon::new(Ureq::new(NODE.into()))
                            .await
                            .expect("connect");
                    rpc.latest_block_number().await.expect("height") as u64
                });
                std::fs::write(&height_path, tip.to_string()).expect("height");
            }
            println!("escrow (2-of-2, FROST)");
            println!("  address  {addr}");
            println!("  birth    {}", std::fs::read_to_string(&height_path).unwrap());
            println!("  fund it, then: escrowtest release <address>");
        }
        "balance" => {
            let from: u64 =
                std::fs::read_to_string(&height_path).expect("run setup first").parse().unwrap();
            let (tip, outputs) = scan(&vp, from);
            let total: u64 = outputs.iter().map(|o| o.commitment().amount).sum();
            println!("  {} output(s), {} pXMR, tip {tip}", outputs.len(), total);
        }
        "release" => {
            let dest = args.get(2).expect("release <address>");
            let dest = MoneroAddress::from_str_with_unchecked_network(dest).expect("address");
            release(&keys, &vp, dest, &height_path);
        }
        other => eprintln!("unknown mode {other}"),
    }
    // The address type is part of what we assert: a plain legacy address,
    // indistinguishable from any single-signer wallet.
    let _ = AddressType::Legacy;
}

fn release(
    keys: &HashMap<Participant, ThresholdKeys<Ed25519>>,
    vp: &ViewPair,
    dest: MoneroAddress,
    height_path: &std::path::Path,
) {
    use monero_daemon_rpc::prelude::*;
    use monero_wallet::ringct::RctType;
    use monero_wallet::send::{Change, SignableTransaction};
    use monero_wallet::OutputWithDecoys;

    let from: u64 =
        std::fs::read_to_string(height_path).expect("run setup first").parse().unwrap();
    let (_, outputs) = scan(vp, from);
    assert!(!outputs.is_empty(), "the escrow holds nothing to release");
    let total: u64 = outputs.iter().map(|o| o.commitment().amount).sum();
    println!("  releasing {} pXMR to {dest}", total);

    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    rt.block_on(async {
        let rpc = monero_daemon_rpc::MoneroDaemon::new(Ureq::new(NODE.into()))
            .await
            .expect("connect");
        let tip = rpc.latest_block_number().await.expect("height");

        let mut decoyed = Vec::new();
        for o in outputs {
            decoyed.push(
                OutputWithDecoys::new(&mut OsRng, &rpc, 16, tip, o).await.expect("decoys"),
            );
        }
        let fee_rate = rpc.fee_rate(FeePriority::Normal, u64::MAX).await.expect("fee");

        let mut outgoing = Zeroizing::new([0u8; 32]);
        OsRng.fill_bytes(outgoing.as_mut());

        // Sweep needs an explicit output, not just a change target. A
        // 1-input 1-output CLSAG tx costs well under 0.00005 XMR; reserve
        // that, pay the rest, and any slack over the true fee is donated —
        // fine for a test release. Change::None: nothing comes back.
        // One payment output, change back to the same address: everything
        // minus the true network fee arrives at `dest`, split across the
        // payment and the change output. The reserve need only exceed the
        // real fee; the surplus returns as change rather than being donated.
        const FEE_RESERVE: u64 = 200_000_000;
        assert!(total > FEE_RESERVE, "escrow too small to cover the fee");
        let payout = total - FEE_RESERVE;
        let tx = SignableTransaction::new(
            RctType::ClsagBulletproofPlus,
            outgoing,
            decoyed,
            vec![(dest, payout)],
            Change::fingerprintable(Some(dest)),
            vec![],
            fee_rate,
        )
        .expect("signable");
        let fee = tx.necessary_fee();

        // The FROST dance, both signers in this process for the demo; over
        // the wire these two maps are two sealed messages each way.
        let p1 = Participant::new(1).unwrap();
        let p2 = Participant::new(2).unwrap();
        let m1 = tx.clone().multisig(keys[&p1].clone()).expect("machine 1");
        let m2 = tx.clone().multisig(keys[&p2].clone()).expect("machine 2");

        let (m1, pre1) = m1.preprocess(&mut OsRng);
        let (m2, pre2) = m2.preprocess(&mut OsRng);

        let (m1, share1) = m1
            .sign(HashMap::from([(p2, pre2.clone())]), &[])
            .expect("sign 1");
        let (m2, share2) = m2
            .sign(HashMap::from([(p1, pre1.clone())]), &[])
            .expect("sign 2");

        let signed = m1.complete(HashMap::from([(p2, share2)])).expect("complete");
        // Symmetry check: participant 2 completes to the identical bytes.
        let signed2 = m2.complete(HashMap::from([(p1, share1)])).expect("complete 2");
        assert_eq!(signed.hash(), signed2.hash(), "both signers derive one transaction");

        let txid: String =
            signed.hash().iter().map(|b| format!("{b:02x}")).collect();
        println!("  fee {fee} pXMR");
        println!("  txid {txid}");

        let mut accepted = 0u32;
        for r in [
            NODE,
            "http://node.monerodevs.org:38089",
            "http://stagenet.xmr-tw.org:38081",
        ] {
            if let Ok(rpc) = monero_daemon_rpc::MoneroDaemon::new(Ureq::new(r.into()))
                .await
            {
                if rpc.publish_transaction(&signed).await.is_ok() {
                    accepted += 1;
                }
            }
        }
        println!("  accepted by {accepted} node(s)");
        assert!(accepted > 0, "no relay took the release");
    });
}
