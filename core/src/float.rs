//! Hot-wallet float sizing (§17.2), and the exposure it forces (O9).
//!
//! O9 says a float on a phone is malware- and seizure-reachable, "mitigated by
//! keeping it small". True, and incomplete in a way that matters: **the float
//! has a floor, and the floor is set by how the user wants to transact rather
//! than by how much risk they want.**
//!
//! §17.2 established that consecutive payment capacity is a **count of unlocked
//! outputs**, not a balance — change returns locked for ten blocks, so each
//! payment costs a whole output. Wanting *k* payments before a top-up therefore
//! means holding at least *k* outputs, and the drain test showed a payment can
//! consume more than one (6 outputs bought 4 payments). So exposure is bounded
//! below by `k × typical payment × slack`, and "keep it small" collides with
//! "be able to buy lunch".
//!
//! §4.4's hardware reserve is what makes this tolerable: the floor applies to the
//! float, and the reserve sits behind a device seed. What this module does is
//! make the number explicit, because a user cannot make that trade against a
//! quantity nobody has computed for them.

/// Empirical slack from `sim --drain`: six unlocked outputs bought four
/// consecutive payments, so roughly 1.5 outputs are consumed per payment.
///
/// Not a safety margin someone chose — a measurement. Input selection belongs to
/// the wallet, which may spend two outputs to cover a fee or to consolidate, and
/// the client does not control it.
pub const OUTPUTS_PER_PAYMENT: f64 = 1.5;

/// What a float must hold to support `payments` consecutive spends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FloatPlan {
    /// Outputs to pre-split into at load time.
    pub outputs: u32,
    /// Total piconero committed — and therefore the amount exposed on the phone.
    pub total_pxmr: u64,
}

/// Size a float for `payments` consecutive spends of about `typical_pxmr`.
///
/// Returns the plan **and thereby the minimum exposure**: there is no way to hold
/// less and still make that many payments, so a client offering "how much risk
/// are you comfortable with?" without showing this is offering a choice the
/// protocol does not actually provide.
pub fn plan(payments: u32, typical_pxmr: u64) -> FloatPlan {
    let outputs = ((payments as f64) * OUTPUTS_PER_PAYMENT).ceil() as u32;
    let outputs = outputs.max(1);
    FloatPlan {
        outputs,
        total_pxmr: (outputs as u64).saturating_mul(typical_pxmr),
    }
}

/// The other direction: how many payments a given exposure buys.
///
/// **A bound, never a promise.** §17.2 forbids telling a user "4 more payments"
/// because input selection is the wallet's decision; "about 4" is honest.
pub fn payments_supported(unlocked_outputs: u32) -> u32 {
    ((unlocked_outputs as f64) / OUTPUTS_PER_PAYMENT).floor() as u32
}

/// Whether a stated risk appetite can support a stated usage pattern.
///
/// Returns `Err` with the shortfall when it cannot. This exists because the two
/// numbers are usually set in different places by different reasoning — a
/// security setting and a convenience setting — and nothing otherwise notices
/// that they contradict each other until the user is at a counter.
pub fn reconcile(
    max_exposure_pxmr: u64,
    payments: u32,
    typical_pxmr: u64,
) -> Result<FloatPlan, u64> {
    let p = plan(payments, typical_pxmr);
    if p.total_pxmr > max_exposure_pxmr {
        return Err(p.total_pxmr - max_exposure_pxmr);
    }
    Ok(p)
}
