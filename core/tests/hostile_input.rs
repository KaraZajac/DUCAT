//! Arbitrary bytes must never panic, hang, or exhaust memory.
//!
//! The codec is the first thing that touches untrusted data. A tap is a blob
//! from a stranger, and §15.10 assumes a hostile presenter — so "it rejects
//! malformed input" is not enough, it must *survive* malformed input. A panic
//! in a payments client is a denial of service at best and a crash-loop at a
//! market stall at worst.
//!
//! Deterministic pseudo-random rather than a fuzzer, so failures reproduce
//! exactly from the seed printed in the assertion.

use ducat_core::cbor::decode;
use ducat_core::sig::{PublicKey, SignedBytes, Suite};
use ducat_core::wire::{open, Accept, FullOffer, Receipt, TapPresent};

/// xorshift64* — small, deterministic, adequate for shaking out parser bugs.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn byte(&mut self) -> u8 {
        (self.next() >> 24) as u8
    }
    fn upto(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            (self.next() % n as u64) as usize
        }
    }
}

fn random_bytes(rng: &mut Rng, max: usize) -> Vec<u8> {
    let n = rng.upto(max);
    (0..n).map(|_| rng.byte()).collect()
}

#[test]
fn random_bytes_never_panic_the_decoder() {
    for seed in 1..=400u64 {
        let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15));
        for _ in 0..40 {
            let input = random_bytes(&mut rng, 600);
            // Any outcome is fine except a panic, which the harness catches as
            // a test failure. Success must additionally re-encode identically —
            // decoding proves canonical form, so this cannot be skipped just
            // because the input was random.
            if let Ok(v) = decode(&input) {
                assert_eq!(
                    v.encode(),
                    input,
                    "seed {}: decode accepted bytes it does not reproduce",
                    seed
                );
            }
        }
    }
}

/// Structured corruption is nastier than pure noise: it keeps enough shape to
/// reach deeper code paths.
#[test]
fn corrupted_real_objects_never_panic() {
    let offer = FullOffer {
        version: 1,
        suite: 1,
        profile: 2,
        payto: vec![0x42; 69],
        amount_pxmr: 2_500_000_000_000,
        supported_versions: vec![1],
        supported_suites: vec![1, 2],
        settle_mode: 0,
        fee_policy: ducat_core::wire::FeePolicy::PayerPays,
        nonce_echo: [0xA5; 16],
    };
    let good = offer.to_value().encode();

    for seed in 1..=300u64 {
        let mut rng = Rng(seed.wrapping_mul(0xD1B5_4A32_D192_ED03));
        let mut bad = good.clone();
        // Flip, truncate, extend — three ways real corruption arrives.
        match rng.upto(3) {
            0 => {
                let n = 1 + rng.upto(6);
                for _ in 0..n {
                    let i = rng.upto(bad.len());
                    bad[i] ^= 1 << rng.upto(8);
                }
            }
            1 => {
                let keep = rng.upto(bad.len());
                bad.truncate(keep);
            }
            _ => {
                let extra = random_bytes(&mut rng, 40);
                bad.extend_from_slice(&extra);
            }
        }

        // Every layer must survive it.
        if let Ok(v) = decode(&bad) {
            let _ = FullOffer::from_value(v.clone());
            let _ = TapPresent::from_value(v.clone());
            let _ = Accept::from_value(v.clone());
            let _ = Receipt::from_value(v);
        }
        let _ = SignedBytes::from_received(bad.clone());
        if let Ok(pk) = PublicKey::from_bytes(Suite::Ed25519X25519, &[7u8; 32]) {
            let _ = open(&bad, &pk);
        }
    }
}

/// Nesting is the classic cheap-to-send, expensive-to-parse attack: a few
/// hundred bytes can ask for unbounded recursion.
#[test]
fn deeply_nested_input_is_bounded_not_fatal() {
    for depth in [10usize, 17, 64, 512, 5000] {
        let mut enc = vec![0x81u8; depth]; // nested single-element arrays
        enc.push(0x00);
        let r = decode(&enc);
        if depth <= 16 {
            assert!(r.is_ok(), "depth {} should decode", depth);
        } else {
            assert!(r.is_err(), "depth {} should be refused", depth);
        }
    }
    // Same shape with maps, which recurse through a different arm.
    let mut enc = Vec::new();
    for _ in 0..2000 {
        enc.extend_from_slice(&[0xA1, 0x01]);
    }
    enc.push(0x00);
    assert!(decode(&enc).is_err(), "deep map nesting must be refused");
}

/// A length header is a claim, not a fact. Believing one is how a 5-byte
/// message becomes a 4 GB allocation.
#[test]
fn enormous_length_claims_do_not_allocate() {
    let claims: &[&[u8]] = &[
        &[0x5A, 0xFF, 0xFF, 0xFF, 0xFF],                         // 4 GB byte string
        &[0x7A, 0xFF, 0xFF, 0xFF, 0xFF],                         // 4 GB text
        &[0x9A, 0xFF, 0xFF, 0xFF, 0xFF],                         // 4 G array items
        &[0xBA, 0xFF, 0xFF, 0xFF, 0xFF],                         // 4 G map pairs
        &[0x5B, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF], // 2^64 bytes
        &[0x9B, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF],
    ];
    for c in claims {
        assert!(decode(c).is_err(), "must refuse {:02x?}", c);
    }
}

/// The envelope is what a relay sees first, and it is the layer that decides
/// whether a signature check even happens.
#[test]
fn malformed_envelopes_never_panic() {
    let pk = PublicKey::from_bytes(Suite::Ed25519X25519, &[3u8; 32]);
    let pk = match pk {
        Ok(k) => k,
        Err(_) => return, // not a valid point; nothing to test against
    };
    for seed in 1..=200u64 {
        let mut rng = Rng(seed.wrapping_mul(0xA076_1D64_78BD_642F));
        let junk = random_bytes(&mut rng, 300);
        let _ = open(&junk, &pk);
    }
    // Envelope-shaped but wrong: right keys, wrong contents.
    for body in [vec![], vec![0x00], vec![0xFF; 64]] {
        for sig in [vec![], vec![0u8; 63], vec![0u8; 64], vec![0u8; 65]] {
            let mut m = std::collections::BTreeMap::new();
            m.insert(1u64, ducat_core::cbor::Value::Bytes(body.clone()));
            m.insert(2u64, ducat_core::cbor::Value::Bytes(sig.clone()));
            let env = ducat_core::cbor::Value::Map(m).encode();
            let _ = open(&env, &pk);
        }
    }
}
