//! §9.5 — costly identity: burning, proving, and checking a stranger's burn.
//!
//! A burn is an ordinary send to the one address nobody holds the keys to
//! (§9.5, `txproof::burn_address`), whose transaction key the send path now
//! hands back. From that key and the persona's name the wallet makes an
//! `OutProofV2` at once; the block height arrives with the chain, and only
//! then is the `BURN_PROOF` envelope signed and kept — a burn without a
//! block is not a burn yet. A stranger's envelope is checked the way §9.5
//! says: it must name the persona presenting it, the proof must verify
//! against our own node for the amount it claims, and the second opinion
//! must have the transaction in a block; *unknown* is never *yes*.
//!
//! Nothing here is published. What a persona shows, it shows in a thread.

use serde::{Deserialize, Serialize};

use ducat_core::trust::{
    burn_message, open_attestation, open_burn_proof, open_vouch, sign_attestation, sign_burn_proof, sign_vouch, Attestation, BurnProof,
    Vouch, ATTESTATION_VERSION, BURN_PROOF_VERSION, MAX_ATTESTATION_NOTE_CHARS, RATING_MAX, RATING_MIN, VOUCH_VERSION,
};
use ducat_mobile::contacts::persona_public_hex;
use ducat_mobile::monero::{monero_second_opinion_nodes, monero_tx_status, TxStatus};
use ducat_mobile::txproof::{monero_burn_address, monero_make_out_proof, monero_verify_out_proof};

use crate::{log, App, Error};

pub use ducat_core::trust::BURN_FLOOR_PXMR;

const TAG: &str = "Trust";
const STORE: &str = "trust";
const NODE_TIMEOUT_MS: u32 = 20_000;

/// One of this desk's own burns.
///
/// The JSON names are the phone's (`Trust.kt`), so that a bundle exported
/// on either client restores on the other.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BurnRecord {
    #[serde(rename = "persona")]
    pub persona_hex: String,
    #[serde(rename = "txid")]
    pub txid_hex: String,
    #[serde(rename = "amount")]
    pub amount_pxmr: u64,
    pub purpose: String,
    /// The `OutProofV2`, made at send time.
    pub proof: String,
    /// The block it is in; zero until the chain says.
    #[serde(default)]
    pub height: u64,
    /// The signed `BURN_PROOF` envelope, hex — written once the height is known.
    #[serde(default, rename = "envelope", skip_serializing_if = "Option::is_none")]
    pub envelope_hex: Option<String>,
    #[serde(rename = "made")]
    pub made_at: u64,
}

/// A stranger's burn, checked and kept beside the contact.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct VerifiedBurn {
    #[serde(rename = "persona")]
    pub persona_hex: String,
    #[serde(rename = "txid")]
    pub txid_hex: String,
    #[serde(rename = "amount")]
    pub amount_pxmr: u64,
    pub height: u64,
    pub purpose: String,
    #[serde(rename = "checked")]
    pub checked_at: u64,
}

fn hexs(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn unhex(h: &str) -> Option<Vec<u8>> {
    let h = h.trim();
    if h.len() % 2 != 0 || !h.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    (0..h.len()).step_by(2).map(|i| u8::from_str_radix(&h[i..i + 2], 16).ok()).collect()
}

impl App {
    /// The burn address for this wallet's network, as text.
    pub fn burn_address(&self) -> String {
        monero_burn_address(self.wallet_stagenet())
    }

    pub fn burns(&self) -> Vec<BurnRecord> {
        self.store(STORE).get("burns").unwrap_or_default()
    }

    fn put_burns(&self, v: &[BurnRecord]) -> Result<(), Error> {
        self.store(STORE).put("burns", &v.to_vec())?;
        Ok(())
    }

    /// The largest finished burn a persona of ours holds, if any.
    pub fn my_burn(&self, persona_hex: &str) -> Option<BurnRecord> {
        self.burns()
            .into_iter()
            .filter(|b| b.persona_hex == persona_hex && b.envelope_hex.is_some())
            .max_by_key(|b| b.amount_pxmr)
    }

    /// Burn `amount_pxmr` under `persona_hex` for `purpose`: send it to the
    /// burn address and make the proof from the transaction key at once.
    /// The record waits for its block before it becomes an envelope.
    pub fn burn(&self, persona_hex: &str, amount_pxmr: u64, purpose: &str) -> Result<BurnRecord, Error> {
        if amount_pxmr < BURN_FLOOR_PXMR {
            return Err(Error::Refused("a burn under the floor counts for nothing".into()));
        }
        let purpose = purpose.trim();
        if purpose.is_empty() || purpose.chars().count() > ducat_core::trust::MAX_PURPOSE_CHARS {
            return Err(Error::Refused("a purpose is a short label".into()));
        }
        let secret = self
            .persona_secret(persona_hex)?
            .ok_or_else(|| Error::Refused("no such persona on this desk".into()))?;
        let persona_pub = unhex(&persona_public_hex(secret).map_err(|e| Error::Refused(format!("{e}")))?)
            .ok_or_else(|| Error::Refused("persona key".into()))?;
        let address = self.burn_address();
        let r = self.send_xmr(&address, amount_pxmr, None, Some("burn"), 1, false)?;
        let message = burn_message(&persona_pub, purpose);
        let proof = monero_make_out_proof(r.txid_hex.clone(), message, vec![r.tx_key_hex.clone()], address, self.wallet_stagenet())
            .map_err(|e| Error::Refused(format!("proof: {e}")))?;
        let rec = BurnRecord {
            persona_hex: persona_hex.to_string(),
            txid_hex: r.txid_hex.clone(),
            amount_pxmr,
            purpose: purpose.to_string(),
            proof,
            height: 0,
            envelope_hex: None,
            made_at: App::now(),
        };
        let mut all = self.burns();
        all.push(rec.clone());
        self.put_burns(&all)?;
        log::info(TAG, format!("burned {} pXMR under {}… ({purpose}); waiting for its block", amount_pxmr, &persona_hex[..8.min(persona_hex.len())]));
        Ok(rec)
    }

    /// Give every burn that has reached a block its envelope. Run by the lap.
    pub fn burn_lap(&self) {
        let mut all = self.burns();
        let mut changed = false;
        let Some(node) = self.last_good_node() else { return };
        for b in all.iter_mut().filter(|b| b.envelope_hex.is_none()) {
            let TxStatus::InBlock { height } = monero_tx_status(node.clone(), b.txid_hex.clone(), NODE_TIMEOUT_MS) else { continue };
            if height == 0 {
                continue;
            }
            let Ok(Some(secret)) = self.persona_secret(&b.persona_hex) else { continue };
            let Ok(pub_hex) = persona_public_hex(secret.clone()) else { continue };
            let (Some(persona), Some(txid)) = (unhex(&pub_hex), unhex(&b.txid_hex)) else { continue };
            let Ok(txid): Result<[u8; 32], _> = txid.try_into() else { continue };
            let Ok(sk): Result<[u8; 32], _> = secret.as_slice().try_into() else { continue };
            let key = ducat_core::sig::SecretKey::ed25519_from_bytes(&sk);
            let object = BurnProof {
                version: BURN_PROOF_VERSION,
                suite: 1,
                txid,
                amount_pxmr: b.amount_pxmr,
                height,
                proof: b.proof.clone(),
                persona,
                purpose: b.purpose.clone(),
            };
            b.height = height;
            b.envelope_hex = Some(hexs(&sign_burn_proof(&object, &key)));
            changed = true;
            log::info(TAG, format!("burn {}… is in block {height}; its proof is ready to show", &b.txid_hex[..12]));
        }
        if changed {
            let _ = self.put_burns(&all);
        }
    }

    pub fn verified_burns(&self) -> Vec<VerifiedBurn> {
        self.store(STORE).get("verified").unwrap_or_default()
    }

    /// What we have verified about a persona's burn, if anything.
    pub fn burn_of(&self, persona_hex: &str) -> Option<VerifiedBurn> {
        self.verified_burns().into_iter().filter(|v| v.persona_hex == persona_hex).max_by_key(|v| v.amount_pxmr)
    }

    /// Check a stranger's burn proof the way §9.5 says, and keep the verdict.
    /// `persona_hex` is the persona presenting it — the message must name it.
    pub fn verify_burn(&self, persona_hex: &str, envelope_hex: &str) -> Result<VerifiedBurn, Error> {
        let env = unhex(envelope_hex).ok_or_else(|| Error::Refused("not hex".into()))?;
        let b = open_burn_proof(&env).map_err(|e| Error::Refused(format!("burn proof: {e:?}")))?;
        if hexs(&b.persona) != persona_hex.to_lowercase() {
            return Err(Error::Refused("the proof names another persona".into()));
        }
        if b.amount_pxmr < BURN_FLOOR_PXMR {
            return Err(Error::Refused("under the floor".into()));
        }
        // A desk that has never used its wallet has no node yet; pick one
        // rather than refuse — checking a stranger's burn is exactly what a
        // desk that has paid nobody yet wants to do.
        let node = self.last_good_node().or_else(|| self.pick_node()).ok_or_else(|| Error::Refused("no Monero node answers right now".into()))?;
        let address = self.burn_address();
        let v = monero_verify_out_proof(node, hexs(&b.txid), address, self.wallet_stagenet(), b.message(), b.proof.clone(), NODE_TIMEOUT_MS)
            .map_err(|e| Error::Refused(format!("the node does not bear it out: {e}")))?;
        if v.amount_pxmr != b.amount_pxmr {
            return Err(Error::Refused("the proof proves a different amount than it claims".into()));
        }
        if v.height == 0 {
            return Err(Error::Refused("not in a block yet".into()));
        }
        // §9.5's third check: the second opinion has it in a block. Unknown,
        // in the pool and unreachable are all "not yet".
        let mut corroborated = false;
        for n in monero_second_opinion_nodes(self.last_good_node(), self.monero_own_url()) {
            if let TxStatus::InBlock { height } = monero_tx_status(n.url.clone(), hexs(&b.txid), NODE_TIMEOUT_MS) {
                if height > 0 {
                    corroborated = true;
                    break;
                }
            }
        }
        if !corroborated {
            return Err(Error::Refused("no second node has it in a block yet".into()));
        }
        let verified = VerifiedBurn {
            persona_hex: persona_hex.to_lowercase(),
            txid_hex: hexs(&b.txid),
            amount_pxmr: b.amount_pxmr,
            height: v.height,
            purpose: b.purpose.clone(),
            checked_at: App::now(),
        };
        let mut all = self.verified_burns();
        all.retain(|x| !(x.persona_hex == verified.persona_hex && x.txid_hex == verified.txid_hex));
        all.push(verified.clone());
        self.store(STORE).put("verified", &all)?;
        log::info(TAG, format!("verified a burn of {} pXMR by {}… at block {}", verified.amount_pxmr, &persona_hex[..8.min(persona_hex.len())], verified.height));
        Ok(verified)
    }
}

/// §9.2 — a rated receipt, given, received, or read about someone.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AttestationRecord {
    /// Who spoke.
    #[serde(rename = "signer")]
    pub signer_hex: String,
    /// Who was spoken about.
    #[serde(rename = "subject")]
    pub subject_hex: String,
    #[serde(rename = "amount")]
    pub amount_pxmr: u64,
    pub rating: u8,
    pub ts: u64,
    #[serde(default, rename = "txid", skip_serializing_if = "Option::is_none")]
    pub txid_hex: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// The signed envelope, hex, so it can be shown again.
    #[serde(rename = "envelope")]
    pub envelope_hex: String,
}

/// What a reader can say about a persona's record: how many receipts it has
/// read, and how many distinct signers among them have a burn this reader
/// verified itself. One voice per signer: a burned persona that rates the
/// same subject ten times is counted once, by its latest receipt, so a record
/// cannot be padded by a friend with one burn.
#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct RecordSummary {
    pub receipts: u32,
    pub weighted: u32,
    /// Mean rating over the weighted signers' latest receipts, times ten (so 47 = 4.7); zero when none.
    pub rating_x10: u32,
}

const ATTEST_PREFIX: &str = "ducat:attest/";
const RECORD_PREFIX: &str = "ducat:record/";
const VOUCH_PREFIX: &str = "ducat:vouch/";
const VOUCHES_PREFIX: &str = "ducat:vouches/";
/// How many envelopes a record link may carry, and a reader will read.
const RECORD_MAX: usize = 64;

/// Join envelopes, newest first, into one `ducat:record/` link that fits a
/// message (`MAX_MESSAGE_CHARS`) — the same packing as the phone's.
fn pack_record_link<'a>(envelopes: impl Iterator<Item = &'a str>) -> String {
    pack_link(RECORD_PREFIX, envelopes)
}

fn pack_link<'a>(prefix: &str, envelopes: impl Iterator<Item = &'a str>) -> String {
    let mut out = String::from(prefix);
    let mut n = 0;
    for env in envelopes.take(RECORD_MAX) {
        let sep = if n == 0 { 0 } else { 1 };
        if out.len() + sep + env.len() > ducat_core::contact::MAX_MESSAGE_CHARS {
            break;
        }
        if n > 0 {
            out.push('.');
        }
        out.push_str(env);
        n += 1;
    }
    out
}

impl App {
    fn attestations(&self, key: &str) -> Vec<AttestationRecord> {
        self.store(STORE).get(key).unwrap_or_default()
    }

    fn put_attestations(&self, key: &str, v: &[AttestationRecord]) -> Result<(), Error> {
        self.store(STORE).put(key, &v.to_vec())?;
        Ok(())
    }

    fn record_from(a: &Attestation, envelope_hex: &str) -> AttestationRecord {
        AttestationRecord {
            signer_hex: hexs(&a.signer),
            subject_hex: hexs(&a.subject),
            amount_pxmr: a.amount_pxmr,
            rating: a.rating,
            ts: a.ts,
            txid_hex: a.txid.map(|t| hexs(&t)),
            note: a.note.clone(),
            envelope_hex: envelope_hex.to_lowercase(),
        }
    }

    /// Rate a counterparty after a settled deal: sign an attestation under
    /// the worn persona and return it as a `ducat:attest/` link to send.
    pub fn attest(&self, subject_hex: &str, amount_pxmr: u64, rating: u8, note: Option<&str>, txid_hex: Option<&str>) -> Result<String, Error> {
        if !(RATING_MIN..=RATING_MAX).contains(&rating) {
            return Err(Error::Refused("a rating is 1 to 5".into()));
        }
        let note = note.map(str::trim).filter(|n| !n.is_empty()).map(str::to_string);
        if note.as_ref().map_or(false, |n| n.chars().count() > MAX_ATTESTATION_NOTE_CHARS) {
            return Err(Error::Refused("a note is one sentence".into()));
        }
        // Signed by the persona that owns the thread, not the worn hat: the
        // link travels in that thread, sealed under its owner, and the
        // subject keeps a receipt only when signer and sender agree.
        let signer_hex = self.thread_persona(subject_hex)?;
        if signer_hex == subject_hex.to_lowercase() {
            return Err(Error::Refused("a persona cannot attest to itself".into()));
        }
        let secret = self.persona_secret(&signer_hex)?.ok_or_else(|| Error::Refused("no such persona".into()))?;
        let Ok(sk): Result<[u8; 32], _> = secret.as_slice().try_into() else { return Err(Error::Refused("persona key".into())) };
        let key = ducat_core::sig::SecretKey::ed25519_from_bytes(&sk);
        let subject = unhex(subject_hex).filter(|b| b.len() == 32).ok_or_else(|| Error::Refused("persona".into()))?;
        let txid = match txid_hex {
            Some(t) => Some(unhex(t).filter(|b| b.len() == 32).and_then(|b| <[u8; 32]>::try_from(b).ok()).ok_or_else(|| Error::Refused("txid".into()))?),
            None => None,
        };
        let a = Attestation {
            version: ATTESTATION_VERSION,
            suite: 1,
            signer: key.public().to_bytes().to_vec(),
            subject,
            amount_pxmr,
            rating,
            ts: App::now(),
            txid,
            note,
        };
        let env = hexs(&sign_attestation(&a, &key));
        let mut given = self.attestations("given");
        given.push(Self::record_from(&a, &env));
        self.put_attestations("given", &given)?;
        Ok(format!("{ATTEST_PREFIX}{env}"))
    }

    /// The persona this desk speaks as in the thread with `contact_hex`:
    /// the relationship's owner, or the worn hat for a stranger.
    fn thread_persona(&self, contact_hex: &str) -> Result<String, Error> {
        match self.contact(contact_hex).map(|c| c.owner).filter(|o| !o.is_empty()) {
            Some(owner) => Ok(owner.to_lowercase()),
            None => Ok(self.worn()?.to_lowercase()),
        }
    }

    /// The receipts others gave the persona this desk speaks as in the
    /// thread with `contact_hex`, as a `ducat:record/` link to send there.
    /// One message carries it, so the newest that fit are sent, sixty-four
    /// at most (§9.5 "How a receipt travels").
    pub fn my_record_link(&self, contact_hex: &str) -> Result<String, Error> {
        let me = self.thread_persona(contact_hex)?;
        let mut mine: Vec<AttestationRecord> = self.attestations("received").into_iter().filter(|r| r.subject_hex == me).collect();
        if mine.is_empty() {
            return Err(Error::Refused("nothing on the record yet".into()));
        }
        mine.sort_by_key(|r| std::cmp::Reverse(r.ts));
        Ok(pack_record_link(mine.iter().map(|r| r.envelope_hex.as_str())))
    }

    /// What we hold about a persona's record, weighted by the signers whose
    /// burns we have verified ourselves (§9.2).
    pub fn record_of(&self, persona_hex: &str) -> RecordSummary {
        let about: Vec<AttestationRecord> = self.attestations("about").into_iter().filter(|r| r.subject_hex == persona_hex).collect();
        let mut out = RecordSummary { receipts: about.len() as u32, ..Default::default() };
        let mut latest: std::collections::BTreeMap<&str, &AttestationRecord> = std::collections::BTreeMap::new();
        for r in &about {
            let keep = latest.get(r.signer_hex.as_str()).map_or(true, |have| r.ts > have.ts);
            if keep {
                latest.insert(&r.signer_hex, r);
            }
        }
        let mut sum = 0u32;
        for (signer, r) in latest {
            if self.burn_of(signer).is_some() {
                out.weighted += 1;
                sum += r.rating as u32 * 10;
            }
        }
        if out.weighted > 0 {
            out.rating_x10 = sum / out.weighted;
        }
        out
    }

    /// Called for every incoming text: a `ducat:attest/` link is a receipt
    /// about one of our personas from the sender; a `ducat:record/` link is
    /// the sender showing what others said about them. Both are opened under
    /// the signer they name, and refused unless the sender is who the object
    /// says it is — a receipt about us must be signed by the sender, and a
    /// record shown by someone must be about them.
    pub(crate) fn ingest_trust_links(&self, from_hex: &str, body: &str) {
        let body = body.trim();
        if let Some(hex) = body.strip_prefix(ATTEST_PREFIX) {
            let _ = self.receive_attestation(from_hex, hex);
        } else if let Some(rest) = body.strip_prefix(RECORD_PREFIX) {
            let _ = self.read_record(from_hex, rest);
        } else if let Some(hex) = body.strip_prefix(VOUCH_PREFIX) {
            let _ = self.receive_vouch(from_hex, hex);
        } else if let Some(rest) = body.strip_prefix(VOUCHES_PREFIX) {
            let _ = self.read_vouches(from_hex, rest);
        }
    }

    pub fn receive_attestation(&self, from_hex: &str, envelope_hex: &str) -> Result<AttestationRecord, Error> {
        let env = unhex(envelope_hex).ok_or_else(|| Error::Refused("not hex".into()))?;
        let a = open_attestation(&env).map_err(|e| Error::Refused(format!("attestation: {e:?}")))?;
        if hexs(&a.signer) != from_hex.to_lowercase() {
            return Err(Error::Refused("the receipt is not signed by the sender".into()));
        }
        let mine = self.persona_hexes();
        if !mine.contains(&hexs(&a.subject)) {
            return Err(Error::Refused("the receipt is not about us".into()));
        }
        let rec = Self::record_from(&a, envelope_hex);
        let mut all = self.attestations("received");
        all.retain(|r| !(r.signer_hex == rec.signer_hex && r.ts == rec.ts));
        all.push(rec.clone());
        self.put_attestations("received", &all)?;
        log::info(TAG, format!("a receipt from {}…: {} stars", &from_hex[..8.min(from_hex.len())], rec.rating));
        Ok(rec)
    }

    pub fn read_record(&self, from_hex: &str, dotted: &str) -> Result<RecordSummary, Error> {
        let mut about = self.attestations("about");
        let mut taken = 0u32;
        for hex in dotted.split('.').filter(|h| !h.is_empty()).take(RECORD_MAX) {
            let Some(env) = unhex(hex) else { continue };
            let Ok(a) = open_attestation(&env) else { continue };
            if hexs(&a.subject) != from_hex.to_lowercase() {
                continue;
            }
            let rec = Self::record_from(&a, hex);
            about.retain(|r| !(r.signer_hex == rec.signer_hex && r.subject_hex == rec.subject_hex && r.ts == rec.ts));
            about.push(rec);
            taken += 1;
        }
        self.put_attestations("about", &about)?;
        log::info(TAG, format!("{}… showed a record: {taken} receipt(s) read", &from_hex[..8.min(from_hex.len())]));
        Ok(self.record_of(from_hex))
    }
}


/// §9.2 — a vouch: *I know this persona*, given, received, or read about
/// someone. The JSON names are the phone's.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct VouchRecord {
    #[serde(rename = "signer")]
    pub signer_hex: String,
    #[serde(rename = "subject")]
    pub subject_hex: String,
    pub ts: u64,
    #[serde(rename = "envelope")]
    pub envelope_hex: String,
}

impl App {
    fn vouches(&self, key: &str) -> Vec<VouchRecord> {
        self.store(STORE).get(key).unwrap_or_default()
    }

    fn put_vouches(&self, key: &str, v: &[VouchRecord]) -> Result<(), Error> {
        self.store(STORE).put(key, &v.to_vec())?;
        Ok(())
    }

    fn vouch_record(v: &Vouch, envelope_hex: &str) -> VouchRecord {
        VouchRecord { signer_hex: hexs(&v.signer), subject_hex: hexs(&v.subject), ts: v.ts, envelope_hex: envelope_hex.to_lowercase() }
    }

    /// Vouch for a contact — *I know this persona* — under the persona this
    /// desk speaks as in that thread, and return the `ducat:vouch/` link to
    /// send there. A vouch says nothing else, so there is nothing to type.
    pub fn vouch(&self, contact_hex: &str) -> Result<String, Error> {
        let signer_hex = self.thread_persona(contact_hex)?;
        if signer_hex == contact_hex.to_lowercase() {
            return Err(Error::Refused("a persona cannot vouch for itself".into()));
        }
        let secret = self.persona_secret(&signer_hex)?.ok_or_else(|| Error::Refused("no such persona".into()))?;
        let Ok(sk): Result<[u8; 32], _> = secret.as_slice().try_into() else { return Err(Error::Refused("persona key".into())) };
        let key = ducat_core::sig::SecretKey::ed25519_from_bytes(&sk);
        let subject = unhex(contact_hex).filter(|b| b.len() == 32).ok_or_else(|| Error::Refused("persona".into()))?;
        let v = Vouch { version: VOUCH_VERSION, suite: 1, signer: key.public().to_bytes().to_vec(), subject, ts: App::now() };
        let env = hexs(&sign_vouch(&v, &key));
        let mut given = self.vouches("vouches_given");
        given.retain(|g| g.subject_hex != contact_hex.to_lowercase());
        given.push(Self::vouch_record(&v, &env));
        self.put_vouches("vouches_given", &given)?;
        Ok(format!("{VOUCH_PREFIX}{env}"))
    }

    /// Have we vouched for this persona, from any of ours?
    pub fn vouched_for(&self, persona_hex: &str) -> bool {
        self.vouches("vouches_given").iter().any(|g| g.subject_hex == persona_hex.to_lowercase())
    }

    /// A vouch about one of our personas, from the sender: kept only when the
    /// sender is the signer — a vouch handed on by anyone else is discarded.
    pub fn receive_vouch(&self, from_hex: &str, envelope_hex: &str) -> Result<VouchRecord, Error> {
        let env = unhex(envelope_hex).ok_or_else(|| Error::Refused("not hex".into()))?;
        let v = open_vouch(&env).map_err(|e| Error::Refused(format!("vouch: {e:?}")))?;
        if hexs(&v.signer) != from_hex.to_lowercase() {
            return Err(Error::Refused("the vouch is not signed by the sender".into()));
        }
        if !self.persona_hexes().contains(&hexs(&v.subject)) {
            return Err(Error::Refused("the vouch is not about us".into()));
        }
        let rec = Self::vouch_record(&v, envelope_hex);
        let mut all = self.vouches("vouches_received");
        all.retain(|r| !(r.signer_hex == rec.signer_hex && r.subject_hex == rec.subject_hex));
        all.push(rec.clone());
        self.put_vouches("vouches_received", &all)?;
        log::info(TAG, format!("{}… vouched for us", &from_hex[..8.min(from_hex.len())]));
        Ok(rec)
    }

    /// The vouches others gave the persona this desk speaks as in the thread
    /// with `contact_hex`, as one `ducat:vouches/` message, newest first.
    pub fn my_vouches_link(&self, contact_hex: &str) -> Result<String, Error> {
        let me = self.thread_persona(contact_hex)?;
        let mut mine: Vec<VouchRecord> = self.vouches("vouches_received").into_iter().filter(|r| r.subject_hex == me).collect();
        if mine.is_empty() {
            return Err(Error::Refused("nobody has vouched for this persona yet".into()));
        }
        mine.sort_by_key(|r| std::cmp::Reverse(r.ts));
        Ok(pack_link(VOUCHES_PREFIX, mine.iter().map(|r| r.envelope_hex.as_str())))
    }

    /// Vouches somebody showed us about themselves: each that opens and is
    /// about the sender is kept, the rest dropped without comment.
    pub fn read_vouches(&self, from_hex: &str, dotted: &str) -> Result<Vec<String>, Error> {
        let mut about = self.vouches("vouches_about");
        let mut taken = 0u32;
        for hex in dotted.split('.').filter(|h| !h.is_empty()).take(RECORD_MAX) {
            let Some(env) = unhex(hex) else { continue };
            let Ok(v) = open_vouch(&env) else { continue };
            if hexs(&v.subject) != from_hex.to_lowercase() {
                continue;
            }
            let rec = Self::vouch_record(&v, hex);
            about.retain(|r| !(r.signer_hex == rec.signer_hex && r.subject_hex == rec.subject_hex));
            about.push(rec);
            taken += 1;
        }
        self.put_vouches("vouches_about", &about)?;
        log::info(TAG, format!("{}… showed {taken} vouch(es)", &from_hex[..8.min(from_hex.len())]));
        Ok(self.known_by(from_hex))
    }

    /// Who among *our* contacts has vouched for this persona — names, for
    /// "2 of your contacts know this person". Computed here, from what we
    /// hold, and never sent anywhere. Our own vouch is not a contact's.
    pub fn known_by(&self, persona_hex: &str) -> Vec<String> {
        let subject = persona_hex.to_lowercase();
        let mine = self.persona_hexes();
        let mut names: Vec<String> = self
            .vouches("vouches_about")
            .into_iter()
            .filter(|r| r.subject_hex == subject && !mine.contains(&r.signer_hex))
            .filter_map(|r| self.contact(&r.signer_hex).map(|c| c.display_name()))
            .collect();
        names.sort();
        names.dedup();
        names
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_app(tag: &str) -> App {
        let dir = std::env::temp_dir().join(format!("ducat-trust-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        App::open(&dir).unwrap()
    }

    /// An attestation signed by `app`'s worn persona at a chosen time, so a
    /// test can write two from one signer without waiting a second.
    fn signed_by(app: &App, subject_hex: &str, rating: u8, ts: u64) -> String {
        let worn = app.worn().unwrap();
        let secret: [u8; 32] = app.persona_secret(&worn).unwrap().unwrap().try_into().unwrap();
        let key = ducat_core::sig::SecretKey::ed25519_from_bytes(&secret);
        let a = Attestation {
            version: ATTESTATION_VERSION,
            suite: 1,
            signer: key.public().to_bytes().to_vec(),
            subject: unhex(subject_hex).unwrap(),
            amount_pxmr: 1_000_000_000_000,
            rating,
            ts,
            txid: None,
            note: None,
        };
        hexs(&sign_attestation(&a, &key))
    }

    fn stage_verified_burn(app: &App, persona_hex: &str) {
        let mut all = app.verified_burns();
        all.push(VerifiedBurn {
            persona_hex: persona_hex.to_string(),
            txid_hex: "00".repeat(32),
            amount_pxmr: BURN_FLOOR_PXMR,
            height: 1,
            purpose: "identity".into(),
            checked_at: 1,
        });
        app.store(STORE).put("verified", &all).unwrap();
    }

    /// The JSON names are a contract with the phone's `Trust.kt`: a bundle
    /// written there is read here, and the other way round.
    #[test]
    fn the_records_are_written_under_the_phones_names() {
        let burn = BurnRecord { persona_hex: "aa".into(), txid_hex: "bb".into(), amount_pxmr: 1, purpose: "identity".into(), proof: "p".into(), height: 2, envelope_hex: Some("cc".into()), made_at: 3 };
        let v = serde_json::to_value(&burn).unwrap();
        let keys: Vec<&str> = v.as_object().unwrap().keys().map(String::as_str).collect();
        assert_eq!(keys, ["amount", "envelope", "height", "made", "persona", "proof", "purpose", "txid"]);
        let seen = VerifiedBurn { persona_hex: "aa".into(), txid_hex: "bb".into(), amount_pxmr: 1, height: 2, purpose: "identity".into(), checked_at: 3 };
        let v = serde_json::to_value(&seen).unwrap();
        let keys: Vec<&str> = v.as_object().unwrap().keys().map(String::as_str).collect();
        assert_eq!(keys, ["amount", "checked", "height", "persona", "purpose", "txid"]);
        let said = AttestationRecord { signer_hex: "aa".into(), subject_hex: "bb".into(), amount_pxmr: 1, rating: 5, ts: 2, txid_hex: Some("cc".into()), note: Some("n".into()), envelope_hex: "dd".into() };
        let v = serde_json::to_value(&said).unwrap();
        let keys: Vec<&str> = v.as_object().unwrap().keys().map(String::as_str).collect();
        assert_eq!(keys, ["amount", "envelope", "note", "rating", "signer", "subject", "ts", "txid"]);
        // The phone omits what is null; so do we, and we read it back.
        let bare = AttestationRecord { txid_hex: None, note: None, ..said };
        let text = serde_json::to_string(&bare).unwrap();
        assert!(!text.contains("txid") && !text.contains("note"));
        assert_eq!(serde_json::from_str::<AttestationRecord>(&text).unwrap(), bare);
    }

    #[test]
    fn a_receipt_travels_to_its_subject_and_is_shown_as_a_record() {
        let rater = temp_app("travels-rater");
        let subject = temp_app("travels-subject");
        let reader = temp_app("travels-reader");
        let (rater_hex, subject_hex) = (rater.worn().unwrap(), subject.worn().unwrap());

        let link = rater.attest(&subject_hex, 5_000_000_000, 5, Some("  prompt, as described  "), None).unwrap();
        assert!(link.starts_with(ATTEST_PREFIX));
        assert_eq!(rater.attestations("given").len(), 1);
        assert_eq!(rater.attestations("given")[0].note.as_deref(), Some("prompt, as described"));

        // Nothing on the record until a receipt arrives.
        assert!(subject.my_record_link(&rater_hex).is_err());
        // Ingested from the thread as the subject sees it: sender = signer.
        subject.ingest_trust_links(&rater_hex, &format!("  {link}\n"));
        let record = subject.my_record_link(&rater_hex).unwrap();
        assert!(record.starts_with(RECORD_PREFIX));

        // A third party reads the record the subject shows it.
        let dotted = record.strip_prefix(RECORD_PREFIX).unwrap();
        let summary = reader.read_record(&subject_hex, dotted).unwrap();
        assert_eq!(summary, RecordSummary { receipts: 1, weighted: 0, rating_x10: 0 });

        // Once the reader has verified the rater's burn, the voice counts.
        stage_verified_burn(&reader, &rater_hex);
        assert_eq!(reader.record_of(&subject_hex), RecordSummary { receipts: 1, weighted: 1, rating_x10: 50 });
    }

    #[test]
    fn one_voice_per_signer_rated_by_the_latest() {
        let rater = temp_app("voice-rater");
        let subject = temp_app("voice-subject");
        let reader = temp_app("voice-reader");
        let (rater_hex, subject_hex) = (rater.worn().unwrap(), subject.worn().unwrap());
        let first = signed_by(&rater, &subject_hex, 5, 1_700_000_000);
        let second = signed_by(&rater, &subject_hex, 3, 1_700_000_001);
        subject.receive_attestation(&rater_hex, &first).unwrap();
        subject.receive_attestation(&rater_hex, &second).unwrap();
        // The same envelope twice is one receipt.
        subject.receive_attestation(&rater_hex, &second).unwrap();
        assert_eq!(subject.attestations("received").len(), 2);

        let dotted = subject.my_record_link(&rater_hex).unwrap();
        stage_verified_burn(&reader, &rater_hex);
        let summary = reader.read_record(&subject_hex, dotted.strip_prefix(RECORD_PREFIX).unwrap()).unwrap();
        assert_eq!(summary, RecordSummary { receipts: 2, weighted: 1, rating_x10: 30 });
    }

    /// The reader's own contacts are the only ones that count: two strangers
    /// vouching for a third say nothing to somebody who knows none of them.
    #[test]
    fn a_vouch_counts_only_when_its_signer_is_one_of_the_readers_contacts() {
        let pat = temp_app("vouch-pat");
        let sam = temp_app("vouch-sam");
        let x = temp_app("vouch-x");
        let reader = temp_app("vouch-reader");
        let (pat_hex, sam_hex, x_hex) = (pat.worn().unwrap(), sam.worn().unwrap(), x.worn().unwrap());
        let pats = pat.vouch(&x_hex).unwrap();
        let sams = sam.vouch(&x_hex).unwrap();
        assert!(pats.starts_with(VOUCH_PREFIX));
        assert!(pat.vouched_for(&x_hex));
        // X keeps both, each sent by its own signer.
        x.ingest_trust_links(&pat_hex, &pats);
        x.ingest_trust_links(&sam_hex, &sams);
        // Handed on by the wrong persona: dropped.
        assert!(x.receive_vouch(&sam_hex, pats.strip_prefix(VOUCH_PREFIX).unwrap()).is_err());
        // Vouching for oneself is refused before signing.
        assert!(pat.vouch(&pat_hex).is_err());
        let shown = x.my_vouches_link(&pat_hex).unwrap();
        assert!(shown.starts_with(VOUCHES_PREFIX));
        assert_eq!(shown.trim_start_matches(VOUCHES_PREFIX).split('.').count(), 2);

        // The reader holds Pat as a contact, not Sam.
        let contact = |hex: &str, name: &str| crate::contacts::Contact {
            persona_hex: hex.to_string(),
            hearted: false,
            petname: Some(name.into()),
            asserted_name: None,
            my_outbox: "VLD0:mine".into(),
            my_outbox_owner_public: vec![1; 32],
            my_outbox_owner_secret: vec![2; 32],
            their_outbox: "VLD0:theirs".into(),
            their_bundle: None,
            their_address: None,
            pending_address: None,
            avatar: None,
            email: None,
            phone: None,
            signal: None,
            pronouns: None,
            my_ring: 32,
            car_model: None,
            car_color: None,
            plate: None,
            car_photo: None,
            their_read_up_to: None,
            card_purpose: None,
            my_card_purpose: None,
            my_card_purpose_at: 0,
            card_inbox: None,
            card_mine: false,
            out_seq: 0,
            out_prev_link: None,
            in_seq: 0,
            in_prev_link: None,
            chat_visible: true,
            owner: reader.worn().unwrap(),
        };
        reader.put_contact(contact(&pat_hex, "Pat")).unwrap();
        let known = reader.read_vouches(&x_hex, shown.strip_prefix(VOUCHES_PREFIX).unwrap()).unwrap();
        assert_eq!(known, vec!["Pat".to_string()]);
        assert_eq!(reader.known_by(&x_hex), vec!["Pat".to_string()]);
        // Now Sam is a contact too: two of the reader's contacts know X.
        reader.put_contact(contact(&sam_hex, "Sam")).unwrap();
        assert_eq!(reader.known_by(&x_hex), vec!["Pat".to_string(), "Sam".to_string()]);
        // A vouch shown by somebody it is not about is dropped.
        assert!(reader.read_vouches(&pat_hex, shown.strip_prefix(VOUCHES_PREFIX).unwrap()).unwrap().is_empty());
    }

    #[test]
    fn a_record_link_is_one_message_of_the_newest_receipts() {
        let env = "ab".repeat(300); // 600 chars, an envelope's size
        let many: Vec<String> = (0..10).map(|i| format!("{:02x}{}", i, &env[2..])).collect();
        let link = pack_record_link(many.iter().map(String::as_str));
        assert!(link.len() <= ducat_core::contact::MAX_MESSAGE_CHARS);
        let got: Vec<&str> = link.trim_start_matches(RECORD_PREFIX).split('.').collect();
        assert_eq!(got.len(), 3, "three of 600 chars fit under 2000 with the prefix");
        assert_eq!(got[0], many[0], "newest first, as given");
        // Never more than the reader will read.
        let short: Vec<String> = (0..100).map(|i| format!("{i:04x}")).collect();
        let link = pack_record_link(short.iter().map(String::as_str));
        assert_eq!(link.trim_start_matches(RECORD_PREFIX).split('.').count(), RECORD_MAX);
    }

    #[test]
    fn receipts_are_refused_unless_the_sender_is_who_the_object_says() {
        let rater = temp_app("refused-rater");
        let subject = temp_app("refused-subject");
        let stranger = temp_app("refused-stranger");
        let (rater_hex, subject_hex, stranger_hex) = (rater.worn().unwrap(), subject.worn().unwrap(), stranger.worn().unwrap());
        let env = signed_by(&rater, &subject_hex, 4, 1_700_000_000);

        // Handed on by someone other than its signer: discarded.
        assert!(subject.receive_attestation(&stranger_hex, &env).is_err());
        // About somebody else: discarded.
        assert!(stranger.receive_attestation(&rater_hex, &env).is_err());
        assert!(subject.attestations("received").is_empty());

        // A record shown by the wrong persona keeps nothing.
        let summary = stranger.read_record(&rater_hex, &env).unwrap();
        assert_eq!(summary.receipts, 0);
        assert!(stranger.attestations("about").is_empty());
        // Garbage between the dots is skipped, not fatal.
        let summary = stranger.read_record(&subject_hex, &format!("zz.{env}..00")).unwrap();
        assert_eq!(summary.receipts, 1);

        // What the signer's own client refuses before signing.
        assert!(rater.attest(&rater_hex, 1, 5, None, None).is_err());
        assert!(rater.attest(&subject_hex, 1, 0, None, None).is_err());
        assert!(rater.attest(&subject_hex, 1, 6, None, None).is_err());
        assert!(rater.attest(&subject_hex, 1, 5, Some(&"x".repeat(141)), None).is_err());
        assert!(rater.attest(&subject_hex, 1, 5, None, Some("not a txid")).is_err());
    }
}
