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
