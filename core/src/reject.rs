//! Typed refusals, per protocol §18.5.
//!
//! Every refusal is one of these codes. The point is not politeness: a
//! conformance suite is meaningless unless two implementations refuse the same
//! input for the same stated reason, so these values are wire constants and
//! their numbering is fixed.

use crate::cbor::CodecError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum RejectCode {
    BadSig = 1,
    Expired = 2,
    /// Nonce already seen within the retention window (§6.2).
    Replay = 3,
    /// `H(FullOffer) != offer_commit` — the offer was swapped after the tap.
    CommitMismatch = 4,
    /// Locally recomputed price disagreed with the quoted one beyond tolerance.
    PriceMismatch = 5,
    UnsupportedVersion = 6,
    UnsupportedSuite = 7,
    UnsupportedProfile = 8,
    /// A field the implementation does not recognise. Strict by design (§18.8):
    /// tolerating unknown fields means signing something you did not display.
    UnknownField = 9,
    /// Encoding was not canonical (§18.1).
    Malformed = 10,
    /// Message is not legal in the current state (§18.4).
    StateViolation = 11,
    InsufficientCapacity = 12,
    UntrustedArbiterSet = 13,
    RateStale = 14,
    Timeout = 15,
    /// Refused by local policy — unbonded counterparty, degraded bond, fee tier
    /// below the provider's minimum (§8.8).
    PolicyRefused = 16,
}

/// A refusal. `detail` is advisory text for humans and logs; §18.5 requires that
/// it never influence an automated decision, so nothing in this crate reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reject {
    pub code: RejectCode,
    pub detail: Option<String>,
}

impl Reject {
    pub fn new(code: RejectCode) -> Self {
        Reject { code, detail: None }
    }

    pub fn with_detail(code: RejectCode, detail: impl Into<String>) -> Self {
        Reject {
            code,
            detail: Some(detail.into()),
        }
    }
}

impl From<CodecError> for Reject {
    fn from(e: CodecError) -> Self {
        // Every codec failure surfaces as MALFORMED. The specific variant is
        // useful in a log and must not be actionable on the wire, since leaking
        // *which* canonicality rule failed helps an attacker probe the decoder.
        Reject::with_detail(RejectCode::Malformed, format!("{:?}", e))
    }
}
