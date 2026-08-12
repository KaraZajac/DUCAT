//! The bridge between `ducat-core` and the client.
//!
//! **This crate adds no logic.** Every function here forwards to `core`, because
//! the alternative — a little arithmetic on the Kotlin side "for now" — is how a
//! second implementation begins. §18.12 exists because a document and its code
//! drift apart silently; a UI and its protocol drift the same way, and faster,
//! since nothing checks a screen against a spec.
//!
//! What the bridge *is* allowed to do is refuse to expose things that would
//! invite a wrong call. `payments_supported` returns an approximation and says
//! so in its name, because §17.2 forbids promising an exact count.

use ducat_core::{bond, float, verify};

uniffi::setup_scaffolding!();

// ---------------------------------------------------------------------------
// §17.2 — the float, and the number the home screen must not overstate
// ---------------------------------------------------------------------------

/// What a float must hold to support a given usage pattern.
#[derive(uniffi::Record)]
pub struct FloatPlan {
    /// Outputs to pre-split into at load time.
    pub outputs: u32,
    /// Total piconero committed — and so the amount exposed on the phone (O9).
    pub total_pxmr: u64,
}

/// Size a float for `payments` consecutive spends of about `typical_pxmr`.
///
/// Returns the plan and, unavoidably, the **minimum exposure**: §17.2 makes
/// capacity a count of unlocked outputs, so there is no way to hold less and
/// still spend that often. A settings screen offering a risk slider without
/// showing this is offering a choice the protocol does not provide.
#[uniffi::export]
pub fn plan_float(payments: u32, typical_pxmr: u64) -> FloatPlan {
    let p = float::plan(payments, typical_pxmr);
    FloatPlan { outputs: p.outputs, total_pxmr: p.total_pxmr }
}

/// **About** how many consecutive payments a given count of unlocked outputs buys.
///
/// Named for the approximation deliberately. The drain test measured six
/// unlocked outputs buying four payments, because input selection belongs to the
/// wallet and a payment may consume more than one output — so §17.2 forbids
/// promising an exact number. A caller reaching for a precise figure will not
/// find one here.
#[uniffi::export]
pub fn approx_payments_supported(unlocked_outputs: u32) -> u32 {
    float::payments_supported(unlocked_outputs)
}

/// Whether a stated risk cap can support a stated usage pattern.
///
/// The two are set in different places by different reasoning — a security
/// setting and a convenience setting — and nothing otherwise notices they
/// contradict each other until the user is at a counter. Returns the shortfall
/// in piconero when they do.
#[derive(uniffi::Record)]
pub struct Reconciliation {
    pub ok: bool,
    pub plan: FloatPlan,
    /// Zero when `ok`.
    pub shortfall_pxmr: u64,
}

#[uniffi::export]
pub fn reconcile_float(max_exposure_pxmr: u64, payments: u32, typical_pxmr: u64) -> Reconciliation {
    match float::reconcile(max_exposure_pxmr, payments, typical_pxmr) {
        Ok(p) => Reconciliation {
            ok: true,
            plan: FloatPlan { outputs: p.outputs, total_pxmr: p.total_pxmr },
            shortfall_pxmr: 0,
        },
        Err(short) => {
            let p = float::plan(payments, typical_pxmr);
            Reconciliation {
                ok: false,
                plan: FloatPlan { outputs: p.outputs, total_pxmr: p.total_pxmr },
                shortfall_pxmr: short,
            }
        }
    }
}

// ---------------------------------------------------------------------------
// §15.5.1 — is the person holding this device entitled to spend?
// ---------------------------------------------------------------------------

/// Assurance that the person present may spend, weakest first.
#[derive(uniffi::Enum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verification {
    /// Tap and go, as contactless does below its floor limit.
    None,
    /// The OS reports the device unlocked — **passive**, and a thief holding the
    /// phone already satisfies it.
    DeviceUnlocked,
    /// A secret entered into this app, deliberately, recently. The knowledge
    /// factor a thief does not have.
    AppSecret,
}

/// User-set thresholds, in the reference currency's **minor units**.
///
/// Never piconero: a threshold stored in piconero drifts every time the rate
/// moves, quietly turning a "$100 limit" into a $70 one after a price rise.
#[derive(uniffi::Record)]
pub struct VerificationPolicy {
    pub device_unlock_at: u64,
    pub app_secret_at: u64,
    pub app_secret_validity_s: u64,
    pub cumulative_at: u64,
    pub cumulative_window_s: u64,
}

#[uniffi::export]
pub fn default_verification_policy() -> VerificationPolicy {
    let d = verify::VerificationPolicy::default();
    VerificationPolicy {
        device_unlock_at: d.device_unlock_at,
        app_secret_at: d.app_secret_at,
        app_secret_validity_s: d.app_secret_validity_s,
        cumulative_at: d.cumulative_at,
        cumulative_window_s: d.cumulative_window_s,
    }
}

#[derive(uniffi::Record)]
pub struct VerificationOutcome {
    pub permitted: bool,
    pub required: Verification,
    pub satisfied: Verification,
}

/// Decide whether this payment may be signed (§15.5.1).
///
/// `rate_is_fresh` reflects §17.7's cached rate, and a stale one **escalates**
/// to the strongest tier rather than relaxing anything: thresholds are
/// denominated in real money, so without a trustworthy rate the client cannot
/// know which rung it is on. Failing the other way would let anyone able to
/// stall a rate feed lower the security requirement.
#[uniffi::export]
pub fn check_verification(
    policy: VerificationPolicy,
    device_unlocked: bool,
    app_secret_age_s: Option<u64>,
    amount_minor: u64,
    spent_in_window_minor: u64,
    rate_is_fresh: bool,
) -> VerificationOutcome {
    let p = verify::VerificationPolicy {
        device_unlock_at: policy.device_unlock_at,
        app_secret_at: policy.app_secret_at,
        app_secret_validity_s: policy.app_secret_validity_s,
        cumulative_at: policy.cumulative_at,
        cumulative_window_s: policy.cumulative_window_s,
    };
    let st = verify::VerificationState { device_unlocked, app_secret_age_s };
    let map = |v: verify::Verification| match v {
        verify::Verification::None => Verification::None,
        verify::Verification::DeviceUnlocked => Verification::DeviceUnlocked,
        verify::Verification::AppSecret => Verification::AppSecret,
    };
    match verify::check_verification(&p, &st, amount_minor, spent_in_window_minor, rate_is_fresh) {
        Ok(tier) => VerificationOutcome {
            permitted: true,
            required: map(tier),
            satisfied: map(st.satisfied(p.app_secret_validity_s)),
        },
        Err(need) => VerificationOutcome {
            permitted: false,
            required: map(need.required),
            satisfied: map(need.satisfied),
        },
    }
}

// ---------------------------------------------------------------------------
// §17.8 — publishing capacity without publishing a balance
// ---------------------------------------------------------------------------

/// The largest ladder value not exceeding `capacity_pxmr`.
///
/// Rounds **down**, always: rounding to nearest would let a bond claim capacity
/// it does not have, and the party who benefits from that overstatement is the
/// one publishing it.
#[uniffi::export]
pub fn capacity_bucket(capacity_pxmr: u64) -> u64 {
    bond::bucket_floor(capacity_pxmr)
}

/// How many bits a published bucket reveals — under 4.1, against 64 for an exact
/// balance. Exposed so a settings screen can state the trade rather than assert it.
#[uniffi::export]
pub fn capacity_leak_bits() -> f64 {
    bond::leaked_bits()
}

/// The protocol version this client speaks, for an about screen (§11).
#[uniffi::export]
pub fn protocol_version() -> String {
    "DUCAT-v1".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The bridge must forward, not reinterpret.
    ///
    /// A wrapper is exactly where a quiet second implementation appears: one
    /// rounding choice, one "+1 for safety", and the app is answering a
    /// different question from the vectors. These compare against `core`
    /// directly rather than against expected constants, so the test fails if the
    /// wrapper starts having opinions.
    #[test]
    fn the_bridge_adds_nothing() {
        for outputs in 0..40u32 {
            assert_eq!(
                approx_payments_supported(outputs),
                float::payments_supported(outputs),
                "capacity diverged at {outputs} outputs"
            );
        }
        for payments in 0..25u32 {
            let a = plan_float(payments, 2_000_000_000);
            let b = float::plan(payments, 2_000_000_000);
            assert_eq!((a.outputs, a.total_pxmr), (b.outputs, b.total_pxmr));
        }
        for cap in [0u64, 1, 999_999_999, 5_000_000_000, u64::MAX] {
            assert_eq!(capacity_bucket(cap), bond::bucket_floor(cap));
        }
    }

    /// §17.2 forbids promising an exact count. The name says "approx"; this
    /// checks the value earns it.
    #[test]
    fn capacity_is_never_overstated_across_the_bridge() {
        for outputs in 0..200u32 {
            let claimed = approx_payments_supported(outputs);
            assert!(
                (claimed as f64) * float::OUTPUTS_PER_PAYMENT <= outputs as f64 + f64::EPSILON,
                "claimed {claimed} payments from {outputs} outputs"
            );
        }
    }

    /// §15.5.1's rule that costs the most to get backwards: a stale rate must
    /// escalate. Failing the other way lets anyone who can stall a rate feed
    /// lower the security requirement.
    #[test]
    fn a_stale_rate_escalates_across_the_bridge() {
        let p = default_verification_policy();
        let small = 1; // well under every threshold
        let fresh = check_verification(
            default_verification_policy(),
            true,
            None,
            small,
            0,
            true,
        );
        assert_eq!(fresh.required, Verification::None);

        let stale = check_verification(p, true, None, small, 0, false);
        assert_eq!(
            stale.required,
            Verification::AppSecret,
            "a stale rate must demand the strongest tier, not the weakest"
        );
        assert!(!stale.permitted, "device-unlocked alone cannot satisfy AppSecret");
    }

    /// A bucket is a ladder value or it is a balance wearing a disguise (§17.8).
    #[test]
    fn buckets_stay_coarse() {
        assert!(capacity_leak_bits() < 5.0);
        assert!(capacity_bucket(4_999_999_999) < 4_999_999_999);
    }
}
