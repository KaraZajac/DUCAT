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
    burn_message, open_burn_proof, sign_burn_proof, BurnProof, BURN_PROOF_VERSION,
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
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BurnRecord {
    pub persona_hex: String,
    pub txid_hex: String,
    pub amount_pxmr: u64,
    pub purpose: String,
    /// The `OutProofV2`, made at send time.
    pub proof: String,
    /// The block it is in; zero until the chain says.
    #[serde(default)]
    pub height: u64,
    /// The signed `BURN_PROOF` envelope, hex — written once the height is known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub envelope_hex: Option<String>,
    pub made_at: u64,
}

/// A stranger's burn, checked and kept beside the contact.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct VerifiedBurn {
    pub persona_hex: String,
    pub txid_hex: String,
    pub amount_pxmr: u64,
    pub height: u64,
    pub purpose: String,
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
        let node = self.last_good_node().ok_or_else(|| Error::Refused("no Monero node yet".into()))?;
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
