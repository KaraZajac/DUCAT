//! Both cipher suites (§4.1), and the properties that differ between them.
//!
//! Core conformance requires *both* suites, because iOS's Secure Enclave holds
//! no Ed25519 key and personas would otherwise fragment by platform.

use ducat_core::cbor::Value;
use ducat_core::cbor_map;
use ducat_core::sig::{ObjectType, PublicKey, SecretKey, SigError, SignedBytes, Suite};
use p256::elliptic_curve::sec1::ToEncodedPoint;

fn obj() -> SignedBytes {
    SignedBytes::from_value(cbor_map! {
        1 => Value::Uint(1),
        2 => Value::Uint(2_500_000_000_000u64),
    })
}

fn ed_key() -> SecretKey {
    SecretKey::ed25519_from_bytes(&[9u8; 32])
}

fn p256_key() -> SecretKey {
    SecretKey::p256_from_bytes(&[9u8; 32]).unwrap()
}

#[test]
fn both_suites_sign_and_verify() {
    for sk in [ed_key(), p256_key()] {
        let pk = sk.public();
        let o = obj();
        let sig = o.sign(ObjectType::Accept, &sk);
        assert!(
            o.verify(ObjectType::Accept, &pk, &sig).is_ok(),
            "{:?} should round-trip",
            sk.suite()
        );
    }
}

#[test]
fn suite_is_a_property_of_the_key() {
    assert_eq!(ed_key().suite(), Suite::Ed25519X25519);
    assert_eq!(p256_key().suite(), Suite::P256);
    assert_eq!(ed_key().public().suite(), Suite::Ed25519X25519);
    assert_eq!(p256_key().public().suite(), Suite::P256);
}

/// The suite is bound into the signature input, so a signature made under one
/// suite must not verify under another even if an attacker supplies a matching
/// key. Combined with the key carrying its own suite, this makes the mismatch
/// unrepresentable rather than merely detectable.
#[test]
fn signatures_do_not_transfer_across_suites() {
    let o = obj();
    let ed_sig = o.sign(ObjectType::Accept, &ed_key());
    let p_sig = o.sign(ObjectType::Accept, &p256_key());

    // Each verifies only under its own key.
    assert!(o.verify(ObjectType::Accept, &ed_key().public(), &ed_sig).is_ok());
    assert!(o.verify(ObjectType::Accept, &p256_key().public(), &p_sig).is_ok());

    // And not under the other's.
    assert!(o.verify(ObjectType::Accept, &p256_key().public(), &ed_sig).is_err());
    assert!(o.verify(ObjectType::Accept, &ed_key().public(), &p_sig).is_err());
}

/// Cross-context replay must hold under P-256 exactly as it does under Ed25519.
#[test]
fn p256_signatures_do_not_transfer_across_object_types() {
    let sk = p256_key();
    let pk = sk.public();
    let o = obj();
    let sig = o.sign(ObjectType::TapPresent, &sk);

    assert!(o.verify(ObjectType::TapPresent, &pk, &sig).is_ok());
    for other in [ObjectType::Accept, ObjectType::Receipt, ObjectType::BondProof] {
        assert!(
            o.verify(other, &pk, &sig).is_err(),
            "must not verify as {:?}",
            other
        );
    }
}

// -- malleability, which is P-256's alone -----------------------------------

/// ECDSA admits two valid signatures per message: `(r, s)` and `(r, n - s)`.
/// Ed25519 does not.
///
/// This matters here more than in most protocols. §6 chains messages by hash,
/// and a completed transaction is a self-verifying transcript held by both
/// parties. A third party who flips `s` in flight leaves both signatures valid
/// while the two transcripts now hash differently — every downstream commitment
/// silently diverges, and a `fast/1` slash claim (§17.5) would carry evidence
/// that verifies yet does not match the counterparty's copy.
///
/// So the high-`s` form must be refused, not silently normalized on receipt:
/// accepting both encodings would mean two distinct byte strings are each "the"
/// signature, and the transcript hash would depend on which one arrived.
#[test]
fn high_s_p256_signatures_are_refused() {
    let sk = p256_key();
    let pk = sk.public();
    let o = obj();
    let sig = o.sign(ObjectType::Accept, &sk);

    // Emitted form is low-s and verifies.
    assert!(o.verify(ObjectType::Accept, &pk, &sig).is_ok());

    // Flip s to n - s. The result is a mathematically valid ECDSA signature
    // over the same message under the same key.
    let flipped = flip_s(&sig);
    assert_ne!(flipped, sig, "flipping s must change the bytes");

    assert_eq!(
        o.verify(ObjectType::Accept, &pk, &flipped),
        Err(SigError::MalleableSignature),
        "the high-s twin must be refused, not accepted as equivalent"
    );
}

/// Compute (r, n - s) from a fixed-form (r, s) signature.
fn flip_s(sig: &[u8; 64]) -> [u8; 64] {
    // Order of the P-256 group.
    const N: [u8; 32] = [
        0xFF, 0xFF, 0xFF, 0xFF, 0x00, 0x00, 0x00, 0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
        0xFF, 0xBC, 0xE6, 0xFA, 0xAD, 0xA7, 0x17, 0x9E, 0x84, 0xF3, 0xB9, 0xCA, 0xC2, 0xFC, 0x63,
        0x25, 0x51,
    ];
    let mut out = *sig;
    // out[32..64] = N - s, big-endian schoolbook subtraction.
    let mut borrow = 0i16;
    for i in (0..32).rev() {
        let d = N[i] as i16 - sig[32 + i] as i16 - borrow;
        if d < 0 {
            out[32 + i] = (d + 256) as u8;
            borrow = 1;
        } else {
            out[32 + i] = d as u8;
            borrow = 0;
        }
    }
    out
}

// -- key encoding ------------------------------------------------------------

/// Public keys are 32 bytes under Ed25519 and 33 compressed under P-256. Length
/// is implied by the suite and never sent separately.
#[test]
fn public_keys_round_trip_through_wire_encoding() {
    for sk in [ed_key(), p256_key()] {
        let pk = sk.public();
        let suite = pk.suite();
        let bytes = pk.to_bytes();
        let expect_len = match suite {
            Suite::Ed25519X25519 => 32,
            Suite::P256 => 33,
        };
        assert_eq!(bytes.len(), expect_len, "{:?} key length", suite);

        let back = PublicKey::from_bytes(suite, &bytes).unwrap();
        assert_eq!(back.suite(), suite);

        // A key that round-tripped must still verify a real signature.
        let o = obj();
        let sig = o.sign(ObjectType::Receipt, &sk);
        assert!(o.verify(ObjectType::Receipt, &back, &sig).is_ok());
    }
}

#[test]
fn malformed_key_material_is_rejected() {
    // Wrong length for the declared suite.
    assert_eq!(
        PublicKey::from_bytes(Suite::Ed25519X25519, &[0u8; 31]).err(),
        Some(SigError::BadKey)
    );
    // Invalid SEC1 tag byte. The underlying parser is lenient here — it reads
    // y-parity from the low bit, so 0x05 would otherwise be accepted and yield
    // the *same key* as 0x03. Two encodings of one key means two canonical
    // objects and two transcript hashes, so the tag is checked explicitly.
    assert_eq!(
        PublicKey::from_bytes(Suite::P256, &[5u8; 33]).err(),
        Some(SigError::BadKey)
    );
    // Compressed point whose x coordinate exceeds the field prime, so it
    // cannot be on the curve. (Note that a *plausible-looking* x such as
    // 0x0202..02 may well be a valid point — "looks arbitrary" is not the
    // same as "off curve", which is why this uses an out-of-range value.)
    let mut off_curve = [0xFFu8; 33];
    off_curve[0] = 0x02;
    assert_eq!(
        PublicKey::from_bytes(Suite::P256, &off_curve).err(),
        Some(SigError::BadKey)
    );
    // An Ed25519-length key presented as P-256: 32 bytes is neither a
    // compressed (33) nor uncompressed (65) SEC1 encoding.
    assert!(PublicKey::from_bytes(Suite::P256, &[0u8; 32]).is_err());
}

/// Signing is deterministic in both suites — Ed25519 by construction, P-256 via
/// RFC 6979. Two clients signing the same object with the same key must produce
/// identical bytes, or transcripts diverge on re-signing.
#[test]
fn signing_is_deterministic() {
    for sk in [ed_key(), p256_key()] {
        let o = obj();
        let a = o.sign(ObjectType::Accept, &sk);
        let b = o.sign(ObjectType::Accept, &sk);
        assert_eq!(a, b, "{:?} signing must be deterministic", sk.suite());
    }
}

/// Exactly one wire encoding per key. SEC1 offers several — compressed
/// (0x02/0x03), uncompressed (0x04), hybrid (0x06/0x07) — and the parser
/// additionally tolerates a malformed tag by reading y-parity from its low bit.
/// Accepting more than one would give a single persona two identities on the
/// wire, and any signed object embedding it two hashes.
#[test]
fn p256_keys_have_exactly_one_legal_encoding() {
    let pk = p256_key().public();
    let compressed = pk.to_bytes();
    assert_eq!(compressed.len(), 33);
    assert!(compressed[0] == 0x02 || compressed[0] == 0x03);

    // The canonical form parses.
    assert!(PublicKey::from_bytes(Suite::P256, &compressed).is_ok());

    // The same key, tag flipped to the lenient-parse variant (0x02|0x04 = 0x06,
    // 0x03|0x04 = 0x07 are hybrid; 0x02+3 = 0x05 is simply invalid). Each must
    // be refused even though the point is genuine.
    for bad_tag in [0x04u8, 0x05, 0x06, 0x07, 0x00, 0xFF] {
        let mut variant = compressed.clone();
        variant[0] = bad_tag;
        assert_eq!(
            PublicKey::from_bytes(Suite::P256, &variant).err(),
            Some(SigError::BadKey),
            "tag {:#04x} must be refused",
            bad_tag
        );
    }

    // Uncompressed is a legal SEC1 encoding of the same point, and still
    // refused: legality in SEC1 is not the standard, uniqueness is.
    match &pk {
        PublicKey::P256(k) => {
            let uncompressed = k.to_encoded_point(false);
            assert_eq!(uncompressed.as_bytes().len(), 65);
            assert_eq!(
                PublicKey::from_bytes(Suite::P256, uncompressed.as_bytes()).err(),
                Some(SigError::BadKey)
            );
        }
        _ => unreachable!(),
    }
}

// -- the declared suite must match the key that signed ----------------------

/// An object carries a `suite` field and is verified with a key that *is* of a
/// suite. Nothing required them to agree, so a mismatch surfaced as `BAD_SIG`.
///
/// That is safe — a signature cannot verify under the wrong curve — but it is
/// the wrong diagnostic, and §18.5 requires two implementations refuse the same
/// object for the same stated reason. A client debugging a suite bug would have
/// been told its signature was bad.
#[test]
fn a_mismatched_suite_declaration_is_refused_as_such() {
    use ducat_core::wire::*;

    let ed = SecretKey::ed25519_from_bytes(&[5u8; 32]);
    let p256 = SecretKey::p256_from_bytes(&[5u8; 32]).unwrap();

    // Build an offer that *claims* suite 1 but sign it with the P-256 key.
    let offer = FullOffer {
        version: 1,
        suite: 1, // the lie
        profile: 2,
        payto: vec![0x42; 69],
        amount_pxmr: 1_000_000_000,
        supported_versions: vec![1],
        supported_suites: vec![1, 2],
        settle_mode: 0,
        fee_policy: FeePolicy::PayerPays,
        nonce_echo: [0x11; 16],
        terms: Terms::default(),
        memo: None,
    };
    let env = seal(
        &SignedBytes::from_value(offer.to_value()),
        ObjectType::FullOffer,
        &p256,
    );

    let err = open(&env, &p256.public()).expect_err("suite mismatch must be refused");
    assert_eq!(
        err.code,
        ducat_core::reject::RejectCode::UnsupportedSuite,
        "must name the actual problem, not report a bad signature"
    );

    // Declaring the truth works.
    let mut honest = offer.clone();
    honest.suite = 2;
    let env = seal(
        &SignedBytes::from_value(honest.to_value()),
        ObjectType::FullOffer,
        &p256,
    );
    assert!(open(&env, &p256.public()).is_ok());

    // And the Ed25519 path still works when it tells the truth.
    let env = seal(
        &SignedBytes::from_value(offer.to_value()),
        ObjectType::FullOffer,
        &ed,
    );
    assert!(open(&env, &ed.public()).is_ok());
}
