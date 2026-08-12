//! Static tags and cancellation (§15.9, §7.3).
//!
//! §15.9 offered persona pinning as the mitigation for a swapped tag. These
//! tests are mostly about establishing what that is actually worth.

use ducat_core::cbor::{decode, Value};
use ducat_core::commit::{commit, Purpose};
use ducat_core::reject::RejectCode;
use ducat_core::sig::{PublicKey, SecretKey, Suite};
use ducat_core::wire::*;

fn jar_key() -> SecretKey {
    SecretKey::ed25519_from_bytes(&[0x41; 32])
}
fn thief_key() -> SecretKey {
    SecretKey::ed25519_from_bytes(&[0x42; 32])
}

/// Verify with the real signature machinery rather than a stub, so these tests
/// fail if signing changes underneath them.
fn verifier(persona: &[u8], body: &[u8], sig: &[u8]) -> bool {
    let Ok(pk) = PublicKey::from_bytes(Suite::Ed25519X25519, persona) else {
        return false;
    };
    let Ok(sb) = ducat_core::sig::SignedBytes::from_received(body.to_vec()) else {
        return false;
    };
    let Ok(arr): Result<[u8; 64], _> = sig.try_into() else {
        return false;
    };
    sb.verify(ducat_core::sig::ObjectType::TapPresent, &pk, &arr)
        .is_ok()
}

fn signed_jar() -> TapStatic {
    let k = jar_key();
    let mut t = TapStatic {
        version: 1,
        suite: 1,
        payto: b"honest-donation-address".to_vec(),
        persona: Some(k.public().to_bytes()),
        sig: None,
    };
    let body = t.signing_body();
    let sb = ducat_core::sig::SignedBytes::from_received(body).unwrap();
    t.sig = Some(sb.sign(ducat_core::sig::ObjectType::TapPresent, &k).to_vec());
    t
}

#[test]
fn an_unsigned_tag_authenticates_nothing() {
    let t = TapStatic {
        version: 1,
        suite: 1,
        payto: b"some-address".to_vec(),
        persona: None,
        sig: None,
    };
    assert_eq!(check_static_tag(&t, verifier).unwrap(), StaticTrust::Anonymous);
}

/// The trap §15.9 walked into. A pinned persona with no signature is a *claim*,
/// not evidence — an attacker prints the charity's name over their own address
/// and the tag looks identical to a payer.
#[test]
fn a_pinned_persona_without_a_signature_is_only_a_claim() {
    let t = TapStatic {
        version: 1,
        suite: 1,
        payto: b"attacker-address".to_vec(),
        persona: Some(jar_key().public().to_bytes()), // claims to be the charity
        sig: None,
    };
    assert_eq!(
        check_static_tag(&t, verifier).unwrap(),
        StaticTrust::Anonymous,
        "an unsigned pin must not be reported as authenticated"
    );
}

#[test]
fn a_signed_tag_proves_the_address_belongs_to_the_persona() {
    let t = signed_jar();
    assert_eq!(
        check_static_tag(&t, verifier).unwrap(),
        StaticTrust::SignedBy(jar_key().public().to_bytes())
    );
}

/// An attacker cannot keep the charity's persona and substitute their address:
/// the signature is over both.
#[test]
fn substituting_the_address_under_a_pinned_persona_fails() {
    let mut t = signed_jar();
    t.payto = b"attacker-address".to_vec();
    assert_eq!(
        check_static_tag(&t, verifier).unwrap_err().code,
        RejectCode::BadSig
    );
}

/// What a signature does *not* fix, stated as a test so nobody mistakes it for
/// a solved problem: an attacker who replaces the whole tag supplies their own
/// persona and a perfectly valid signature over it. The result verifies. Only a
/// payer who independently knows which persona to expect is protected.
#[test]
fn a_wholly_replaced_tag_still_verifies_and_that_is_the_residual_risk() {
    let k = thief_key();
    let mut t = TapStatic {
        version: 1,
        suite: 1,
        payto: b"thief-address".to_vec(),
        persona: Some(k.public().to_bytes()),
        sig: None,
    };
    let sb = ducat_core::sig::SignedBytes::from_received(t.signing_body()).unwrap();
    t.sig = Some(sb.sign(ducat_core::sig::ObjectType::TapPresent, &k).to_vec());

    // It verifies — as it must, the thief signed it honestly.
    let trust = check_static_tag(&t, verifier).unwrap();
    assert_eq!(trust, StaticTrust::SignedBy(k.public().to_bytes()));

    // The defence is that it is a *different persona* than expected, which only
    // helps a payer who knows what to expect.
    assert_ne!(trust, StaticTrust::SignedBy(jar_key().public().to_bytes()));
}

#[test]
fn a_signature_without_a_persona_is_malformed() {
    let t = TapStatic {
        version: 1,
        suite: 1,
        payto: b"x".to_vec(),
        persona: None,
        sig: Some(vec![0u8; 64]),
    };
    assert_eq!(
        TapStatic::from_value(t.to_value()).unwrap_err().code,
        RejectCode::Malformed
    );
}

#[test]
fn static_tags_round_trip() {
    for t in [signed_jar(), TapStatic { version: 1, suite: 1, payto: b"a".to_vec(), persona: None, sig: None }] {
        let enc = t.to_value().encode();
        assert_eq!(TapStatic::from_value(decode(&enc).unwrap()).unwrap(), t);
        assert_eq!(decode(&enc).unwrap().encode(), enc);
    }
}

// -- CANCEL -----------------------------------------------------------------

#[test]
fn a_cancellation_must_match_the_signed_fee() {
    let terms = Terms { cancellation_pxmr: 2_000_000_000, ..Terms::default() };
    let accept_bytes = b"pretend-accept".to_vec();
    let good = Cancel {
        version: 1, suite: 1,
        prior_accept: commit(Purpose::ChainLink, &accept_bytes),
        fee_pxmr: terms.cancellation_pxmr,
        timestamp: 1_800_000_000,
    };
    check_cancel(&good, &accept_bytes, &terms).expect("matching fee is fine");

    // A party inventing a different figure than the confirm screen showed.
    let mut greedy = good.clone();
    greedy.fee_pxmr = terms.cancellation_pxmr * 3;
    assert_eq!(
        check_cancel(&greedy, &accept_bytes, &terms).unwrap_err().code,
        RejectCode::PriceMismatch
    );
}

#[test]
fn a_cancellation_must_reference_its_own_accept() {
    let terms = Terms { cancellation_pxmr: 0, ..Terms::default() };
    let c = Cancel {
        version: 1, suite: 1,
        prior_accept: [0x99; 32],
        fee_pxmr: 0,
        timestamp: 1,
    };
    assert_eq!(
        check_cancel(&c, b"the-real-accept", &terms).unwrap_err().code,
        RejectCode::CommitMismatch
    );
}
