//! Persona and wallet backup — export and import, under the user's passphrase.
//!
//! O12 said losing a device loses the persona and every rendezvous keyed to it.
//! For a payment application that is disqualifying, and the usual crypto answer
//! — a seed phrase the user must transcribe and store — is exactly the friction
//! that makes people avoid this category of software.
//!
//! This is deliberately not social recovery, not custody, and not a service.
//! It is one encrypted artifact the user exports and keeps, and can import
//! anywhere. Nothing is uploaded; there is no server to ask.
//!
//! # A conflict this resolved
//!
//! §4.1 recommended hardware-backing persona keys wherever available. Hardware
//! backing means the key **cannot be exported** — that is its purpose — so the
//! recommendation quietly made personas unbackupable, and a lost phone
//! unrecoverable no matter what this module did.
//!
//! The resolution is to split by *replaceability*:
//!
//! - **Persona keys are software and exportable.** They are irreplaceable:
//!   losing one destroys every persistent contact (§16) and the ability to keep
//!   accruing reputation (§9.2). The protection is the passphrase.
//! - **Device keys stay hardware-backed and are never exported** (§4.2). They
//!   are replaceable: a restored persona simply revokes the lost device's
//!   delegation and issues a new one.
//!
//! Hardware backing where it is disposable, exportability where it is not.
//!
//! # What this is not
//!
//! This bundle holds **credentials**, not **records**. It restores who the user
//! is, what they can spend, who can reach them, and what they have authorised.
//! It does not restore their transaction history — receipts are a separate,
//! continuously growing archive under §7.4, with its own export.
//!
//! Keeping them apart is not only about size. The two have opposite refresh
//! needs: a credential bundle exported a year ago is still completely valid,
//! while a receipt archive from a year ago is a year out of date. Folding them
//! together would force the user to re-export constantly to keep a bundle whose
//! important half never changes.

use argon2::Argon2;
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};

use crate::cbor::{decode, Value};
use crate::reject::{Reject, RejectCode};
use crate::verify::VerificationPolicy;
use std::collections::BTreeMap;

/// Format version, bound as additional authenticated data so a backup cannot be
/// silently reinterpreted under different rules.
pub const BACKUP_VERSION: u64 = 1;

const MAGIC: &[u8] = b"DUCAT-BACKUP-v1";

mod k {
    pub const VERSION: u64 = 0;
    pub const PERSONA_SUITE: u64 = 1;
    pub const PERSONA_SECRET: u64 = 2;
    pub const MONERO_SEED: u64 = 3;
    pub const MONERO_RESTORE_HEIGHT: u64 = 4;
    pub const RENDEZVOUS: u64 = 5;
    pub const MANDATES: u64 = 6;
    pub const CREATED: u64 = 7;
    pub const ATTESTATION_RECORDS: u64 = 8;
    pub const CVM_DEVICE_UNLOCK_AT: u64 = 9;
    pub const CVM_APP_SECRET_AT: u64 = 10;
    pub const CVM_APP_SECRET_VALIDITY_S: u64 = 11;
    pub const CVM_CUMULATIVE_AT: u64 = 12;
    pub const CVM_CUMULATIVE_WINDOW_S: u64 = 13;
    pub const ESCROW_SHARES: u64 = 14;
    pub const DISPLAY_NAME: u64 = 15;
    pub const PUBLISH_PAYTO: u64 = 16;
    // §16.9's profile. Restored because a persona that comes back without its
    // face and its pronouns is not the same person to anyone who knew them.
    pub const AVATAR: u64 = 17;
    pub const EMAIL: u64 = 18;
    pub const PHONE: u64 = 19;
    pub const SIGNAL: u64 = 20;
    pub const PRONOUNS: u64 = 21;
    // §16.12's relationships. A wallet restores money; these restore the
    // people, without whom the money has nobody to go to.
    pub const CONTACTS: u64 = 22;
    pub const PREKEY_SIGNED: u64 = 23;
    pub const PREKEY_ONE_TIME: u64 = 24;
    pub const PREKEY_NEXT_ID: u64 = 25;
    pub const APP_STATE: u64 = 26;
    pub const ESCROW_ID: u64 = 0;
    // Sub-keys of one CONTACTS entry.
    pub const C_PERSONA: u64 = 0;
    pub const C_MY_OUTBOX: u64 = 1;
    pub const C_MY_OWNER_PUB: u64 = 2;
    pub const C_MY_OWNER_SEC: u64 = 3;
    pub const C_THEIR_OUTBOX: u64 = 4;
    pub const C_THEIR_BUNDLE: u64 = 5;
    pub const C_THEIR_PAYTO: u64 = 6;
    pub const C_PETNAME: u64 = 7;
    pub const C_ASSERTED: u64 = 8;
    pub const C_IN_SEQ: u64 = 9;
    pub const C_OUT_SEQ: u64 = 10;
    pub const C_IN_PREV: u64 = 11;
    pub const C_OUT_PREV: u64 = 12;
    pub const ESCROW_KEY_FILE: u64 = 1;
    pub const ESCROW_RESTORE_HEIGHT: u64 = 2;
}

/// One multisig membership, stored as the wallet's own key file (§4.3.3).
///
/// Not a seed. A share is **not** derivable from the wallet seed — measured on
/// v0.18.5.1: two wallets with byte-identical key material produced
/// `prepare_multisig` outputs sharing a 101-character prefix and then diverging
/// for 88 characters of fresh randomness. Restoring the seed does not restore
/// the share, so the share itself has to be carried.
///
/// Carrying it works because restore does not need an RPC method. `.keys` placed
/// in a wallet directory and opened with `open_wallet` yields a wallet reporting
/// `multisig: true, ready: true, threshold 2, total 3` at the correct group
/// address — verified against stagenet. That routes entirely around the missing
/// `restore_multisig_wallet` call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EscrowShare {
    /// Which escrow this belongs to.
    pub escrow_id: Vec<u8>,
    /// The wallet key file, opaque to DUCAT. About 2.3 KB for a 2-of-3 — the
    /// multi-megabyte companion file next to it is scan cache, which rebuilds
    /// itself and must not be backed up.
    pub key_file: Vec<u8>,
    /// Where a restored copy starts scanning. Same rule and the same asymmetry
    /// as `monero_restore_height`.
    pub restore_height: u64,
}

/// Everything a user needs to become themselves again on another device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Backup {
    pub persona_suite: u8,
    pub persona_secret: Vec<u8>,
    /// Monero's 25-word Electrum-style mnemonic, which restores spend and view
    /// keys.
    pub monero_seed: String,
    /// **Load-bearing, and wrong in both directions.**
    ///
    /// Too low is expensive: with no height at all a wallet rescans from
    /// genesis, measured at roughly 106 hours against a remote node versus 35
    /// seconds from a recent one (Phase 0b). The user watches a zero balance for
    /// four days and concludes the software ate their money.
    ///
    /// Too high is *silent and total*. Setting this to the current height at
    /// export — the obvious implementation — makes the restored wallet scan
    /// forward from after every output it owns. Demonstrated against stagenet in
    /// `sim --restore`: correct seed, correct address, zero balance, no error
    /// raised anywhere.
    ///
    /// The rule: **at or below the block holding the oldest unspent output, and
    /// as close to it as possible.** Anything older is already spent, so
    /// skipping it costs nothing but the time it would have taken to scan.
    /// Recompute on each export rather than freezing a creation height, or the
    /// restore gets slower every year for no benefit.
    pub monero_restore_height: u64,
    /// Rendezvous record keys (§16.4). Without these a restored persona keeps
    /// its identity and loses every contact — it can be paid, but nobody it
    /// knows can reach it.
    pub rendezvous: Vec<Vec<u8>>,
    /// Writer keys for the DHT records holding this persona's attestations
    /// (§9.2).
    ///
    /// The persona key alone restores *identity*, not *standing*. Attestations
    /// are receipts signed to the persona and published in records the persona
    /// controls — and control means holding the record's own writer key, which
    /// is not derived from the persona key. Lose it and the attestations remain
    /// visible but frozen: the user can never add another, so their reputation
    /// stops at the moment the device died. That is a slow, silent failure
    /// nobody would attribute to the backup.
    pub attestation_records: Vec<Vec<u8>>,
    /// Mandates this user has granted (§7.3), so revocation survives a restore.
    /// A standing authorisation the user can no longer see is one they cannot
    /// revoke.
    pub mandates: Vec<Vec<u8>>,
    /// The user's verification thresholds (§15.5.1).
    ///
    /// Not a credential, and included anyway. Losing it is not a security
    /// failure — the defaults are stricter than most users' settings, so a
    /// restore without it fails safe — but it is an operational one. A merchant
    /// who raised their floor limit for a high-volume counter and restores to
    /// the default finds their terminal demanding a secret on every sale, with
    /// nothing to explain why. Silently reverting a deliberate setting is its
    /// own kind of data loss.
    ///
    /// Restoring a policy cannot weaken anything a counterparty could exploit:
    /// §15.5.1 keeps verification entirely off the wire, so these thresholds are
    /// only ever the user's own instruction to their own client.
    pub verification: VerificationPolicy,
    /// Multisig memberships for escrows that are currently open (§4.3.3).
    ///
    /// The one part of this bundle with a *freshness* requirement. Everything
    /// else stays valid indefinitely — a persona key from last year is still
    /// the persona. An escrow share exists only for the life of one escrow, so
    /// a bundle exported before an escrow opened does not contain it, and a
    /// bundle is only as useful as its most recent export.
    ///
    /// Captured when the ceremony reports `ready`, never before: a half-formed
    /// multisig restores as a half-formed multisig, which is the stranded state
    /// §8.2 already warns about.
    pub escrow_shares: Vec<EscrowShare>,
    /// The name this persona hands out on cards (§7.5).
    ///
    /// Not a credential and included anyway, for the same reason as the
    /// verification thresholds above: a restored persona that has forgotten its
    /// own name hands out cards nobody recognises, and the user has no way to
    /// know that is what happened.
    pub display_name: Option<String>,
    /// Whether this persona publishes an address so contacts can pay without
    /// asking (§16.12).
    ///
    /// A **privacy** setting, so restoring it wrong is worse than losing it.
    /// Defaulting to on would silently publish an address for someone who had
    /// deliberately kept it private; the decode therefore treats absence as
    /// off, which is the safe direction and the original default.
    pub publish_payto: bool,
    /// §16.9's profile, so a restore is the same person rather than a stranger
    /// with the same keys.
    ///
    /// These are **not** re-validated on the way out of a backup: they were
    /// checked when they were entered and when they were published, and a
    /// bundle that fails to open because an old email no longer parses is a
    /// wallet held hostage to a formatting rule. Publishing them again puts
    /// them through the decoder that does check.
    pub avatar: Option<Vec<u8>>,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub signal: Option<String>,
    pub pronouns: Option<u64>,
    /**
     * The relationships (§16.12), typed so a *different* client can restore
     * them: each entry carries the persona, both outbox keys, our outbox's
     * owner keypair — without which the log is readable and unwritable, the
     * exact stranding measured in the field — their cached prekey bundle, and
     * the chain counters, without which the next message in either direction
     * is refused as out of order.
     */
    pub contacts: Vec<BackupContact>,
    /**
     * Our prekey store (§16.11), and the trade is stated rather than implied:
     * **a backup holding one-time secrets can rewind forward secrecy to the
     * moment it was made.** Delete-on-use is the entire property, and a copy
     * that predates the use undoes the delete for anyone holding the file.
     * They are included anyway, because the alternative strands every message
     * sealed to them in flight — an availability failure certain to happen at
     * exactly the moment of device loss — and because §4.3 already names the
     * bundle a complete spending credential, so the marginal exposure rides a
     * file that must already be guarded absolutely.
     */
    pub prekey_signed_secret: Option<Vec<u8>>,
    pub prekey_one_time: Vec<(u64, Vec<u8>)>,
    pub prekey_next_id: u64,
    /**
     * Same-client continuity — threads, tabs, presentation — as one opaque
     * blob. Deliberately untyped: its contents are implementation-defined and
     * carry no interop promise, which is what keeps the typed fields above an
     * honest list of what another client needs.
     */
    pub app_state: Option<Vec<u8>>,
    pub created: u64,
}

/// One relationship, as another implementation would need it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackupContact {
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

impl BackupContact {
    fn to_value(&self) -> Value {
        let mut m = BTreeMap::new();
        m.insert(k::C_PERSONA, Value::Bytes(self.persona.clone()));
        m.insert(k::C_MY_OUTBOX, Value::Text(self.my_outbox_key.clone()));
        m.insert(k::C_MY_OWNER_PUB, Value::Bytes(self.my_outbox_owner_public.clone()));
        m.insert(k::C_MY_OWNER_SEC, Value::Bytes(self.my_outbox_owner_secret.clone()));
        m.insert(k::C_THEIR_OUTBOX, Value::Text(self.their_outbox_key.clone()));
        if let Some(b) = &self.their_bundle {
            m.insert(k::C_THEIR_BUNDLE, Value::Bytes(b.clone()));
        }
        if let Some(p) = &self.their_payto {
            m.insert(k::C_THEIR_PAYTO, Value::Text(p.clone()));
        }
        if let Some(p) = &self.petname {
            m.insert(k::C_PETNAME, Value::Text(p.clone()));
        }
        if let Some(a) = &self.asserted_name {
            m.insert(k::C_ASSERTED, Value::Text(a.clone()));
        }
        m.insert(k::C_IN_SEQ, Value::Uint(self.in_seq));
        m.insert(k::C_OUT_SEQ, Value::Uint(self.out_seq));
        if let Some(p) = &self.in_prev {
            m.insert(k::C_IN_PREV, Value::Bytes(p.clone()));
        }
        if let Some(p) = &self.out_prev {
            m.insert(k::C_OUT_PREV, Value::Bytes(p.clone()));
        }
        Value::Map(m)
    }

    fn from_value(v: &Value) -> Result<Self, Reject> {
        let m = match v {
            Value::Map(m) => m,
            _ => return Err(Reject::new(RejectCode::Malformed)),
        };
        let text = |key: u64| m.get(&key).and_then(|v| v.as_text()).map(|s| s.to_string());
        let bytes = |key: u64| m.get(&key).and_then(|v| v.as_bytes()).map(|b| b.to_vec());
        Ok(BackupContact {
            persona: bytes(k::C_PERSONA).ok_or_else(|| Reject::new(RejectCode::Malformed))?,
            my_outbox_key: text(k::C_MY_OUTBOX)
                .ok_or_else(|| Reject::new(RejectCode::Malformed))?,
            my_outbox_owner_public: bytes(k::C_MY_OWNER_PUB).unwrap_or_default(),
            my_outbox_owner_secret: bytes(k::C_MY_OWNER_SEC).unwrap_or_default(),
            their_outbox_key: text(k::C_THEIR_OUTBOX)
                .ok_or_else(|| Reject::new(RejectCode::Malformed))?,
            their_bundle: bytes(k::C_THEIR_BUNDLE),
            their_payto: text(k::C_THEIR_PAYTO),
            petname: text(k::C_PETNAME),
            asserted_name: text(k::C_ASSERTED),
            in_seq: m.get(&k::C_IN_SEQ).and_then(|v| v.as_uint()).unwrap_or(0),
            out_seq: m.get(&k::C_OUT_SEQ).and_then(|v| v.as_uint()).unwrap_or(0),
            in_prev: bytes(k::C_IN_PREV),
            out_prev: bytes(k::C_OUT_PREV),
        })
    }
}

impl Backup {
    fn to_value(&self) -> Value {
        let mut m = BTreeMap::new();
        m.insert(k::VERSION, Value::Uint(BACKUP_VERSION));
        if let Some(n) = &self.display_name {
            m.insert(k::DISPLAY_NAME, Value::Text(n.clone()));
        }
        // Only encoded when true: absent means off, which is the safe default
        // for a privacy setting and keeps one meaning to one encoding.
        if self.publish_payto {
            m.insert(k::PUBLISH_PAYTO, Value::Uint(1));
        }
        if let Some(a) = &self.avatar {
            m.insert(k::AVATAR, Value::Bytes(a.clone()));
        }
        if let Some(e) = &self.email {
            m.insert(k::EMAIL, Value::Text(e.clone()));
        }
        if let Some(p) = &self.phone {
            m.insert(k::PHONE, Value::Text(p.clone()));
        }
        if let Some(sg) = &self.signal {
            m.insert(k::SIGNAL, Value::Text(sg.clone()));
        }
        if let Some(p) = self.pronouns {
            m.insert(k::PRONOUNS, Value::Uint(p));
        }
        if !self.contacts.is_empty() {
            m.insert(
                k::CONTACTS,
                Value::Array(self.contacts.iter().map(|c| c.to_value()).collect()),
            );
        }
        if let Some(sk) = &self.prekey_signed_secret {
            m.insert(k::PREKEY_SIGNED, Value::Bytes(sk.clone()));
        }
        if !self.prekey_one_time.is_empty() {
            let mut ot = BTreeMap::new();
            for (id, sk) in &self.prekey_one_time {
                ot.insert(*id, Value::Bytes(sk.clone()));
            }
            m.insert(k::PREKEY_ONE_TIME, Value::Map(ot));
        }
        if self.prekey_next_id > 0 {
            m.insert(k::PREKEY_NEXT_ID, Value::Uint(self.prekey_next_id));
        }
        if let Some(b) = &self.app_state {
            m.insert(k::APP_STATE, Value::Bytes(b.clone()));
        }
        m.insert(k::PERSONA_SUITE, Value::Uint(self.persona_suite as u64));
        m.insert(k::PERSONA_SECRET, Value::Bytes(self.persona_secret.clone()));
        m.insert(k::MONERO_SEED, Value::Text(self.monero_seed.clone()));
        m.insert(
            k::MONERO_RESTORE_HEIGHT,
            Value::Uint(self.monero_restore_height),
        );
        m.insert(
            k::RENDEZVOUS,
            Value::Array(self.rendezvous.iter().map(|r| Value::Bytes(r.clone())).collect()),
        );
        m.insert(
            k::MANDATES,
            Value::Array(self.mandates.iter().map(|r| Value::Bytes(r.clone())).collect()),
        );
        m.insert(
            k::ATTESTATION_RECORDS,
            Value::Array(
                self.attestation_records
                    .iter()
                    .map(|r| Value::Bytes(r.clone()))
                    .collect(),
            ),
        );
        m.insert(
            k::CVM_DEVICE_UNLOCK_AT,
            Value::Uint(self.verification.device_unlock_at),
        );
        m.insert(k::CVM_APP_SECRET_AT, Value::Uint(self.verification.app_secret_at));
        m.insert(
            k::CVM_APP_SECRET_VALIDITY_S,
            Value::Uint(self.verification.app_secret_validity_s),
        );
        m.insert(k::CVM_CUMULATIVE_AT, Value::Uint(self.verification.cumulative_at));
        m.insert(
            k::CVM_CUMULATIVE_WINDOW_S,
            Value::Uint(self.verification.cumulative_window_s),
        );
        m.insert(
            k::ESCROW_SHARES,
            Value::Array(
                self.escrow_shares
                    .iter()
                    .map(|e| {
                        let mut em = BTreeMap::new();
                        em.insert(k::ESCROW_ID, Value::Bytes(e.escrow_id.clone()));
                        em.insert(k::ESCROW_KEY_FILE, Value::Bytes(e.key_file.clone()));
                        em.insert(k::ESCROW_RESTORE_HEIGHT, Value::Uint(e.restore_height));
                        Value::Map(em)
                    })
                    .collect(),
            ),
        );
        m.insert(k::CREATED, Value::Uint(self.created));
        Value::Map(m)
    }

    fn from_value(v: Value) -> Result<Self, Reject> {
        let m = v
            .as_map()
            .ok_or_else(|| Reject::with_detail(RejectCode::Malformed, "backup must be a map"))?;
        let get = |key: u64| {
            m.get(&key)
                .ok_or_else(|| Reject::with_detail(RejectCode::Malformed, "missing backup field"))
        };
        let ver = get(k::VERSION)?.as_uint().unwrap_or(0);
        if ver != BACKUP_VERSION {
            return Err(Reject::with_detail(
                RejectCode::UnsupportedVersion,
                format!("backup version {} is not supported", ver),
            ));
        }
        let arr = |key: u64| -> Result<Vec<Vec<u8>>, Reject> {
            match get(key)? {
                Value::Array(items) => items
                    .iter()
                    .map(|i| {
                        i.as_bytes()
                            .map(|b| b.to_vec())
                            .ok_or_else(|| Reject::new(RejectCode::Malformed))
                    })
                    .collect(),
                _ => Err(Reject::new(RejectCode::Malformed)),
            }
        };
        let policy = VerificationPolicy {
            device_unlock_at: get(k::CVM_DEVICE_UNLOCK_AT)?.as_uint().unwrap_or(0),
            app_secret_at: get(k::CVM_APP_SECRET_AT)?.as_uint().unwrap_or(0),
            app_secret_validity_s: get(k::CVM_APP_SECRET_VALIDITY_S)?.as_uint().unwrap_or(0),
            cumulative_at: get(k::CVM_CUMULATIVE_AT)?.as_uint().unwrap_or(0),
            cumulative_window_s: get(k::CVM_CUMULATIVE_WINDOW_S)?.as_uint().unwrap_or(0),
        };
        // A policy that fails §15.5.1's own construction check must not be
        // installed just because it arrived in a bundle. An import is a trust
        // boundary like any other, and an inverted ladder — a larger payment
        // asking less than a smaller one — is exactly the state that check
        // exists to prevent.
        policy.validate()?;

        Ok(Backup {
            persona_suite: get(k::PERSONA_SUITE)?.as_uint().unwrap_or(0) as u8,
            persona_secret: get(k::PERSONA_SECRET)?
                .as_bytes()
                .ok_or_else(|| Reject::new(RejectCode::Malformed))?
                .to_vec(),
            monero_seed: get(k::MONERO_SEED)?
                .as_text()
                .ok_or_else(|| Reject::new(RejectCode::Malformed))?
                .to_string(),
            monero_restore_height: get(k::MONERO_RESTORE_HEIGHT)?.as_uint().unwrap_or(0),
            // Optional, and both default to the safe direction: a bundle from
            // before these existed restores with no name and no publishing,
            // rather than inventing either.
            display_name: m.get(&k::DISPLAY_NAME).and_then(|v| v.as_text()).map(|s| s.to_string()),
            publish_payto: m.get(&k::PUBLISH_PAYTO).and_then(|v| v.as_uint()).unwrap_or(0) == 1,
            avatar: m.get(&k::AVATAR).and_then(|v| v.as_bytes()).map(|b| b.to_vec()),
            email: m.get(&k::EMAIL).and_then(|v| v.as_text()).map(|s| s.to_string()),
            phone: m.get(&k::PHONE).and_then(|v| v.as_text()).map(|s| s.to_string()),
            signal: m.get(&k::SIGNAL).and_then(|v| v.as_text()).map(|s| s.to_string()),
            pronouns: m.get(&k::PRONOUNS).and_then(|v| v.as_uint()),
            contacts: match m.get(&k::CONTACTS) {
                Some(Value::Array(a)) => a
                    .iter()
                    .map(BackupContact::from_value)
                    .collect::<Result<Vec<_>, _>>()?,
                _ => Vec::new(),
            },
            prekey_signed_secret: m
                .get(&k::PREKEY_SIGNED)
                .and_then(|v| v.as_bytes())
                .map(|b| b.to_vec()),
            prekey_one_time: match m.get(&k::PREKEY_ONE_TIME) {
                Some(Value::Map(ot)) => ot
                    .iter()
                    .filter_map(|(id, v)| v.as_bytes().map(|b| (*id, b.to_vec())))
                    .collect(),
                _ => Vec::new(),
            },
            prekey_next_id: m.get(&k::PREKEY_NEXT_ID).and_then(|v| v.as_uint()).unwrap_or(0),
            app_state: m.get(&k::APP_STATE).and_then(|v| v.as_bytes()).map(|b| b.to_vec()),
            rendezvous: arr(k::RENDEZVOUS)?,
            attestation_records: arr(k::ATTESTATION_RECORDS)?,
            mandates: arr(k::MANDATES)?,
            verification: policy,
            escrow_shares: {
                let items = match get(k::ESCROW_SHARES)? {
                    Value::Array(a) => a.clone(),
                    _ => return Err(Reject::new(RejectCode::Malformed)),
                };
                let mut out = Vec::with_capacity(items.len());
                for it in &items {
                    let em = it.as_map().ok_or_else(|| Reject::new(RejectCode::Malformed))?;
                    let fetch = |kk: u64| {
                        em.get(&kk).ok_or_else(|| {
                            Reject::with_detail(RejectCode::Malformed, "missing escrow share field")
                        })
                    };
                    let key_file = fetch(k::ESCROW_KEY_FILE)?
                        .as_bytes()
                        .ok_or_else(|| Reject::new(RejectCode::Malformed))?
                        .to_vec();
                    // An empty key file is not a share. Accepting one would put
                    // an entry in the user's escrow list that restores to
                    // nothing, which reads as recoverable and is not.
                    if key_file.is_empty() {
                        return Err(Reject::with_detail(
                            RejectCode::Malformed,
                            "an escrow share with no key file restores nothing",
                        ));
                    }
                    out.push(EscrowShare {
                        escrow_id: fetch(k::ESCROW_ID)?
                            .as_bytes()
                            .ok_or_else(|| Reject::new(RejectCode::Malformed))?
                            .to_vec(),
                        key_file,
                        restore_height: fetch(k::ESCROW_RESTORE_HEIGHT)?.as_uint().unwrap_or(0),
                    });
                }
                out
            },
            created: get(k::CREATED)?.as_uint().unwrap_or(0),
        })
    }
}

/// Argon2id, memory-hard on purpose. A backup file is an *offline* target: an
/// attacker who obtains one can grind at it indefinitely, on whatever hardware
/// they like, with no rate limit anyone can impose. Making each guess expensive
/// in memory as well as time is the only defence, because memory is what GPUs
/// and ASICs cannot cheaply parallelise.
///
/// 64 MiB / 3 passes, above OWASP's 19 MiB floor and still a few hundred
/// milliseconds on a phone — which the user pays once, at import.
///
/// **These are pinned deliberately rather than taken from `Argon2::default()`.**
/// A crate default that shifted in a later release would silently derive a
/// different key and render every existing backup permanently unopenable, with
/// no error that points at the cause. The parameters are part of the format, and
/// `MAGIC` carries the version that selects them: a future v2 changes the magic
/// string, so `import` knows which parameters to use *before* it derives
/// anything.
const KDF_MEM_KIB: u32 = 64 * 1024;
const KDF_PASSES: u32 = 3;
const KDF_LANES: u32 = 1;

/// The same Argon2id, for a key that is not a backup's.
///
/// A desk keeps its stores in files rather than behind an Android Keystore, so
/// it needs a key from a passphrase too — and it must not be the *same* key as
/// the backup's, or a stolen backup passphrase would open the live store and a
/// stolen store would open the backup. `context` is the domain separator: it
/// is hashed in ahead of the passphrase, so two purposes with one passphrase
/// derive two unrelated keys. The parameters are shared deliberately, because
/// they are the reviewed ones (§4.3).
pub fn derive_for(context: &[u8], passphrase: &[u8], salt: &[u8; 16]) -> Result<[u8; 32], Reject> {
    let mut input = Vec::with_capacity(context.len() + 1 + passphrase.len());
    input.extend_from_slice(context);
    input.push(0x1f); // a byte no context string contains: an unambiguous join
    input.extend_from_slice(passphrase);
    derive(&input, salt)
}

fn derive(passphrase: &[u8], salt: &[u8; 16]) -> Result<[u8; 32], Reject> {
    let params = argon2::Params::new(KDF_MEM_KIB, KDF_PASSES, KDF_LANES, Some(32))
        .map_err(|_| Reject::with_detail(RejectCode::Malformed, "bad kdf parameters"))?;
    let argon = Argon2::new(
        argon2::Algorithm::Argon2id,
        argon2::Version::V0x13,
        params,
    );
    let mut key = [0u8; 32];
    argon
        .hash_password_into(passphrase, salt, &mut key)
        .map_err(|_| Reject::with_detail(RejectCode::Malformed, "key derivation failed"))?;
    Ok(key)
}

/// Encrypt a backup under a passphrase.
///
/// `salt` and `nonce` must be freshly random per export. They are stored in the
/// clear, which is correct — they are not secrets, and reusing either would be.
pub fn export(
    backup: &Backup,
    passphrase: &[u8],
    salt: [u8; 16],
    nonce: [u8; 24],
) -> Result<Vec<u8>, Reject> {
    if passphrase.len() < 8 {
        return Err(Reject::with_detail(
            RejectCode::PolicyRefused,
            "passphrase is too short to protect a wallet",
        ));
    }
    let key = derive(passphrase, &salt)?;
    let cipher = XChaCha20Poly1305::new((&key).into());
    let plaintext = backup.to_value().encode();

    // The magic string is authenticated but not encrypted, so a file of another
    // format cannot be coerced into decrypting as this one.
    let ct = cipher
        .encrypt(
            XNonce::from_slice(&nonce),
            Payload {
                msg: &plaintext,
                aad: MAGIC,
            },
        )
        .map_err(|_| Reject::with_detail(RejectCode::Malformed, "encryption failed"))?;

    let mut out = Vec::with_capacity(MAGIC.len() + 16 + 24 + ct.len());
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&salt);
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&ct);
    Ok(out)
}

/// Decrypt a backup.
///
/// A wrong passphrase and a tampered file are the same error on purpose: the
/// AEAD cannot distinguish them, and pretending otherwise would leak whether a
/// guess was close.
pub fn import(blob: &[u8], passphrase: &[u8]) -> Result<Backup, Reject> {
    let head = MAGIC.len() + 16 + 24;
    if blob.len() < head + 16 {
        return Err(Reject::with_detail(
            RejectCode::Malformed,
            "backup file is truncated",
        ));
    }
    if &blob[..MAGIC.len()] != MAGIC {
        return Err(Reject::with_detail(
            RejectCode::Malformed,
            "not a DUCAT backup file",
        ));
    }
    let salt: [u8; 16] = blob[MAGIC.len()..MAGIC.len() + 16].try_into().unwrap();
    let nonce: [u8; 24] = blob[MAGIC.len() + 16..head].try_into().unwrap();

    let key = derive(passphrase, &salt)?;
    let cipher = XChaCha20Poly1305::new((&key).into());
    let plaintext = cipher
        .decrypt(
            XNonce::from_slice(&nonce),
            Payload {
                msg: &blob[head..],
                aad: MAGIC,
            },
        )
        .map_err(|_| {
            Reject::with_detail(
                RejectCode::BadSig,
                "wrong passphrase, or the backup has been altered",
            )
        })?;

    Backup::from_value(decode(&plaintext)?)
}
