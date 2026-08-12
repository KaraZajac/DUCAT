//! Payer verification — who is doing the signing.
//!
//! WYSIWYS (§15.5) establishes that a payer sees exactly what they sign. It says
//! nothing about *whether the person holding the device should be signing at
//! all*. A stolen unlocked phone is a bearer instrument: A1 working as designed,
//! and also how people lose money.
//!
//! EMV's answer is proportionality — no verification below a floor, stronger
//! verification above it — and this follows the same shape.
//!
//! # This never goes on the wire
//!
//! Everything here is **local policy**, evaluated by the payer's own client. The
//! payee never learns which tier was satisfied and cannot request one. If a
//! counterparty could influence verification it could ask for the weakest, which
//! is a downgrade attack EMV spent years patching. A payee's only options remain
//! accept or decline.

use crate::reject::{Reject, RejectCode};

/// Assurance that the person present is entitled to spend, weakest first.
///
/// The gap between `DeviceUnlocked` and `AppSecret` is the important one and is
/// easy to collapse by accident. A device unlocked twenty minutes ago is a
/// *passive* fact that a thief holding the phone already satisfies. A secret
/// entered into this app just now is an *active* knowledge factor they do not
/// have.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Verification {
    /// Tap and go, as contactless does below its floor limit.
    None = 0,
    /// The operating system reports the device unlocked — biometric or
    /// passcode, satisfied passively and possibly some time ago.
    DeviceUnlocked = 1,
    /// A secret entered into this application, deliberately, recently.
    AppSecret = 2,
}

/// What the payer's client can currently attest to.
#[derive(Debug, Clone, Copy)]
pub struct VerificationState {
    pub device_unlocked: bool,
    /// Seconds since a secret was last entered in-app; `None` if never.
    pub app_secret_age_s: Option<u64>,
}

impl VerificationState {
    /// The strongest tier currently satisfied, given how stale each is.
    pub fn satisfied(&self, validity_s: u64) -> Verification {
        if matches!(self.app_secret_age_s, Some(age) if age <= validity_s) {
            Verification::AppSecret
        } else if self.device_unlocked {
            Verification::DeviceUnlocked
        } else {
            Verification::None
        }
    }
}

/// User-set thresholds, in the **reference currency's minor units** rather than
/// piconero.
///
/// A user thinks in money, not in atomic units, and a threshold stored in
/// piconero would silently drift every time the exchange rate moved — a "$100
/// limit" quietly becoming a $70 one after a price rise.
#[derive(Debug, Clone, Copy)]
pub struct VerificationPolicy {
    /// At or above this, the device must be unlocked.
    pub device_unlock_at: u64,
    /// At or above this, a secret must be entered in-app.
    pub app_secret_at: u64,
    /// How long an in-app entry stays good. Short, or "deliberate" decays into
    /// "happened at some point today".
    pub app_secret_validity_s: u64,
    /// Cumulative spend in a rolling window that also demands the top tier.
    /// A per-transaction limit alone does not stop twenty payments just under
    /// it, which is how a lifted phone is actually drained.
    pub cumulative_at: u64,
    pub cumulative_window_s: u64,
}

impl Default for VerificationPolicy {
    /// Deliberately conservative. A user who never opens settings should still
    /// be protected, and the cost of being wrong in this direction is one extra
    /// unlock rather than an emptied wallet.
    fn default() -> Self {
        VerificationPolicy {
            device_unlock_at: 2_000,       // $20.00
            app_secret_at: 10_000,         // $100.00
            app_secret_validity_s: 120,
            cumulative_at: 20_000,         // $200.00 in a rolling window
            cumulative_window_s: 3_600,
        }
    }
}

impl VerificationPolicy {
    /// Thresholds must ascend, or the ladder inverts and a larger payment asks
    /// for less than a smaller one. Rejected at construction rather than
    /// producing a policy that is quietly nonsense.
    pub fn validate(&self) -> Result<(), Reject> {
        if self.app_secret_at < self.device_unlock_at {
            return Err(Reject::with_detail(
                RejectCode::PolicyRefused,
                "verification thresholds must ascend: a larger payment cannot require less",
            ));
        }
        if self.app_secret_validity_s == 0 {
            return Err(Reject::with_detail(
                RejectCode::PolicyRefused,
                "an in-app secret with zero validity can never be satisfied",
            ));
        }
        Ok(())
    }

    /// What this payment demands.
    pub fn required(&self, amount_minor: u64, spent_in_window_minor: u64) -> Verification {
        // Velocity counts the payment being considered, not just history —
        // otherwise the transaction that crosses the line is the one that gets
        // through.
        let cumulative = spent_in_window_minor.saturating_add(amount_minor);
        if amount_minor >= self.app_secret_at || cumulative >= self.cumulative_at {
            Verification::AppSecret
        } else if amount_minor >= self.device_unlock_at {
            Verification::DeviceUnlocked
        } else {
            Verification::None
        }
    }
}

/// Why a payment may not be signed yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerificationNeeded {
    pub required: Verification,
    pub satisfied: Verification,
}

/// Decide whether this payment may be signed.
///
/// `rate_is_fresh` reflects §17.7's cached exchange rate. **A stale rate
/// escalates to the strongest tier rather than relaxing anything**: the
/// thresholds are denominated in real money, so without a trustworthy rate the
/// client cannot know which rung it is on. Failing the other way would let an
/// attacker who can stall a rate feed lower the verification requirement, which
/// turns a liveness problem into a security one.
pub fn check_verification(
    policy: &VerificationPolicy,
    state: &VerificationState,
    amount_minor: u64,
    spent_in_window_minor: u64,
    rate_is_fresh: bool,
) -> Result<Verification, VerificationNeeded> {
    let required = if rate_is_fresh {
        policy.required(amount_minor, spent_in_window_minor)
    } else {
        Verification::AppSecret
    };
    let satisfied = state.satisfied(policy.app_secret_validity_s);
    if satisfied >= required {
        Ok(required)
    } else {
        Err(VerificationNeeded {
            required,
            satisfied,
        })
    }
}
