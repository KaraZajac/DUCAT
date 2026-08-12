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
    pub created: u64,
}

impl Backup {
    fn to_value(&self) -> Value {
        let mut m = BTreeMap::new();
        m.insert(k::VERSION, Value::Uint(BACKUP_VERSION));
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
            rendezvous: arr(k::RENDEZVOUS)?,
            attestation_records: arr(k::ATTESTATION_RECORDS)?,
            mandates: arr(k::MANDATES)?,
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
