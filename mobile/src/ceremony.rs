//! The bond/escrow ceremony engine (§17.9): stateful threshold machines held
//! by ceremony id, advanced one wire message at a time.
//!
//! `escrowtest.rs` proved the crypto with both machines in one function. A
//! real client cannot do that: each round arrives as a separate sealed
//! message, minutes apart, in a poll cycle that returns to the UI between
//! them. So the machines live here in a slot map keyed by the 32-byte
//! ceremony id — exactly as the Veilid node lives in its own slot — and each
//! exported step takes the counterparties' wire bytes and returns ours, for
//! the caller to seal as a `DkgRound`/`FrostRound` message and to feed back
//! what it receives. DUCAT carries the bytes; this module is the only place
//! that understands them, and it never lets one party see another's secret.
//!
//! Two-party (2-of-2) is what a bond needs; the API takes explicit
//! participant lists so 2-of-3 with an arbiter set drops in unchanged.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use ciphersuite::Ciphersuite;
use dkg_pedpop::{
    Commitments, EncryptedMessage, EncryptionKeyMessage, KeyGenMachine, KeyMachine, SecretShare,
    SecretShareMachine,
};
use modular_frost::{
    curve::Ed25519,
    dkg::{Participant, ThresholdKeys, ThresholdParams},
};
use rand_core::OsRng;
use zeroize::Zeroizing;

use crate::contacts::ContactError;

/// Where a DKG stands, between the sealed messages that advance it.
enum DkgStage {
    /// Round 1 done: committed, holding the machine that wants everyone's
    /// commitments to produce shares.
    Committed(SecretShareMachine<Ed25519>),
    /// Round 2 done: shares sent, holding the machine that wants the shares
    /// addressed to us to finish.
    Shared(KeyMachine<Ed25519>),
}

fn dkgs() -> &'static Mutex<HashMap<([u8; 32], u16), DkgStage>> {
    static M: OnceLock<Mutex<HashMap<([u8; 32], u16), DkgStage>>> = OnceLock::new();
    M.get_or_init(|| Mutex::new(HashMap::new()))
}

/// The finished keys wait here until the caller persists them, keyed the same
/// way, so `dkg_finish` can return just the address and the caller fetches
/// the secret share bytes deliberately.
fn finished() -> &'static Mutex<HashMap<([u8; 32], u16), ThresholdKeys<Ed25519>>> {
    static M: OnceLock<Mutex<HashMap<([u8; 32], u16), ThresholdKeys<Ed25519>>>> = OnceLock::new();
    M.get_or_init(|| Mutex::new(HashMap::new()))
}

fn cid(bytes: &[u8]) -> Result<[u8; 32], ContactError> {
    bytes
        .try_into()
        .map_err(|_| ContactError::Refused("ceremony id is 32 bytes".into()))
}

fn params(i: u16, t: u16, n: u16) -> Result<ThresholdParams, ContactError> {
    let p = Participant::new(i).ok_or_else(|| ContactError::Refused("participant is 1..".into()))?;
    ThresholdParams::new(t, n, p).map_err(|e| ContactError::Refused(format!("params: {e:?}")))
}

/// A counterparty's wire message, tagged by their participant index.
#[derive(uniffi::Record)]
pub struct FromParty {
    pub participant: u16,
    pub bytes: Vec<u8>,
}

/// Ours to send, tagged by who it is addressed to.
#[derive(uniffi::Record)]
pub struct ToParty {
    pub participant: u16,
    pub bytes: Vec<u8>,
}

/// Round 1 — commit. Returns the broadcast commitment bytes to seal to every
/// other party as a `DkgRound{round:0}` (§17.9). The context binds this key
/// to one escrow and MUST be the ceremony id both sides agreed on.
#[uniffi::export]
pub fn dkg_commit(
    ceremony_id: Vec<u8>,
    i: u16,
    t: u16,
    n: u16,
) -> Result<Vec<u8>, ContactError> {
    let id = cid(&ceremony_id)?;
    let (ss, commitment) =
        KeyGenMachine::<Ed25519>::new(params(i, t, n)?, id).generate_coefficients(&mut OsRng);
    dkgs().lock().unwrap().insert((id, i), DkgStage::Committed(ss));
    Ok(commitment.serialize())
}

/// Round 2 — share. Given every other party's commitment, returns the
/// encrypted share addressed to each of them (`DkgRound{round:1}`). Consumes
/// the round-1 machine and stores the round-2 one.
#[uniffi::export]
pub fn dkg_share(
    ceremony_id: Vec<u8>,
    i: u16,
    t: u16,
    n: u16,
    commitments: Vec<FromParty>,
) -> Result<Vec<ToParty>, ContactError> {
    let id = cid(&ceremony_id)?;
    let stage = dkgs()
        .lock()
        .unwrap()
        .remove(&(id, i))
        .ok_or_else(|| ContactError::Refused("no dkg in progress for this ceremony".into()))?;
    let DkgStage::Committed(ss) = stage else {
        return Err(ContactError::Refused("dkg is not at the commit stage".into()));
    };

    let mut map = HashMap::new();
    for c in commitments {
        let p = Participant::new(c.participant)
            .ok_or_else(|| ContactError::Refused("participant is 1..".into()))?;
        let msg = EncryptionKeyMessage::<Ed25519, Commitments<Ed25519>>::read(
            &mut &c.bytes[..],
            params(i, t, n)?,
        )
        .map_err(|e| ContactError::Refused(format!("commitment: {e}")))?;
        map.insert(p, msg);
    }

    let (km, shares) = ss
        .generate_secret_shares(&mut OsRng, map)
        .map_err(|e| ContactError::Refused(format!("shares: {e:?}")))?;
    dkgs().lock().unwrap().insert((id, i), DkgStage::Shared(km));

    Ok(shares
        .into_iter()
        .map(|(p, m)| ToParty { participant: u16::from(p), bytes: m.serialize() })
        .collect())
}

/// Round 3 — finish. Given the shares addressed to us, completes the key and
/// returns the group's public key bytes (the escrow's spend key, which no
/// party holds in full). The `ThresholdKeys` are kept for `dkg_take_keys`.
#[uniffi::export]
pub fn dkg_finish(
    ceremony_id: Vec<u8>,
    i: u16,
    t: u16,
    n: u16,
    shares: Vec<FromParty>,
    stagenet: bool,
) -> Result<String, ContactError> {
    let id = cid(&ceremony_id)?;
    let stage = dkgs()
        .lock()
        .unwrap()
        .remove(&(id, i))
        .ok_or_else(|| ContactError::Refused("no dkg in progress for this ceremony".into()))?;
    let DkgStage::Shared(km) = stage else {
        return Err(ContactError::Refused("dkg is not at the share stage".into()));
    };

    let mut map = HashMap::new();
    for s in shares {
        let p = Participant::new(s.participant)
            .ok_or_else(|| ContactError::Refused("participant is 1..".into()))?;
        let msg = EncryptedMessage::<Ed25519, SecretShare<<Ed25519 as Ciphersuite>::F>>::read(
            &mut &s.bytes[..],
            params(i, t, n)?,
        )
        .map_err(|e| ContactError::Refused(format!("share: {e}")))?;
        map.insert(p, msg);
    }

    let keys = km
        .calculate_share(&mut OsRng, map)
        .map_err(|e| ContactError::Refused(format!("calculate: {e:?}")))?
        .complete();
    let addr = group_address(&keys, stagenet)?;
    finished().lock().unwrap().insert((id, i), keys);
    Ok(addr)
}

/// The escrow's funding address from its keys: spend = the group key nobody
/// holds alone, view = derived from it (§8.2, fresh per group).
fn group_address(keys: &ThresholdKeys<Ed25519>, stagenet: bool) -> Result<String, ContactError> {
    use monero_wallet::address::Network;
    use monero_wallet::ed25519::Scalar as MScalar;
    use monero_wallet::ViewPair;
    let group = keys.group_key();
    let mut material = b"DUCAT-ESCROW-VIEW-v0".to_vec();
    material.extend_from_slice(group.compress().as_bytes());
    let view = Zeroizing::new(MScalar::hash(&material));
    let spend = monero_wallet::ed25519::Point::from(group.0);
    let vp = ViewPair::new(spend, view)
        .map_err(|e| ContactError::Refused(format!("view pair: {e:?}")))?;
    let net = if stagenet { Network::Stagenet } else { Network::Mainnet };
    Ok(vp.legacy_address(net).to_string())
}

/// Hand the finished `ThresholdKeys` to the caller for persistence, once. The
/// caller stores these as the party's only escrow secret; there is no copy of
/// the other party's share anywhere on this device.
#[uniffi::export]
pub fn dkg_take_keys(ceremony_id: Vec<u8>, i: u16) -> Result<Vec<u8>, ContactError> {
    let id = cid(&ceremony_id)?;
    let keys = finished()
        .lock()
        .unwrap()
        .remove(&(id, i))
        .ok_or_else(|| ContactError::Refused("no finished dkg for this ceremony".into()))?;
    Ok(keys.serialize().to_vec())
}

/// Abandon a ceremony's in-memory state (the peer sent `CeremonyAbort`, or it
/// timed out). Idempotent — an unknown id is already gone.
#[uniffi::export]
pub fn ceremony_abort(ceremony_id: Vec<u8>, i: u16) {
    if let Ok(id) = cid(&ceremony_id) {
        dkgs().lock().unwrap().remove(&(id, i));
        finished().lock().unwrap().remove(&(id, i));
    }
}

