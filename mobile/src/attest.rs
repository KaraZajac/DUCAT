//! §9.2's rated receipt for the phone: signing and opening an `ATTESTATION`
//! live here, beside the burn proof's envelope in `txproof.rs`, for the same
//! reason — it is a wire object, and both clients must produce the same
//! bytes under the same refusals. What a reader then concludes from a
//! receipt (whose word it is, whether that persona burned anything, one
//! voice per signer) is the client's arithmetic, not this module's.

use ducat_core::sig::SecretKey;
use ducat_core::trust::{open_attestation, open_vouch, sign_attestation, sign_vouch, Attestation, Vouch, ATTESTATION_VERSION, VOUCH_VERSION};

pub use ducat_core::trust::{MAX_ATTESTATION_NOTE_CHARS, RATING_MAX, RATING_MIN};

#[derive(Debug, Clone, thiserror::Error, uniffi::Error)]
pub enum AttestError {
    #[error("{0}")]
    Malformed(String),
}

fn malformed(s: impl Into<String>) -> AttestError {
    AttestError::Malformed(s.into())
}

fn hex_of(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn unhex(s: &str) -> Option<Vec<u8>> {
    let s = s.trim();
    if s.len() % 2 != 0 {
        return None;
    }
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok()).collect()
}

/// Everything signing an `ATTESTATION` needs, in one record (one by-value
/// buffer: several segfault on arm64).
#[derive(Debug, Clone, uniffi::Record)]
pub struct AttestationIn {
    /// The signer's 32-byte persona secret; the signer named inside is
    /// derived from it, so the two cannot disagree.
    pub persona_secret: Vec<u8>,
    /// Who is spoken about — a persona's public key, hex.
    pub subject_hex: String,
    /// What settled between them, in pXMR.
    pub amount_pxmr: u64,
    /// 1..=5.
    pub rating: u8,
    /// Seconds since the epoch.
    pub ts: u64,
    /// The transaction the deal settled on, hex, when there was one.
    pub txid_hex: Option<String>,
    /// One sentence, at most 140 characters, or nothing.
    pub note: Option<String>,
}

/// An `ATTESTATION` opened: the signature checked under the signer named
/// inside, the shape checked the way the wire checks it, and nothing more.
#[derive(Debug, Clone, uniffi::Record)]
pub struct AttestationView {
    pub signer_hex: String,
    pub subject_hex: String,
    pub amount_pxmr: u64,
    pub rating: u8,
    pub ts: u64,
    pub txid_hex: Option<String>,
    pub note: Option<String>,
}

/// Sign a rated receipt under the persona whose secret is given. The object
/// is read back through the wire's own reader before it is sealed, so every
/// refusal a stranger would make — a rating outside 1..=5, a note longer
/// than a sentence, a persona attesting to itself — is made here.
#[uniffi::export]
pub fn attestation_sign(input: AttestationIn) -> Result<Vec<u8>, AttestError> {
    let secret: [u8; 32] = input.persona_secret.as_slice().try_into().map_err(|_| malformed("persona secret is not 32 bytes"))?;
    let key = SecretKey::ed25519_from_bytes(&secret);
    let subject = unhex(&input.subject_hex).filter(|b| b.len() == 32).ok_or_else(|| malformed("subject is not a persona"))?;
    let txid = match input.txid_hex.as_deref().map(str::trim).filter(|t| !t.is_empty()) {
        Some(t) => Some(unhex(t).and_then(|b| <[u8; 32]>::try_from(b).ok()).ok_or_else(|| malformed("txid is not 32 bytes of hex"))?),
        None => None,
    };
    let note = input.note.as_deref().map(str::trim).filter(|n| !n.is_empty()).map(str::to_string);
    let a = Attestation {
        version: ATTESTATION_VERSION,
        suite: 1,
        signer: key.public().to_bytes().to_vec(),
        subject,
        amount_pxmr: input.amount_pxmr,
        rating: input.rating,
        ts: input.ts,
        txid,
        note,
    };
    Attestation::from_value(a.to_value()).map_err(|e| malformed(format!("{e:?}")))?;
    Ok(sign_attestation(&a, &key))
}

/// Open a rated receipt. Whose word it is, is the caller's to weigh: a
/// receipt about you must have come from its signer; a record somebody
/// shows must be about them.
#[uniffi::export]
pub fn attestation_open(envelope: Vec<u8>) -> Result<AttestationView, AttestError> {
    let a = open_attestation(&envelope).map_err(|e| malformed(format!("{e:?}")))?;
    Ok(AttestationView {
        signer_hex: hex_of(&a.signer),
        subject_hex: hex_of(&a.subject),
        amount_pxmr: a.amount_pxmr,
        rating: a.rating,
        ts: a.ts,
        txid_hex: a.txid.map(|t| hex_of(&t)),
        note: a.note,
    })
}

/// Everything signing a `VOUCH` needs: the signer's secret, the subject, a
/// time. Nothing else travels — a vouch says *I know this persona* and no more.
#[derive(Debug, Clone, uniffi::Record)]
pub struct VouchIn {
    pub persona_secret: Vec<u8>,
    pub subject_hex: String,
    pub ts: u64,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct VouchView {
    pub signer_hex: String,
    pub subject_hex: String,
    pub ts: u64,
}

/// Sign a vouch under the persona whose secret is given; the wire's own
/// refusals (nowhen, oneself) are made here.
#[uniffi::export]
pub fn vouch_sign(input: VouchIn) -> Result<Vec<u8>, AttestError> {
    let secret: [u8; 32] = input.persona_secret.as_slice().try_into().map_err(|_| malformed("persona secret is not 32 bytes"))?;
    let key = SecretKey::ed25519_from_bytes(&secret);
    let subject = unhex(&input.subject_hex).filter(|b| b.len() == 32).ok_or_else(|| malformed("subject is not a persona"))?;
    let v = Vouch { version: VOUCH_VERSION, suite: 1, signer: key.public().to_bytes().to_vec(), subject, ts: input.ts };
    Vouch::from_value(v.to_value()).map_err(|e| malformed(format!("{e:?}")))?;
    Ok(sign_vouch(&v, &key))
}

/// Open a vouch. Whether its signer is anyone the reader knows is the
/// reader's question.
#[uniffi::export]
pub fn vouch_open(envelope: Vec<u8>) -> Result<VouchView, AttestError> {
    let v = open_vouch(&envelope).map_err(|e| malformed(format!("{e:?}")))?;
    Ok(VouchView { signer_hex: hex_of(&v.signer), subject_hex: hex_of(&v.subject), ts: v.ts })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_vouch_round_trips_and_refuses_nowhen_and_oneself() {
        let me = hex_of(&SecretKey::ed25519_from_bytes(&[0x21; 32]).public().to_bytes());
        let them = hex_of(&SecretKey::ed25519_from_bytes(&[0x22; 32]).public().to_bytes());
        let env = vouch_sign(VouchIn { persona_secret: vec![0x21; 32], subject_hex: them.clone(), ts: 1_700_000_000 }).unwrap();
        let v = vouch_open(env).unwrap();
        assert_eq!((v.signer_hex, v.subject_hex, v.ts), (me.clone(), them.clone(), 1_700_000_000));
        assert!(vouch_sign(VouchIn { persona_secret: vec![0x21; 32], subject_hex: them.clone(), ts: 0 }).is_err());
        assert!(vouch_sign(VouchIn { persona_secret: vec![0x21; 32], subject_hex: me, ts: 1 }).is_err());
        assert!(vouch_sign(VouchIn { persona_secret: vec![1; 31], subject_hex: them, ts: 1 }).is_err());
    }

    fn input() -> AttestationIn {
        AttestationIn {
            persona_secret: vec![0x21; 32],
            subject_hex: hex_of(&SecretKey::ed25519_from_bytes(&[0x22; 32]).public().to_bytes()),
            amount_pxmr: 5_000_000_000,
            rating: 5,
            ts: 1_700_000_000,
            txid_hex: Some("ab".repeat(32)),
            note: Some("  prompt, as described  ".into()),
        }
    }

    #[test]
    fn a_signed_receipt_opens_to_what_was_signed() {
        let env = attestation_sign(input()).unwrap();
        let v = attestation_open(env).unwrap();
        assert_eq!(v.signer_hex, hex_of(&SecretKey::ed25519_from_bytes(&[0x21; 32]).public().to_bytes()));
        assert_eq!(v.subject_hex, input().subject_hex);
        assert_eq!((v.amount_pxmr, v.rating, v.ts), (5_000_000_000, 5, 1_700_000_000));
        assert_eq!(v.txid_hex.as_deref(), Some("ab".repeat(32).as_str()));
        assert_eq!(v.note.as_deref(), Some("prompt, as described"));
    }

    #[test]
    fn what_the_wire_refuses_is_refused_before_signing() {
        let me = hex_of(&SecretKey::ed25519_from_bytes(&[0x21; 32]).public().to_bytes());
        for (why, bad) in [
            ("rating 0", AttestationIn { rating: 0, ..input() }),
            ("rating 6", AttestationIn { rating: 6, ..input() }),
            ("no time", AttestationIn { ts: 0, ..input() }),
            ("about itself", AttestationIn { subject_hex: me, ..input() }),
            ("note too long", AttestationIn { note: Some("x".repeat(141)), ..input() }),
            ("short txid", AttestationIn { txid_hex: Some("abcd".into()), ..input() }),
            ("short subject", AttestationIn { subject_hex: "abcd".into(), ..input() }),
            ("short secret", AttestationIn { persona_secret: vec![1; 31], ..input() }),
        ] {
            assert!(attestation_sign(bad).is_err(), "{why} was signed");
        }
        // A torn envelope does not open.
        let mut env = attestation_sign(input()).unwrap();
        let last = env.len() - 1;
        env[last] ^= 1;
        assert!(attestation_open(env).is_err());
    }
}
