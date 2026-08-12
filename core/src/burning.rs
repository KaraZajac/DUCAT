//! The burning bug (O15): two outputs, one key image, one spendable coin.
//!
//! Monero derives an output's key image from its one-time public key. Two
//! outputs sharing that key therefore share a key image, and the network accepts
//! only the first spend — the second is permanently unspendable.
//!
//! **The attack is arithmetic, not cryptography.** A merchant expecting 1 XMR
//! receives a transaction carrying two outputs of 0.5 to the same one-time key.
//! A wallet that sums what it received sees 1.0, accepts, and hands over the
//! goods. It can spend 0.5. The merchant is out the difference, and nothing
//! anywhere failed a signature check.
//!
//! §15.10's fresh-subaddress rule narrows the window and does not close it: the
//! sender chooses the transaction key, so it can drive two outputs to the same
//! one-time key inside a single transaction regardless of how fresh the
//! recipient's subaddress is.
//!
//! # Why detect rather than adopt immune outputs
//!
//! `monero-wallet` offers burning-bug-immune "guaranteed" outputs, and its own
//! source says they are *"not officially specified by the Monero project … No
//! support outside of monero-wallet is promised."* Accepting funds into a format
//! only one implementation understands would make those funds non-bearer in
//! practice — against A1, and against §11's many-clients goal. Detection keeps
//! the coins standard.
//!
//! # The rule
//!
//! **Outputs sharing a one-time key count once, at the maximum — never summed.**
//! That is the whole mitigation, and `sum()` is the bug.

use std::collections::BTreeMap;

use crate::reject::{Reject, RejectCode};

/// One output the wallet believes it received.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReceivedOutput {
    /// The one-time (stealth) public key. Identity for this purpose: two outputs
    /// sharing it are one spendable coin.
    pub one_time_key: [u8; 32],
    pub amount_pxmr: u64,
    pub txid: [u8; 32],
}

/// What a set of received outputs is actually worth.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Creditable {
    /// Spendable value, counting duplicates once at their maximum.
    pub total_pxmr: u64,
    /// What a naive `sum()` would have reported. Kept so a client can show the
    /// size of the discrepancy rather than silently quieting it.
    pub naive_sum_pxmr: u64,
    /// One-time keys that appeared more than once.
    pub burned_keys: Vec<[u8; 32]>,
}

impl Creditable {
    /// Whether any duplicate was present. **Always worth surfacing**, even when
    /// the total still covers the price: a duplicate one-time key does not occur
    /// by accident, so its presence is evidence about the counterparty and not
    /// merely an accounting detail to absorb.
    pub fn burn_detected(&self) -> bool {
        !self.burned_keys.is_empty()
    }
}

/// Value a set of outputs is really worth.
///
/// Monero's own wallet keeps the largest of a duplicate set, and this matches
/// that: it is the only choice that is both safe and not self-punishing, since
/// exactly one of the group is spendable and the recipient may as well have the
/// biggest.
pub fn creditable(outputs: &[ReceivedOutput]) -> Creditable {
    let mut best: BTreeMap<[u8; 32], u64> = BTreeMap::new();
    let mut counts: BTreeMap<[u8; 32], u32> = BTreeMap::new();
    let mut naive: u64 = 0;
    for o in outputs {
        naive = naive.saturating_add(o.amount_pxmr);
        *counts.entry(o.one_time_key).or_insert(0) += 1;
        let e = best.entry(o.one_time_key).or_insert(0);
        if o.amount_pxmr > *e {
            *e = o.amount_pxmr;
        }
    }
    let burned_keys = counts
        .iter()
        .filter(|(_, n)| **n > 1)
        .map(|(k, _)| *k)
        .collect();
    Creditable {
        total_pxmr: best.values().fold(0u64, |a, b| a.saturating_add(*b)),
        naive_sum_pxmr: naive,
        burned_keys,
    }
}

/// Whether these outputs pay `required_pxmr` (§17.3, `fast/1` acceptance).
///
/// The error deliberately names the burn rather than reporting a plain shortfall.
/// A merchant told "underpaid" retries or argues about the price; a merchant told
/// the payment carried duplicate one-time keys knows it was constructed, and that
/// distinction is the difference between a mistake and an attack.
pub fn check_payment(
    outputs: &[ReceivedOutput],
    required_pxmr: u64,
) -> Result<Creditable, Reject> {
    let c = creditable(outputs);
    if c.total_pxmr < required_pxmr {
        if c.burn_detected() {
            return Err(Reject::with_detail(
                RejectCode::PriceMismatch,
                format!(
                    "payment carries {} duplicated one-time key(s): {} pXMR was sent \
                     but only {} is spendable — this is a burning-bug construction, \
                     not a shortfall",
                    c.burned_keys.len(),
                    c.naive_sum_pxmr,
                    c.total_pxmr
                ),
            ));
        }
        return Err(Reject::with_detail(
            RejectCode::PriceMismatch,
            "payment is short of the accepted amount",
        ));
    }
    Ok(c)
}
