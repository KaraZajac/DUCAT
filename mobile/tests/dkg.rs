//! The bond ceremony's crypto, as a permanent guard (§17.9).
//!
//! `escrowtest.rs` proves the whole path against the live chain; this proves
//! the part that must never regress silently, in milliseconds with no
//! network: a two-party PedPoP DKG where neither side holds the other's
//! secret, followed by a FROST co-signature that only exists because both
//! shares signed. If the vendored dkg-pedpop or the frost stack ever drifts
//! back into the multiexp conflict, this test goes red before anything ships.

use std::collections::HashMap;

use ciphersuite::Ciphersuite;
use dkg_pedpop::{
    Commitments, EncryptedMessage, EncryptionKeyMessage, KeyGenMachine, SecretShare,
};
use modular_frost::{
    algorithm::IetfSchnorr,
    curve::{Ed25519, IetfEd25519Hram},
    dkg::{Participant, ThresholdKeys, ThresholdParams},
    sign::{AlgorithmMachine, PreprocessMachine, SignMachine, SignatureMachine, Writable},
};
use rand_core::OsRng;

const CONTEXT: [u8; 32] = *b"DUCAT-dkg-test-context-v0-pad!!!";

fn params(i: u16) -> ThresholdParams {
    ThresholdParams::new(2, 2, Participant::new(i).unwrap()).unwrap()
}

fn read_commitments(
    bytes: &[u8],
    p: ThresholdParams,
) -> EncryptionKeyMessage<Ed25519, Commitments<Ed25519>> {
    EncryptionKeyMessage::read(&mut &bytes[..], p).unwrap()
}

fn read_share(
    bytes: &[u8],
    p: ThresholdParams,
) -> EncryptedMessage<Ed25519, SecretShare<<Ed25519 as Ciphersuite>::F>> {
    EncryptedMessage::read(&mut &bytes[..], p).unwrap()
}

/// Two independent parties build one key, exchanging only serialized wire
/// bytes — the exact shape §17.9 seals over the thread.
fn two_party_dkg() -> (ThresholdKeys<Ed25519>, ThresholdKeys<Ed25519>) {
    let p1 = Participant::new(1).unwrap();
    let p2 = Participant::new(2).unwrap();

    let (ss1, c1) =
        KeyGenMachine::<Ed25519>::new(params(1), CONTEXT).generate_coefficients(&mut OsRng);
    let (ss2, c2) =
        KeyGenMachine::<Ed25519>::new(params(2), CONTEXT).generate_coefficients(&mut OsRng);
    let c1_wire = c1.serialize();
    let c2_wire = c2.serialize();

    let (km1, shares1) = ss1
        .generate_secret_shares(
            &mut OsRng,
            HashMap::from([(p2, read_commitments(&c2_wire, params(1)))]),
        )
        .unwrap();
    let (km2, shares2) = ss2
        .generate_secret_shares(
            &mut OsRng,
            HashMap::from([(p1, read_commitments(&c1_wire, params(2)))]),
        )
        .unwrap();
    let s1to2 = shares1[&p2].serialize();
    let s2to1 = shares2[&p1].serialize();

    let keys1 = km1
        .calculate_share(&mut OsRng, HashMap::from([(p2, read_share(&s2to1, params(1)))]))
        .unwrap()
        .complete();
    let keys2 = km2
        .calculate_share(&mut OsRng, HashMap::from([(p1, read_share(&s1to2, params(2)))]))
        .unwrap()
        .complete();
    (keys1, keys2)
}

#[test]
fn dkg_builds_one_key_with_no_dealer() {
    let (keys1, keys2) = two_party_dkg();
    // Both parties, one group key — the whole point.
    assert_eq!(keys1.group_key(), keys2.group_key());
    // And each holds a *different* share: no dealer, no shared secret.
    assert_ne!(keys1.serialize().to_vec(), keys2.serialize().to_vec());
}

#[test]
fn frost_co_sign_agrees_only_with_both_shares() {
    let (keys1, keys2) = two_party_dkg();
    let p1 = Participant::new(1).unwrap();
    let p2 = Participant::new(2).unwrap();

    let msg = b"release the deposit to the agreed destination";
    let algo = IetfSchnorr::<Ed25519, IetfEd25519Hram>::ietf();
    let m1 = AlgorithmMachine::new(algo.clone(), keys1);
    let m2 = AlgorithmMachine::new(algo, keys2);

    // Preprocess, exchanged as bytes.
    let (m1, pre1) = m1.preprocess(&mut OsRng);
    let (m2, pre2) = m2.preprocess(&mut OsRng);
    let pre1_wire = pre1.serialize();
    let pre2_wire = pre2.serialize();
    let pre2_at_1 = m1.read_preprocess(&mut &pre2_wire[..]).unwrap();
    let pre1_at_2 = m2.read_preprocess(&mut &pre1_wire[..]).unwrap();

    // Signature shares, exchanged as bytes.
    let (m1, share1) = m1.sign(HashMap::from([(p2, pre2_at_1)]), msg).unwrap();
    let (m2, share2) = m2.sign(HashMap::from([(p1, pre1_at_2)]), msg).unwrap();
    let share1_wire = share1.serialize();
    let share2_wire = share2.serialize();
    let share2_at_1 = m1.read_share(&mut &share2_wire[..]).unwrap();
    let share1_at_2 = m2.read_share(&mut &share1_wire[..]).unwrap();

    let sig1 = m1.complete(HashMap::from([(p2, share2_at_1)])).unwrap();
    let sig2 = m2.complete(HashMap::from([(p1, share1_at_2)])).unwrap();

    // Both signers derive the identical signature — the release exists only
    // because both shares signed, and it does not matter who assembles it.
    // (modular-frost's own tests cover that the signature verifies; here the
    // property DUCAT depends on is that the two completions agree.)
    assert_eq!(sig1.serialize(), sig2.serialize());
    assert!(!sig1.serialize().is_empty());
}
