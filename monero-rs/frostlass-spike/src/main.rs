//! O1: FROSTLASS given the same treatment wallet2 got.
//!
//! Everything empirical in this project so far is Monero's native multisig —
//! the 2-round/134 s ceremony, the bond seized by arbiter + recovery, the
//! 2,286-byte key file. §8.2 intends to ship FROSTLASS instead, and the spec has
//! been carrying its claims on the strength of a README.
//!
//! The decisive question is not performance. **Monero's native multisig supports
//! only N-of-N and (N−1)-of-N**, so 2-of-3 exists and 3-of-5 does not — which is
//! why §9.3's "multiple arbiters can co-sign" was unbuildable on the tested path.
//! If FROSTLASS forms a working 3-of-5, that limitation is an implementation
//! choice rather than a property of Monero.

use std::collections::HashMap;
use std::time::Instant;

use dkg::{Participant, ThresholdKeys};
use monero_wallet::{
    address::Network,
    ed25519::{Point, Scalar},
    ViewPair,
};
use rand_core::{OsRng, RngCore};
use zeroize::Zeroizing;

use modular_frost::curve::Ed25519;

fn keygen(t: u16, n: u16) -> (HashMap<Participant, ThresholdKeys<Ed25519>>, f64) {
    let started = Instant::now();
    let keys = dkg_dealer::key_gen::<_, Ed25519>(&mut OsRng, t, n)
        .expect("dealer key generation");
    (keys, started.elapsed().as_secs_f64())
}

/// Build the group's Monero address.
///
/// Worth noting what this shows: the group **spend** key is the FROST group key,
/// which no participant knows the private half of. The **view** key is ordinary
/// shared secret material distributed alongside — it is not derived from the
/// spend key, because nobody holds that. A DUCAT escrow must therefore carry the
/// view key in the ceremony, and every participant sees every payment into the
/// escrow. That is correct for an escrow and would be wrong for a bond.
fn address_of(keys: &ThresholdKeys<Ed25519>, view: &Zeroizing<Scalar>) -> String {
    let spend_pub = Point::from(keys.group_key().0);
    let mut outgoing = Zeroizing::new([0u8; 32]);
    OsRng.fill_bytes(outgoing.as_mut());
    let vp = ViewPair::new(spend_pub, view.clone()).expect("view pair");
    vp.legacy_address(Network::Stagenet).to_string()
}

/// Persist the group so funding and spending can be separate runs — an output
/// needs ten blocks to unlock, and regenerating keys would change the address.
fn save_group(keys: &HashMap<Participant, ThresholdKeys<Ed25519>>, view: &Zeroizing<Scalar>, addr: &str) {
    std::fs::create_dir_all("group").unwrap();
    for (i, k) in keys {
        std::fs::write(format!("group/share-{}.bin", u16::from(*i)), k.serialize().to_vec()).unwrap();
    }
    let mut vb = Vec::new();
    view.write(&mut vb).unwrap();
    std::fs::write("group/view.hex", hex::encode(&vb)).unwrap();
    std::fs::write("group/address.txt", addr).unwrap();
}

fn main() {
    if let Some(i) = std::env::args().position(|a| a == "--spend") {
        let to = std::env::args().nth(i + 1).expect("--spend <address>");
        tokio::runtime::Runtime::new().unwrap().block_on(spend::run(&to));
        return;
    }
    if std::env::args().any(|a| a == "--keygen") {
        let (keys, secs) = keygen(3, 5);
        let view = Zeroizing::new(Scalar::random(&mut OsRng));
        let addr = address_of(&keys[&Participant::new(1).unwrap()], &view);
        save_group(&keys, &view, &addr);
        println!("\n3-of-5 group formed in {:.3}s", secs);
        println!("address: {addr}");
        println!("saved to group/ — fund this address, then run with --spend");
        return;
    }

    println!("\n\x1b[1mFROSTLASS — thresholds Monero's native multisig cannot express\x1b[0m\n");

    let view = Zeroizing::new(Scalar::random(&mut OsRng));

    for (t, n, note) in [
        (2u16, 3u16, "wallet2 can do this: 2-of-3 is (N-1)-of-N"),
        (3, 5, "wallet2 CANNOT do this: threshold below N-1"),
        (2, 5, "wallet2 cannot do this either"),
        (7, 11, "arbitrary thresholds, for the sake of the argument"),
    ] {
        let (keys, secs) = keygen(t, n);
        let one = &keys[&Participant::new(1).unwrap()];
        let addr = address_of(one, &view);
        let share = one.serialize();
        println!("  {t}-of-{n}");
        println!("    {note}");
        println!("    group key   {}", hex::encode(one.group_key().0.compress().to_bytes()));
        println!("    address     {}…", &addr[..24]);
        println!("    share       {} bytes, serialized", share.len());
        println!("    keygen      {:.3}s", secs);
        println!();
    }

    println!("  \x1b[1mWhat this settles and what it does not\x1b[0m");
    println!("    Settled: arbitrary t-of-n forms. Monero's native scheme admits only");
    println!("    N-of-N and (N-1)-of-N, so 3-of-5 does not exist there at all.");
    println!();
    println!("    A share is linear in n, not fixed — 32 bytes per participant, and");
    println!("    independent of t. 151 / 215 / 407 bytes for n = 3 / 5 / 11. An earlier");
    println!("    draft of this program claimed fixed size and its own output refuted it.");
    println!("    Linear is still the point: wallet2 gives each member a combinatorial");
    println!("    set of keys, and its 2-of-3 wallet file measured 2,286 bytes against");
    println!("    151 here — the same group, 15x smaller, and backup-able without the");
    println!("    file-copy workaround §4.3.3 needs.");
    println!();
    println!("    NOT settled by this run: keys come from a trusted dealer, because");
    println!("    dkg 0.6.1 ships no interactive DKG. A real deployment needs one, and");
    println!("    a dealer who keeps the polynomial holds every share. Signing and");
    println!("    settlement are exercised separately.");
}

// ---------------------------------------------------------------------------
// --spend: the half that actually exercises FROSTLASS
// ---------------------------------------------------------------------------
//
// Forming a group tests key generation. Signing a real CLSAG with a subset of
// that group tests the thing §8.2 wants to ship, on the chain, with money.

mod spend {
    use super::*;
    use monero_simple_request_rpc::{prelude::*, SimpleRequestTransport};
    use monero_wallet::{
        address::MoneroAddress,
        ringct::RctType,
        send::{Change, SignableTransaction},
        OutputWithDecoys, Scanner,
    };

    const RELAYS: &[&str] = &[
        "http://xmr-lux.boldsuck.org:38081",
        "http://node.monerodevs.org:38089",
        "http://stagenet.xmr-tw.org:38081",
    ];

    fn load() -> (HashMap<Participant, ThresholdKeys<Ed25519>>, Zeroizing<Scalar>, String) {
        let mut keys = HashMap::new();
        for i in 1u16..=5 {
            let raw = std::fs::read(format!("group/share-{i}.bin")).expect("run --keygen first");
            let k = ThresholdKeys::<Ed25519>::read(&mut raw.as_slice()).expect("share");
            keys.insert(Participant::new(i).unwrap(), k);
        }
        let vh = std::fs::read_to_string("group/view.hex").unwrap();
        let vb = hex::decode(vh.trim()).unwrap();
        let view = Zeroizing::new(Scalar::read(&mut vb.as_slice()).unwrap());
        let addr = std::fs::read_to_string("group/address.txt").unwrap();
        (keys, view, addr)
    }

    pub async fn run(pay_to: &str) {
        let (keys, view, addr) = load();
        let one = &keys[&Participant::new(1).unwrap()];
        let vp = ViewPair::new(Point::from(one.group_key().0), view.clone()).unwrap();
        println!("\n\x1b[1mFROSTLASS — signing a real CLSAG with 3 of 5\x1b[0m\n");
        println!("  group    {}…", &addr[..28]);

        let mut rpc = None;
        for r in RELAYS {
            if let Ok(c) = SimpleRequestTransport::new(r.to_string()).await {
                if c.latest_block_number().await.is_ok() {
                    println!("  node     {r}");
                    rpc = Some(c);
                    break;
                }
            }
        }
        let rpc = rpc.expect("no stagenet node reachable");
        let tip = rpc.latest_block_number().await.unwrap();

        // Find the funding output. Scanning a window back from the tip rather
        // than tracking a height: the funding is recent by construction, and a
        // client that cannot find its own money without external bookkeeping
        // has not really scanned.
        let mut scanner = Scanner::new(vp.clone());
        let mut found = None;
        let start = tip.saturating_sub(40);
        for h in start..=tip {
            let block = match rpc.block_by_number(h).await {
                Ok(b) => b,
                Err(_) => continue,
            };
            let sb = match rpc.expand_to_scannable_block(block).await {
                Ok(b) => b,
                Err(_) => continue,
            };
            let outs = scanner.scan(sb).unwrap().not_additionally_locked();
            if let Some(o) = outs.into_iter().next() {
                println!("  found    {} pXMR at height {}", o.commitment().amount, h);
                found = Some((o, h));
                break;
            }
        }
        let (output, height) = found.expect("no unlocked output for the group — still locked?");

        // Build.
        let decoyed = OutputWithDecoys::new(&mut OsRng, &rpc, 16, height, output.clone())
            .await
            .expect("decoy selection");
        let dest = MoneroAddress::from_str_with_unchecked_network(pay_to).expect("address");
        let fee_rate = rpc.fee_rate(FeePriority::Unimportant, u64::MAX).await.unwrap();
        let mut outgoing = Zeroizing::new([0u8; 32]);
        OsRng.fill_bytes(outgoing.as_mut());
        let amount = output.commitment().amount / 2;

        let tx = SignableTransaction::new(
            RctType::ClsagBulletproofPlus,
            outgoing,
            vec![decoyed],
            vec![(dest, amount)],
            Change::new(vp.clone(), None),
            vec![],
            fee_rate,
        )
        .expect("signable transaction");

        // Sign with participants 1, 2, 3 — three of five. wallet2 has no such
        // configuration to sign with.
        let signers = [1u16, 2, 3];
        println!("  signers  {:?} of 5", signers);
        let started = Instant::now();
        let mut machines = HashMap::new();
        for i in signers {
            let p = Participant::new(i).unwrap();
            machines.insert(p, tx.clone().multisig(keys[&p].clone()).expect("multisig machine"));
        }
        let signed = modular_frost::tests::sign_without_caching(&mut OsRng, machines, &[]);
        println!("  signed   in {:.3}s", started.elapsed().as_secs_f64());
        println!("  txid     {}", hex::encode(signed.hash()));

        // §8.7.2, learned the hard way twice in this spike: one relay accepted a
        // transaction, returned success, and propagated nothing. `Ok(())` from a
        // single node means that node took it, not that the network has it.
        // So submit everywhere — nodes deduplicate, making this free — and then
        // verify on a node we did not submit through.
        let mut accepted_by = Vec::new();
        for r in RELAYS {
            let Ok(t) = SimpleRequestTransport::new(r.to_string()).await else { continue };
            if t.publish_transaction(&signed).await.is_ok() {
                accepted_by.push(*r);
            }
        }
        println!("  submitted to {} relay(s)", accepted_by.len());

        let hash = hex::encode(signed.hash());
        let mut seen_on = Vec::new();
        for r in RELAYS {
            // Asking the relay that accepted it whether it has it proves nothing.
            if accepted_by.len() == 1 && accepted_by.first() == Some(r) {
                continue;
            }
            let Ok(t) = SimpleRequestTransport::new(r.to_string()).await else { continue };
            // `get_transactions` with the pool included. The typed
            // `transaction()` helper resolves only *confirmed* transactions, so
            // using it here reported every freshly-broadcast transaction as lost
            // — a false negative this program printed once before this comment
            // existed. A propagation check that cannot see the mempool is
            // checking the wrong thing: propagation is precisely the window
            // before confirmation.
            let body = format!("{{\"txs_hashes\":[\"{hash}\"]}}");
            let Ok(raw) = t.rpc_call("get_transactions", Some(body), 1 << 20).await
            else { continue };
            let parsed: serde_json::Value =
                serde_json::from_str(&raw).unwrap_or(serde_json::Value::Null);
            let found = parsed["txs"].as_array().map(|a| !a.is_empty()).unwrap_or(false);
            if found {
                seen_on.push(*r);
            }
        }
        if seen_on.is_empty() {
            println!("\n  \x1b[31mnot visible on any relay\x1b[0m — {hash} went nowhere");
        } else {
            println!("\n  \x1b[32mpropagated\x1b[0m — visible on {} relay(s): {:?}", seen_on.len(), seen_on);
        }
    }
}

// ---------------------------------------------------------------------------
// A real ceremony: exists, does not compose (see README)
// ---------------------------------------------------------------------------
//
// An earlier run of this spike reported that "dkg 0.6.1 ships no interactive
// DKG", and the spec recorded that as the largest gap in §8.2's plan. **That was
// wrong and is retracted.** The DKG was split into its own crate: `dkg-pedpop`
// 0.6.0 implements PedPoP — Pedersen DKG with proof of possession, the
// construction the FROST paper specifies — as a three-round protocol with blame
// assignment, which is exactly what mutually distrusting parties need.
//
// The code to drive it was written and is not here, because it does not build.
// Two independent defects, both established by compiling rather than reading:
//
//   1. `dkg-pedpop` 0.6.0 declares `multiexp` with `features = ["std"]` while
//      its own source uses `multiexp::BatchVerifier`, which lives behind the
//      `batch` feature. It does not build standalone at all.
//
//   2. Forcing that feature exposes the real problem. `dkg-pedpop` 0.6.0 pins
//      `multiexp 0.4`, while `modular-frost 0.11.1` — which `monero-wallet`
//      0.2.0 requires for multisig — pins `multiexp 0.5`. Both end up in the
//      tree, and `dkg-pedpop` hands a `BatchVerifier` from 0.4.2 to
//      `schnorr-signatures` 0.5.3, which wants the 0.5.1 type.
//
// So key generation and threshold signing cannot both be obtained from
// crates.io today. The gap is smaller than "nobody has built this" and larger
// than "add a dependency".
