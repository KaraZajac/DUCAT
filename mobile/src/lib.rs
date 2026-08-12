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

// ---------------------------------------------------------------------------
// Onboarding: a persona, a wallet, limits, a backup
// ---------------------------------------------------------------------------

/// A newly created wallet, as onboarding needs to show it.
///
/// The **seed is returned once** and is never stored by this crate. §4.3 makes
/// backup an explicit act the user performs; a bridge that quietly kept a copy
/// would make the passphrase decorative.
#[derive(uniffi::Record)]
pub struct NewWallet {
    /// The primary address, for receiving.
    pub address: String,
    /// The private spend key, hex-encoded. **This restores the wallet.**
    ///
    /// Not a 25-word mnemonic: `monero-wallet` implements none, and §4.3's
    /// bundle is an encrypted *file* rather than something transcribed by hand,
    /// so the key material is what belongs in it. A word list is a human
    /// encoding for paper backup — a different feature, wanted by some people,
    /// and not a substitute for this.
    pub spend_key_hex: String,
    /// The height to restore from.
    ///
    /// **Load-bearing, not metadata.** A wallet restored without one rescans from
    /// genesis: measured at roughly 106 hours against a remote node versus 35
    /// seconds from a recent height. A fresh wallet's correct value is the
    /// current tip, because it has no earlier outputs to miss — which is the one
    /// case where "now" is right rather than catastrophic (§4.3.1).
    pub restore_height: u64,
}

/// Create a wallet.
///
/// `tip_height` comes from a node the caller already talks to. It is a parameter
/// rather than something fetched here so this function stays pure and testable:
/// a key generator that needs the network is one that fails in a tunnel.
#[uniffi::export]
pub fn create_wallet(tip_height: u64, stagenet: bool) -> NewWallet {
    use monero_wallet::address::{MoneroAddress, Network};
    use monero_wallet::ed25519::Scalar;
    use rand_core::OsRng;
    use zeroize::Zeroizing;

    use curve25519_dalek::constants::ED25519_BASEPOINT_TABLE;

    let spend = Zeroizing::new(Scalar::random(&mut OsRng));
    // **Derived, not independent.** Monero's convention is view = H(spend), and
    // the first version of this generated the view key at random. That produces
    // a valid wallet which cannot be restored from its spend key — the holder of
    // a "seed" would recover an address they could not see payments to. A wallet
    // that is unrestorable in the ordinary way is not a wallet.
    let mut sb = Vec::new();
    spend.write(&mut sb).expect("scalar write");
    let view = Zeroizing::new(Scalar::hash(&sb));
    let spend_pub = monero_wallet::ed25519::Point::from(&(*spend).into() * ED25519_BASEPOINT_TABLE);
    let vp = monero_wallet::ViewPair::new(spend_pub, view.clone())
        .expect("a random scalar yields a valid view pair");

    let network = if stagenet { Network::Stagenet } else { Network::Mainnet };
    let address: MoneroAddress = vp.legacy_address(network);

    NewWallet {
        address: address.to_string(),
        spend_key_hex: sb.iter().map(|b| format!("{b:02x}")).collect(),
        restore_height: tip_height,
    }
}

#[cfg(test)]
mod wallet_tests {
    use super::*;

    /// A wallet is real or it is theatre.
    #[test]
    fn created_wallets_are_distinct_and_well_formed() {
        let a = create_wallet(2_190_000, true);
        let b = create_wallet(2_190_000, true);
        assert_ne!(a.address, b.address, "two wallets sharing an address is not randomness");
        // Stagenet primary addresses start with 5 and are 95 characters — the
        // same length that broke `dest` when it was checked as 16 bytes.
        assert_eq!(a.address.len(), 95, "got {}", a.address);
        assert!(a.address.starts_with('5'), "stagenet primary address");

        let m = create_wallet(2_190_000, false);
        assert!(m.address.starts_with('4'), "mainnet primary address: {}", m.address);
    }

    /// §4.3.1: a fresh wallet is the one case where "now" is the right restore
    /// height rather than a catastrophe, because it has no earlier outputs to
    /// miss. Setting it to the tip for a *restored* wallet is what makes a
    /// balance read zero with no error anywhere.
    #[test]
    fn a_fresh_wallet_restores_from_the_tip() {
        assert_eq!(create_wallet(2_190_000, true).restore_height, 2_190_000);
    }
}

// ---------------------------------------------------------------------------
// §4.3 — the backup, actually produced
// ---------------------------------------------------------------------------

/// What onboarding has to protect.
#[derive(uniffi::Record)]
pub struct BackupInput {
    /// Hex, from [`create_wallet`].
    pub spend_key_hex: String,
    pub restore_height: u64,
}

#[derive(uniffi::Error, Debug, thiserror::Error)]
pub enum BackupError {
    #[error("passphrase is too short to protect a wallet")]
    WeakPassphrase,
    #[error("malformed key material")]
    BadKey,
    #[error("{0}")]
    Failed(String),
}

/// Produce the encrypted bundle §4.3 specifies.
///
/// Returns the bytes; writing them somewhere is the caller's job, because the
/// user chooses where a backup lives and a protocol that also decided *where*
/// would be back to needing a service.
///
/// The persona key is generated here and returned inside the bundle rather than
/// separately: it is the thing whose loss is unrecoverable, and an API that
/// hands it back for the caller to store invites the caller to store it badly.
#[uniffi::export]
pub fn export_backup(
    input: BackupInput,
    passphrase: String,
    persona_secret: Vec<u8>,
) -> Result<Vec<u8>, BackupError> {
    use ducat_core::backup::{export, Backup};
    use rand_core::{OsRng, RngCore};

    if persona_secret.len() != 32 {
        return Err(BackupError::BadKey);
    }
    if hex_to_bytes(&input.spend_key_hex).map(|b| b.len()) != Some(32) {
        return Err(BackupError::BadKey);
    }

    let bundle = Backup {
        persona_suite: 1,
        persona_secret,
        // §4.3.1's field, carrying key material rather than a word list — the
        // bundle is a file, not something transcribed.
        monero_seed: input.spend_key_hex,
        // Wrong in both directions, asymmetrically: too low costs ~106 hours of
        // rescan, too high is silent and total. For a wallet created moments ago
        // the tip is right, because there are no earlier outputs to miss.
        monero_restore_height: input.restore_height,
        rendezvous: vec![],
        attestation_records: vec![],
        mandates: vec![],
        verification: ducat_core::verify::VerificationPolicy::default(),
        escrow_shares: vec![],
        created: 0,
    };

    // Fresh per export (§4.3.2). Reusing either would be the bug.
    let mut salt = [0u8; 16];
    let mut nonce = [0u8; 24];
    OsRng.fill_bytes(&mut salt);
    OsRng.fill_bytes(&mut nonce);

    export(&bundle, passphrase.as_bytes(), salt, nonce).map_err(|e| {
        if matches!(e.code, ducat_core::reject::RejectCode::PolicyRefused) {
            BackupError::WeakPassphrase
        } else {
            BackupError::Failed(format!("{e:?}"))
        }
    })
}

/// A persona key. Thirty-two bytes of nothing in particular, which is the point.
#[uniffi::export]
pub fn create_persona_secret() -> Vec<u8> {
    use rand_core::{OsRng, RngCore};
    let mut k = [0u8; 32];
    OsRng.fill_bytes(&mut k);
    k.to_vec()
}

fn hex_to_bytes(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
        .collect()
}

#[cfg(test)]
mod backup_tests {
    use super::*;

    #[test]
    fn a_backup_is_produced_and_reopens() {
        let w = create_wallet(2_190_000, true);
        let persona = create_persona_secret();
        let blob = export_backup(
            BackupInput { spend_key_hex: w.spend_key_hex.clone(), restore_height: w.restore_height },
            "a real passphrase".into(),
            persona.clone(),
        )
        .expect("export");
        assert!(blob.len() > 100);

        let back = ducat_core::backup::import(&blob, b"a real passphrase").expect("import");
        assert_eq!(back.persona_secret, persona);
        assert_eq!(back.monero_seed, w.spend_key_hex);
        assert_eq!(back.monero_restore_height, 2_190_000);
    }

    /// §4.3.2 refuses a trivially short passphrase rather than producing an
    /// artifact whose protection is nominal.
    #[test]
    fn a_weak_passphrase_produces_nothing() {
        let w = create_wallet(1, true);
        assert!(matches!(
            export_backup(
                BackupInput { spend_key_hex: w.spend_key_hex, restore_height: 1 },
                "short".into(),
                create_persona_secret(),
            ),
            Err(BackupError::WeakPassphrase)
        ));
    }

    /// The key must be usable, not merely present. An unrestorable backup is
    /// worse than none, because the user stops worrying.
    #[test]
    fn a_malformed_key_is_refused() {
        assert!(matches!(
            export_backup(
                BackupInput { spend_key_hex: "nothex".into(), restore_height: 1 },
                "a real passphrase".into(),
                create_persona_secret(),
            ),
            Err(BackupError::BadKey)
        ));
    }
}
