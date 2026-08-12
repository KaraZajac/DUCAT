//! Conformance coverage for §18.1–18.3, following the required-coverage list
//! in §18.9. These are the vectors a second implementation must also pass.

use ducat_core::cbor::{decode, CodecError, Value, MAX_DEPTH};
use ducat_core::cbor_map;
use ducat_core::sig::{sig_input, ObjectType, SecretKey, SignedBytes, Suite};
use std::collections::BTreeMap;

fn key(seed: u8) -> SecretKey {
    SecretKey::ed25519_from_bytes(&[seed; 32])
}

// §18.9(1) — integer boundaries. Each value must use the shortest head, and
// every longer encoding of the same value must be rejected.
#[test]
fn integer_boundaries_use_shortest_form() {
    let cases: &[(u64, &[u8])] = &[
        (0, &[0x00]),
        (23, &[0x17]),
        (24, &[0x18, 0x18]),
        (255, &[0x18, 0xFF]),
        (256, &[0x19, 0x01, 0x00]),
        (65535, &[0x19, 0xFF, 0xFF]),
        (65536, &[0x1A, 0x00, 0x01, 0x00, 0x00]),
        (4294967295, &[0x1A, 0xFF, 0xFF, 0xFF, 0xFF]),
        (4294967296, &[0x1B, 0, 0, 0, 1, 0, 0, 0, 0]),
        (u64::MAX, &[0x1B, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]),
    ];
    for (n, expect) in cases {
        let enc = Value::Uint(*n).encode();
        assert_eq!(&enc[..], *expect, "encoding of {}", n);
        assert_eq!(decode(expect).unwrap(), Value::Uint(*n));
    }
}

#[test]
fn overlong_integer_encodings_are_rejected() {
    // Each of these encodes a value that had a shorter legal form.
    let overlong: &[&[u8]] = &[
        &[0x18, 0x00],                          // 0 in 1-byte form
        &[0x18, 0x17],                          // 23 in 1-byte form
        &[0x19, 0x00, 0x18],                    // 24 in 2-byte form
        &[0x19, 0x00, 0xFF],                    // 255 in 2-byte form
        &[0x1A, 0x00, 0x00, 0x01, 0x00],        // 256 in 4-byte form
        &[0x1B, 0, 0, 0, 0, 0, 0, 0xFF, 0xFF],  // 65535 in 8-byte form
    ];
    for enc in overlong {
        assert_eq!(
            decode(enc),
            Err(CodecError::NonCanonicalInt),
            "should reject overlong {:02x?}",
            enc
        );
    }
}

// §18.1 — map keys ascending and distinct.
#[test]
fn map_keys_must_ascend_and_be_distinct() {
    // {1: 0, 2: 0} — canonical.
    assert!(decode(&[0xA2, 0x01, 0x00, 0x02, 0x00]).is_ok());
    // {2: 0, 1: 0} — descending.
    assert_eq!(
        decode(&[0xA2, 0x02, 0x00, 0x01, 0x00]),
        Err(CodecError::NonCanonicalMapOrder)
    );
    // {1: 0, 1: 0} — duplicate. Must be an error, not a silent overwrite.
    assert_eq!(
        decode(&[0xA2, 0x01, 0x00, 0x01, 0x00]),
        Err(CodecError::NonCanonicalMapOrder)
    );
}

#[test]
fn non_integer_map_keys_are_rejected() {
    // {"a": 0} — text key.
    assert_eq!(
        decode(&[0xA1, 0x61, 0x61, 0x00]),
        Err(CodecError::NonIntegerMapKey)
    );
}

// §18.2 — money is integers, so a float anywhere is malformed.
#[test]
fn floats_are_rejected_everywhere() {
    assert_eq!(decode(&[0xF9, 0x00, 0x00]), Err(CodecError::FloatForbidden));
    assert_eq!(
        decode(&[0xFA, 0x00, 0x00, 0x00, 0x00]),
        Err(CodecError::FloatForbidden)
    );
    assert_eq!(
        decode(&[0xFB, 0, 0, 0, 0, 0, 0, 0, 0]),
        Err(CodecError::FloatForbidden)
    );
    // Nested inside a map value, which is where a naive decoder would miss it.
    assert_eq!(
        decode(&[0xA1, 0x01, 0xF9, 0x00, 0x00]),
        Err(CodecError::FloatForbidden)
    );
}

#[test]
fn tags_and_indefinite_lengths_are_rejected() {
    assert_eq!(decode(&[0xC0, 0x00]), Err(CodecError::TagForbidden));
    assert_eq!(decode(&[0x5F]), Err(CodecError::IndefiniteLength)); // indefinite bytes
    assert_eq!(decode(&[0x9F]), Err(CodecError::IndefiniteLength)); // indefinite array
    assert_eq!(decode(&[0xBF]), Err(CodecError::IndefiniteLength)); // indefinite map
}

#[test]
fn trailing_bytes_are_rejected() {
    // A signed object is exactly its bytes, never a prefix of them.
    assert_eq!(decode(&[0x00, 0x00]), Err(CodecError::TrailingBytes(1)));
}

#[test]
fn truncated_input_is_rejected() {
    assert_eq!(decode(&[0x19, 0x01]), Err(CodecError::Truncated)); // 2-byte head, 1 byte
    assert_eq!(decode(&[0x42, 0xFF]), Err(CodecError::Truncated)); // 2-byte string, 1 byte
    assert_eq!(decode(&[]), Err(CodecError::Truncated));
}

#[test]
fn oversized_length_headers_do_not_allocate() {
    // Claims 2^32 array items in 5 bytes. Must fail on exhausted input rather
    // than attempting to reserve capacity for the claim.
    assert_eq!(
        decode(&[0x9A, 0xFF, 0xFF, 0xFF, 0xFF]),
        Err(CodecError::Truncated)
    );
    // Same for byte strings.
    assert_eq!(
        decode(&[0x5A, 0xFF, 0xFF, 0xFF, 0xFF]),
        Err(CodecError::Truncated)
    );
}

#[test]
fn deep_nesting_is_bounded() {
    // MAX_DEPTH+2 nested single-element arrays.
    let mut enc = vec![0x81u8; MAX_DEPTH + 2];
    enc.push(0x00);
    assert_eq!(decode(&enc), Err(CodecError::TooDeep));
}

#[test]
fn invalid_utf8_is_rejected() {
    assert_eq!(decode(&[0x62, 0xFF, 0xFE]), Err(CodecError::InvalidUtf8));
}

/// The property the whole codec exists to provide: a decode that succeeds
/// proves the input was already canonical, so re-encoding is byte-identical.
#[test]
fn decode_encode_roundtrip_is_byte_identical() {
    let v = cbor_map! {
        1 => Value::Uint(1),
        2 => Value::Bytes(vec![0xDE, 0xAD, 0xBE, 0xEF]),
        3 => Value::Text("ride/1".into()),
        4 => Value::Array(vec![Value::Uint(0), Value::Uint(24), Value::Uint(65536)]),
        5 => Value::Bool(true),
        6 => Value::Null,
        7 => cbor_map! { 1 => Value::Uint(2_500_000_000_000u64) },
    };
    let enc = v.encode();
    let dec = decode(&enc).unwrap();
    assert_eq!(dec, v);
    assert_eq!(dec.encode(), enc);
}

/// A map built in scrambled insertion order must still encode canonically —
/// otherwise two clients populating the same fields in different orders would
/// produce different hashes for the same logical object.
#[test]
fn insertion_order_does_not_affect_encoding() {
    let mut a = BTreeMap::new();
    for k in [9u64, 1, 300, 24, 2] {
        a.insert(k, Value::Uint(k));
    }
    let mut b = BTreeMap::new();
    for k in [1u64, 2, 9, 24, 300] {
        b.insert(k, Value::Uint(k));
    }
    assert_eq!(Value::Map(a).encode(), Value::Map(b).encode());
}

// ---------------------------------------------------------------- signing --

// §18.9(3) — cross-context replay. The headline test of §18.3.
#[test]
fn signature_does_not_transfer_across_object_types() {
    let sk = key(7);
    let vk = sk.public();
    let obj = SignedBytes::from_value(cbor_map! { 1 => Value::Uint(42) });

    let sig = obj.sign(ObjectType::TapPresent, &sk);

    // Valid in its own context.
    assert!(obj.verify(ObjectType::TapPresent, &vk, &sig).is_ok());

    // Presented as any other type, it must fail — same key, same bytes.
    for other in [
        ObjectType::Accept,
        ObjectType::Receipt,
        ObjectType::BondProof,
        ObjectType::ContactOffer,
        ObjectType::Attestation,
    ] {
        assert!(
            obj.verify(other, &vk, &sig).is_err(),
            "TapPresent signature must not verify as {:?}",
            other
        );
    }
}

/// Separators must prevent boundary ambiguity between adjacent variable-length
/// fields. Without the 0x00 bytes, a crafted label/suite pairing could produce
/// a colliding input.
#[test]
fn domain_separation_inputs_are_unambiguous() {
    let body = b"body";
    let a = sig_input(ObjectType::Accept, Suite::Ed25519X25519, body);
    let b = sig_input(ObjectType::Accept, Suite::P256, body);
    assert_ne!(a, b, "suite must be bound into the signature input");

    let c = sig_input(ObjectType::TapPresent, Suite::Ed25519X25519, body);
    assert_ne!(a, c, "object type must be bound into the signature input");

    assert!(a.starts_with(b"DUCAT-v1\x00"), "protocol prefix present");
}

/// §18.3 — verification runs against received bytes, and non-canonical input is
/// refused before a signature is even considered.
#[test]
fn non_canonical_bytes_are_rejected_on_receipt() {
    // {1: 23} with 23 written in the one-byte-argument form. 23 is the last
    // value that must use the immediate form, so this is overlong by one step.
    // (Note 0x18 0x18 would NOT be an error: 24 is the first value that legally
    // requires the one-byte form. The boundary is easy to get backwards, which
    // is why it is tested from both sides.)
    let bad = vec![0xA1, 0x01, 0x18, 0x17];
    assert!(matches!(
        SignedBytes::from_received(bad),
        Err(ducat_core::sig::SigError::NonCanonical(_))
    ));
}

#[test]
fn tampering_with_any_byte_invalidates_the_signature() {
    let sk = key(3);
    let vk = sk.public();
    let obj = SignedBytes::from_value(cbor_map! {
        1 => Value::Uint(1),
        2 => Value::Uint(2_500_000_000_000u64), // an amount in piconero
    });
    let sig = obj.sign(ObjectType::Accept, &sk);

    let mut bytes = obj.bytes().to_vec();
    let last = bytes.len() - 1;
    bytes[last] ^= 0x01; // smallest possible change to the amount
    let tampered = SignedBytes::from_received(bytes).unwrap();
    assert!(tampered.verify(ObjectType::Accept, &vk, &sig).is_err());
}

/// §18.2 — a fare expressed in piconero must survive exactly. This vector
/// exists specifically to fail a float-based implementation: 2.5 XMR in
/// piconero is not representable in an f64 without loss at this magnitude's
/// neighbours, and a client that round-trips through a double will not
/// reproduce these bytes.
#[test]
fn piconero_amounts_survive_exactly() {
    let amounts: &[u64] = &[
        1,                      // one piconero
        2_500_000_000_000,      // 2.5 XMR
        9_007_199_254_740_993,  // 2^53 + 1, first integer f64 cannot represent
        18_446_744_073_709_551_615, // u64::MAX
    ];
    for a in amounts {
        let v = cbor_map! { 1 => Value::Uint(*a) };
        let enc = v.encode();
        let back = decode(&enc).unwrap();
        assert_eq!(
            back.as_map().unwrap().get(&1).unwrap().as_uint().unwrap(),
            *a
        );
    }
}
