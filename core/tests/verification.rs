//! Payer verification (the EMV CVM analogue).
//!
//! WYSIWYS says the payer sees what they sign. This says whether they are
//! entitled to sign it at all — the question a stolen unlocked phone answers
//! "yes" to unless something asks.

use ducat_core::reject::RejectCode;
use ducat_core::verify::*;

/// The user's own example: unlocked phone is fine around $50, a PIN above $100.
fn users_policy() -> VerificationPolicy {
    VerificationPolicy {
        device_unlock_at: 500,        // $5
        app_secret_at: 10_000,        // $100
        app_secret_validity_s: 120,
        cumulative_at: 30_000,        // $300/hour
        cumulative_window_s: 3_600,
    }
}

fn unlocked_only() -> VerificationState {
    VerificationState { device_unlocked: true, app_secret_age_s: None }
}
fn pin_just_entered() -> VerificationState {
    VerificationState { device_unlocked: true, app_secret_age_s: Some(5) }
}
fn locked() -> VerificationState {
    VerificationState { device_unlocked: false, app_secret_age_s: None }
}

#[test]
fn a_coffee_needs_nothing() {
    let p = users_policy();
    assert_eq!(p.required(300, 0), Verification::None);
    assert!(check_verification(&p, &locked(), 300, 0, true).is_ok());
}

#[test]
fn fifty_dollars_needs_the_phone_unlocked() {
    let p = users_policy();
    assert_eq!(p.required(5_000, 0), Verification::DeviceUnlocked);
    assert!(check_verification(&p, &unlocked_only(), 5_000, 0, true).is_ok());

    let needed = check_verification(&p, &locked(), 5_000, 0, true).unwrap_err();
    assert_eq!(needed.required, Verification::DeviceUnlocked);
    assert_eq!(needed.satisfied, Verification::None);
}

/// The distinction that matters. A thief holding an unlocked phone satisfies
/// "device unlocked" trivially — that is a passive fact about the handset, not
/// evidence about the person. Above the top threshold they must produce
/// something they know.
#[test]
fn a_hundred_dollars_needs_a_secret_the_thief_does_not_have() {
    let p = users_policy();
    assert_eq!(p.required(10_000, 0), Verification::AppSecret);

    let needed = check_verification(&p, &unlocked_only(), 10_000, 0, true).unwrap_err();
    assert_eq!(needed.required, Verification::AppSecret);
    assert_eq!(
        needed.satisfied,
        Verification::DeviceUnlocked,
        "an unlocked phone must not satisfy a knowledge factor"
    );

    assert!(check_verification(&p, &pin_just_entered(), 10_000, 0, true).is_ok());
}

/// "Deliberate" decays into "happened at some point today" without a window.
#[test]
fn an_old_secret_stops_counting() {
    let p = users_policy();
    let stale = VerificationState {
        device_unlocked: true,
        app_secret_age_s: Some(p.app_secret_validity_s + 1),
    };
    assert_eq!(stale.satisfied(p.app_secret_validity_s), Verification::DeviceUnlocked);
    assert!(check_verification(&p, &stale, 10_000, 0, true).is_err());

    let fresh = VerificationState {
        device_unlocked: true,
        app_secret_age_s: Some(p.app_secret_validity_s),
    };
    assert_eq!(fresh.satisfied(p.app_secret_validity_s), Verification::AppSecret);
}

/// A per-transaction limit alone does not stop twenty payments just under it,
/// which is how a lifted phone is actually drained.
#[test]
fn velocity_catches_what_a_per_payment_limit_misses() {
    let p = users_policy();
    // Each payment is below the app-secret threshold on its own.
    assert_eq!(p.required(9_000, 0), Verification::DeviceUnlocked);
    // But once the hour's running total crosses the cumulative line, it escalates.
    assert_eq!(p.required(9_000, 25_000), Verification::AppSecret);

    // The payment that crosses the line is itself caught, rather than being the
    // last one to slip through.
    assert_eq!(p.required(5_000, 25_000), Verification::AppSecret);
}

/// §17.7's rate can go stale. Thresholds are denominated in real money, so
/// without a trustworthy rate the client cannot tell which rung it is on.
///
/// Failing toward *less* verification would let anyone who can stall a rate
/// feed lower the security requirement — turning a liveness problem into a
/// security one.
#[test]
fn a_stale_exchange_rate_escalates_rather_than_relaxes() {
    let p = users_policy();
    // A trivial amount that would normally need nothing at all.
    assert_eq!(p.required(100, 0), Verification::None);
    let needed = check_verification(&p, &unlocked_only(), 100, 0, false).unwrap_err();
    assert_eq!(
        needed.required,
        Verification::AppSecret,
        "an unknown rate must demand more, never less"
    );
    // And it is still satisfiable — escalation must not be a dead end.
    assert!(check_verification(&p, &pin_just_entered(), 100, 0, false).is_ok());
}

/// An inverted ladder would ask for less as the amount grows.
#[test]
fn thresholds_must_ascend() {
    let mut p = users_policy();
    p.app_secret_at = p.device_unlock_at - 1;
    assert_eq!(p.validate().unwrap_err().code, RejectCode::PolicyRefused);

    p = users_policy();
    p.app_secret_validity_s = 0;
    assert_eq!(p.validate().unwrap_err().code, RejectCode::PolicyRefused);

    assert!(users_policy().validate().is_ok());
    assert!(VerificationPolicy::default().validate().is_ok());
}

/// A user who never opens settings should still be protected.
#[test]
fn the_default_policy_is_conservative() {
    let p = VerificationPolicy::default();
    assert_eq!(p.required(10_000, 0), Verification::AppSecret, "$100 untrusted by default");
    assert_eq!(p.required(2_000, 0), Verification::DeviceUnlocked, "$20 needs unlock");
    assert!(p.cumulative_at > 0, "an unset velocity limit is no velocity limit");
}

/// The ordering is load-bearing: `satisfied >= required` decides everything, so
/// a mis-ordered enum would silently accept a weaker tier.
#[test]
fn stronger_verification_satisfies_weaker_requirements() {
    assert!(Verification::AppSecret > Verification::DeviceUnlocked);
    assert!(Verification::DeviceUnlocked > Verification::None);

    let p = users_policy();
    // Someone who entered a PIN can also make a small payment without being
    // asked again.
    assert!(check_verification(&p, &pin_just_entered(), 100, 0, true).is_ok());
}
