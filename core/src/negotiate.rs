//! Version and cipher-suite negotiation, per protocol §18.6.
//!
//! # A correction to the spec's rule
//!
//! §18.6 says the reader selects "the highest mutually supported" of each.
//! That is right for versions and **wrong for suites**, because it assumes the
//! numeric identifier encodes preference. It does not, and cannot:
//!
//! * Suite 1 is Ed25519/X25519. Suite 2 is P-256, which exists only because
//!   iOS's Secure Enclave holds no Ed25519 key (§4.1). P-256 is a *fallback
//!   forced by hardware*, not an upgrade — so "highest wins" would silently
//!   prefer the weaker option on every platform pair that supports both.
//! * Identifiers are allocated in registration order. A suite added later
//!   because it is cheaper, or narrower, or needed by one platform, would
//!   outrank everything before it purely by arriving late.
//! * Preference can legitimately be context-dependent: a hardware-backed P-256
//!   key may be a better choice on one device than a software Ed25519 key,
//!   and no global ordering expresses that.
//!
//! So suites are negotiated over an **explicit preference list held by the
//! payer**, and the numeric identifier carries no ordering meaning. The payer
//! decides because the payer is the party whose money is at risk — the same
//! reasoning that puts `ACCEPT` in the payer's hands (§18.4.1).
//!
//! # Downgrade resistance
//!
//! Stripping strong options from the advertised set is the classic attack. The
//! defence is already structural in the protocol and needs no new machinery:
//! the advertised set lives inside `FullOffer`, and `TapPresent.offer_commit`
//! is a commitment to the whole of `FullOffer` (§15.3). Removing a suite
//! changes the offer, changes its digest, and fails the commitment check before
//! negotiation is even reached. `verify_no_downgrade` makes that explicit.

use std::collections::BTreeSet;

use crate::commit::{commit, commit_eq, Purpose};
use crate::reject::{Reject, RejectCode};
use crate::sig::Suite;

/// What one side advertises it can speak.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Supported {
    pub versions: Vec<u16>,
    pub suites: Vec<Suite>,
}

/// Local negotiation policy.
#[derive(Debug, Clone)]
pub struct Policy {
    /// Suites this client will use, **most preferred first**. Order here is the
    /// only thing that decides suite selection.
    pub preference: Vec<Suite>,
    /// Suites permitted at all. This is the intersection of the client's own
    /// policy with the market's `suite_floor` (§10.1): a market may narrow what
    /// its participants accept, and may never widen it.
    pub permitted: BTreeSet<Suite>,
    pub versions: Vec<u16>,
}

impl Policy {
    /// Permit everything in `preference`, with no market restriction.
    pub fn new(preference: Vec<Suite>, versions: Vec<u16>) -> Self {
        let permitted = preference.iter().copied().collect();
        Policy {
            preference,
            permitted,
            versions,
        }
    }

    /// Apply a market's permitted set. Narrowing only — a market that named a
    /// suite this client rejects does not thereby re-enable it.
    pub fn restrict_to_market(mut self, market_permits: &BTreeSet<Suite>) -> Self {
        self.permitted = self.permitted.intersection(market_permits).copied().collect();
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Selection {
    pub version: u16,
    pub suite: Suite,
}

/// Choose a version and suite, or refuse.
pub fn negotiate(offered: &Supported, policy: &Policy) -> Result<Selection, Reject> {
    // Versions *do* have a natural order: higher is newer, and newness is the
    // whole point of a version number.
    let version = offered
        .versions
        .iter()
        .filter(|v| policy.versions.contains(v))
        .copied()
        .max()
        .ok_or_else(|| {
            Reject::with_detail(
                RejectCode::UnsupportedVersion,
                "no mutually supported protocol version",
            )
        })?;

    // Suites are chosen by the payer's declared preference over what is both
    // offered and permitted. First match wins; the numeric id is never compared.
    let suite = policy
        .preference
        .iter()
        .find(|s| policy.permitted.contains(s) && offered.suites.contains(s))
        .copied()
        .ok_or_else(|| {
            Reject::with_detail(
                RejectCode::UnsupportedSuite,
                "no mutually supported and permitted cipher suite",
            )
        })?;

    Ok(Selection { version, suite })
}

/// Confirm the advertised set reached us as the presenter sent it.
///
/// `offer_bytes` is the canonical `FullOffer` as received; `tap_commit` is the
/// `offer_commit` carried by the bootstrap. A mismatch means the offer was
/// altered in flight — which includes, but is not limited to, having strong
/// suites stripped out of it.
///
/// Callers MUST run this before `negotiate`. Negotiating first and checking
/// afterwards would mean selecting a suite from an attacker-chosen menu and
/// only then noticing, which is the bug this ordering exists to prevent.
pub fn verify_no_downgrade(offer_bytes: &[u8], tap_commit: &[u8; 32]) -> Result<(), Reject> {
    let actual = commit(Purpose::Offer, offer_bytes);
    if commit_eq(&actual, tap_commit) {
        Ok(())
    } else {
        Err(Reject::with_detail(
            RejectCode::CommitMismatch,
            "FullOffer does not match the offer_commit carried by the tap",
        ))
    }
}
