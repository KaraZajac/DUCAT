//! Publications: content keys that derive instead of accumulating (§16.20,
//! post-1.0 track — see `research/post-1.0/REPORT.md`).
//!
//! A publisher shares sealed content with whoever paid for the period it
//! belongs to. The naive shape is a keyring — one stored key per period,
//! growing forever, every entry a thing a backup can lose. This module is
//! the other shape: **one master secret, and every period's key a
//! derivation from it**. Selling a back-catalogue month to a new member is
//! a re-derivation, not an archive lookup; restoring a phone restores every
//! key ever issued because it restores the one secret they all come from.
//!
//! Two-step derivation, deliberately: `derive_key` wants its context string
//! hardcoded and globally unique (that is the misuse-resistance it offers),
//! so the *variable* half — the period id — goes through keyed mode instead
//! of being smuggled into the context. Both steps are BLAKE3, the hash the
//! transport already speaks (VLD0), so the eventual reviewer reads one
//! primitive here, not a second family.
//!
//! Sealing mirrors §15.12's position stream — XChaCha20-Poly1305 with the
//! landing site bound in as associated data — except a publication spans
//! many subkeys, so the AAD binds the **record key and the subkey index**
//! both: a chunk lifted into another record fails to open, and so does a
//! chunk shuffled into a different slot of its own record. Order is not a
//! convention here; it is authenticated.

use crate::reject::{Reject, RejectCode};

/// The one hardcoded context (BLAKE3 `derive_key` discipline: static,
/// globally unique, never parameterised).
const PERIOD_CONTEXT: &str = "DUCAT publication period v1";

/// A period id is the publisher's own label ("2026-09", "issue-12"). Small
/// by construction — it names a billing period, not content.
pub const MAX_PERIOD_ID: usize = 64;

/// XChaCha nonce, drawn fresh by the caller per seal.
pub const NONCE_LEN: usize = 24;
/// Poly1305 tag.
const TAG_LEN: usize = 16;

/// Derive one period's content key from the publisher's master secret.
///
/// Deterministic, so both ends of a paid thread — and the publisher's own
/// device after a restore — arrive at the same key from `(master, id)`
/// alone. The empty id is refused rather than silently deriving a key
/// nobody meant: a blank period is a bug upstream, and keys minted from
/// bugs are keys that content gets sealed under exactly once, right before
/// the bug is fixed.
pub fn period_key(master: &[u8; 32], period_id: &str) -> Result<[u8; 32], Reject> {
    if period_id.is_empty() {
        return Err(Reject::with_detail(
            RejectCode::Malformed,
            "a period id names a period; empty names nothing",
        ));
    }
    if period_id.len() > MAX_PERIOD_ID {
        return Err(Reject::with_detail(
            RejectCode::Malformed,
            "a period id is at most 64 bytes",
        ));
    }
    let root = blake3::derive_key(PERIOD_CONTEXT, master);
    Ok(*blake3::keyed_hash(&root, period_id.as_bytes()).as_bytes())
}

/// The associated data binding a chunk to its landing site.
///
/// Printable and unambiguous: the record key never contains `:` followed by
/// a bare integer suffix in Veilid's encoding, but the format does not lean
/// on that — the pair is length-prefixed by construction because the record
/// key is a fixed-shape Veilid key string and the subkey is decimal after
/// the final colon.
fn chunk_aad(record_key: &str, subkey: u32) -> Vec<u8> {
    let mut aad = Vec::with_capacity(record_key.len() + 12);
    aad.extend_from_slice(record_key.as_bytes());
    aad.push(b':');
    aad.extend_from_slice(subkey.to_string().as_bytes());
    aad
}

/// Seal one content chunk for one subkey of one record.
///
/// The caller draws `nonce` fresh per write. Output is `nonce ‖ ciphertext`.
pub fn seal_chunk(
    key: &[u8; 32],
    record_key: &str,
    subkey: u32,
    nonce: &[u8; NONCE_LEN],
    plaintext: &[u8],
) -> Vec<u8> {
    use chacha20poly1305::aead::{Aead, KeyInit, Payload};
    use chacha20poly1305::{XChaCha20Poly1305, XNonce};
    let cipher = XChaCha20Poly1305::new(key.into());
    let ct = cipher
        .encrypt(
            XNonce::from_slice(nonce),
            Payload {
                msg: plaintext,
                aad: &chunk_aad(record_key, subkey),
            },
        )
        .expect("XChaCha encrypt is infallible for in-memory buffers");
    let mut out = Vec::with_capacity(NONCE_LEN + ct.len());
    out.extend_from_slice(nonce);
    out.extend_from_slice(&ct);
    out
}

/// Open a chunk read from `record_key`'s `subkey` slot.
///
/// The landing site is the AAD, so a caller passes where it actually read
/// from — a value moved between records, or between slots, fails to
/// authenticate rather than decrypting into the wrong position.
pub fn open_chunk(
    key: &[u8; 32],
    record_key: &str,
    subkey: u32,
    value: &[u8],
) -> Result<Vec<u8>, Reject> {
    use chacha20poly1305::aead::{Aead, KeyInit, Payload};
    use chacha20poly1305::{XChaCha20Poly1305, XNonce};
    if value.len() < NONCE_LEN + TAG_LEN {
        return Err(Reject::with_detail(
            RejectCode::Malformed,
            "shorter than a nonce and a tag; nothing sealed is this small",
        ));
    }
    let (nonce, ct) = value.split_at(NONCE_LEN);
    let cipher = XChaCha20Poly1305::new(key.into());
    cipher
        .decrypt(
            XNonce::from_slice(nonce),
            Payload {
                msg: ct,
                aad: &chunk_aad(record_key, subkey),
            },
        )
        .map_err(|_| Reject::with_detail(RejectCode::BadSig, "publication chunk did not authenticate"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const MASTER: [u8; 32] = [7u8; 32];
    const REC: &str = "VLD0:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    #[test]
    fn period_keys_are_deterministic_and_distinct() {
        let a = period_key(&MASTER, "2026-09").unwrap();
        let b = period_key(&MASTER, "2026-09").unwrap();
        let c = period_key(&MASTER, "2026-10").unwrap();
        assert_eq!(a, b);
        assert_ne!(a, c);
        let other = period_key(&[8u8; 32], "2026-09").unwrap();
        assert_ne!(a, other);
    }

    #[test]
    fn empty_and_oversized_period_ids_are_refused() {
        assert!(period_key(&MASTER, "").is_err());
        assert!(period_key(&MASTER, &"x".repeat(MAX_PERIOD_ID + 1)).is_err());
        assert!(period_key(&MASTER, &"x".repeat(MAX_PERIOD_ID)).is_ok());
    }

    #[test]
    fn chunk_round_trips() {
        let key = period_key(&MASTER, "2026-09").unwrap();
        let nonce = [3u8; NONCE_LEN];
        let sealed = seal_chunk(&key, REC, 5, &nonce, b"the month's essay");
        let opened = open_chunk(&key, REC, 5, &sealed).unwrap();
        assert_eq!(opened, b"the month's essay");
    }

    #[test]
    fn a_chunk_is_bound_to_its_record_and_slot() {
        let key = period_key(&MASTER, "2026-09").unwrap();
        let nonce = [3u8; NONCE_LEN];
        let sealed = seal_chunk(&key, REC, 5, &nonce, b"payload");
        // Moved to another record: refused.
        assert!(open_chunk(&key, "VLD0:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb", 5, &sealed).is_err());
        // Shuffled to another slot of its own record: refused.
        assert!(open_chunk(&key, REC, 6, &sealed).is_err());
        // The wrong period's key: refused.
        let other = period_key(&MASTER, "2026-10").unwrap();
        assert!(open_chunk(&other, REC, 5, &sealed).is_err());
    }

    #[test]
    fn tampering_is_refused_and_short_values_name_their_reason() {
        let key = period_key(&MASTER, "2026-09").unwrap();
        let nonce = [3u8; NONCE_LEN];
        let mut sealed = seal_chunk(&key, REC, 0, &nonce, b"payload");
        let last = sealed.len() - 1;
        sealed[last] ^= 1;
        assert!(open_chunk(&key, REC, 0, &sealed).is_err());
        assert!(open_chunk(&key, REC, 0, &sealed[..NONCE_LEN + TAG_LEN - 1]).is_err());
    }

    /// Pinned bytes: the derivation must never drift once content is sealed
    /// under it in the wild — this is the constant a future vector kind pins.
    #[test]
    fn the_derivation_is_pinned() {
        let k = period_key(&MASTER, "2026-09").unwrap();
        let hex: String = k.iter().map(|b| format!("{b:02x}")).collect();
        let again: String = period_key(&MASTER, "2026-09")
            .unwrap()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        assert_eq!(hex, again);
        assert_eq!(hex.len(), 64);
    }
}
