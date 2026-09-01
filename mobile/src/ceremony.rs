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
    sign::{PreprocessMachine, SignMachine, SignatureMachine, Writable},
};
use monero_wallet::send::TransactionSignMachine;
use rand_core::{OsRng, RngCore};
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

/// A slot map's guard, poisoned or not.
///
/// Every export here runs under uniffi's catch_unwind, so a panic inside one
/// reaches the caller as an exception — and leaves the lock it held poisoned
/// for the life of the process. `lock().unwrap()` then panicked on every later
/// ceremony call, each reporting the first failure, until the app was killed.
/// The maps hold whole machines, never half-updated ones (each call inserts
/// or removes one entry), so a poisoned guard's contents are sound to use.
fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
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
    lock(dkgs()).insert((id, i), DkgStage::Committed(ss));
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
    // The peers' bytes are read before the machine is taken. A round's
    // machine lives only here (§17.9 — nothing can rebuild it), so taking it
    // first meant one malformed frame from one peer ended the ceremony for
    // this device: the next honest frame found "no dkg in progress". Refusing
    // the frame and keeping the machine costs nothing and leaves the
    // retransmit a machine to advance.
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

    let stage = lock(dkgs())
        .remove(&(id, i))
        .ok_or_else(|| ContactError::Refused("no dkg in progress for this ceremony".into()))?;
    let DkgStage::Committed(ss) = stage else {
        // Put back what was not ours to take: a late duplicate of round 0
        // must not destroy the round-1 machine it found in the slot.
        lock(dkgs()).insert((id, i), stage);
        return Err(ContactError::Refused("dkg is not at the commit stage".into()));
    };

    let (km, shares) = ss
        .generate_secret_shares(&mut OsRng, map)
        .map_err(|e| ContactError::Refused(format!("shares: {e:?}")))?;
    lock(dkgs()).insert((id, i), DkgStage::Shared(km));

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
    // Bytes first, machine second — see dkg_share.
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

    let stage = lock(dkgs())
        .remove(&(id, i))
        .ok_or_else(|| ContactError::Refused("no dkg in progress for this ceremony".into()))?;
    let DkgStage::Shared(km) = stage else {
        lock(dkgs()).insert((id, i), stage);
        return Err(ContactError::Refused("dkg is not at the share stage".into()));
    };

    let keys = km
        .calculate_share(&mut OsRng, map)
        .map_err(|e| ContactError::Refused(format!("calculate: {e:?}")))?
        .complete();
    let addr = group_address(&keys, stagenet)?;
    lock(finished()).insert((id, i), keys);
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
    let keys = lock(finished())
        .remove(&(id, i))
        .ok_or_else(|| ContactError::Refused("no finished dkg for this ceremony".into()))?;
    Ok(keys.serialize().to_vec())
}

/// Abandon a ceremony's in-memory state (the peer sent `CeremonyAbort`, or it
/// timed out). Idempotent — an unknown id is already gone.
#[uniffi::export]
pub fn ceremony_abort(ceremony_id: Vec<u8>, i: u16) {
    if let Ok(id) = cid(&ceremony_id) {
        lock(dkgs()).remove(&(id, i));
        lock(finished()).remove(&(id, i));
        lock(frosts()).remove(&(id, i));
    }
}

// ===== FROST release (§17.9, kinds 9) =====
//
// The escrow's DKG built one key nobody holds; spending from it is the same
// trick run backwards: one `SignableTransaction` both parties agree on, a
// preprocess each, a signature share each, and either side can assemble the
// final transaction. `escrowtest.rs` proved these exact calls on stagenet
// with both machines in one process; here the machine waits in a slot
// between the sealed messages that advance it, exactly as the DKG ones do.
//
// Wire shape (2-of-2):
//   round 0, proposer → co-signer:  [tx][preprocess_A]
//   round 1, co-signer → proposer:  [preprocess_B][share_B]
//   the proposer completes and broadcasts; the txid travels as a receipt.
//
// The co-signer answers in ONE step (preprocess + share together) because,
// holding A's preprocess already, nothing is gained by a fourth message.

/// The proposer's signing machine, parked between round 0 and round 1.
fn frosts() -> &'static Mutex<HashMap<([u8; 32], u16), TransactionSignMachine>> {
    static M: OnceLock<Mutex<HashMap<([u8; 32], u16), TransactionSignMachine>>> = OnceLock::new();
    M.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Length-prefixed framing, because a payload carries several parts and the
/// parts are not self-delimiting.
fn frame(out: &mut Vec<u8>, part: &[u8]) {
    out.extend_from_slice(&(part.len() as u32).to_le_bytes());
    out.extend_from_slice(part);
}

fn unframe<'a>(buf: &mut &'a [u8]) -> Result<&'a [u8], ContactError> {
    if buf.len() < 4 {
        return Err(ContactError::Refused("truncated frame".into()));
    }
    let len = u32::from_le_bytes(buf[..4].try_into().unwrap()) as usize;
    // Subtract to compare, never add. `len` is a u32 widened to usize, and on
    // a 32-bit target — armeabi-v7a is one, and it ships — `4 + len` wraps for
    // a length near u32::MAX. Release builds have overflow checks off, so it
    // wraps silently to something small, the bounds test passes, and the slice
    // below panics with start > end: a remote crash spelled out by four bytes
    // a counterparty chose. `buf.len() - 4` cannot underflow because the
    // length check above has already returned.
    if len > buf.len() - 4 {
        return Err(ContactError::Refused("truncated frame body".into()));
    }
    let part = &buf[4..4 + len];
    *buf = &buf[4 + len..];
    Ok(part)
}

fn read_keys(bytes: &[u8]) -> Result<ThresholdKeys<Ed25519>, ContactError> {
    ThresholdKeys::read(&mut &bytes[..])
        .map_err(|e| ContactError::Refused(format!("keys: {e}")))
}

/// Scan the chain for the escrow's outputs, from `from_height`, over the
/// same one-block-at-a-time path the example used. Errors instead of
/// panicking: on a phone, "the node hiccuped" must surface as a message.
fn scan_escrow(
    keys: &ThresholdKeys<Ed25519>,
    node_url: &str,
    from_height: u64,
) -> Result<(u64, Vec<monero_wallet::WalletOutput>, u64), ContactError> {
    use monero_daemon_rpc::prelude::*;
    use monero_wallet::Scanner;

    let group = keys.group_key();
    let mut material = b"DUCAT-ESCROW-VIEW-v0".to_vec();
    material.extend_from_slice(group.compress().as_bytes());
    let view = Zeroizing::new(monero_wallet::ed25519::Scalar::hash(&material));
    let vp = monero_wallet::ViewPair::new(monero_wallet::ed25519::Point::from(group.0), view)
        .map_err(|e| ContactError::Refused(format!("view pair: {e:?}")))?;

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| ContactError::Refused(format!("runtime: {e}")))?;
    rt.block_on(async {
        let rpc = monero_daemon_rpc::MoneroDaemon::new(crate::monero::UreqTransport::new(
            node_url.to_string(),
        ))
        .await
        .map_err(|e| ContactError::Refused(format!("connect: {e:?}")))?;
        let tip = rpc
            .latest_block_number()
            .await
            .map_err(|e| ContactError::Refused(format!("height: {e:?}")))? as u64;
        let mut scanner = Scanner::new(vp);
        let mut outputs = Vec::new();
        // The youngest output's height rides back out: Monero's ten-block
        // rule is judged against it, and WalletOutput does not carry it.
        let mut youngest = 0u64;
        let mut h = from_height;
        while h <= tip {
            // A block this scan cannot read is the whole answer's problem, not
            // that block's: it used to be skipped, and a skipped block is an
            // output nobody saw. From a balance that reads as "not funded yet"
            // (survivable: the next poll looks again) — from a release
            // proposal it is a sweep of *part* of the escrow, co-signed and
            // broadcast, with the rest left behind under a key nobody can
            // spend alone twice.
            let sb = crate::monero::scannable_block(&rpc, h)
                .await
                .map_err(|e| ContactError::Refused(format!("block {h}: {e}")))?;
            let found = scanner
                .scan(sb)
                .map_err(|e| ContactError::Refused(format!("scan block {h}: {e:?}")))?;
            for o in found.not_additionally_locked() {
                outputs.push(o);
                youngest = youngest.max(h);
            }
            h += 1;
        }
        Ok((tip, outputs, youngest))
    })
}

/// What an escrow currently holds, by this party's own scan (§17.5: a
/// payment is verified by finding the output, never by believing a note
/// from the party who benefits from being believed). `from_height` is the
/// chain height near the ceremony's build — an escrow minted minutes ago
/// needs minutes of chain, not the wallet's whole history.
///
/// **What arrived, not what remains.** The scan finds this escrow's outputs;
/// it cannot tell whether they have since been spent, and that is not an
/// omission to fix here. Deciding an output is spent means recognising its key
/// image, and a multisig output's key image does not exist for any one party —
/// it is assembled from the participants' partial images during signing. So a
/// single party genuinely cannot answer the question from the chain alone.
///
/// What stands in for it is the ceremony's own stage: a device that co-signed
/// a release knows the escrow is spent because it helped spend it, and stops
/// asking. Anything reading this figure after a release, or with no ceremony
/// state behind it, is reading history — `escrowtest` keeps no such state, so
/// it will offer to spend an escrow that is already gone and find out from the
/// relays. One did on 2026-08-23: proposed and co-signed cleanly, then every
/// relay refused the finished transaction with no reason given, at a fee well
/// above the minimum. Undiagnosed, and an already-spent escrow is the
/// explanation that fits a scan which cannot see spends.
#[uniffi::export]
pub fn escrow_balance(
    keys: Vec<u8>,
    node_url: String,
    from_height: u64,
) -> Result<u64, ContactError> {
    let keys = read_keys(&keys)?;
    let (_, outputs, _) = scan_escrow(&keys, &node_url, from_height)?;
    Ok(outputs.iter().map(|o| o.commitment().amount).sum())
}

/// What the proposer sends and shows: the wire payload plus the figures the
/// screen states before anything is signed.
#[derive(uniffi::Record)]
pub struct FrostProposal {
    pub payload: Vec<u8>,
    /// Everything the escrow held.
    pub total_pxmr: u64,
    /// What arrives at the destination (total minus the fee reserve; the
    /// reserve's surplus over the true fee returns to the destination too,
    /// as change).
    pub payout_pxmr: u64,
}

/// The co-signer's answer: its wire payload, the fee, and where the money
/// actually goes — read out of the bytes being signed, not out of the note
/// that arrived with them.
#[derive(uniffi::Record)]
pub struct FrostCosign {
    pub payload: Vec<u8>,
    pub fee_pxmr: u64,
    /// Every output of the transaction this answer signs, so the caller can
    /// check that what it put in front of somebody is what they agreed to.
    pub destinations: Vec<TxDestination>,
}

/// One output of a proposed transaction.
#[derive(uniffi::Record)]
pub struct TxDestination {
    /// The address, exactly as the transaction spells it. Empty for change
    /// named by view pair instead of by address — nothing DUCAT builds, and
    /// nothing a co-signer could recognise, so it is deliberately nameless.
    pub address: String,
    /// What this output receives. Zero for the residual claimant, whose
    /// share is whatever the fixed outputs and the fee leave behind.
    pub amount_pxmr: u64,
    /// The change output: the residual claimant, who absorbs the remainder.
    pub residual: bool,
}

/// More inputs or outputs than any release DUCAT builds. The bound stops a
/// malformed payload asking for a huge allocation before it is rejected.
const MAX_TX_PARTS: usize = 256;

/// Read a transaction's outputs out of its own serialisation.
///
/// 0.2.0 keeps `SignableTransaction::payments` private with no accessor, so
/// this walks the crate's encoding: a header, then the inputs — consumed by
/// the crate's own `OutputWithDecoys::read`, which leaves the cursor exactly
/// on the payment vector. Only the payment tags are read here, and they are
/// a length-prefixed address string and a little-endian amount.
///
/// Callers must pass `tx.serialize()` of an already-parsed transaction, never
/// the bytes off the wire. Then this reads the crate's own re-encoding of the
/// very object that will be signed, and no disagreement between this walk and
/// `SignableTransaction::read` is possible — which is the whole point, since
/// a consent screen fed by a second, laxer parser is worse than no screen.
fn read_destinations(serialized: &[u8]) -> Result<Vec<TxDestination>, ContactError> {
    use monero_wallet::address::MoneroAddress;
    use monero_wallet::extra::{MAX_ARBITRARY_DATA_SIZE, MAX_EXTRA_SIZE_BY_RELAY_RULE};
    use monero_wallet::interface::FeeRate;
    use monero_wallet::io::{read_byte, read_bytes, read_u32, read_u64, read_vec};
    use monero_wallet::OutputWithDecoys;
    use std::io;

    fn address<R: io::Read>(r: &mut R) -> io::Result<String> {
        let raw = read_vec(read_byte, Some(MoneroAddress::SIZE_UPPER_BOUND.0), r)?;
        String::from_utf8(raw).map_err(|_| io::Error::other("address is not utf-8"))
    }

    fn payment<R: io::Read>(r: &mut R) -> io::Result<TxDestination> {
        Ok(match read_byte(r)? {
            0 => TxDestination {
                address: address(r)?,
                amount_pxmr: read_u64(r)?,
                residual: false,
            },
            1 => TxDestination { address: address(r)?, amount_pxmr: 0, residual: true },
            // Change named by a view pair: spend key, view key, subaddress.
            // Consumed to keep the walk aligned, but there is no address to
            // put in front of anybody, so it is reported nameless and every
            // caller checking for its own address will refuse.
            2 | 3 => {
                let _spend: [u8; 32] = read_bytes(r)?;
                let _view: [u8; 32] = read_bytes(r)?;
                let (_major, _minor) = (read_u32(r)?, read_u32(r)?);
                TxDestination { address: String::new(), amount_pxmr: 0, residual: true }
            }
            _ => Err(io::Error::other("unknown payment kind"))?,
        })
    }

    fn reading(e: io::Error) -> ContactError {
        ContactError::Refused(format!("reading the proposed outputs: {e}"))
    }

    let mut r = serialized;
    let _rct_type = read_byte(&mut r).map_err(reading)?;
    let _outgoing_view_key: [u8; 32] = read_bytes(&mut r).map_err(reading)?;
    read_vec(OutputWithDecoys::read, Some(MAX_TX_PARTS), &mut r).map_err(reading)?;
    let payments = read_vec(payment, Some(MAX_TX_PARTS), &mut r).map_err(reading)?;

    // Keep reading to the end, and insist it lands exactly there.
    //
    // Everything above is a second walk over an encoding this crate owns, and
    // a second walk can drift: an upstream layout change, a payment kind that
    // grows a field, an input whose length this version reads differently.
    // Drift would not announce itself — it would put a confident, wrong set of
    // destinations in front of somebody about to sign. So finish the structure
    // and require the tail to be consumed to the byte. Nothing here is used;
    // reaching the end without residue is the whole result.
    read_vec(
        |r| read_vec(read_byte, Some(MAX_ARBITRARY_DATA_SIZE), r),
        Some(MAX_EXTRA_SIZE_BY_RELAY_RULE),
        &mut r,
    )
    .map_err(reading)?;
    FeeRate::read(&mut r).map_err(reading)?;
    if !r.is_empty() {
        return Err(ContactError::Refused(
            "the proposed transaction did not read to its end".into(),
        ));
    }
    Ok(payments)
}

/// What a proposed release actually pays, and to whom — without keys, so a
/// client can show it to somebody before they agree to sign it.
///
/// §17.5's rule applied to consent: the amount travelling beside a proposal
/// is written by the party who benefits from being believed, so the screen
/// has to be drawn from the payload instead.
#[uniffi::export]
pub fn frost_destinations(payload: Vec<u8>) -> Result<Vec<TxDestination>, ContactError> {
    use monero_wallet::send::SignableTransaction;

    let mut buf = payload.as_slice();
    let tx_bytes = unframe(&mut buf)?;
    let tx = SignableTransaction::read(&mut &tx_bytes[..])
        .map_err(|e| ContactError::Refused(format!("transaction: {e}")))?;
    read_destinations(&tx.serialize())
}

/// Reserved from the swept total to cover the network fee; the surplus
/// returns as change. The multisig release measured ~0.00012 XMR on
/// stagenet, so 0.0002 covers it with margin (escrowtest, 2026-08-15).
const FEE_RESERVE: u64 = 200_000_000;

/// One fixed slice of a split release: this much, to this address.
#[derive(uniffi::Record)]
pub struct SplitOut {
    pub dest: String,
    pub amount_pxmr: u64,
}

/// Round 0 — propose the release as a sweep to one destination. The common
/// case (a deposit coming home, a fare with no margin) and a thin wrapper:
/// every release is a split with an empty fixed list.
#[uniffi::export]
pub fn frost_propose(
    ceremony_id: Vec<u8>,
    i: u16,
    keys: Vec<u8>,
    dest: String,
    node_url: String,
    from_height: u64,
) -> Result<FrostProposal, ContactError> {
    frost_propose_split(ceremony_id, i, keys, Vec::new(), dest, node_url, from_height)
}

/// Round 0 — propose a **split** release: the fixed slices in `payments`,
/// and everything left after them and the network fee to `residual_dest`.
///
/// One transaction, several destinations — the primitive under everything
/// the escrow ladder promises: a rider's margin coming home beside the
/// driver's fare, a MAD escrow returning two deposits, a negotiated 80/20
/// settlement, an arbiter's partial ruling. The residual claimant pays the
/// fee, which is the right default: the party being made whole should not
/// have their fixed slice nibbled by fee estimation.
///
/// Zero-amount slices are skipped rather than refused — "no margin this
/// time" is a sweep, not an error.
#[uniffi::export]
pub fn frost_propose_split(
    ceremony_id: Vec<u8>,
    i: u16,
    keys: Vec<u8>,
    payments: Vec<SplitOut>,
    residual_dest: String,
    node_url: String,
    from_height: u64,
) -> Result<FrostProposal, ContactError> {
    use monero_daemon_rpc::prelude::*;
    use monero_wallet::address::MoneroAddress;
    use monero_wallet::ringct::RctType;
    use monero_wallet::send::{Change, SignableTransaction};
    use monero_wallet::OutputWithDecoys;

    let id = cid(&ceremony_id)?;
    let keys = read_keys(&keys)?;
    let dest = MoneroAddress::from_str_with_unchecked_network(&residual_dest)
        .map_err(|e| ContactError::Refused(format!("destination: {e:?}")))?;
    let mut fixed = Vec::new();
    for p in payments.iter().filter(|p| p.amount_pxmr > 0) {
        fixed.push((
            MoneroAddress::from_str_with_unchecked_network(&p.dest)
                .map_err(|e| ContactError::Refused(format!("split destination: {e:?}")))?,
            p.amount_pxmr,
        ));
    }

    let (tip, outputs, youngest) = scan_escrow(&keys, &node_url, from_height)?;
    if outputs.is_empty() {
        return Err(ContactError::Refused("the escrow holds nothing to release".into()));
    }
    // Monero's ten-block rule: an output younger than that is unspendable and
    // the daemon refuses the ring with an unexplained invalid_input. Say the
    // real reason and how long is left — on a slow stagenet this is the
    // common case, not the corner (found live, 2026-08-16: five rejected
    // releases that were nothing but a 7-of-10-confirmations wait).
    if youngest + 10 > tip + 1 {
        let left = youngest + 10 - (tip + 1);
        // "the escrow", not "the fare": this same release ends a taxi ride, a
        // room booking, a hired kayak and a second-hand bicycle, and a buyer
        // reading about a fare has been handed somebody else's vocabulary.
        // The shape is stable on purpose — the app matches the count out of it
        // to say this in the reader's own language, and falls back to these
        // words when it cannot.
        return Err(ContactError::Refused(format!(
            "the escrow needs {left} more confirmation(s) before it can move"
        )));
    }
    let total: u64 = outputs.iter().map(|o| o.commitment().amount).sum();
    // Checked: the slices are the caller's numbers, and a sum that wrapped
    // would pass the cover test below with a payout it never had.
    let owed = fixed
        .iter()
        .try_fold(FEE_RESERVE, |acc, (_, a)| acc.checked_add(*a))
        .ok_or_else(|| ContactError::Refused("the split does not add up".into()))?;
    if total <= owed {
        return Err(ContactError::Refused(
            "the escrow cannot cover the split and the fee".into(),
        ));
    }
    let payout = total - owed;

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| ContactError::Refused(format!("runtime: {e}")))?;
    let tx = rt.block_on(async {
        let rpc = monero_daemon_rpc::MoneroDaemon::new(crate::monero::UreqTransport::new(
            node_url.clone(),
        ))
        .await
        .map_err(|e| ContactError::Refused(format!("connect: {e:?}")))?;
        let tip = rpc
            .latest_block_number()
            .await
            .map_err(|e| ContactError::Refused(format!("height: {e:?}")))?;
        let mut decoyed = Vec::new();
        for o in outputs {
            decoyed.push(
                OutputWithDecoys::new(&mut OsRng, &rpc, 16, tip, o)
                    .await
                    .map_err(|e| ContactError::Refused(format!("decoys: {e:?}")))?,
            );
        }
        let fee_rate = rpc
            .fee_rate(FeePriority::Normal, u64::MAX)
            .await
            .map_err(|e| ContactError::Refused(format!("fee: {e:?}")))?;
        let mut outgoing = Zeroizing::new([0u8; 32]);
        OsRng.fill_bytes(outgoing.as_mut());
        // The residual claimant is the change address, so the true fee comes
        // out of their side and every fixed slice arrives exactly as named.
        // With no fixed slices this is the original sweep, byte for byte.
        let explicit = if fixed.is_empty() { vec![(dest, payout)] } else { fixed };
        SignableTransaction::new(
            RctType::ClsagBulletproofPlus,
            outgoing,
            decoyed,
            explicit,
            Change::fingerprintable(Some(dest)),
            vec![],
            fee_rate,
        )
        .map_err(|e| ContactError::Refused(format!("signable: {e:?}")))
    })?;

    let machine = tx
        .clone()
        .multisig(keys)
        .map_err(|e| ContactError::Refused(format!("multisig: {e:?}")))?;
    let (sign_machine, preprocess) = machine.preprocess(&mut OsRng);

    let mut payload = Vec::new();
    frame(&mut payload, &tx.serialize());
    frame(&mut payload, &preprocess.serialize());
    lock(frosts()).insert((id, i), sign_machine);

    Ok(FrostProposal { payload, total_pxmr: total, payout_pxmr: payout })
}

/// Round 1 — co-sign. Reads the proposed transaction and the proposer's
/// preprocess, preprocesses and signs in one step, and returns
/// `[preprocess][share]`. Nothing is kept: the co-signer's part is finished.
///
/// `proposer` names who round 0 came from: in a 2-of-3 the co-signer could
/// be either other participant, so "3 minus me" stopped being arithmetic
/// the moment the arbiter existed.
#[uniffi::export]
pub fn frost_cosign(
    ceremony_id: Vec<u8>,
    i: u16,
    proposer: u16,
    keys: Vec<u8>,
    payload: Vec<u8>,
) -> Result<FrostCosign, ContactError> {
    use monero_wallet::send::SignableTransaction;

    let _id = cid(&ceremony_id)?;
    let keys = read_keys(&keys)?;
    let their = Participant::new(proposer)
        .ok_or_else(|| ContactError::Refused("participant is 1..".into()))?;

    let mut buf = payload.as_slice();
    let tx_bytes = unframe(&mut buf)?;
    let pre_a_bytes = unframe(&mut buf)?;

    let tx = SignableTransaction::read(&mut &tx_bytes[..])
        .map_err(|e| ContactError::Refused(format!("transaction: {e}")))?;
    let fee = tx.necessary_fee();
    // Read from the parsed transaction's own re-encoding, so what the caller
    // is told it signed is what it signed.
    let destinations = read_destinations(&tx.serialize())?;

    let machine = tx
        .multisig(keys)
        .map_err(|e| ContactError::Refused(format!("multisig: {e:?}")))?;
    let (sign_machine, preprocess) = machine.preprocess(&mut OsRng);
    let pre_a = sign_machine
        .read_preprocess(&mut &pre_a_bytes[..])
        .map_err(|e| ContactError::Refused(format!("their preprocess: {e}")))?;
    let (_, share) = sign_machine
        .sign(HashMap::from([(their, pre_a)]), &[])
        .map_err(|e| ContactError::Refused(format!("sign: {e:?}")))?;

    let mut out = Vec::new();
    frame(&mut out, &preprocess.serialize());
    frame(&mut out, &share.serialize());
    Ok(FrostCosign { payload: out, fee_pxmr: fee, destinations })
}

/// Round 2 — complete and broadcast. Consumes the parked machine, folds in
/// the co-signer's preprocess and share, assembles the transaction, and
/// pushes it to the network. Returns the txid. `cosigner` names whose
/// answer this is — with an arbiter in the roster it is a choice, not
/// arithmetic.
#[uniffi::export]
pub fn frost_complete(
    ceremony_id: Vec<u8>,
    i: u16,
    cosigner: u16,
    payload: Vec<u8>,
    node_url: String,
) -> Result<String, ContactError> {

    let id = cid(&ceremony_id)?;
    let their = Participant::new(cosigner)
        .ok_or_else(|| ContactError::Refused("participant is 1..".into()))?;
    // Bytes first, machine second (see dkg_share): a co-sign that does not
    // parse must not take the proposer's machine with it.
    let mut buf = payload.as_slice();
    let pre_b_bytes = unframe(&mut buf)?;
    let share_b_bytes = unframe(&mut buf)?;

    let sign_machine = lock(frosts())
        .remove(&(id, i))
        .ok_or_else(|| ContactError::Refused("no release in progress for this ceremony".into()))?;
    let pre_b = match sign_machine.read_preprocess(&mut &pre_b_bytes[..]) {
        Ok(p) => p,
        Err(e) => {
            lock(frosts()).insert((id, i), sign_machine);
            return Err(ContactError::Refused(format!("their preprocess: {e}")));
        }
    };
    let (sig_machine, _our_share) = sign_machine
        .sign(HashMap::from([(their, pre_b)]), &[])
        .map_err(|e| ContactError::Refused(format!("sign: {e:?}")))?;
    let share_b = sig_machine
        .read_share(&mut &share_b_bytes[..])
        .map_err(|e| ContactError::Refused(format!("their share: {e}")))?;
    let tx = sig_machine
        .complete(HashMap::from([(their, share_b)]))
        .map_err(|e| ContactError::Refused(format!("complete: {e:?}")))?;

    let txid: String = tx.hash().iter().map(|b| format!("{b:02x}")).collect();

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| ContactError::Refused(format!("runtime: {e}")))?;
    let (accepted, last_err) = rt.block_on(crate::monero::relay(&tx, &node_url));
    if accepted == 0 {
        // Debug aid while the daemons' reasons stay empty through the
        // wrapper: the raw bytes let a curl to sendrawtransaction read the
        // full flag set (double_spend, invalid_input, …) off any node.
        if std::env::var("DUCAT_DUMP_TX").is_ok() {
            let hex: String = tx.serialize().iter().map(|b| format!("{b:02x}")).collect();
            eprintln!("DUCAT_TX_HEX {hex}");
        }
        return Err(ContactError::Refused(format!("no relay took the release: {last_err}")));
    }
    Ok(txid)
}


#[cfg(test)]
mod frame_tests {
    use super::{frame, unframe};

    /// A frame's length prefix is four bytes a counterparty chose.
    ///
    /// It used to be compared as `buf.len() < 4 + len`, and `len` is a u32
    /// widened to usize. On a 32-bit target — armeabi-v7a, which ships — that
    /// addition wraps for a length near u32::MAX, and release builds have
    /// overflow checks off, so it wraps *silently* to something small: the
    /// bounds test passes, and `&buf[4..wrapped]` panics with start > end.
    /// Four bytes on the wire, and the app is gone. Comparing by subtraction
    /// removes the arithmetic that could wrap rather than checking it after.
    #[test]
    fn a_length_prefix_cannot_be_made_to_wrap() {
        for len in [u32::MAX, u32::MAX - 1, u32::MAX - 3, 0x8000_0000, 0x7FFF_FFFF] {
            let mut bytes = len.to_le_bytes().to_vec();
            bytes.extend_from_slice(b"only a few real bytes");
            let mut buf = bytes.as_slice();
            assert!(
                unframe(&mut buf).is_err(),
                "a frame claiming {len} bytes out of {} was accepted",
                bytes.len(),
            );
        }
    }

    /// And the ordinary shape still round-trips, including the empty part and
    /// several parts in sequence — the reason framing exists at all.
    #[test]
    fn frames_round_trip() {
        let mut out = Vec::new();
        frame(&mut out, b"first");
        frame(&mut out, b"");
        frame(&mut out, b"third part, longer");
        let mut buf = out.as_slice();
        assert_eq!(unframe(&mut buf).unwrap(), b"first");
        assert_eq!(unframe(&mut buf).unwrap(), b"");
        assert_eq!(unframe(&mut buf).unwrap(), b"third part, longer");
        assert!(buf.is_empty(), "the whole payload should be consumed");
        assert!(unframe(&mut buf).is_err(), "reading past the end must refuse");
    }

    /// Truncation at every boundary, since a short read is what a dropped
    /// connection and a doctored payload look like alike.
    #[test]
    fn truncation_is_refused_everywhere() {
        let mut out = Vec::new();
        frame(&mut out, b"a body worth cutting");
        for cut in 0 .. out.len() {
            let mut buf = &out[.. cut];
            assert!(unframe(&mut buf).is_err(), "a frame cut at {cut} was accepted");
        }
        let mut whole = out.as_slice();
        assert!(unframe(&mut whole).is_ok(), "the uncut frame must still read");
    }
}

#[cfg(test)]
mod destination_tests {
    use super::read_destinations;

    /// A whole `SignableTransaction` serialisation, minus the inputs — the
    /// walk insists on reaching the end, so the tail has to be there. Counts
    /// stay under 128 so every varint here is the single byte it encodes to.
    fn tx(payments: &[Vec<u8>]) -> Vec<u8> {
        let mut v = vec![0u8]; // rct type
        v.extend_from_slice(&[7u8; 32]); // outgoing view key
        v.push(0); // no inputs
        v.push(payments.len() as u8);
        for p in payments {
            v.extend_from_slice(p);
        }
        v.push(0); // no arbitrary data
        v.extend_from_slice(&3000u64.to_le_bytes()); // fee rate: per weight
        v.extend_from_slice(&10u64.to_le_bytes()); // fee rate: mask
        v
    }

    fn pay(addr: &str, amount: u64) -> Vec<u8> {
        let mut v = vec![0u8, addr.len() as u8];
        v.extend_from_slice(addr.as_bytes());
        v.extend_from_slice(&amount.to_le_bytes());
        v
    }

    fn change(addr: &str) -> Vec<u8> {
        let mut v = vec![1u8, addr.len() as u8];
        v.extend_from_slice(addr.as_bytes());
        v
    }

    /// Change named by view pair: two keys and a subaddress index.
    fn view_pair_change() -> Vec<u8> {
        let mut v = vec![2u8];
        v.extend_from_slice(&[1u8; 32]);
        v.extend_from_slice(&[2u8; 32]);
        v.extend_from_slice(&0u32.to_le_bytes());
        v.extend_from_slice(&0u32.to_le_bytes());
        v
    }

    /// The shape every DUCAT release has: fixed slices, then the residual
    /// claimant. Address and amount have to come back exactly, because a
    /// co-signer compares them against an address it minted itself.
    #[test]
    fn reads_a_split() {
        let d = read_destinations(&tx(&[pay("5RIDER", 900), change("5DRIVER")])).unwrap();
        assert_eq!(d.len(), 2);
        assert_eq!(d[0].address, "5RIDER");
        assert_eq!(d[0].amount_pxmr, 900);
        assert!(!d[0].residual);
        assert_eq!(d[1].address, "5DRIVER");
        assert!(d[1].residual);
    }

    /// A sweep: nothing fixed, one address taking everything.
    #[test]
    fn reads_a_sweep() {
        let d = read_destinations(&tx(&[change("5ALL")])).unwrap();
        assert_eq!(d.len(), 1);
        assert!(d[0].residual);
        assert_eq!(d[0].amount_pxmr, 0);
    }

    /// A payment nobody can name must not be silently skipped: it is reported
    /// nameless, and the walk stays aligned so what follows still reads. An
    /// output that vanished from the list would be money leaving unannounced.
    #[test]
    fn view_pair_change_is_nameless_and_keeps_alignment() {
        let d = read_destinations(&tx(&[view_pair_change(), pay("5AFTER", 42)])).unwrap();
        assert_eq!(d.len(), 2);
        assert_eq!(d[0].address, "");
        assert!(d[0].residual);
        assert_eq!(d[1].address, "5AFTER");
        assert_eq!(d[1].amount_pxmr, 42);
    }

    /// Anything it cannot account for is refused rather than guessed at.
    #[test]
    fn refuses_what_it_cannot_read() {
        let mut unknown_tag = tx(&[]);
        unknown_tag[34] = 1; // one payment...
        unknown_tag.insert(35, 9); // ...of a kind that does not exist
        assert!(read_destinations(&unknown_tag).is_err());

        let full = tx(&[pay("5RIDER", 900), change("5DRIVER")]);
        for cut in [0, 1, 20, 34, 36, 40] {
            assert!(
                read_destinations(&full[.. cut.min(full.len())]).is_err(),
                "a transaction truncated at {cut} was read as complete",
            );
        }
    }

    /// The inputs are really stepped over, not assumed away.
    ///
    /// Every other fixture here declares zero inputs, which skips the one part
    /// of the walk this code does not own — the crate's own
    /// `OutputWithDecoys::read`. A real release has inputs with real decoys,
    /// and building one by hand needs valid curve points, so what is pinned
    /// here is the direction that keeps somebody safe: an input the crate
    /// cannot read stops the walk rather than letting it guess where the
    /// payments begin. Combined with the end-of-buffer check above, a
    /// misalignment over real inputs can only ever refuse.
    #[test]
    fn an_unreadable_input_stops_the_walk() {
        let mut v = vec![0u8];
        v.extend_from_slice(&[7u8; 32]);
        v.push(1); // one input...
        v.extend_from_slice(&[0xFFu8; 64]); // ...that is not one
        v.push(1);
        v.extend_from_slice(&pay("5RIDER", 900));
        v.push(0);
        v.extend_from_slice(&3000u64.to_le_bytes());
        v.extend_from_slice(&10u64.to_le_bytes());
        assert!(
            read_destinations(&v).is_err(),
            "an input that could not be read was walked past anyway",
        );
    }

    /// The alignment check itself. A walk that ended one byte early or late
    /// would still produce a plausible-looking list of destinations, so the
    /// test is that leftovers are fatal rather than ignored — that is the
    /// property standing between an upstream layout change and a consent
    /// screen quietly stating the wrong thing.
    #[test]
    fn a_walk_that_does_not_land_on_the_end_is_refused() {
        let full = tx(&[pay("5RIDER", 900), change("5DRIVER")]);
        assert!(read_destinations(&full).is_ok(), "the honest fixture must read");

        let mut trailing = full.clone();
        trailing.push(0);
        assert!(
            read_destinations(&trailing).is_err(),
            "a transaction with a byte left over was read as complete",
        );

        // One payment short of what the vector declares: the walk consumes
        // the change output as if it were the tail and lands off the end.
        let mut miscounted = full.clone();
        miscounted[34] = 1;
        assert!(
            read_destinations(&miscounted).is_err(),
            "a payment count that did not match the payments was accepted",
        );
    }
}
