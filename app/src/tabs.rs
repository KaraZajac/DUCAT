//! Tabs: a running bill with one counterparty — the bar tab, the till's
//! sale, a fare, a kiosk order, a subscription's issue. One shape for all
//! of them, because what happens after "settle" is the same: a bill goes
//! out (§16.13), the chain is watched for money that matches it, and a
//! receipt goes back when it lands. The phone's `Tabs.kt`.
//!
//! States: `open` → `settled` (billed, watching) → `paid` | `paid_oob` |
//! `cancelled`. A tab never goes backwards except a settle whose bill
//! provably never left.

use std::sync::Mutex;

use ducat_mobile::monero::monero_scan_pool;
use serde::{Deserialize, Serialize};

use crate::contacts::{bump, referent, BillItem, Contact, StoredMessage, CONTACTS};
use crate::mailbox::Outgoing;
use crate::wallet::{format_xmr, WalletEntry};
use crate::{log, App, Error};

const TAG: &str = "Tabs";
/// A tab opened by a screen and never settled is swept after a day —
/// except the bar's, which a person opened on purpose and closes.
pub const ABANDONED_MS: u64 = 24 * 60 * 60 * 1000;
/// `word_seq` for "the receipt is owed and has not gone out".
pub const WORD_UNSENT: i64 = -2;

pub const ORIGIN_BAR: &str = "bar";
pub const ORIGIN_POS: &str = "pos";
pub const ORIGIN_TAXI: &str = "taxi";
pub const ORIGIN_KIOSK: &str = "kiosk";
pub const ORIGIN_PUB: &str = "pub";

static TABS: Mutex<()> = Mutex::new(());
static LAST_OWED: Mutex<Option<String>> = Mutex::new(None);

fn bar() -> String {
    ORIGIN_BAR.into()
}

fn minus_one() -> i64 {
    -1
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RunningTab {
    pub id: String,
    #[serde(default = "bar")]
    pub origin: String,
    #[serde(rename = "persona")]
    pub persona_hex: String,
    /// Millis.
    #[serde(rename = "opened")]
    pub opened_at: u64,
    #[serde(default)]
    pub lines: Vec<BillItem>,
    #[serde(default)]
    pub tax: Option<u64>,
    pub state: String,
    #[serde(rename = "total", default)]
    pub settled_total: u64,
    /// Millis.
    #[serde(rename = "settled_at", default)]
    pub settled_at: u64,
    /// Key images already in the wallet when the bill went out — none of
    /// them can be its payment.
    #[serde(rename = "known", default)]
    pub known_kis: Vec<String>,
    #[serde(rename = "paid_ki", default)]
    pub paid_ki: Option<String>,
    #[serde(rename = "seen_tx", default)]
    pub seen_tx: Option<String>,
    #[serde(rename = "tip_at_bill", default)]
    pub tip_at_bill: u64,
    #[serde(rename = "billed_minor", default)]
    pub billed_minor: Option<u32>,
    #[serde(rename = "bill_seq", default = "minus_one")]
    pub bill_seq: i64,
    #[serde(rename = "paid_total", default)]
    pub paid_pxmr: u64,
    #[serde(rename = "word_seq", default = "minus_one")]
    pub word_seq: i64,
}

impl RunningTab {
    pub fn total_pxmr(&self) -> u64 {
        if self.state == "open" {
            self.lines.iter().map(|l| l.amount_pxmr).sum::<u64>() + self.tax.unwrap_or(0)
        } else {
            self.settled_total
        }
    }

    pub fn kept_elsewhere(&self) -> bool {
        self.origin != ORIGIN_BAR
    }

    pub fn take_pxmr(&self) -> u64 {
        if self.paid_pxmr > 0 {
            self.paid_pxmr
        } else {
            self.total_pxmr()
        }
    }

    pub fn tip_pxmr(&self) -> u64 {
        self.paid_pxmr.saturating_sub(self.settled_total)
    }

    /// The bill row this tab sent, in the thread.
    pub fn bill_in<'a>(&self, thread: &'a [StoredMessage]) -> Option<&'a StoredMessage> {
        let bills: Vec<&StoredMessage> = thread.iter().filter(|m| m.outgoing && m.kind == 1).collect();
        if self.bill_seq < 0 {
            return bills.iter().rev().find(|m| m.amount_pxmr == self.settled_total).copied();
        }
        let at_seq: Vec<&StoredMessage> = bills.iter().filter(|m| m.seq as i64 == self.bill_seq).copied().collect();
        at_seq
            .iter()
            .filter(|m| m.timestamp * 1000 + 60_000 >= self.settled_at)
            .min_by_key(|m| m.timestamp)
            .or_else(|| at_seq.last())
            .copied()
    }

    /// Whether a note arrived where this tab's bill asked for it.
    pub fn paid_where_billed(&self, want_minor: Option<u32>, minor: u32) -> bool {
        match self.billed_minor {
            Some(b) => minor == b,
            None => minor == 0 || want_minor.is_none() || Some(minor) == want_minor,
        }
    }

    /// The amounts this counterparty said they paid for this bill (§16.13's
    /// notices), or paid unprompted after it.
    fn said(&self, thread: &[StoredMessage]) -> Vec<u64> {
        let bill = self.bill_in(thread);
        thread
            .iter()
            .filter(|m| {
                !m.outgoing
                    && m.kind == 2
                    && m.timestamp * 1000 + 60_000 >= self.settled_at
                    && m.amount_pxmr >= self.settled_total
                    && (m.re_seq.is_none() || bill.map_or(false, |b| referent(thread, m).map_or(false, |r| std::ptr::eq(r, b))))
            })
            .map(|m| m.amount_pxmr)
            .collect()
    }
}

pub fn bill_note(origin: &str) -> &'static str {
    match origin {
        ORIGIN_TAXI => "Your fare",
        ORIGIN_KIOSK => "Your order",
        ORIGIN_POS => "Your bill",
        ORIGIN_PUB => "Subscription",
        _ => "Your tab",
    }
}

pub const RECEIPT_NOTE: &str = "Receipt — thank you";
pub const RECEIPT_NOTE_OOB: &str = "Receipt — settled outside DUCAT. Thank you";
pub const TIP_LINE: &str = "Tip — thank you";

/// Unique on this desk: the clock, the process, and a counter for two
/// tabs opened in the same millisecond.
fn new_id() -> String {
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    format!("{:x}-{:x}-{:x}", crate::contacts::now_ms(), std::process::id(), n)
}

impl App {
    pub fn tabs(&self) -> Vec<RunningTab> {
        self.store(CONTACTS).get("tabs_v1").unwrap_or_default()
    }

    pub fn tab(&self, id: &str) -> Option<RunningTab> {
        self.tabs().into_iter().find(|t| t.id == id)
    }

    fn save_tabs(&self, tabs: &[RunningTab]) -> Result<(), Error> {
        self.store(CONTACTS).put("tabs_v1", &tabs)?;
        bump();
        Ok(())
    }

    pub fn open_tab(&self, persona_hex: &str, origin: &str) -> Result<RunningTab, Error> {
        let _g = TABS.lock().unwrap_or_else(|e| e.into_inner());
        self.open_tab_locked(persona_hex, origin)
    }

    fn open_tab_locked(&self, persona_hex: &str, origin: &str) -> Result<RunningTab, Error> {
        let t = RunningTab {
            id: new_id(),
            origin: origin.to_string(),
            persona_hex: persona_hex.to_string(),
            opened_at: crate::contacts::now_ms(),
            lines: Vec::new(),
            tax: None,
            state: "open".into(),
            settled_total: 0,
            settled_at: 0,
            known_kis: Vec::new(),
            paid_ki: None,
            seen_tx: None,
            tip_at_bill: 0,
            billed_minor: None,
            bill_seq: -1,
            paid_pxmr: 0,
            word_seq: -1,
        };
        let mut all = self.tabs();
        all.push(t.clone());
        self.save_tabs(&all)?;
        Ok(t)
    }

    /// The open tab with this person at this origin, or a fresh one.
    pub fn open_or_resume_tab(&self, persona_hex: &str, origin: &str) -> Result<RunningTab, Error> {
        let _g = TABS.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(t) = self.tabs().into_iter().find(|t| t.persona_hex == persona_hex && t.origin == origin && t.state == "open") {
            return Ok(t);
        }
        self.open_tab_locked(persona_hex, origin)
    }

    pub fn mutate_tab<F: FnOnce(RunningTab) -> RunningTab>(&self, id: &str, f: F) -> Result<Option<RunningTab>, Error> {
        let _g = TABS.lock().unwrap_or_else(|e| e.into_inner());
        self.mutate_tab_locked(id, f)
    }

    fn mutate_tab_locked<F: FnOnce(RunningTab) -> RunningTab>(&self, id: &str, f: F) -> Result<Option<RunningTab>, Error> {
        let mut all = self.tabs();
        let Some(i) = all.iter().position(|t| t.id == id) else { return Ok(None) };
        let next = f(all[i].clone());
        all[i] = next.clone();
        self.save_tabs(&all)?;
        Ok(Some(next))
    }

    pub fn delete_tab(&self, id: &str) -> Result<(), Error> {
        let _g = TABS.lock().unwrap_or_else(|e| e.into_inner());
        let all: Vec<RunningTab> = self.tabs().into_iter().filter(|t| t.id != id).collect();
        self.save_tabs(&all)
    }

    /// Tabs a screen opened and walked away from.
    pub fn sweep_abandoned_tabs(&self, keep: &[String]) -> usize {
        let _g = TABS.lock().unwrap_or_else(|e| e.into_inner());
        let now = crate::contacts::now_ms();
        let all = self.tabs();
        let (dead, live): (Vec<RunningTab>, Vec<RunningTab>) = all
            .into_iter()
            .partition(|t| t.state == "open" && t.origin != ORIGIN_BAR && !keep.contains(&t.id) && now.saturating_sub(t.opened_at) >= ABANDONED_MS);
        if dead.is_empty() {
            return 0;
        }
        let _ = self.save_tabs(&live);
        log::info(TAG, format!("swept {} abandoned tab(s)", dead.len()));
        dead.len()
    }

    /// Key images already spent on a receipt, so a second tab cannot claim
    /// the same note.
    pub fn claimed_kis(&self) -> Vec<String> {
        self.store(CONTACTS).get("claimed_kis_v1").unwrap_or_default()
    }

    fn mark_tab_paid(&self, id: &str, ki: &str, paid_pxmr: u64) -> Result<Option<RunningTab>, Error> {
        let _g = TABS.lock().unwrap_or_else(|e| e.into_inner());
        let mut all = self.tabs();
        let Some(i) = all.iter().position(|t| t.id == id) else { return Ok(None) };
        let mut next = all[i].clone();
        next.state = "paid".into();
        next.paid_ki = Some(ki.to_string());
        next.paid_pxmr = paid_pxmr;
        all[i] = next.clone();
        let mut claimed = self.claimed_kis();
        if !claimed.iter().any(|k| k == ki) {
            claimed.push(ki.to_string());
        }
        self.store(CONTACTS).update(|m| {
            m.insert("tabs_v1".into(), serde_json::to_value(&all).unwrap_or(serde_json::Value::Null));
            m.insert("claimed_kis_v1".into(), serde_json::to_value(&claimed).unwrap_or(serde_json::Value::Null));
        })?;
        bump();
        Ok(Some(next))
    }

    /// Bill it: the tab freezes at its total, remembers what the wallet
    /// already held, and the bill goes out with the subaddress this person
    /// pays. A send that failed after the row was committed leaves the tab
    /// settled — the bill exists, it is only late.
    pub fn settle_tab(&self, tab: &RunningTab) -> Result<RunningTab, Error> {
        let contact = self.contact(&tab.persona_hex).ok_or_else(|| Error::Refused("that contact is gone".into()))?;
        let total: u64 = tab.lines.iter().map(|l| l.amount_pxmr).sum::<u64>() + tab.tax.unwrap_or(0);
        let payto = self.address_for(&tab.persona_hex);
        let tip = self.tip();
        let known: Vec<String> = self.entries().into_iter().filter(|e| tip == 0 || e.height == 0 || e.height > tip).map(|e| e.key_image).collect();
        let lines = tab.lines.clone();
        let tax = tab.tax;
        self.mutate_tab(&tab.id, |t| RunningTab {
            state: "settled".into(),
            lines: lines.clone(),
            tax,
            settled_total: total,
            settled_at: crate::contacts::now_ms(),
            known_kis: known.clone(),
            tip_at_bill: tip,
            ..t
        })?
        .ok_or_else(|| Error::Refused("that tab is gone".into()))?;
        let seq_before = self.contact(&tab.persona_hex).map_or(contact.out_seq, |c| c.out_seq);
        let out = Outgoing {
            body: bill_note(&tab.origin).into(),
            kind: 1,
            amount_pxmr: Some(total),
            payto: payto.clone(),
            items: tab.lines.clone(),
            tax_pxmr: tab.tax,
            ..Default::default()
        };
        if let Err(e) = self.send(&contact, out) {
            if !self.bill_committed(&tab.persona_hex, total, seq_before) {
                self.mutate_tab(&tab.id, |t| if t.state == "settled" && t.seen_tx.is_none() { RunningTab { state: "open".into(), ..t } } else { t })?;
                return Err(e);
            }
            log::warn(TAG, format!("bill to {} is committed but not delivered: {e}", contact.display_name()));
        }
        let bill_seq = self.thread(&tab.persona_hex).iter().rev().find(|m| m.outgoing && m.kind == 1).map_or(-1, |m| m.seq as i64);
        let main = self.wallet_address();
        let billed_minor = self.minor_of(&tab.persona_hex).filter(|_| payto.is_some() && payto != main);
        let settled = self
            .mutate_tab(&tab.id, |t| RunningTab { bill_seq, billed_minor, ..t })?
            .ok_or_else(|| Error::Refused("that tab is gone".into()))?;
        log::info(TAG, format!("settled {} tab with {}: {} XMR", tab.origin, contact.display_name(), format_xmr(total)));
        Ok(settled)
    }

    fn bill_committed(&self, persona_hex: &str, total: u64, seq_before: u64) -> bool {
        self.thread(persona_hex)
            .iter()
            .rev()
            .find(|m| m.outgoing)
            .map_or(false, |m| m.kind == 1 && m.amount_pxmr == total && m.seq >= seq_before && !m.delivered)
    }

    /// Move a settled tab to a closed state and say so in the thread. The
    /// word is sent after the state lands; if it cannot go out, `revert`
    /// decides whether the tab goes back to settled or the word is owed.
    fn close_tab<F: FnOnce(&Contact) -> Result<Contact, Error>>(&self, tab: &RunningTab, state: &str, revert: bool, word: F) -> Result<Option<RunningTab>, Error> {
        let mut applied = false;
        let closed = self.mutate_tab(&tab.id, |t| {
            if t.state == "settled" && t.seen_tx.is_none() {
                applied = true;
                RunningTab { state: state.into(), paid_pxmr: if state == "paid_oob" { t.settled_total } else { t.paid_pxmr }, ..t }
            } else {
                t
            }
        })?;
        let Some(closed) = closed else { return Ok(None) };
        if !applied {
            return Ok(None);
        }
        let Some(contact) = self.contact(&tab.persona_hex) else {
            log::warn(TAG, format!("{} tab closed as {state} with its contact gone", tab.origin));
            return Ok(Some(closed));
        };
        let seq_before = contact.out_seq;
        match word(&contact) {
            Ok(sent) => self.mutate_tab(&tab.id, |t| RunningTab { word_seq: sent.out_seq as i64 - 1, ..t }),
            Err(e) => {
                let row = self.thread(&tab.persona_hex).into_iter().rev().find(|m| m.outgoing).filter(|m| m.seq >= seq_before && !m.delivered);
                if let Some(row) = row {
                    log::warn(TAG, format!("{state} word to {} is committed but not delivered: {e}", contact.display_name()));
                    return self.mutate_tab(&tab.id, |t| RunningTab { word_seq: row.seq as i64, ..t });
                }
                if revert {
                    self.mutate_tab(&tab.id, |t| if t.state == state { RunningTab { state: "settled".into(), paid_pxmr: 0, ..t } } else { t })?;
                } else {
                    self.mutate_tab(&tab.id, |t| RunningTab { word_seq: WORD_UNSENT, ..t })?;
                }
                Err(e)
            }
        }
    }

    /// Paid in cash, or by card, or never — settled outside DUCAT.
    pub fn mark_tab_paid_outside(&self, tab: &RunningTab) -> Result<Option<RunningTab>, Error> {
        let r = self.close_tab(tab, "paid_oob", false, |c| self.oob_receipt(tab, c))?;
        if r.is_some() {
            log::info(TAG, format!("{} tab settled outside DUCAT ({} XMR)", tab.origin, format_xmr(tab.settled_total)));
        }
        Ok(r)
    }

    fn oob_receipt(&self, tab: &RunningTab, contact: &Contact) -> Result<Contact, Error> {
        self.send(
            contact,
            Outgoing {
                body: RECEIPT_NOTE_OOB.into(),
                kind: 3,
                amount_pxmr: Some(tab.settled_total),
                re_seq: (tab.bill_seq >= 0).then_some(tab.bill_seq as u64),
                re_own: tab.bill_seq >= 0,
                items: tab.lines.clone(),
                tax_pxmr: tab.tax,
                oob: true,
                ..Default::default()
            },
        )
    }

    pub fn send_oob_receipt(&self, tab: &RunningTab) -> Result<Option<RunningTab>, Error> {
        if tab.state != "paid_oob" || tab.word_seq != WORD_UNSENT {
            return Ok(Some(tab.clone()));
        }
        let contact = self.contact(&tab.persona_hex).ok_or_else(|| Error::Refused("that contact is gone".into()))?;
        self.word(tab, &contact, |c| self.oob_receipt(tab, c))
    }

    /// The receipt a paid tab still owes.
    pub fn send_chain_receipt(&self, tab: &RunningTab) -> Result<Option<RunningTab>, Error> {
        if tab.state != "paid" || tab.word_seq != WORD_UNSENT {
            return Ok(Some(tab.clone()));
        }
        let Some(contact) = self.contact(&tab.persona_hex) else {
            log::warn(TAG, format!("{} tab paid with its contact gone; no receipt", tab.origin));
            return self.mutate_tab(&tab.id, |t| RunningTab { word_seq: -1, ..t });
        };
        let paid = if tab.paid_pxmr > 0 { tab.paid_pxmr } else { tab.settled_total };
        let tip = paid.saturating_sub(tab.settled_total);
        let mut lines = tab.lines.clone();
        if tip > 0 {
            lines.push(BillItem { description: TIP_LINE.into(), amount_pxmr: tip });
        }
        let txid = tab.paid_ki.as_deref().and_then(|ki| self.entries().into_iter().find(|e| e.key_image == ki)).map(|e| e.tx_hash_hex).filter(|t| !t.is_empty());
        let thread = self.thread(&tab.persona_hex);
        let bill_seq = tab.bill_in(&thread).map(|b| b.seq);
        let r = self.word(tab, &contact, |c| {
            self.send(
                c,
                Outgoing {
                    body: RECEIPT_NOTE.into(),
                    kind: 3,
                    amount_pxmr: Some(paid),
                    items: lines.clone(),
                    tax_pxmr: tab.tax,
                    txid_hex: txid.clone(),
                    re_seq: bill_seq,
                    re_own: bill_seq.is_some(),
                    ..Default::default()
                },
            )
        })?;
        log::info(TAG, format!("{} receipt sent on retry", tab.origin));
        Ok(r)
    }

    fn word<F: FnOnce(&Contact) -> Result<Contact, Error>>(&self, tab: &RunningTab, contact: &Contact, send: F) -> Result<Option<RunningTab>, Error> {
        let seq_before = contact.out_seq;
        match send(contact) {
            Ok(sent) => self.mutate_tab(&tab.id, |t| RunningTab { word_seq: sent.out_seq as i64 - 1, ..t }),
            Err(e) => {
                let row = self.thread(&tab.persona_hex).into_iter().rev().find(|m| m.outgoing).filter(|m| m.seq >= seq_before && !m.delivered);
                if let Some(row) = row {
                    log::warn(TAG, format!("receipt to {} is committed but not delivered: {e}", contact.display_name()));
                    return self.mutate_tab(&tab.id, |t| RunningTab { word_seq: row.seq as i64, ..t });
                }
                Err(e)
            }
        }
    }

    /// Withdraw a bill that has not been paid.
    pub fn cancel_tab(&self, tab: &RunningTab) -> Result<Option<RunningTab>, Error> {
        let amount = format_xmr(tab.settled_total);
        let r = self.close_tab(tab, "cancelled", true, |c| self.send(c, Outgoing::text(&format!("That bill for {amount} XMR is cancelled — nothing to pay."))))?;
        if r.is_some() {
            log::info(TAG, format!("{} tab cancelled ({} XMR)", tab.origin, format_xmr(tab.settled_total)));
        }
        Ok(r)
    }

    /// The mempool, for a bill that is waiting: a payment seen there is
    /// "settling now" on the screen minutes before the chain has it.
    pub fn pool_sight(&self, node: &str) {
        let waiting: Vec<RunningTab> = self.tabs().into_iter().filter(|t| t.state == "settled" && t.seen_tx.is_none()).collect();
        if waiting.is_empty() {
            return;
        }
        let Some(spend) = self.spend_key_hex() else { return };
        let Ok(hits) = monero_scan_pool(node.to_string(), spend, 40, self.subaddress_count()) else { return };
        if hits.is_empty() {
            return;
        }
        let ours = self.our_txids();
        let mut claimed_tx: Vec<String> = self.tabs().into_iter().filter_map(|t| t.seen_tx).collect();
        let mut waiting = waiting;
        waiting.sort_by_key(|t| t.settled_at);
        for tab in waiting {
            let thread = self.thread(&tab.persona_hex);
            let said = tab.said(&thread);
            let want_minor = tab.billed_minor.or_else(|| self.minor_of(&tab.persona_hex));
            let hit = hits.iter().find(|h| {
                !claimed_tx.contains(&h.tx_hash_hex)
                    && !ours.contains(&h.tx_hash_hex.to_lowercase())
                    && tab.paid_where_billed(want_minor, h.minor)
                    && (h.amount_pxmr == tab.settled_total || said.contains(&h.amount_pxmr))
            });
            let Some(hit) = hit else { continue };
            claimed_tx.push(hit.tx_hash_hex.clone());
            let tx = hit.tx_hash_hex.clone();
            let _ = self.mutate_tab(&tab.id, |t| RunningTab { seen_tx: Some(tx.clone()), ..t });
            log::info(TAG, format!("pool sighting for {} tab: {}… — {} XMR seen, settling now", tab.origin, &hit.tx_hash_hex[..16.min(hit.tx_hash_hex.len())], format_xmr(hit.amount_pxmr)));
        }
    }

    /// Match arrived notes to settled tabs and send receipts. Second
    /// opinion first: one node's view of a transaction is a claim.
    pub fn reconcile_tabs(&self) {
        let mut settled: Vec<RunningTab> = self.tabs().into_iter().filter(|t| t.state == "settled").collect();
        if settled.is_empty() {
            return;
        }
        settled.sort_by_key(|t| t.settled_at);
        let ours = self.our_txids();
        let entries: Vec<WalletEntry> = self.entries().into_iter().filter(|e| !ours.contains(&e.tx_hash_hex.to_lowercase())).collect();
        let mut claimed: Vec<String> = self.tabs().into_iter().filter_map(|t| t.paid_ki).collect();
        claimed.extend(self.claimed_kis());
        for tab in settled {
            let want_minor = tab.billed_minor.or_else(|| self.minor_of(&tab.persona_hex));
            let thread = self.thread(&tab.persona_hex);
            let bill = tab.bill_in(&thread);
            let said = tab.said(&thread);
            // Notices that name some *other* bill: an amount that matches
            // ours by coincidence must not be taken.
            let named_elsewhere: Vec<u64> = thread
                .iter()
                .filter(|m| {
                    !m.outgoing
                        && m.kind == 2
                        && m.re_seq.is_some()
                        && !m.re_own
                        && !bill.map_or(false, |b| referent(&thread, m).map_or(false, |r| std::ptr::eq(r, b)))
                        && m.timestamp * 1000 + 60_000 >= tab.settled_at
                })
                .map(|m| m.amount_pxmr)
                .collect();
            let matches = |e: &WalletEntry| -> bool {
                !e.key_image.is_empty()
                    && !tab.known_kis.contains(&e.key_image)
                    && !claimed.contains(&e.key_image)
                    && (tab.tip_at_bill == 0 || e.height > tab.tip_at_bill)
                    && tab.paid_where_billed(want_minor, e.minor)
                    && ((e.amount_pxmr == tab.settled_total && !named_elsewhere.contains(&e.amount_pxmr)) || said.contains(&e.amount_pxmr))
            };
            let hit = entries
                .iter()
                .find(|e| tab.seen_tx.as_deref().map_or(false, |s| e.tx_hash_hex.eq_ignore_ascii_case(s)) && matches(e))
                .or_else(|| entries.iter().find(|e| matches(e)));
            let Some(hit) = hit else { continue };
            if !self.settles(&hit.tx_hash_hex) {
                continue;
            }
            claimed.push(hit.key_image.clone());
            let Some(contact) = self.contact(&tab.persona_hex) else { continue };
            let tip = hit.amount_pxmr.saturating_sub(tab.settled_total);
            let mut receipt_lines = tab.lines.clone();
            if tip > 0 {
                receipt_lines.push(BillItem { description: TIP_LINE.into(), amount_pxmr: tip });
            }
            let Ok(Some(_)) = self.mark_tab_paid(&tab.id, &hit.key_image, hit.amount_pxmr) else { continue };
            log::info(
                TAG,
                format!(
                    "{} paid {} XMR by {}{}",
                    tab.origin,
                    format_xmr(hit.amount_pxmr),
                    contact.display_name(),
                    if tip > 0 { format!(" (tip {})", format_xmr(tip)) } else { String::new() }
                ),
            );
            let bill_seq = bill.map(|b| b.seq);
            let txid = Some(hit.tx_hash_hex.clone()).filter(|t| !t.is_empty());
            let r = self.word(&tab, &contact, |c| {
                self.send(
                    c,
                    Outgoing {
                        body: RECEIPT_NOTE.into(),
                        kind: 3,
                        amount_pxmr: Some(hit.amount_pxmr),
                        items: receipt_lines.clone(),
                        tax_pxmr: tab.tax,
                        txid_hex: txid.clone(),
                        re_seq: bill_seq,
                        re_own: bill_seq.is_some(),
                        ..Default::default()
                    },
                )
            });
            match r {
                Ok(_) => log::info(TAG, "receipt sent"),
                Err(e) => {
                    let _ = self.mutate_tab(&tab.id, |t| RunningTab { word_seq: WORD_UNSENT, ..t });
                    log::warn(TAG, format!("receipt failed after mark: {e}"));
                }
            }
        }
    }

    /// One owed receipt per turn, round robin.
    pub fn send_owed_receipts(&self) {
        let owed: Vec<RunningTab> = self.tabs().into_iter().filter(|t| t.state == "paid" && t.word_seq == WORD_UNSENT).collect();
        if owed.is_empty() {
            return;
        }
        let pick = {
            let mut last = LAST_OWED.lock().unwrap_or_else(|e| e.into_inner());
            let at = last.as_deref().and_then(|l| owed.iter().position(|t| t.id == l)).map_or(0, |i| (i + 1) % owed.len());
            *last = Some(owed[at].id.clone());
            owed[at].clone()
        };
        if let Err(e) = self.send_chain_receipt(&pick) {
            log::warn(TAG, format!("receipt retry for {} tab: {e}", pick.origin));
        }
    }

    /// The tabs' turn on the wallet lane: sightings in the pool, matches
    /// on the chain, receipts still owed.
    pub fn tabs_lap(&self, node: &str) {
        self.send_owed_receipts();
        self.reconcile_tabs();
        self.pool_sight(node);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tab(state: &str) -> RunningTab {
        RunningTab {
            id: "t1".into(),
            origin: ORIGIN_POS.into(),
            persona_hex: "ab".into(),
            opened_at: 1,
            lines: vec![BillItem { description: "coffee".into(), amount_pxmr: 10 }],
            tax: Some(1),
            state: state.into(),
            settled_total: 11,
            settled_at: 1_000_000,
            known_kis: vec![],
            paid_ki: None,
            seen_tx: None,
            tip_at_bill: 0,
            billed_minor: Some(3),
            bill_seq: 4,
            paid_pxmr: 0,
            word_seq: -1,
        }
    }

    #[test]
    fn a_tab_totals_its_lines_while_open_and_its_bill_after() {
        let mut t = tab("open");
        assert_eq!(t.total_pxmr(), 11);
        t.state = "settled".into();
        t.settled_total = 99;
        assert_eq!(t.total_pxmr(), 99);
        t.paid_pxmr = 120;
        assert_eq!(t.tip_pxmr(), 21);
        assert_eq!(t.take_pxmr(), 120);
    }

    #[test]
    fn payment_must_land_on_the_subaddress_the_bill_named() {
        let t = tab("settled");
        assert!(t.paid_where_billed(None, 3));
        assert!(!t.paid_where_billed(None, 0));
        let mut u = t.clone();
        u.billed_minor = None;
        assert!(u.paid_where_billed(Some(2), 0));
        assert!(u.paid_where_billed(Some(2), 2));
        assert!(!u.paid_where_billed(Some(2), 5));
    }

    #[test]
    fn the_bill_row_is_found_by_seq_then_by_amount() {
        let t = tab("settled");
        let thread = vec![
            StoredMessage { outgoing: true, seq: 4, kind: 1, amount_pxmr: 11, timestamp: 1_000, ..Default::default() },
            StoredMessage { outgoing: true, seq: 5, kind: 1, amount_pxmr: 11, timestamp: 2_000, ..Default::default() },
        ];
        assert_eq!(t.bill_in(&thread).map(|m| m.seq), Some(4));
        let mut u = t.clone();
        u.bill_seq = -1;
        assert_eq!(u.bill_in(&thread).map(|m| m.seq), Some(5));
    }

    #[test]
    fn tabs_round_trip_through_the_table() {
        let dir = std::env::temp_dir().join(format!("ducat-tabs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let app = App::open(&dir).unwrap();
        let t = app.open_or_resume_tab("ab", ORIGIN_BAR).unwrap();
        assert_eq!(app.open_or_resume_tab("ab", ORIGIN_BAR).unwrap().id, t.id);
        assert_ne!(app.open_or_resume_tab("ab", ORIGIN_POS).unwrap().id, t.id);
        let raw = std::fs::read_to_string(app.root().join("prefs/ducat_contacts.json")).unwrap();
        assert!(raw.contains("\"tabs_v1\""));
        let got = app.mutate_tab(&t.id, |mut t| { t.lines.push(BillItem { description: "beer".into(), amount_pxmr: 5 }); t }).unwrap().unwrap();
        assert_eq!(got.total_pxmr(), 5);
        assert_eq!(app.tab(&t.id).unwrap().lines.len(), 1);
        assert_eq!(app.sweep_abandoned_tabs(&[]), 0);
    }
}
