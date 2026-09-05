//! The wallet: the notes this desk owns, what it can spend, the notes it
//! has promised away, and the node it asks — the phone's `Wallet2.kt` and
//! `WalletStore`, in the same table under the same keys.
//!
//! Two rules carried over whole:
//! - **A send claims its notes before it broadcasts.** The intent is
//!   written first; the chain retires it (the notes turn up spent, or they
//!   provably never left), and an exception never does. See §15.11.
//! - **The scan is a step, not a sync.** Two hundred blocks per turn, the
//!   node's tip recorded beside how far we got, so the screen can say how
//!   long is left instead of spinning.

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

use ducat_mobile::monero::{
    monero_default_nodes, monero_fee_estimate, monero_pick_node, monero_rate, monero_scan, monero_send, monero_spent,
    monero_subaddress, MoneroError, OwnedOutput, SendResult,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::contacts::{bump, bytes_b64, now_ms, CONTACTS};
use crate::{log, App, Error};

const TAG: &str = "Wallet";

/// Monero's unlock: a note spends ten blocks after it lands.
pub const LOCK_BLOCKS: u64 = 10;
/// How long a send intent whose notes never turned up spent is held
/// before its notes are released — a relay that never happened.
const INTENT_GIVE_UP_SECS: u64 = 30 * 60;
/// Blocks per scan step.
const WINDOW: u32 = 200;
/// The network this desk lives on; mainnet is a build away, not a toggle.
pub const NETTYPE: &str = "stagenet";

impl From<MoneroError> for Error {
    fn from(e: MoneroError) -> Self {
        Error::Refused(e.to_string())
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct Balances {
    pub spendable_pxmr: u64,
    pub locked_pxmr: u64,
    pub spendable_outputs: usize,
    pub blocks_to_unlock: u64,
    pub scanned_to: u64,
    pub tip: u64,
    pub scan_rate: f64,
    pub scan_from: u64,
    pub error: Option<String>,
    pub syncing: bool,
    pub blocks_left: u64,
    pub progress: f32,
    pub seconds_left: Option<u64>,
}

/// One note, as kept. Field names follow the phone's JSON.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WalletEntry {
    #[serde(rename = "amt")]
    pub amount_pxmr: u64,
    #[serde(rename = "h")]
    pub height: u64,
    #[serde(default)]
    pub spent: bool,
    #[serde(rename = "ki")]
    pub key_image: String,
    #[serde(with = "bytes_b64", default)]
    pub blob: Vec<u8>,
    #[serde(rename = "tx", default)]
    pub tx_hash_hex: String,
    #[serde(rename = "ts", default)]
    pub timestamp: u64,
    #[serde(default)]
    pub minor: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SendIntent {
    pub id: String,
    #[serde(rename = "to")]
    pub to_address: String,
    #[serde(rename = "amt")]
    pub amount_pxmr: u64,
    #[serde(rename = "kis", default)]
    pub key_images: Vec<String>,
    #[serde(default)]
    pub contact: Option<String>,
    #[serde(default)]
    pub note: Option<String>,
    pub ts: u64,
    #[serde(default)]
    pub donate: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SentPayment {
    #[serde(rename = "txid")]
    pub txid_hex: String,
    #[serde(rename = "amt")]
    pub amount_pxmr: u64,
    #[serde(default)]
    pub fee: u64,
    #[serde(rename = "to", default)]
    pub to_address: String,
    #[serde(default)]
    pub contact: Option<String>,
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default)]
    pub ts: u64,
    #[serde(default)]
    pub donate: bool,
    #[serde(default)]
    pub recovered: bool,
    #[serde(rename = "kis", default)]
    pub key_images: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Quote {
    pub amount_pxmr: u64,
    pub fee_pxmr: u64,
    pub notes: usize,
    pub minutes_to_confirm: u32,
    pub total_pxmr: u64,
    pub remaining_pxmr: u64,
    pub affordable: bool,
    pub fee_known: bool,
}

#[derive(Clone, Debug)]
pub struct SendPlan {
    pub notes: Vec<WalletEntry>,
    pub amount_pxmr: u64,
    pub total_in_pxmr: u64,
    pub fee_pxmr: u64,
}

impl SendPlan {
    pub fn enough(&self) -> bool {
        self.total_in_pxmr >= self.amount_pxmr + self.fee_pxmr
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum SyncBlocker {
    None,
    NoWallet,
    NoNode,
    Failing,
}

#[derive(Clone, Debug, Serialize)]
pub struct Shown {
    pub primary: String,
    pub secondary: Option<String>,
    pub notional: bool,
    pub stale: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct FiatView {
    pub text: String,
    /// Stagenet coin: the number is what it would be worth, not what it is.
    pub notional: bool,
    pub stale: bool,
}

static WALLET: Mutex<()> = Mutex::new(());
static FEE_CACHE: Mutex<Option<HashMap<String, (u64, u64)>>> = Mutex::new(None);
static RATE_FAILED_AT: Mutex<u64> = Mutex::new(0);

pub fn format_xmr(pxmr: u64) -> String {
    let whole = pxmr / 1_000_000_000_000;
    let micro = (pxmr % 1_000_000_000_000) / 1_000_000;
    if whole == 0 && micro == 0 && pxmr > 0 {
        "<0.000001".to_string()
    } else {
        format!("{whole}.{micro:06}")
    }
}

pub fn exact_xmr(pxmr: u64) -> String {
    format!("{}.{:012}", pxmr / 1_000_000_000_000, pxmr % 1_000_000_000_000)
}

/// "0.02" → 20_000_000_000. Twelve decimals at most; anything else is None.
pub fn parse_xmr(s: &str) -> Option<u64> {
    let t = s.trim().replace(',', ".");
    if t.is_empty() {
        return None;
    }
    let (whole, frac) = match t.split_once('.') {
        Some((w, f)) => (w, f),
        None => (t.as_str(), ""),
    };
    if frac.len() > 12 || !whole.chars().all(|c| c.is_ascii_digit()) || !frac.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let whole: u64 = if whole.is_empty() { 0 } else { whole.parse().ok()? };
    let mut f = frac.to_string();
    while f.len() < 12 {
        f.push('0');
    }
    let frac: u64 = if f.is_empty() { 0 } else { f.parse().ok()? };
    whole.checked_mul(1_000_000_000_000)?.checked_add(frac)
}

fn take<T: serde::de::DeserializeOwned>(m: &serde_json::Map<String, Value>, key: &str) -> Option<T> {
    m.get(key).cloned().and_then(crate::store::value_as)
}

impl App {
    fn w(&self) -> crate::store::Store {
        self.store(CONTACTS)
    }

    // ----- keys and addresses ------------------------------------------------------

    pub fn wallet_save(&self, address: &str, spend_key_hex: &str, restore_height: u64, stagenet: bool) -> Result<(), Error> {
        self.w().update(|m| {
            m.insert("wallet_address".into(), Value::from(address));
            m.insert("wallet_spend".into(), Value::from(spend_key_hex));
            m.insert("wallet_height".into(), Value::from(restore_height.to_string()));
            m.insert("wallet_stagenet".into(), Value::from(stagenet));
        })?;
        bump();
        Ok(())
    }

    pub fn wallet_address(&self) -> Option<String> {
        self.w().get_string("wallet_address")
    }

    pub fn spend_key_hex(&self) -> Option<String> {
        self.w().get_string("wallet_spend")
    }

    pub fn wallet_stagenet(&self) -> bool {
        self.w().get::<bool>("wallet_stagenet").unwrap_or(true)
    }

    pub fn restore_height(&self) -> u64 {
        self.w().get_string("wallet_height").and_then(|s| s.parse().ok()).unwrap_or(0)
    }

    /// Mint a wallet if there is none, dated at the node's tip so it never
    /// scans history it cannot have.
    pub fn ensure_wallet(&self) -> Result<String, Error> {
        if let Some(a) = self.wallet_address() {
            return Ok(a);
        }
        let _g = WALLET.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(a) = self.wallet_address() {
            return Ok(a);
        }
        let tip = match self.last_good_node() {
            Some(url) => ducat_mobile::monero::monero_probe(url, 8_000).height,
            None => self.pick_node_status().map(|s| s.height).unwrap_or(0),
        };
        let w = ducat_mobile::create_wallet(tip, true);
        self.wallet_save(&w.address, &w.spend_key_hex, w.restore_height, true)?;
        log::info(TAG, format!("minted a wallet at height {tip}: {}…", &w.address[..12]));
        Ok(w.address)
    }

    /// §15.10: the subaddress a counterparty pays to — one minor per
    /// persona, allocated on first ask.
    pub fn address_for(&self, persona_hex: &str) -> Option<String> {
        let spend = self.spend_key_hex()?;
        let minor = self.minor_for(persona_hex);
        monero_subaddress(spend, minor, self.wallet_stagenet()).ok().or_else(|| self.wallet_address())
    }

    pub fn minor_for(&self, persona_hex: &str) -> u32 {
        let _g = WALLET.lock().unwrap_or_else(|e| e.into_inner());
        self.w()
            .update(|m| {
                let key = format!("sub_minor_{persona_hex}");
                if let Some(have) = m.get(&key).and_then(Value::as_u64).filter(|v| *v != 0) {
                    return have as u32;
                }
                let next = m.get("sub_next").and_then(Value::as_u64).unwrap_or(1) as u32;
                m.insert(key, Value::from(next));
                m.insert("sub_next".into(), Value::from(next + 1));
                next
            })
            .unwrap_or(0)
    }

    pub fn minor_of(&self, persona_hex: &str) -> Option<u32> {
        self.w().get::<u32>(&format!("sub_minor_{persona_hex}")).filter(|v| *v != 0)
    }

    pub fn subaddress_count(&self) -> u32 {
        self.w().get::<u32>("sub_next").unwrap_or(1).saturating_sub(1)
    }

    /// A minor allocated to a card moves to whoever answered it.
    pub fn adopt_minor(&self, card_key: &str, persona_hex: &str) {
        let _g = WALLET.lock().unwrap_or_else(|e| e.into_inner());
        let _ = self.w().update(|m| {
            let from = format!("sub_minor_{card_key}");
            let to = format!("sub_minor_{persona_hex}");
            let mv = m.get(&from).and_then(Value::as_u64).unwrap_or(0);
            if mv != 0 && m.get(&to).and_then(Value::as_u64).unwrap_or(0) == 0 {
                m.insert(to, Value::from(mv));
                m.remove(&from);
            }
        });
    }

    pub fn persona_for_minor(&self, minor: u32) -> Option<String> {
        if minor == 0 {
            return None;
        }
        self.w().view(|m| {
            m.iter()
                .find(|(k, v)| k.starts_with("sub_minor_") && v.as_u64() == Some(minor as u64))
                .map(|(k, _)| k.trim_start_matches("sub_minor_").to_string())
        })
    }

    // ----- send intents and history ---------------------------------------------

    pub fn send_intents(&self) -> Vec<SendIntent> {
        self.w().get("send_intents").unwrap_or_default()
    }

    fn record_send_intent(&self, to: &str, amount_pxmr: u64, key_images: Vec<String>, contact: Option<&str>, note: Option<&str>, donation: bool) -> Result<String, Error> {
        let id = format!("{:x}-{:x}", now_ms(), rand_u64());
        let intent = SendIntent {
            id: id.clone(),
            to_address: to.to_string(),
            amount_pxmr,
            key_images,
            contact: contact.map(String::from),
            note: note.map(String::from),
            ts: App::now(),
            donate: donation,
        };
        self.w().update(|m| {
            let mut list: Vec<SendIntent> = take(m, "send_intents").unwrap_or_default();
            list.push(intent);
            m.insert("send_intents".into(), serde_json::to_value(&list).unwrap_or(Value::Null));
        })?;
        Ok(id)
    }

    /// The only writer of `wallet_sends`: an intent becomes history when
    /// the chain says so or a node took the transaction. A blank txid is a
    /// send recovered from its notes turning up spent.
    pub fn resolve_send_intent(&self, id: &str, txid_hex: &str, fee_pxmr: u64) -> Result<(), Error> {
        let _g = WALLET.lock().unwrap_or_else(|e| e.into_inner());
        self.w().update(|m| {
            let mut intents: Vec<SendIntent> = take(m, "send_intents").unwrap_or_default();
            let Some(at) = intents.iter().position(|i| i.id == id) else { return };
            let it = intents.remove(at);
            let kis: HashSet<String> = it.key_images.iter().cloned().collect();
            let mut sends: Vec<SentPayment> = take(m, "wallet_sends").unwrap_or_default();
            sends.push(SentPayment {
                txid_hex: txid_hex.to_string(),
                amount_pxmr: it.amount_pxmr,
                fee: fee_pxmr,
                to_address: it.to_address,
                contact: it.contact,
                note: it.note,
                ts: App::now(),
                donate: it.donate,
                recovered: txid_hex.is_empty(),
                key_images: if txid_hex.is_empty() { it.key_images.clone() } else { Vec::new() },
            });
            m.insert("send_intents".into(), serde_json::to_value(&intents).unwrap_or(Value::Null));
            m.insert("wallet_sends".into(), serde_json::to_value(&sends).unwrap_or(Value::Null));
            if let Some(mut outs) = take::<Vec<WalletEntry>>(m, "wallet_outputs") {
                for o in outs.iter_mut().filter(|o| kis.contains(&o.key_image)) {
                    o.spent = true;
                }
                m.insert("wallet_outputs".into(), serde_json::to_value(&outs).unwrap_or(Value::Null));
            }
        })?;
        bump();
        Ok(())
    }

    pub fn drop_send_intent(&self, id: &str) -> Result<(), Error> {
        let _g = WALLET.lock().unwrap_or_else(|e| e.into_inner());
        self.w().update(|m| {
            let mut intents: Vec<SendIntent> = take(m, "send_intents").unwrap_or_default();
            intents.retain(|i| i.id != id);
            m.insert("send_intents".into(), serde_json::to_value(&intents).unwrap_or(Value::Null));
        })?;
        Ok(())
    }

    pub fn sends(&self) -> Vec<SentPayment> {
        self.w().get("wallet_sends").unwrap_or_default()
    }

    pub fn our_txids(&self) -> HashSet<String> {
        self.sends().into_iter().map(|s| s.txid_hex.to_lowercase()).collect()
    }

    // ----- the scan ---------------------------------------------------------------

    pub fn scanned_to(&self) -> u64 {
        self.w().get("wallet_scanned_to").unwrap_or(0)
    }

    pub fn tip(&self) -> u64 {
        self.w().get("wallet_tip").unwrap_or(0)
    }

    pub fn scan_rate(&self) -> f64 {
        self.w().get("wallet_rate").unwrap_or(0.0)
    }

    pub fn last_scan_error(&self) -> Option<String> {
        self.w().get_string("wallet_scan_error")
    }

    fn record_scan_error(&self, msg: Option<&str>) {
        let _ = match msg {
            Some(m) => self.w().put("wallet_scan_error", &m),
            None => self.w().remove("wallet_scan_error"),
        };
        bump();
    }

    pub fn entries(&self) -> Vec<WalletEntry> {
        self.w().get("wallet_outputs").unwrap_or_default()
    }

    fn record_scan(&self, scanned_to: u64, tip: u64, found: &[OwnedOutput]) -> Result<(), Error> {
        let _g = WALLET.lock().unwrap_or_else(|e| e.into_inner());
        self.w().update(|m| {
            let now = now_ms();
            let last_at = m.get("wallet_scan_at").and_then(Value::as_u64).unwrap_or(0);
            let last_to = m.get("wallet_scanned_to").and_then(Value::as_u64).unwrap_or(0);
            if last_at > 0 && scanned_to > last_to {
                let secs = (now.saturating_sub(last_at)) as f64 / 1000.0;
                if secs > 0.5 {
                    let observed = (scanned_to - last_to) as f64 / secs;
                    let prev = m.get("wallet_rate").and_then(Value::as_f64).unwrap_or(0.0);
                    let blended = if prev > 0.0 { prev * 0.7 + observed * 0.3 } else { observed };
                    m.insert("wallet_rate".into(), Value::from(blended));
                }
            }
            m.insert("wallet_scan_at".into(), Value::from(now));
            let mut by_ki: Vec<WalletEntry> = take(m, "wallet_outputs").unwrap_or_default();
            for o in found {
                if o.key_image_hex.is_empty() {
                    continue;
                }
                let spent = by_ki.iter().find(|e| e.key_image == o.key_image_hex).map_or(false, |e| e.spent);
                by_ki.retain(|e| e.key_image != o.key_image_hex);
                by_ki.push(WalletEntry {
                    amount_pxmr: o.amount_pxmr,
                    height: o.height,
                    spent,
                    key_image: o.key_image_hex.clone(),
                    blob: o.blob.clone(),
                    tx_hash_hex: o.tx_hash_hex.clone(),
                    timestamp: o.timestamp,
                    minor: o.minor,
                });
            }
            m.insert("wallet_outputs".into(), serde_json::to_value(&by_ki).unwrap_or(Value::Null));
            m.insert("wallet_scanned_to".into(), Value::from(scanned_to));
            m.insert("wallet_tip".into(), Value::from(tip));
        })?;
        bump();
        Ok(())
    }

    fn record_spent(&self, spent: &HashSet<String>) -> Result<(), Error> {
        let _g = WALLET.lock().unwrap_or_else(|e| e.into_inner());
        self.w().update(|m| {
            let mut outs: Vec<WalletEntry> = take(m, "wallet_outputs").unwrap_or_default();
            for o in outs.iter_mut().filter(|o| spent.contains(&o.key_image)) {
                o.spent = true;
            }
            m.insert("wallet_outputs".into(), serde_json::to_value(&outs).unwrap_or(Value::Null));
        })?;
        Ok(())
    }

    /// Forget every note and scan again from `height`.
    pub fn rescan_from(&self, height: u64) -> Result<(), Error> {
        let _g = WALLET.lock().unwrap_or_else(|e| e.into_inner());
        self.w().update(|m| {
            m.insert("wallet_height".into(), Value::from(height.to_string()));
            m.insert("wallet_scanned_to".into(), Value::from(height));
            for k in ["wallet_outputs", "wallet_rate", "wallet_scan_at", "wallet_scan_error"] {
                m.remove(k);
            }
        })?;
        bump();
        Ok(())
    }

    /// One step of the scan against `node_url`. True when it moved.
    pub fn scan_step(&self, node_url: &str) -> bool {
        let Some(spend) = self.spend_key_hex() else {
            return false;
        };
        let from = match self.scanned_to() {
            0 => self.restore_height(),
            n => n,
        };
        match monero_scan(node_url.to_string(), spend, from, WINDOW, self.subaddress_count()) {
            Ok(r) => {
                let known: HashSet<String> = self.entries().into_iter().map(|e| e.key_image).collect();
                if let Err(e) = self.record_scan(r.scanned_to, r.tip, &r.outputs) {
                    log::warn(TAG, format!("could not keep the scan: {e}"));
                }
                self.record_scan_error(None);
                self.node_succeeded();
                let ours = self.our_txids();
                for o in r.outputs.iter().filter(|o| !known.contains(&o.key_image_hex)) {
                    if ours.contains(&o.tx_hash_hex.to_lowercase()) {
                        log::info(TAG, format!("change back: {} XMR", format_xmr(o.amount_pxmr)));
                    } else {
                        log::info(TAG, format!("received {} XMR at block {}", format_xmr(o.amount_pxmr), o.height));
                    }
                }
                if r.blocks_failed > 0 {
                    log::warn(TAG, format!("scanned to {} — {} block(s) unreadable", r.scanned_to, r.blocks_failed));
                }
                r.scanned_to > from
            }
            Err(e) => {
                log::warn(TAG, format!("scan failed: {e}"));
                self.record_scan_error(Some(&e.to_string()));
                if self.node_failed() {
                    log::warn(TAG, "node demoted after repeated failures — will re-probe");
                }
                false
            }
        }
    }

    /// Ask the chain which of our notes are gone, and let it retire or
    /// release the send intents.
    pub fn refresh_spent(&self, node_url: &str) {
        let entries: Vec<WalletEntry> = self.entries().into_iter().filter(|e| !e.key_image.is_empty()).collect();
        if entries.is_empty() {
            return;
        }
        let kis: Vec<String> = entries.iter().map(|e| e.key_image.clone()).collect();
        match monero_spent(node_url.to_string(), kis.clone()) {
            Ok(spent) => {
                let chain_spent: HashSet<String> = kis.iter().zip(spent.iter()).filter(|(_, s)| **s).map(|(k, _)| k.clone()).collect();
                let _ = self.record_spent(&chain_spent);
                let answered: HashSet<String> = kis.into_iter().collect();
                let now = App::now();
                for intent in self.send_intents() {
                    let mine: HashSet<String> = intent.key_images.iter().cloned().collect();
                    if mine.iter().any(|k| chain_spent.contains(k)) {
                        log::warn(TAG, format!("send intent {} resolved by chain — recording without txid", intent.id));
                        let _ = self.resolve_send_intent(&intent.id, "", 0);
                    } else if now.saturating_sub(intent.ts) >= INTENT_GIVE_UP_SECS
                        && !mine.is_empty()
                        && mine.iter().all(|k| answered.contains(k) && !chain_spent.contains(k))
                    {
                        log::warn(TAG, format!("send intent {} never relayed — releasing its notes", intent.id));
                        let _ = self.drop_send_intent(&intent.id);
                    }
                }
            }
            Err(e) => log::warn(TAG, format!("spent check: {e}")),
        }
    }

    fn usable_notes(&self) -> Vec<WalletEntry> {
        let tip = self.tip();
        let in_flight: HashSet<String> = self.send_intents().into_iter().flat_map(|i| i.key_images).collect();
        let mut usable: Vec<WalletEntry> = self
            .entries()
            .into_iter()
            .filter(|e| !e.spent && !e.blob.is_empty() && tip > 0 && e.height + LOCK_BLOCKS <= tip && !in_flight.contains(&e.key_image))
            .collect();
        usable.sort_by(|a, b| b.amount_pxmr.cmp(&a.amount_pxmr));
        usable
    }

    pub fn balances(&self) -> Balances {
        let tip = self.tip();
        let in_flight: HashSet<String> = self.send_intents().into_iter().flat_map(|i| i.key_images).collect();
        let unspent: Vec<WalletEntry> = self.entries().into_iter().filter(|e| !e.spent && !e.blob.is_empty() && !in_flight.contains(&e.key_image)).collect();
        let (unlocked, locked): (Vec<&WalletEntry>, Vec<&WalletEntry>) = unspent.iter().partition(|e| tip > 0 && e.height + LOCK_BLOCKS <= tip);
        let soonest = locked.iter().map(|e| (e.height + LOCK_BLOCKS).saturating_sub(tip)).min().unwrap_or(0);
        let scanned_to = self.scanned_to();
        let scan_from = self.restore_height();
        let rate = self.scan_rate();
        let blocks_left = tip.saturating_sub(scanned_to);
        let span = tip.saturating_sub(scan_from);
        Balances {
            spendable_pxmr: unlocked.iter().map(|e| e.amount_pxmr).sum(),
            locked_pxmr: locked.iter().map(|e| e.amount_pxmr).sum(),
            spendable_outputs: unlocked.len(),
            blocks_to_unlock: soonest,
            scanned_to,
            tip,
            scan_rate: rate,
            scan_from,
            error: self.last_scan_error(),
            syncing: tip > 0 && scanned_to < tip,
            blocks_left,
            progress: if tip == 0 || span == 0 { 0.0 } else { ((scanned_to.saturating_sub(scan_from)) as f32 / span as f32).clamp(0.0, 1.0) },
            seconds_left: if rate > 0.01 && blocks_left > 0 { Some((blocks_left as f64 / rate) as u64) } else { None },
        }
    }

    pub fn blocker(&self) -> SyncBlocker {
        if self.spend_key_hex().is_none() {
            SyncBlocker::NoWallet
        } else if self.last_scan_error().is_some() {
            SyncBlocker::Failing
        } else if self.tip() == 0 {
            SyncBlocker::NoNode
        } else {
            SyncBlocker::None
        }
    }

    // ----- fees and plans -----------------------------------------------------------

    /// A fee for `inputs` notes at `priority`, from the node, cached a
    /// minute; zero (cached fifteen seconds) when no node answers.
    pub fn fee_for(&self, inputs: usize, priority: u32) -> u64 {
        let Some(node) = self.last_good_node() else { return 0 };
        let key = format!("{node}|{inputs}|{priority}");
        let now = now_ms();
        {
            let g = FEE_CACHE.lock().unwrap_or_else(|e| e.into_inner());
            if let Some((fee, at)) = g.as_ref().and_then(|c| c.get(&key)).copied() {
                let ttl = if fee == 0 { 15_000 } else { 60_000 };
                if now.saturating_sub(at) < ttl {
                    return fee;
                }
            }
        }
        let fee = match monero_fee_estimate(node, inputs.max(1) as u32, 2, priority) {
            Ok(f) => f.fee_pxmr,
            Err(e) => {
                log::warn(TAG, format!("fee estimate: {e}"));
                0
            }
        };
        let mut g = FEE_CACHE.lock().unwrap_or_else(|e| e.into_inner());
        let c = g.get_or_insert_with(HashMap::new);
        if c.len() > 64 {
            c.clear();
        }
        c.insert(key, (fee, now));
        fee
    }

    pub fn minutes_to_confirm(priority: u32) -> u32 {
        match priority {
            0 => 20,
            1 => 6,
            2 => 4,
            _ => 2,
        }
    }

    pub fn plan(&self, amount_pxmr: u64, priority: u32) -> SendPlan {
        let usable = self.usable_notes();
        let mut picked: Vec<WalletEntry> = Vec::new();
        let mut total = 0u64;
        let mut fee;
        for n in usable {
            fee = self.fee_for(picked.len(), priority);
            if total >= amount_pxmr + fee {
                break;
            }
            total += n.amount_pxmr;
            picked.push(n);
        }
        fee = self.fee_for(picked.len(), priority);
        SendPlan { notes: picked, amount_pxmr, total_in_pxmr: total, fee_pxmr: fee }
    }

    pub fn quote(&self, amount_pxmr: u64, priority: u32) -> Quote {
        let b = self.balances();
        let plan = self.plan(amount_pxmr, priority);
        let inputs = if plan.notes.is_empty() { b.spendable_outputs } else { plan.notes.len() };
        let fee = self.fee_for(inputs, priority);
        let total = amount_pxmr + fee;
        Quote {
            amount_pxmr,
            fee_pxmr: fee,
            notes: inputs,
            minutes_to_confirm: App::minutes_to_confirm(priority),
            total_pxmr: total,
            remaining_pxmr: b.spendable_pxmr.saturating_sub(total),
            affordable: plan.enough(),
            fee_known: fee > 0,
        }
    }

    /// The most a single send can carry: every note worth more than its
    /// own share of the fee, minus the fee for that many.
    pub fn max_sendable(&self, priority: u32) -> u64 {
        let b = self.balances();
        if b.spendable_outputs == 0 {
            return 0;
        }
        let all = b.spendable_outputs;
        let fee_one = self.fee_for(1, priority);
        let fee_all = self.fee_for(all, priority);
        let per_note = if all > 1 { fee_all.saturating_sub(fee_one) / (all as u64 - 1) } else { 0 };
        let worth: Vec<WalletEntry> = self.usable_notes().into_iter().filter(|e| e.amount_pxmr > per_note).collect();
        if worth.is_empty() {
            return 0;
        }
        let sum: u64 = worth.iter().map(|e| e.amount_pxmr).sum();
        sum.saturating_sub(self.fee_for(worth.len(), priority))
    }

    /// Send. The intent is recorded before the build; only a failure that
    /// provably built nothing releases the notes.
    pub fn send_xmr(&self, to_address: &str, amount_pxmr: u64, contact_hex: Option<&str>, note: Option<&str>, priority: u32, donation: bool) -> Result<SendResult, Error> {
        let spend = self.spend_key_hex().ok_or_else(|| Error::Refused("no wallet on this desk".into()))?;
        let node = self.last_good_node().or_else(|| self.pick_node()).ok_or_else(|| Error::Refused("no Monero node answers right now".into()))?;
        let plan = self.plan(amount_pxmr, priority);
        if !plan.enough() {
            return Err(Error::Refused(format!(
                "not enough unlocked — {} of {} XMR needed with the fee",
                format_xmr(plan.total_in_pxmr),
                format_xmr(amount_pxmr + plan.fee_pxmr)
            )));
        }
        let intent = self.record_send_intent(to_address, amount_pxmr, plan.notes.iter().map(|n| n.key_image.clone()).collect(), contact_hex, note, donation)?;
        log::info(TAG, format!("sending {} XMR using {} note(s) to {}…", format_xmr(amount_pxmr), plan.notes.len(), &to_address[..12.min(to_address.len())]));
        match monero_send(node, spend, plan.notes.iter().map(|n| n.blob.clone()).collect(), to_address.to_string(), amount_pxmr, priority) {
            Ok(r) => {
                log::info(TAG, format!("sent {}… fee {} XMR, accepted by {} node(s)", &r.txid_hex[..16.min(r.txid_hex.len())], format_xmr(r.fee_pxmr), r.accepted_by));
                self.resolve_send_intent(&intent, &r.txid_hex, r.fee_pxmr)?;
                Ok(r)
            }
            Err(e) => {
                let why = e.to_string();
                log::error(TAG, format!("send failed: {why}"));
                if is_node_trouble(&why) {
                    self.node_unreachable();
                    log::warn(TAG, "node did not answer — demoted, next try re-probes");
                } else if self.node_failed() {
                    log::warn(TAG, "node demoted after repeated failures — will re-probe");
                }
                if never_left(&why) {
                    let _ = self.drop_send_intent(&intent);
                    log::info(TAG, "nothing was built — the notes are free again");
                }
                Err(e.into())
            }
        }
    }

    // ----- the node -------------------------------------------------------------------

    pub fn monero_own_url(&self) -> Option<String> {
        self.w().get_string("monero_own_node").filter(|s| !s.trim().is_empty())
    }

    pub fn set_monero_own_url(&self, url: Option<&str>) -> Result<(), Error> {
        match url.map(str::trim).filter(|u| !u.is_empty()) {
            Some(u) => self.w().put("monero_own_node", &u)?,
            None => self.w().remove("monero_own_node")?,
        }
        // A new node is a new pick.
        self.w().remove("monero_last_good")?;
        bump();
        Ok(())
    }

    pub fn last_good_node(&self) -> Option<String> {
        self.w().get_string("monero_last_good")
    }

    fn node_succeeded(&self) {
        let _ = self.w().put("monero_node_fails", &0u32);
    }

    /// Three strikes and the node is forgotten; true when that happened.
    fn node_failed(&self) -> bool {
        let n = self.w().get::<u32>("monero_node_fails").unwrap_or(0) + 1;
        if n >= 3 {
            let _ = self.w().update(|m| {
                m.remove("monero_last_good");
                m.insert("monero_node_fails".into(), Value::from(0));
            });
            true
        } else {
            let _ = self.w().put("monero_node_fails", &n);
            false
        }
    }

    fn node_unreachable(&self) {
        let _ = self.w().update(|m| {
            m.remove("monero_last_good");
            m.insert("monero_node_fails".into(), Value::from(0));
        });
    }

    fn pick_node_status(&self) -> Option<ducat_mobile::monero::MoneroNodeStatus> {
        match monero_pick_node(monero_default_nodes(self.monero_own_url()), NETTYPE.into(), 8_000) {
            Ok(s) => {
                let _ = self.w().put("monero_last_good", &s.url);
                log::info(TAG, format!("picked node {} at height {}", s.url, s.height));
                Some(s)
            }
            Err(e) => {
                log::warn(TAG, format!("no node: {e}"));
                None
            }
        }
    }

    /// Probe the candidates and remember the first usable one.
    pub fn pick_node(&self) -> Option<String> {
        self.pick_node_status().map(|s| s.url)
    }

    /// One turn of the wallet lane: a node, a wallet, a scan step, and the
    /// spent check when the scan moved.
    pub fn wallet_lap(&self) {
        let Some(node) = self.last_good_node().or_else(|| self.pick_node()) else { return };
        if self.wallet_address().is_none() {
            if let Err(e) = self.ensure_wallet() {
                log::warn(TAG, format!("could not mint a wallet: {e}"));
                return;
            }
        }
        if self.scan_step(&node) {
            self.refresh_spent(&node);
        }
        self.tabs_lap(&node);
        self.reconcile_donations();
        self.enrich_ledger(&node, 6);
        self.publications_lap();
        self.rates_refresh();
    }

    // ----- rates (§ fiat view) ----------------------------------------------------------

    pub fn rate_enabled(&self) -> bool {
        self.w().get("rate_enabled").unwrap_or(true)
    }

    pub fn set_rate_enabled(&self, v: bool) -> Result<(), Error> {
        self.w().put("rate_enabled", &v)?;
        bump();
        Ok(())
    }

    pub fn prefer_fiat(&self) -> bool {
        self.w().get("rate_prefer_fiat").unwrap_or(true)
    }

    pub fn set_prefer_fiat(&self, v: bool) -> Result<(), Error> {
        self.w().put("rate_prefer_fiat", &v)?;
        bump();
        Ok(())
    }

    pub fn rate_currency(&self) -> String {
        self.w().get_string("rate_currency").unwrap_or_else(|| "USD".into())
    }

    pub fn set_rate_currency(&self, code: &str) -> Result<(), Error> {
        self.w().update(|m| {
            m.insert("rate_currency".into(), Value::from(code.to_uppercase()));
            m.remove("rate_value");
        })?;
        bump();
        Ok(())
    }

    /// The cached rate and its stamp (seconds), for callers outside.
    pub fn rate_cached_pair(&self) -> Option<(f64, u64)> {
        self.rate_cached()
    }

    /// An amount the way the reader wants it: fiat first when preferred
    /// and a rate is known, XMR beside it — or XMR alone.
    pub fn show_amount(&self, pxmr: u64) -> Shown {
        let xmr = format!("{} XMR", format_xmr(pxmr));
        match self.rate_view(pxmr) {
            Some(f) if self.prefer_fiat() => Shown { primary: f.text, secondary: Some(xmr), notional: f.notional, stale: f.stale },
            Some(f) => Shown { primary: xmr, secondary: Some(f.text), notional: f.notional, stale: f.stale },
            None => Shown { primary: xmr, secondary: None, notional: false, stale: false },
        }
    }

    fn rate_cached(&self) -> Option<(f64, u64)> {
        let v: f64 = self.w().get("rate_value").unwrap_or(0.0);
        let at: u64 = self.w().get("rate_at").unwrap_or(0);
        (v > 0.0 && at > 0).then_some((v, at))
    }

    /// Stale after half an hour — or when stamped in the future, which a
    /// wound-forward clock produced and which froze the refresh for good.
    pub fn rate_stale(&self) -> bool {
        let Some((_, at)) = self.rate_cached() else { return true };
        let now = App::now() as i64;
        let age = now - at as i64;
        age > 1800 || age < -60
    }

    pub fn rates_refresh(&self) {
        if !self.rate_enabled() || !self.rate_stale() {
            return;
        }
        {
            let failed = *RATE_FAILED_AT.lock().unwrap_or_else(|e| e.into_inner());
            if now_ms().saturating_sub(failed) < 60_000 {
                return;
            }
        }
        let currency = self.rate_currency();
        match monero_rate(currency.clone(), 12_000, self.rate_cached().map(|c| c.0)) {
            Ok(r) => {
                let _ = self.w().update(|m| {
                    m.insert("rate_value".into(), Value::from(r.per_xmr));
                    m.insert("rate_at".into(), Value::from(r.fetched_at));
                    m.insert("rate_source".into(), Value::from(r.source.clone()));
                    if currency.eq_ignore_ascii_case("USD") {
                        m.insert("rate_usd".into(), Value::from(r.per_xmr));
                    }
                });
                *RATE_FAILED_AT.lock().unwrap_or_else(|e| e.into_inner()) = 0;
                bump();
            }
            Err(e) => {
                *RATE_FAILED_AT.lock().unwrap_or_else(|e| e.into_inner()) = now_ms();
                log::warn(TAG, format!("rate: {e}"));
            }
        }
    }

    pub fn rate_view(&self, pxmr: u64) -> Option<FiatView> {
        if !self.rate_enabled() {
            return None;
        }
        let (rate, _) = self.rate_cached()?;
        let amount = pxmr as f64 / 1e12 * rate;
        Some(FiatView { text: format!("{} {:.2}", self.rate_currency(), amount), notional: self.wallet_stagenet(), stale: self.rate_stale() })
    }
}

fn rand_u64() -> u64 {
    let mut b = [0u8; 8];
    getrandom_fill(&mut b);
    u64::from_le_bytes(b)
}

fn getrandom_fill(b: &mut [u8]) {
    // A send intent id only has to be unique on this desk; the clock and
    // the address of a fresh allocation are plenty.
    let seed = now_ms() ^ (Box::into_raw(Box::new(0u8)) as u64).rotate_left(17);
    let mut x = seed | 1;
    for v in b.iter_mut() {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        *v = x as u8;
    }
}

fn is_node_trouble(why: &str) -> bool {
    let w = why.to_lowercase();
    ["timed out", "timeout", "interfaceerror", "network error", "connection", "unexpected eof", "decoys"].iter().any(|k| w.contains(k))
}

/// Failures that happen before anything is signed or relayed — the notes
/// were never at risk and go straight back into the float.
fn never_left(why: &str) -> bool {
    [
        "nothing to spend",
        "spend key is not",
        "scalar:",
        "view pair:",
        "address:",
        "stored output:",
        "runtime:",
        "connect:",
        "height:",
        "decoys:",
        "fee rate:",
        "not enough in the notes you picked",
        "too many notes at once",
        "no notes selected",
        "no destination",
        "could not build the transaction",
        "signing:",
    ]
    .iter()
    .any(|p| why.starts_with(p))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xmr_amounts_parse_and_print_exactly() {
        assert_eq!(parse_xmr("0.02"), Some(20_000_000_000));
        assert_eq!(parse_xmr("1"), Some(1_000_000_000_000));
        assert_eq!(parse_xmr(".5"), Some(500_000_000_000));
        assert_eq!(parse_xmr("0.000000000001"), Some(1));
        assert_eq!(parse_xmr("0.0000000000001"), None);
        assert_eq!(parse_xmr("abc"), None);
        assert_eq!(format_xmr(20_000_000_000), "0.020000");
        assert_eq!(format_xmr(1), "<0.000001");
        assert_eq!(exact_xmr(1_500_000_000_000), "1.500000000000");
    }

    #[test]
    fn a_send_claims_its_notes_and_the_chain_retires_the_claim() {
        let dir = std::env::temp_dir().join(format!("ducat-wallet-{}-intents", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let app = App::open(&dir).unwrap();
        let notes = vec![
            WalletEntry { amount_pxmr: 5, height: 1, spent: false, key_image: "a".into(), blob: vec![1], tx_hash_hex: "t1".into(), timestamp: 0, minor: 0 },
            WalletEntry { amount_pxmr: 7, height: 1, spent: false, key_image: "b".into(), blob: vec![2], tx_hash_hex: "t2".into(), timestamp: 0, minor: 0 },
        ];
        app.w().put("wallet_outputs", &notes).unwrap();
        app.w().put("wallet_tip", &100u64).unwrap();
        assert_eq!(app.balances().spendable_pxmr, 12);
        let id = app.record_send_intent("4addr", 5, vec!["a".into()], None, Some("lunch"), false).unwrap();
        // Claimed: the note is out of the float while the intent stands.
        assert_eq!(app.balances().spendable_pxmr, 7);
        app.resolve_send_intent(&id, "deadbeef", 1).unwrap();
        assert!(app.send_intents().is_empty());
        let sent = app.sends();
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].txid_hex, "deadbeef");
        assert_eq!(sent[0].note.as_deref(), Some("lunch"));
        assert!(app.entries().iter().find(|e| e.key_image == "a").unwrap().spent);
        assert_eq!(app.balances().spendable_pxmr, 7);
        // A dropped intent frees its notes.
        let id2 = app.record_send_intent("4addr", 7, vec!["b".into()], None, None, false).unwrap();
        assert_eq!(app.balances().spendable_pxmr, 0);
        app.drop_send_intent(&id2).unwrap();
        assert_eq!(app.balances().spendable_pxmr, 7);
    }

    #[test]
    fn each_persona_gets_its_own_minor_once() {
        let dir = std::env::temp_dir().join(format!("ducat-wallet-{}-minors", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let app = App::open(&dir).unwrap();
        assert_eq!(app.minor_for("p1"), 1);
        assert_eq!(app.minor_for("p2"), 2);
        assert_eq!(app.minor_for("p1"), 1);
        assert_eq!(app.subaddress_count(), 2);
        assert_eq!(app.persona_for_minor(2).as_deref(), Some("p2"));
        assert_eq!(app.minor_for("card_x"), 3);
        app.adopt_minor("card_x", "p3");
        assert_eq!(app.minor_of("p3"), Some(3));
        assert_eq!(app.minor_of("card_x"), None);
    }
}
