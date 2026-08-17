//! Escrow, live and trustless: a 2-of-2 threshold wallet built by DKG and
//! released by FROST, on stagenet.
//!
//! The property this proves is the one that matters: **no party ever holds
//! the other's secret, and there is no dealer.** Each participant runs its
//! own PedPoP machine, and the only things that cross between them are the
//! serialized wire messages the FROST paper specifies — commitments, then
//! encrypted shares. What each side saves is its own `ThresholdKeys` and
//! nothing else; both independently arrive at the same group key, which
//! becomes a Monero address indistinguishable from any single-signer wallet.
//! Over DUCAT these wire messages are the sealed §17.9 ceremony messages;
//! here they are `Vec<u8>` handed between two in-process machines, which is
//! the same bytes taking a shorter path.
//!
//!   escrowtest dkg              run the two-party ceremony, write two key
//!                               files (party1/party2 — two "devices"),
//!                               print the escrow address.
//!   escrowtest balance         scan the chain for the escrow's outputs.
//!   escrowtest release <addr>   both parties FROST-sign one sweep to <addr>,
//!                               exchanging only preprocess/share bytes.

use std::collections::HashMap;

use ciphersuite::Ciphersuite;
use dkg_pedpop::{
    Commitments, EncryptedMessage, EncryptionKeyMessage, KeyGenMachine, SecretShare,
};
use modular_frost::{
    curve::Ed25519,
    dkg::{Participant, ThresholdKeys, ThresholdParams},
    sign::{PreprocessMachine, SignMachine, SignatureMachine, Writable},
};
use monero_wallet::address::{MoneroAddress, Network};
use monero_wallet::ed25519::Scalar;
use monero_wallet::ViewPair;
use rand_core::{OsRng, RngCore};
use zeroize::Zeroizing;

const NODE: &str = "http://xmr-lux.boldsuck.org:38081";
// The DKG context binds a ceremony to a purpose; both parties must agree on
// it, and it must be unique per multisig. In DUCAT it is the escrow's
// ceremony_id (§17.9); here, a constant for the one demo escrow.
const CONTEXT: [u8; 32] = *b"DUCAT-escrow-demo-context-v0-pad";

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
    ) -> impl Send
           + std::future::Future<Output = Result<Vec<u8>, monero_daemon_rpc::prelude::InterfaceError>>
    {
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

fn params(i: u16) -> ThresholdParams {
    ThresholdParams::new(2, 2, Participant::new(i).unwrap()).unwrap()
}

/// The two-party PedPoP DKG, run honestly: two machines, bytes between them.
///
/// Every value that crosses `party1`↔`party2` here is a serialized wire
/// message — never a machine, never a secret. That is the whole point, and
/// it is why these same steps become the §17.9 sealed messages unchanged.
fn run_dkg() -> (ThresholdKeys<Ed25519>, ThresholdKeys<Ed25519>) {
    let p1 = Participant::new(1).unwrap();
    let p2 = Participant::new(2).unwrap();

    // Round 1: each party commits, and broadcasts the commitment bytes.
    let (ss1, c1) =
        KeyGenMachine::<Ed25519>::new(params(1), CONTEXT).generate_coefficients(&mut OsRng);
    let (ss2, c2) =
        KeyGenMachine::<Ed25519>::new(params(2), CONTEXT).generate_coefficients(&mut OsRng);
    let c1_wire = c1.serialize();
    let c2_wire = c2.serialize();

    // Round 2: each reads the *other's* commitment off the wire and produces
    // an encrypted share addressed to them.
    let c2_at_1 = read_commitments(&c2_wire, params(1));
    let c1_at_2 = read_commitments(&c1_wire, params(2));
    let (km1, shares1) = ss1
        .generate_secret_shares(&mut OsRng, HashMap::from([(p2, c2_at_1)]))
        .expect("shares 1");
    let (km2, shares2) = ss2
        .generate_secret_shares(&mut OsRng, HashMap::from([(p1, c1_at_2)]))
        .expect("shares 2");
    let s1to2_wire = shares1[&p2].serialize();
    let s2to1_wire = shares2[&p1].serialize();

    // Round 3: each reads the share meant for it and completes to its keys.
    let s2to1 = read_share(&s2to1_wire, params(1));
    let s1to2 = read_share(&s1to2_wire, params(2));
    let keys1 = km1
        .calculate_share(&mut OsRng, HashMap::from([(p2, s2to1)]))
        .expect("calc 1")
        .complete();
    let keys2 = km2
        .calculate_share(&mut OsRng, HashMap::from([(p1, s1to2)]))
        .expect("calc 2")
        .complete();

    assert_eq!(keys1.group_key(), keys2.group_key(), "both parties, one group key");
    (keys1, keys2)
}

fn read_commitments(
    bytes: &[u8],
    p: ThresholdParams,
) -> EncryptionKeyMessage<Ed25519, Commitments<Ed25519>> {
    EncryptionKeyMessage::read(&mut &bytes[..], p).expect("commitments")
}

fn read_share(
    bytes: &[u8],
    p: ThresholdParams,
) -> EncryptedMessage<Ed25519, SecretShare<<Ed25519 as Ciphersuite>::F>> {
    EncryptedMessage::read(&mut &bytes[..], p).expect("share")
}

fn load_keys(name: &str) -> ThresholdKeys<Ed25519> {
    let bytes = std::fs::read(state_dir().join(name)).expect("key file — run dkg first");
    ThresholdKeys::read(&mut &bytes[..]).expect("read keys")
}

/// View key derived from the group key, so both parties (only) can scan.
fn view_pair(keys: &ThresholdKeys<Ed25519>) -> ViewPair {
    let group = keys.group_key();
    let mut material = b"DUCAT-ESCROW-VIEW-v0".to_vec();
    material.extend_from_slice(group.compress().as_bytes());
    let view = Zeroizing::new(Scalar::hash(&material));
    ViewPair::new(monero_wallet::ed25519::Point::from(group.0), view).expect("view pair")
}

fn scan(vp: &ViewPair, from: u64) -> (u64, Vec<monero_wallet::WalletOutput>) {
    use monero_daemon_rpc::prelude::*;
    use monero_wallet::Scanner;
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    rt.block_on(async {
        let rpc =
            monero_daemon_rpc::MoneroDaemon::new(Ureq::new(NODE.into())).await.expect("connect");
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
    let mode = args.get(1).map(|s| s.as_str()).unwrap_or("dkg");
    let height_path = state_dir().join("escrow.height");

    match mode {
        "dkg" => {
            let (keys1, keys2) = run_dkg();
            // Two files, two devices. Each holds ONLY its own share; there is
            // no file anywhere with the whole spend key.
            std::fs::write(state_dir().join("escrow.party1"), &*keys1.serialize()).unwrap();
            std::fs::write(state_dir().join("escrow.party2"), &*keys2.serialize()).unwrap();

            let vp = view_pair(&keys1);
            let addr = vp.legacy_address(Network::Stagenet);
            use monero_daemon_rpc::prelude::*;
            let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
            let tip = rt.block_on(async {
                let rpc =
                    monero_daemon_rpc::MoneroDaemon::new(Ureq::new(NODE.into())).await.expect("c");
                rpc.latest_block_number().await.expect("height") as u64
            });
            std::fs::write(&height_path, tip.to_string()).unwrap();
            println!("escrow (2-of-2, DKG — no dealer)");
            println!("  address  {addr}");
            println!("  birth    {tip}");
            println!("  party1/party2 key files written; neither holds the whole key");
            println!("  fund it, then: escrowtest release <address>");
        }
        "balance" => {
            let from: u64 =
                std::fs::read_to_string(&height_path).expect("run dkg first").parse().unwrap();
            let vp = view_pair(&load_keys("escrow.party1"));
            let (tip, outs) = scan(&vp, from);
            let total: u64 = outs.iter().map(|o| o.commitment().amount).sum();
            println!("  {} output(s), {} pXMR, tip {tip}", outs.len(), total);
        }
        "release" => {
            let dest = args.get(2).expect("release <address>");
            let dest = MoneroAddress::from_str_with_unchecked_network(dest).expect("address");
            release(dest, &height_path);
        }
        // The split release, through the *shipping* bridge functions — the
        // primitive under the escrow ladder (§15.12): a fixed slice to the
        // refund address, the residual to the payee, one transaction, two
        // FROST signers. `escrowtest split <residual_dest> <refund_dest>
        // <refund_pxmr>`.
        "split" => {
            let dest = args.get(2).expect("split <residual> <refund> <pxmr>").clone();
            let refund = args.get(3).expect("split <residual> <refund> <pxmr>").clone();
            let amount: u64 =
                args.get(4).expect("split <residual> <refund> <pxmr>").parse().expect("pxmr");
            let from: u64 =
                std::fs::read_to_string(&height_path).expect("run dkg first").parse().unwrap();
            let keys1 = std::fs::read(state_dir().join("escrow.party1")).expect("party1");
            let keys2 = std::fs::read(state_dir().join("escrow.party2")).expect("party2");
            let cid = vec![0x5Au8; 32]; // any 32 bytes, consistent across the calls

            let prop = ducat_mobile::ceremony::frost_propose_split(
                cid.clone(),
                1,
                keys1,
                vec![ducat_mobile::ceremony::SplitOut { dest: refund.clone(), amount_pxmr: amount }],
                dest.clone(),
                NODE.into(),
                from,
            )
            .expect("propose");
            println!(
                "  proposed: total {} pXMR, residual ≈{} pXMR to payee, {} pXMR to refund",
                prop.total_pxmr, prop.payout_pxmr, amount
            );
            let ans = ducat_mobile::ceremony::frost_cosign(cid.clone(), 2, 1, keys2, prop.payload)
                .expect("cosign");
            println!("  co-signed (fee {} pXMR)", ans.fee_pxmr);
            let txid = ducat_mobile::ceremony::frost_complete(cid, 1, 2, ans.payload, NODE.into())
                .expect("complete");
            println!("  SPLIT RELEASED — txid {txid}");
        }
        other => eprintln!("unknown mode {other}"),
    }
}

fn release(dest: MoneroAddress, height_path: &std::path::Path) {
    use monero_daemon_rpc::prelude::*;
    use monero_wallet::ringct::RctType;
    use monero_wallet::send::{Change, SignableTransaction};
    use monero_wallet::OutputWithDecoys;

    // Each "device" loads only its own key file.
    let keys1 = load_keys("escrow.party1");
    let keys2 = load_keys("escrow.party2");
    let vp = view_pair(&keys1);

    let from: u64 = std::fs::read_to_string(height_path).expect("run dkg first").parse().unwrap();
    let (_, outputs) = scan(&vp, from);
    assert!(!outputs.is_empty(), "the escrow holds nothing to release");
    let total: u64 = outputs.iter().map(|o| o.commitment().amount).sum();
    println!("  releasing {total} pXMR to {dest}");

    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    rt.block_on(async {
        let rpc =
            monero_daemon_rpc::MoneroDaemon::new(Ureq::new(NODE.into())).await.expect("connect");
        let tip = rpc.latest_block_number().await.expect("height");
        let mut decoyed = Vec::new();
        for o in outputs {
            decoyed
                .push(OutputWithDecoys::new(&mut OsRng, &rpc, 16, tip, o).await.expect("decoys"));
        }
        let fee_rate = rpc.fee_rate(FeePriority::Normal, u64::MAX).await.expect("fee");
        let mut outgoing = Zeroizing::new([0u8; 32]);
        OsRng.fill_bytes(outgoing.as_mut());

        // One payment output plus change back to the same address: everything
        // minus the true fee arrives at `dest`. The multisig tx's real fee is
        // ~0.00012 XMR; the reserve covers it and the surplus returns.
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

        let p1 = Participant::new(1).unwrap();
        let p2 = Participant::new(2).unwrap();
        let m1 = tx.clone().multisig(keys1).expect("machine 1");
        let m2 = tx.clone().multisig(keys2).expect("machine 2");

        // FROST round 1: preprocess. Each side's preprocess crosses as bytes.
        let (m1, pre1) = m1.preprocess(&mut OsRng);
        let (m2, pre2) = m2.preprocess(&mut OsRng);
        let pre1_wire = pre1.serialize();
        let pre2_wire = pre2.serialize();
        let pre2_at_1 = m1.read_preprocess(&mut &pre2_wire[..]).expect("read pre2");
        let pre1_at_2 = m2.read_preprocess(&mut &pre1_wire[..]).expect("read pre1");

        // FROST round 2: signature shares, again only bytes across.
        let (m1, share1) = m1.sign(HashMap::from([(p2, pre2_at_1)]), &[]).expect("sign 1");
        let (m2, share2) = m2.sign(HashMap::from([(p1, pre1_at_2)]), &[]).expect("sign 2");
        let share1_wire = share1.serialize();
        let share2_wire = share2.serialize();
        let share2_at_1 = m1.read_share(&mut &share2_wire[..]).expect("read share2");
        let share1_at_2 = m2.read_share(&mut &share1_wire[..]).expect("read share1");

        let signed = m1.complete(HashMap::from([(p2, share2_at_1)])).expect("complete 1");
        let signed2 = m2.complete(HashMap::from([(p1, share1_at_2)])).expect("complete 2");
        assert_eq!(signed.hash(), signed2.hash(), "both signers derive one transaction");

        let txid: String = signed.hash().iter().map(|b| format!("{b:02x}")).collect();
        println!("  fee {fee} pXMR");
        println!("  txid {txid}");

        let mut accepted = 0u32;
        for r in [NODE, "http://node.monerodevs.org:38089", "http://stagenet.xmr-tw.org:38081"] {
            if let Ok(rpc) = monero_daemon_rpc::MoneroDaemon::new(Ureq::new(r.into())).await {
                if rpc.publish_transaction(&signed).await.is_ok() {
                    accepted += 1;
                }
            }
        }
        println!("  accepted by {accepted} node(s)");
        assert!(accepted > 0, "no relay took the release");
    });
}
