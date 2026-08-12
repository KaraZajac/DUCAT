//! Commitments over canonical objects.
//!
//! The protocol hashes canonical CBOR in several unrelated places —
//! `offer_commit = H(FullOffer)` (§15.3), `H(RECEIPT)` inside the CONTACT bind
//! (§16.3), the message chain in §6 — and a bare hash carries no record of
//! which of those it was for. That is the same weakness §18.3 fixes for
//! signatures, so the same fix applies: every commitment names its purpose.
//!
//! Domain separation is free here, and its absence would mean a digest computed
//! for one role could be presented as another wherever an attacker can arrange
//! for the underlying bytes to coincide.

use sha2::{Digest, Sha256};

use crate::sig::DOMAIN;

/// What a commitment is *for*. Part of the hash input, therefore a wire
/// constant: changing a label invalidates every commitment of that kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Purpose {
    /// `offer_commit` in `TapPresent` — binds the offer delivered over the
    /// channel to the bootstrap that advertised it (§15.3).
    Offer,
    /// `H(RECEIPT)` as used by the CONTACT bind (§16.3).
    Receipt,
    /// Predecessor link in the §6 message chain.
    ChainLink,
    /// `H(genesis descriptor)` for a self-certifying `market_id` (§10.1).
    MarketGenesis,
}

impl Purpose {
    fn label(self) -> &'static [u8] {
        match self {
            Purpose::Offer => b"offer_commit",
            Purpose::Receipt => b"receipt",
            Purpose::ChainLink => b"chain",
            Purpose::MarketGenesis => b"market_genesis",
        }
    }
}

/// Commit to canonical bytes for a stated purpose.
///
/// Input layout mirrors §18.3's signature input, with 0x00 separators so that
/// adjacent variable-length fields cannot be re-parsed into different
/// boundaries.
pub fn commit(purpose: Purpose, canonical_bytes: &[u8]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(DOMAIN);
    h.update([0x00]);
    h.update(purpose.label());
    h.update([0x00]);
    h.update(canonical_bytes);
    h.finalize().into()
}

/// Constant-time-ish comparison for commitments.
///
/// Commitments are public values, so timing is not the threat it is for keys —
/// but a byte-at-a-time early return in a hot verification path is a habit
/// worth not forming.
pub fn commit_eq(a: &[u8; 32], b: &[u8; 32]) -> bool {
    let mut diff = 0u8;
    for i in 0..32 {
        diff |= a[i] ^ b[i];
    }
    diff == 0
}
