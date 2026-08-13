//! §16.11 — HPKE against RFC 9180's published vectors, and prekey consumption.
//!
//! The known-answer test matters more than the round-trip one. A round trip
//! proves this code agrees with itself; a KAT proves it agrees with the
//! standard, which is the only claim worth making about an interoperable
//! primitive. Both prior conformance efforts here (O21, the second
//! implementation) share an author with the reference — RFC 9180's vectors do
//! not, so this is the first external check in the project.

use ducat_core::hpke::*;

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
}

/// RFC 9180 A.2 — DHKEM(X25519, HKDF-SHA256), HKDF-SHA256, ChaCha20Poly1305,
/// base mode, encryption 0.
const IKM_E: &str = "909a9b35d3dc4713a5e72a4da274b55d3d3821a37e5d099e74a647db583a904b";
const PK_EM: &str = "1afa08d3dec047a643885163f1180476fa7ddb54c6a8029ea33f95796bf2ac4a";
const IKM_R: &str = "1ac01f181fdf9f352797655161c58b75c656a6cc2716dcb66372da835542e1df";
const PK_RM: &str = "4310ee97d88cc1f088a5576c77ab0cf5c3ac797f3d95139c6c84b5429c59662a";
const SK_RM: &str = "8057991eef8f1f1af18f4a9491d16a1ce333f695d4db8e38da75975c4478e0fb";
const INFO: &str = "4f6465206f6e2061204772656369616e2055726e";
const AAD: &str = "436f756e742d30";
const PT: &str = "4265617574792069732074727574682c20747275746820626561757479";
const CT: &str = "1c5250d8034ec2b784ba2cfd69dbdb8af406cfe3ff938e131f0def8c8b60b4db21993c62ce81883d2dd1b51a28";

/// Yields fixed bytes so the ephemeral key is the RFC's rather than a random
/// one. Test-only, and the reason `core` takes its CSPRNG as a parameter.
struct FixedRng(Vec<u8>, usize);

impl hpke::rand_core::TryRng for FixedRng {
    type Error = core::convert::Infallible;
    fn try_next_u32(&mut self) -> Result<u32, Self::Error> {
        let mut b = [0u8; 4];
        self.try_fill_bytes(&mut b)?;
        Ok(u32::from_le_bytes(b))
    }
    fn try_next_u64(&mut self) -> Result<u64, Self::Error> {
        let mut b = [0u8; 8];
        self.try_fill_bytes(&mut b)?;
        Ok(u64::from_le_bytes(b))
    }
    fn try_fill_bytes(&mut self, dst: &mut [u8]) -> Result<(), Self::Error> {
        for d in dst.iter_mut() {
            *d = self.0[self.1 % self.0.len()];
            self.1 += 1;
        }
        Ok(())
    }
}
impl hpke::rand_core::TryCryptoRng for FixedRng {}

#[test]
fn derives_the_rfc_9180_keypairs() {
    let (_, pk_e) = derive_keypair(&unhex(IKM_E));
    assert_eq!(hex(&pk_e), PK_EM, "ephemeral public key does not match RFC 9180 A.2");
    let (sk_r, pk_r) = derive_keypair(&unhex(IKM_R));
    assert_eq!(hex(&pk_r), PK_RM, "recipient public key does not match RFC 9180 A.2");
    assert_eq!(hex(&sk_r), SK_RM, "recipient secret key does not match RFC 9180 A.2");
}

/// The claim: sealing with the RFC's ephemeral key reproduces the RFC's
/// ciphertext byte for byte.
#[test]
fn seals_to_the_rfc_9180_ciphertext() {
    let mut rng = FixedRng(unhex(IKM_E), 0);
    let pk_r: [u8; 32] = unhex(PK_RM).try_into().unwrap();
    let (enc, ct) = seal(&mut rng, &pk_r, &unhex(INFO), &unhex(AAD), &unhex(PT)).unwrap();
    assert_eq!(hex(&enc), PK_EM, "encapsulated key does not match the RFC");
    assert_eq!(hex(&ct), CT, "ciphertext does not match the RFC");
}

#[test]
fn opens_the_rfc_9180_ciphertext() {
    let sk_r: [u8; 32] = unhex(SK_RM).try_into().unwrap();
    let pt = open(&sk_r, &unhex(PK_EM), &unhex(INFO), &unhex(AAD), &unhex(CT)).unwrap();
    assert_eq!(hex(&pt), PT);
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

// --- DUCAT's own use of it ------------------------------------------------

fn kp(seed: u8) -> ([u8; 32], [u8; 32]) {
    derive_keypair(&[seed; 32])
}

#[test]
fn a_ciphertext_does_not_open_under_a_different_info() {
    let (sk, pk) = kp(1);
    let mut rng = FixedRng(vec![9u8; 32], 0);
    let (enc, ct) = seal(&mut rng, &pk, &message_info(1), b"", b"hello").unwrap();
    // Same keys, different purpose. §18.3's domain separation, in the KEM.
    assert!(open(&sk, &enc, &message_info(2), b"", &ct).is_err());
    assert_eq!(open(&sk, &enc, &message_info(1), b"", &ct).unwrap(), b"hello");
}

#[test]
fn aad_binds_the_ciphertext_to_its_conversation() {
    let (sk, pk) = kp(2);
    let mut rng = FixedRng(vec![9u8; 32], 0);
    let (enc, ct) = seal(&mut rng, &pk, &message_info(1), b"thread-A", b"hi").unwrap();
    assert!(open(&sk, &enc, &message_info(1), b"thread-B", &ct).is_err());
}

/// The property the whole design exists for: once the one-time key is gone,
/// the ciphertext is undecryptable by anyone, including its recipient.
#[test]
fn a_consumed_prekey_makes_the_message_unrecoverable() {
    let (signed_sk, signed_pk) = kp(3);
    let (ot_sk, ot_pk) = kp(4);

    let bundle = PreKeyBundle {
        version: 1,
        suite: 1,
        signed_prekey: signed_pk,
        one_time: vec![PreKey { id: 7, public: ot_pk }],
        expiry: 9_999_999,
    };
    let (chosen, is_one_time) = bundle.select();
    assert!(is_one_time, "a one-time key was available and must be preferred");
    assert_eq!(chosen.id, 7);

    let mut rng = FixedRng(vec![9u8; 32], 0);
    let (enc, ct) = seal(&mut rng, &chosen.public, &message_info(1), b"", b"secret").unwrap();
    let sealed = SealedMessage { version: 1, suite: 1, prekey_id: 7, enc, ciphertext: ct };

    let mut store = PreKeyStore::new(signed_sk);
    store.insert_one_time(7, ot_sk);
    assert_eq!(store.remaining(), 1);

    let (pt, was_one_time) = store.open_and_consume(&sealed, &message_info(1), b"").unwrap();
    assert_eq!(pt, b"secret");
    assert!(was_one_time);
    assert_eq!(store.remaining(), 0, "the key must be gone");

    // Seizing the phone now recovers nothing for this message.
    let err = store.open_and_consume(&sealed, &message_info(1), b"").unwrap_err();
    assert!(format!("{err:?}").contains("StateViolation"), "{err:?}");
}

/// Otherwise anyone who can reach the rendezvous exhausts a recipient's
/// one-time keys with garbage and forces them onto the weaker fallback.
#[test]
fn a_failed_open_does_not_burn_the_prekey() {
    let (signed_sk, _) = kp(5);
    let (ot_sk, ot_pk) = kp(6);
    let mut rng = FixedRng(vec![9u8; 32], 0);
    let (enc, mut ct) = seal(&mut rng, &ot_pk, &message_info(1), b"", b"secret").unwrap();
    ct[0] ^= 0xFF;

    let mut store = PreKeyStore::new(signed_sk);
    store.insert_one_time(9, ot_sk);
    let sealed = SealedMessage { version: 1, suite: 1, prekey_id: 9, enc, ciphertext: ct };
    assert!(store.open_and_consume(&sealed, &message_info(1), b"").is_err());
    assert_eq!(store.remaining(), 1, "a forged message must not consume a key");
}

/// Exhaustion is not failure, but it is a real weakening and the caller is told.
#[test]
fn an_empty_bundle_falls_back_to_the_signed_prekey_and_says_so() {
    let (_, signed_pk) = kp(7);
    let bundle = PreKeyBundle {
        version: 1, suite: 1, signed_prekey: signed_pk, one_time: vec![], expiry: 1,
    };
    let (chosen, is_one_time) = bundle.select();
    assert!(!is_one_time, "the caller must be able to see the downgrade");
    assert_eq!(chosen.id, SIGNED_PREKEY_ID);
}

#[test]
fn the_signed_prekey_is_not_consumed() {
    let (signed_sk, signed_pk) = kp(8);
    let mut rng = FixedRng(vec![9u8; 32], 0);
    let (enc, ct) = seal(&mut rng, &signed_pk, &message_info(1), b"", b"hi").unwrap();
    let sealed = SealedMessage {
        version: 1, suite: 1, prekey_id: SIGNED_PREKEY_ID, enc, ciphertext: ct,
    };
    let mut store = PreKeyStore::new(signed_sk);
    let (pt, was_one_time) = store.open_and_consume(&sealed, &message_info(1), b"").unwrap();
    assert_eq!(pt, b"hi");
    assert!(!was_one_time);
    // Still usable — which is exactly why it is the weaker option.
    assert!(store.open_and_consume(&sealed, &message_info(1), b"").is_ok());
}

// --- encodings ------------------------------------------------------------

#[test]
fn bundles_and_sealed_messages_round_trip() {
    let b = PreKeyBundle {
        version: 1, suite: 1, signed_prekey: [1u8; 32],
        one_time: vec![PreKey { id: 1, public: [2u8; 32] }, PreKey { id: 2, public: [3u8; 32] }],
        expiry: 500,
    };
    assert_eq!(PreKeyBundle::from_value(b.to_value()).unwrap(), b);

    let m = SealedMessage {
        version: 1, suite: 1, prekey_id: 4, enc: vec![7u8; 32], ciphertext: vec![8u8; 40],
    };
    assert_eq!(SealedMessage::from_value(m.to_value()).unwrap(), m);
}

/// A duplicate id makes "delete after use" ambiguous, and that deletion is the
/// only thing forward secrecy rests on.
#[test]
fn duplicate_prekey_ids_are_refused() {
    let b = PreKeyBundle {
        version: 1, suite: 1, signed_prekey: [1u8; 32],
        one_time: vec![PreKey { id: 5, public: [2u8; 32] }, PreKey { id: 5, public: [3u8; 32] }],
        expiry: 500,
    };
    assert!(PreKeyBundle::from_value(b.to_value()).is_err());
}

#[test]
fn a_one_time_key_may_not_claim_the_signed_prekeys_id() {
    let b = PreKeyBundle {
        version: 1, suite: 1, signed_prekey: [1u8; 32],
        one_time: vec![PreKey { id: SIGNED_PREKEY_ID, public: [2u8; 32] }],
        expiry: 500,
    };
    assert!(PreKeyBundle::from_value(b.to_value()).is_err());
}

#[test]
fn an_oversized_ciphertext_is_refused_before_any_key_is_touched() {
    let m = SealedMessage {
        version: 1, suite: 1, prekey_id: 1, enc: vec![7u8; 32],
        ciphertext: vec![0u8; MAX_CIPHERTEXT + 1],
    };
    assert!(SealedMessage::from_value(m.to_value()).is_err());
}


/// Deleting a one-time secret without pruning the published bundle is worse
/// than not deleting at all.
///
/// The bundle keeps advertising the key, senders take the first one-time entry,
/// and so the *first* key consumed is offered forever — every later message is
/// refused, and identically after a re-fetch, because the stale bundle is what
/// gets re-served. It looks like a network fault and is a bookkeeping one.
///
/// Shipped in the Android app and found only by two devices talking: the first
/// message worked and every one after it failed with "that key is gone".
#[test]
fn a_consumed_prekey_must_not_stay_advertised() {
    let (signed_sk, signed_pk) = kp(20);
    let (ot1_sk, ot1_pk) = kp(21);
    let (_, ot2_pk) = kp(22);

    let mut bundle = PreKeyBundle {
        version: 1, suite: 1, signed_prekey: signed_pk,
        one_time: vec![PreKey { id: 1, public: ot1_pk }, PreKey { id: 2, public: ot2_pk }],
        expiry: 9_999_999,
    };

    let mut rng = FixedRng(vec![9u8; 32], 0);
    let (chosen, _) = bundle.select();
    assert_eq!(chosen.id, 1, "senders take the first entry");
    let (enc, ct) = seal(&mut rng, &chosen.public, &message_info(1), b"", b"hi").unwrap();

    let mut store = PreKeyStore::new(signed_sk);
    store.insert_one_time(1, ot1_sk);
    let sealed = SealedMessage { version: 1, suite: 1, prekey_id: 1, enc, ciphertext: ct };
    let (_, consumed) = store.open_and_consume(&sealed, &message_info(1), b"").unwrap();
    assert!(consumed);

    // The half-delete: secret gone, advertisement intact.
    assert_eq!(store.remaining(), 0);
    assert_eq!(bundle.select().0.id, 1, "still offering the burned key");

    // Pruning is what makes the next sender pick a key that works.
    bundle.one_time.retain(|k| k.id != 1);
    assert_eq!(bundle.select().0.id, 2);
}


/// The sender must withdraw a key from its *cached* copy of a bundle too.
///
/// `select` takes the first one-time entry, so a sender that never prunes seals
/// every message to the same key: the first is accepted, the receiver burns it,
/// and every message after that comes back as an unknown prekey. It looks like
/// the receiver breaking after one message.
///
/// This is the same defect as `a_consumed_prekey_must_not_stay_advertised` on
/// the opposite side of the wire — which is why fixing it there did not fix it
/// here, and why both need a test.
#[test]
fn a_sender_must_prune_its_cached_bundle_after_each_message() {
    let (_, pk1) = kp(30);
    let (_, pk2) = kp(31);
    let (_, signed_pk) = kp(32);
    let mut bundle = PreKeyBundle {
        version: 1, suite: 1, signed_prekey: signed_pk,
        one_time: vec![PreKey { id: 1, public: pk1 }, PreKey { id: 2, public: pk2 }],
        expiry: 9_999_999,
    };

    let first = bundle.select().0.id;
    assert_eq!(first, 1);
    // Without pruning, the very next call hands back the same key.
    assert_eq!(bundle.select().0.id, 1, "select is not stateful, and must not be");

    bundle.one_time.retain(|k| k.id != first);
    assert_eq!(bundle.select().0.id, 2, "pruning is what advances the sender");

    bundle.one_time.retain(|k| k.id != 2);
    let (fallback, fs) = bundle.select();
    assert_eq!(fallback.id, SIGNED_PREKEY_ID);
    assert!(!fs, "an exhausted sender must be told it lost forward secrecy");
}
