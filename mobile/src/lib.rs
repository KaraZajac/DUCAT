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

pub mod ceremony;
pub mod contacts;
pub mod monero;
pub mod node;

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
    /// The name handed out on cards (§7.5). A restored persona without it
    /// hands out cards nobody recognises.
    pub display_name: Option<String>,
    /// Whether contacts may pay without asking (§16.12). A privacy setting, so
    /// it travels rather than silently reverting to a default the user did not
    /// pick — in either direction.
    pub publish_payto: bool,
    /// §16.9's profile. Optional throughout, and carried so a restore does not
    /// quietly drop what someone chose to publish about themselves.
    pub profile: crate::contacts::Profile,
    /// §16.12's relationships, typed so another client can restore them.
    pub contacts: Vec<ContactBackup>,
    /// §16.11's store. The forward-secrecy trade is stated on the core field.
    pub prekey_signed_secret: Option<Vec<u8>>,
    pub prekey_one_time: Vec<PrekeyEntry>,
    pub prekey_next_id: u64,
    /// Same-client continuity (threads, tabs); opaque, no interop promise.
    pub app_state: Option<Vec<u8>>,
    /// §4.3.3's open escrows. The one part of a bundle with a freshness
    /// requirement, and the one whose absence costs money rather than
    /// convenience: on the two-party rung a lost share is an escrow that can
    /// never be released, by anyone, ever.
    pub escrow_shares: Vec<EscrowShareEntry>,
}

/// One relationship, across the bridge.
#[derive(uniffi::Record, Clone, Default)]
pub struct ContactBackup {
    pub persona: Vec<u8>,
    pub my_outbox_key: String,
    pub my_outbox_owner_public: Vec<u8>,
    pub my_outbox_owner_secret: Vec<u8>,
    pub their_outbox_key: String,
    pub their_bundle: Option<Vec<u8>>,
    pub their_payto: Option<String>,
    pub petname: Option<String>,
    pub asserted_name: Option<String>,
    pub in_seq: u64,
    pub out_seq: u64,
    pub in_prev: Option<Vec<u8>>,
    pub out_prev: Option<Vec<u8>>,
}

#[derive(uniffi::Record, Clone)]
pub struct PrekeyEntry {
    pub id: u64,
    pub secret: Vec<u8>,
}

/// One open escrow's membership, across the bridge (§4.3.3).
///
/// `share` is this client's whole ceremony record — the FROST key package and
/// the state around it that says which escrow it belongs to and how far it
/// got. Opaque by design, exactly as the core field describes: another client
/// implementing the same protocol restores its own shape from its own export,
/// and nothing here promises interop on the bytes.
///
/// The height is the same asymmetry as the wallet's, for the same reason: a
/// restored share that starts scanning after the funding transaction reports an
/// empty escrow, which looks identical to one that was never funded.
#[derive(uniffi::Record, Clone)]
pub struct EscrowShareEntry {
    pub escrow_id: Vec<u8>,
    pub share: Vec<u8>,
    pub restore_height: u64,
}

fn contact_to_core(c: &ContactBackup) -> ducat_core::backup::BackupContact {
    ducat_core::backup::BackupContact {
        persona: c.persona.clone(),
        my_outbox_key: c.my_outbox_key.clone(),
        my_outbox_owner_public: c.my_outbox_owner_public.clone(),
        my_outbox_owner_secret: c.my_outbox_owner_secret.clone(),
        their_outbox_key: c.their_outbox_key.clone(),
        their_bundle: c.their_bundle.clone(),
        their_payto: c.their_payto.clone(),
        petname: c.petname.clone(),
        asserted_name: c.asserted_name.clone(),
        in_seq: c.in_seq,
        out_seq: c.out_seq,
        in_prev: c.in_prev.clone(),
        out_prev: c.out_prev.clone(),
    }
}

fn contact_from_core(c: &ducat_core::backup::BackupContact) -> ContactBackup {
    ContactBackup {
        persona: c.persona.clone(),
        my_outbox_key: c.my_outbox_key.clone(),
        my_outbox_owner_public: c.my_outbox_owner_public.clone(),
        my_outbox_owner_secret: c.my_outbox_owner_secret.clone(),
        their_outbox_key: c.their_outbox_key.clone(),
        their_bundle: c.their_bundle.clone(),
        their_payto: c.their_payto.clone(),
        petname: c.petname.clone(),
        asserted_name: c.asserted_name.clone(),
        in_seq: c.in_seq,
        out_seq: c.out_seq,
        in_prev: c.in_prev.clone(),
        out_prev: c.out_prev.clone(),
    }
}

/// Roughly four centuries of two-minute blocks. Anything beyond this is not a
/// height, it is a sentinel or a mistake — and both restore to nothing.
const MAX_PLAUSIBLE_HEIGHT: u64 = 100_000_000;

#[derive(uniffi::Error, Debug, thiserror::Error)]
pub enum BackupError {
    #[error("restore height is above any plausible chain height")]
    ImplausibleRestoreHeight,
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
    // **A restore height above the chain is the silent, total failure.**
    //
    // §4.3.1: too low costs ~106 hours of rescan; too high means the wallet
    // scans forward from after every output it owns, finds nothing, and reports
    // a zero balance with no error anywhere. The app's first version passed 0
    // (genesis — expensive), so it was "fixed" with a u64::MAX sentinel, which
    // is the catastrophic direction. A phone-written backup carrying
    // 18446744073709551615 was opened here and proved it.
    //
    // Genesis is slow and recoverable. A height above the tip is fast and
    // unrecoverable. Between those, refuse the second.
    if input.restore_height > MAX_PLAUSIBLE_HEIGHT {
        return Err(BackupError::ImplausibleRestoreHeight);
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
        // §4.3.3, and the reason the backup screen talks about freshness at
        // all. This was `vec![]` while the screen said an escrow needs a newer
        // bundle — so the screen was asking people to re-export for something
        // the export then threw away. On the three-party rung the other two
        // can still release without the lost share; on the two-party rung
        // nobody can, and the deposit is gone for good.
        escrow_shares: input
            .escrow_shares
            .iter()
            .map(|e| ducat_core::backup::EscrowShare {
                escrow_id: e.escrow_id.clone(),
                key_file: e.share.clone(),
                restore_height: e.restore_height,
            })
            .collect(),
        // Carried through from the caller: these are the user's own settings,
        // and a backup that quietly drops them restores a persona that has
        // forgotten its name and its mind about being paid.
        display_name: input.display_name.clone(),
        publish_payto: input.publish_payto,
        avatar: input.profile.avatar.clone(),
        email: input.profile.email.clone(),
        phone: input.profile.phone.clone(),
        signal: input.profile.signal.clone(),
        pronouns: input.profile.pronouns.map(|p| p as u64),
        contacts: input.contacts.iter().map(contact_to_core).collect(),
        prekey_signed_secret: input.prekey_signed_secret.clone(),
        prekey_one_time: input
            .prekey_one_time
            .iter()
            .map(|e| (e.id, e.secret.clone()))
            .collect(),
        prekey_next_id: input.prekey_next_id,
        app_state: input.app_state.clone(),
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
/// A desk's store key, from a passphrase.
///
/// The phone keeps its secrets in EncryptedSharedPreferences with a key the
/// Android Keystore holds and never hands over. A desktop has no such box, so
/// the desk derives its key from a passphrase the operator types — same
/// Argon2id parameters as §4.3's backup, domain-separated so the two keys are
/// unrelated even when the passphrase is the same.
///
/// `salt` is stored beside the data in the clear, which is correct: a salt is
/// not a secret, and reusing one would be.
#[uniffi::export]
pub fn vault_key(passphrase: String, salt: Vec<u8>) -> Result<Vec<u8>, BackupError> {
    let salt: [u8; 16] = salt
        .try_into()
        .map_err(|_| BackupError::Failed("a vault salt is 16 bytes".into()))?;
    ducat_core::backup::derive_for(b"DUCAT-DESK-VAULT-v1", passphrase.as_bytes(), &salt)
        .map(|k| k.to_vec())
        .map_err(|e| BackupError::Failed(format!("vault key: {e:?}")))
}

/// Sixteen fresh bytes, for a salt or a nonce the caller stores in the clear.
#[uniffi::export]
pub fn random_bytes(n: u32) -> Vec<u8> {
    use rand_core::{OsRng, RngCore};
    let mut b = vec![0u8; n as usize];
    OsRng.fill_bytes(&mut b);
    b
}

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
            BackupInput { spend_key_hex: w.spend_key_hex.clone(), restore_height: w.restore_height, display_name: None, publish_payto: false, profile: Default::default(), contacts: vec![], prekey_signed_secret: None, prekey_one_time: vec![], prekey_next_id: 0, app_state: None },
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
                BackupInput { spend_key_hex: w.spend_key_hex, restore_height: 1, display_name: None, publish_payto: false, profile: Default::default(), contacts: vec![], prekey_signed_secret: None, prekey_one_time: vec![], prekey_next_id: 0, app_state: None },
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
                BackupInput { spend_key_hex: "nothex".into(), restore_height: 1, display_name: None, publish_payto: false, profile: Default::default(), contacts: vec![], prekey_signed_secret: None, prekey_one_time: vec![], prekey_next_id: 0, app_state: None },
                "a real passphrase".into(),
                create_persona_secret(),
            ),
            Err(BackupError::BadKey)
        ));
    }
}

#[cfg(test)]
mod restore_height_tests {
    use super::*;

    /// The direction that loses money silently.
    #[test]
    fn a_height_above_the_chain_is_refused() {
        let w = create_wallet(1, true);
        assert!(matches!(
            export_backup(
                BackupInput { spend_key_hex: w.spend_key_hex.clone(), restore_height: u64::MAX, display_name: None, publish_payto: false, profile: Default::default(), contacts: vec![], prekey_signed_secret: None, prekey_one_time: vec![], prekey_next_id: 0, app_state: None },
                "a real passphrase".into(),
                create_persona_secret(),
            ),
            Err(BackupError::ImplausibleRestoreHeight)
        ));
    }

    /// Genesis is slow and recoverable, so it is allowed. §4.3.1's two
    /// directions are not symmetric and the API must not treat them as if they
    /// were.
    #[test]
    fn genesis_is_slow_but_permitted() {
        let w = create_wallet(0, true);
        assert!(export_backup(
            BackupInput { spend_key_hex: w.spend_key_hex, restore_height: 0, display_name: None, publish_payto: false, profile: Default::default(), contacts: vec![], prekey_signed_secret: None, prekey_one_time: vec![], prekey_next_id: 0, app_state: None },
            "a real passphrase".into(),
            create_persona_secret(),
        )
        .is_ok());
    }
}

// ---------------------------------------------------------------------------
// §4.3 — importing one back
// ---------------------------------------------------------------------------

/// What a bundle restores.
#[derive(uniffi::Record)]
pub struct RestoredBackup {
    pub spend_key_hex: String,
    pub restore_height: u64,
    pub persona_secret: Vec<u8>,
    pub display_name: Option<String>,
    pub publish_payto: bool,
    /// §16.9's profile, restored with everything else. A persona that comes
    /// back without its face and its pronouns is not the same person to anyone
    /// who knew them.
    pub profile: crate::contacts::Profile,
    pub contacts: Vec<ContactBackup>,
    pub prekey_signed_secret: Option<Vec<u8>>,
    pub prekey_one_time: Vec<PrekeyEntry>,
    pub prekey_next_id: u64,
    pub app_state: Option<Vec<u8>>,
    /// Escrow shares carried in the bundle (§4.3.3). Zero is the normal case.
    pub escrow_count: u32,
    /// The shares themselves. This used to be the count alone, which told a
    /// restoring device how much it had just failed to restore.
    pub escrow_shares: Vec<EscrowShareEntry>,
}

/// Open a bundle.
///
/// A wrong passphrase and a tampered file are the same error, deliberately: the
/// AEAD cannot distinguish them, and reporting them differently would tell an
/// attacker whether a guess was close.
#[uniffi::export]
pub fn import_backup(blob: Vec<u8>, passphrase: String) -> Result<RestoredBackup, BackupError> {
    let b = ducat_core::backup::import(&blob, passphrase.as_bytes())
        .map_err(|e| BackupError::Failed(format!("{:?}", e.code)))?;
    Ok(RestoredBackup {
        display_name: b.display_name.clone(),
        publish_payto: b.publish_payto,
        profile: crate::contacts::Profile {
            avatar: b.avatar.clone(),
            email: b.email.clone(),
            phone: b.phone.clone(),
            signal: b.signal.clone(),
            pronouns: b.pronouns.map(|p| p as u32),
            // The car does not ride the typed backup fields (it is profile
            // presentation, restored via app_state like the rest of MyProfile).
            car_model: None,
            car_color: None,
            plate: None,
        },
        contacts: b.contacts.iter().map(contact_from_core).collect(),
        prekey_signed_secret: b.prekey_signed_secret.clone(),
        prekey_one_time: b
            .prekey_one_time
            .iter()
            .map(|(id, sk)| PrekeyEntry { id: *id, secret: sk.clone() })
            .collect(),
        prekey_next_id: b.prekey_next_id,
        app_state: b.app_state.clone(),
        spend_key_hex: b.monero_seed,
        restore_height: b.monero_restore_height,
        persona_secret: b.persona_secret,
        escrow_count: b.escrow_shares.len() as u32,
        escrow_shares: b
            .escrow_shares
            .iter()
            .map(|e| EscrowShareEntry {
                escrow_id: e.escrow_id.clone(),
                share: e.key_file.clone(),
                restore_height: e.restore_height,
            })
            .collect(),
    })
}

/// The address a restored key controls, so a user can confirm they restored what
/// they meant to before trusting it with anything.
#[uniffi::export]
pub fn address_for_spend_key(spend_key_hex: String, stagenet: bool) -> Result<String, BackupError> {
    use curve25519_dalek::constants::ED25519_BASEPOINT_TABLE;
    use monero_wallet::address::Network;
    use monero_wallet::ed25519::Scalar;
    use zeroize::Zeroizing;

    let bytes = hex_to_bytes(&spend_key_hex).ok_or(BackupError::BadKey)?;
    if bytes.len() != 32 {
        return Err(BackupError::BadKey);
    }
    let spend = Zeroizing::new(
        Scalar::read(&mut bytes.as_slice()).map_err(|_| BackupError::BadKey)?,
    );
    let mut sb = Vec::new();
    spend.write(&mut sb).map_err(|_| BackupError::BadKey)?;
    let view = Zeroizing::new(Scalar::hash(&sb));
    let spend_pub = monero_wallet::ed25519::Point::from(&(*spend).into() * ED25519_BASEPOINT_TABLE);
    let vp = monero_wallet::ViewPair::new(spend_pub, view)
        .map_err(|_| BackupError::BadKey)?;
    let network = if stagenet { Network::Stagenet } else { Network::Mainnet };
    Ok(vp.legacy_address(network).to_string())
}

#[cfg(test)]
mod import_tests {
    use super::*;

    /// The round trip that matters: the restored key must control the same
    /// address, or the backup restored *something* and not the wallet.
    #[test]
    fn a_restored_key_controls_the_same_address() {
        let w = create_wallet(1000, true);
        let blob = export_backup(
            BackupInput { spend_key_hex: w.spend_key_hex.clone(), restore_height: 1000, display_name: None, publish_payto: false, profile: Default::default(), contacts: vec![], prekey_signed_secret: None, prekey_one_time: vec![], prekey_next_id: 0, app_state: None },
            "a real passphrase".into(),
            create_persona_secret(),
        )
        .unwrap();
        let r = import_backup(blob, "a real passphrase".into()).unwrap();
        assert_eq!(r.spend_key_hex, w.spend_key_hex);
        assert_eq!(
            address_for_spend_key(r.spend_key_hex, true).unwrap(),
            w.address,
            "a restored key that controls a different address has restored nothing"
        );
    }

    #[test]
    fn a_wrong_passphrase_is_indistinguishable_from_tampering() {
        let w = create_wallet(1, true);
        let blob = export_backup(
            BackupInput { spend_key_hex: w.spend_key_hex, restore_height: 1, display_name: None, publish_payto: false, profile: Default::default(), contacts: vec![], prekey_signed_secret: None, prekey_one_time: vec![], prekey_next_id: 0, app_state: None },
            "a real passphrase".into(),
            create_persona_secret(),
        )
        .unwrap();
        let wrong = import_backup(blob.clone(), "not the passphrase".into())
            .err()
            .map(|e| e.to_string());
        let mut torn = blob;
        let n = torn.len() - 1;
        torn[n] ^= 1;
        let tampered = import_backup(torn, "a real passphrase".into())
            .err()
            .map(|e| e.to_string());
        assert!(wrong.is_some() && wrong == tampered, "{wrong:?} vs {tampered:?}");
    }
}

#[cfg(test)]
mod subaddress_tests {
    /// A subaddress is real or it is theatre: stagenet subaddresses start
    /// with 7, mainnet with 8, and minor 0 is refused as the primary.
    #[test]
    fn per_contact_addresses_derive() {
        let w = crate::create_wallet(0, true);
        let s1 = crate::monero::monero_subaddress(w.spend_key_hex.clone(), 1, true).unwrap();
        let s2 = crate::monero::monero_subaddress(w.spend_key_hex.clone(), 2, true).unwrap();
        assert!(s1.starts_with('7'), "stagenet subaddress: {s1}");
        assert_ne!(s1, s2, "two contacts, two addresses");
        assert_ne!(s1, w.address, "a subaddress is not the primary");
        assert!(crate::monero::monero_subaddress(w.spend_key_hex, 0, true).is_err());
    }
}
