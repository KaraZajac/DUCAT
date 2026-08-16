//! The ceremony engine (§17.9), driven exactly as a client drives it.
//!
//! Where dkg.rs exercises the raw threshold machines, this exercises DUCAT's
//! own engine — the slot-map API two devices call across poll cycles. Two
//! parties, distinct ceremony ids per party is wrong (they must share one),
//! each calling only its own step functions, handing the other only the
//! returned wire bytes. If both derive the same funding address, the engine
//! carries the ceremony correctly end to end.

use ducat_mobile::ceremony::{
    dkg_commit, dkg_finish, dkg_share, dkg_take_keys, FromParty,
};

#[test]
fn engine_builds_one_escrow_across_two_parties() {
    // One shared ceremony id — in DUCAT this is the escrow's 32-byte context.
    let id = vec![0x5c; 32];

    // Round 1: each party commits. Only the returned bytes cross.
    let c1 = dkg_commit(id.clone(), 1, 2, 2).unwrap();
    let c2 = dkg_commit(id.clone(), 2, 2, 2).unwrap();

    // Round 2: each is handed the *other's* commitment, returns shares.
    let s1 = dkg_share(id.clone(), 1, 2, 2, vec![FromParty { participant: 2, bytes: c2 }]).unwrap();
    let s2 = dkg_share(id.clone(), 2, 2, 2, vec![FromParty { participant: 1, bytes: c1 }]).unwrap();

    // The share party 1 produced is addressed to party 2, and vice versa.
    let s1_to_2 = s1.into_iter().find(|t| t.participant == 2).unwrap().bytes;
    let s2_to_1 = s2.into_iter().find(|t| t.participant == 1).unwrap().bytes;

    // Round 3: each finishes with the share meant for it.
    let addr1 =
        dkg_finish(id.clone(), 1, 2, 2, vec![FromParty { participant: 2, bytes: s2_to_1 }], true)
            .unwrap();
    let addr2 =
        dkg_finish(id.clone(), 2, 2, 2, vec![FromParty { participant: 1, bytes: s1_to_2 }], true)
            .unwrap();

    // Both parties, one escrow address — no dealer, only wire bytes crossed.
    assert_eq!(addr1, addr2);
    assert!(addr1.starts_with('5'), "a stagenet address");

    // Each device can take only its own keys, once, and they differ.
    let k1 = dkg_take_keys(id.clone(), 1).unwrap();
    let k2 = dkg_take_keys(id.clone(), 2).unwrap();
    assert_ne!(k1, k2, "each party holds a different share");
    assert!(dkg_take_keys(id, 1).is_err(), "keys are handed over exactly once");
}

/// The arbiter shape: three parties, threshold two. The same engine calls,
/// one more member in every list — which is the property the 2-of-2 API was
/// built to have, and this pins it. Any two of the three shares can later
/// sign; the third exists so a lost phone or a dispute cannot strand the
/// deposit (§17.9).
#[test]
fn engine_builds_one_escrow_across_three_parties() {
    let id = vec![0x5d; 32];

    let c1 = dkg_commit(id.clone(), 1, 2, 3).unwrap();
    let c2 = dkg_commit(id.clone(), 2, 2, 3).unwrap();
    let c3 = dkg_commit(id.clone(), 3, 2, 3).unwrap();

    // Every party hands the engine the OTHER TWO commitments.
    let from = |p: u16, b: &Vec<u8>| FromParty { participant: p, bytes: b.clone() };
    let s1 = dkg_share(id.clone(), 1, 2, 3, vec![from(2, &c2), from(3, &c3)]).unwrap();
    let s2 = dkg_share(id.clone(), 2, 2, 3, vec![from(1, &c1), from(3, &c3)]).unwrap();
    let s3 = dkg_share(id.clone(), 3, 2, 3, vec![from(1, &c1), from(2, &c2)]).unwrap();

    let to = |v: &Vec<ducat_mobile::ceremony::ToParty>, p: u16| {
        v.iter().find(|t| t.participant == p).unwrap().bytes.clone()
    };

    // Each finishes with the two shares addressed to it.
    let addr1 = dkg_finish(
        id.clone(), 1, 2, 3,
        vec![from(2, &to(&s2, 1)), from(3, &to(&s3, 1))],
        true,
    )
    .unwrap();
    let addr2 = dkg_finish(
        id.clone(), 2, 2, 3,
        vec![from(1, &to(&s1, 2)), from(3, &to(&s3, 2))],
        true,
    )
    .unwrap();
    let addr3 = dkg_finish(
        id.clone(), 3, 2, 3,
        vec![from(1, &to(&s1, 3)), from(2, &to(&s2, 3))],
        true,
    )
    .unwrap();

    // Three parties, one address, still no dealer.
    assert_eq!(addr1, addr2);
    assert_eq!(addr2, addr3);

    let k1 = dkg_take_keys(id.clone(), 1).unwrap();
    let k2 = dkg_take_keys(id.clone(), 2).unwrap();
    let k3 = dkg_take_keys(id, 3).unwrap();
    assert_ne!(k1, k2);
    assert_ne!(k2, k3);
    assert_ne!(k1, k3);
}
