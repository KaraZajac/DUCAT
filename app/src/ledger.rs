//! The ledger: every note that came in and every send that went out, as
//! events with a running balance — money and its paperwork joined: a
//! receipt names the transaction it was for, a payment notice names who
//! paid. The phone's `Ledger.kt`, with the exports a desk is for.

use std::collections::{HashMap, HashSet};

use ducat_mobile::monero::{monero_block_time, monero_output_meta, monero_tx_details};
use serde::{Deserialize, Serialize};

use crate::contacts::{bump, BillItem, ReceiptRecord, CONTACTS};
use crate::tabs::RunningTab;
use crate::wallet::{SentPayment, WalletEntry, LOCK_BLOCKS};
use crate::{log, App, Error};

const TAG: &str = "Ledger";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum Direction {
    Received,
    Sent,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum Source {
    Notice,
    OurRecord,
    /// A kiosk order paid by a plain wallet: the amount and address named it.
    Order,
    Unknown,
}

/// A transaction as the chain describes it, kept once fetched.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChainTx {
    pub txid: String,
    #[serde(rename = "v", default)]
    pub version: u32,
    #[serde(rename = "fee", default)]
    pub fee_pxmr: u64,
    #[serde(rename = "ki", default)]
    pub key_images: Vec<String>,
    #[serde(rename = "in", default)]
    pub input_count: u32,
    #[serde(rename = "out", default)]
    pub output_count: u32,
    #[serde(rename = "ring", default)]
    pub ring_size: u32,
    #[serde(rename = "lock", default)]
    pub additional_timelock: u64,
    #[serde(rename = "extra", default)]
    pub extra_len: u32,
    #[serde(rename = "cb", default)]
    pub coinbase: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct Event {
    pub txid: String,
    pub height: u64,
    pub timestamp: u64,
    pub direction: Direction,
    pub amount_pxmr: u64,
    pub fee_pxmr: u64,
    pub net_pxmr: i64,
    pub balance_after_pxmr: i64,
    pub counterparty: Option<String>,
    pub address: Option<String>,
    pub donation: bool,
    pub source: Source,
    pub note: Option<String>,
    pub ours: Vec<WalletEntry>,
    pub consumed: Vec<WalletEntry>,
    pub pending: bool,
    pub locked: bool,
    pub unlocks_in_blocks: u64,
    pub unexplained: bool,
    pub provisional: bool,
    pub sort_height: u64,
    pub items: Vec<BillItem>,
    pub tax_pxmr: Option<u64>,
    pub receipted: bool,
    pub contact_hex: Option<String>,
    pub receipt_by: Option<String>,
    pub receipt_at: u64,
}

impl Event {
    fn blank(direction: Direction) -> Event {
        Event {
            txid: String::new(),
            height: 0,
            timestamp: 0,
            direction,
            amount_pxmr: 0,
            fee_pxmr: 0,
            net_pxmr: 0,
            balance_after_pxmr: 0,
            counterparty: None,
            address: None,
            donation: false,
            source: Source::Unknown,
            note: None,
            ours: Vec::new(),
            consumed: Vec::new(),
            pending: false,
            locked: false,
            unlocks_in_blocks: 0,
            unexplained: false,
            provisional: false,
            sort_height: 0,
            items: Vec::new(),
            tax_pxmr: None,
            receipted: false,
            contact_hex: None,
            receipt_by: None,
            receipt_at: 0,
        }
    }
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Summary {
    pub in_pxmr: u64,
    pub out_pxmr: u64,
    pub net_pxmr: i64,
    pub fees_pxmr: u64,
    pub in_count: usize,
    pub out_count: usize,
    pub tax_collected_pxmr: u64,
    pub donations_pxmr: u64,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct DoorTake {
    pub count: usize,
    pub take_pxmr: u64,
    pub tip_pxmr: u64,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct BusinessSummary {
    pub by_origin: Vec<(String, DoorTake)>,
    pub tax_collected_pxmr: u64,
    pub outstanding_count: usize,
    pub outstanding_pxmr: u64,
    pub sales_count: usize,
    pub sales_pxmr: u64,
}

/// Turn the wallet's notes and send records into events. Pure, so it can
/// be tested without a chain.
pub fn assemble(entries: &[WalletEntry], tip: u64, chain_of: &dyn Fn(&str) -> Option<ChainTx>, send_records: &[SentPayment], name_of: &dyn Fn(Option<&str>) -> Option<String>, announced: &HashMap<String, (String, Option<String>)>) -> Vec<Event> {
    let sends: HashMap<String, &SentPayment> = send_records.iter().map(|s| (s.txid_hex.to_lowercase(), s)).collect();
    let mut recovered_by_ki: HashMap<String, &SentPayment> = HashMap::new();
    for r in send_records.iter().filter(|r| r.recovered) {
        for k in &r.key_images {
            recovered_by_ki.insert(k.clone(), r);
        }
    }
    let mut paired: HashSet<String> = HashSet::new();
    let our_kis: HashSet<&str> = entries.iter().filter(|e| !e.key_image.is_empty()).map(|e| e.key_image.as_str()).collect();
    let by_ki: HashMap<&str, &WalletEntry> = entries.iter().map(|e| (e.key_image.as_str(), e)).collect();
    let mut grouped: Vec<(String, Vec<WalletEntry>)> = Vec::new();
    for e in entries {
        let key = if e.tx_hash_hex.is_empty() { format!("ki:{}", e.key_image) } else { e.tx_hash_hex.to_lowercase() };
        match grouped.iter_mut().find(|(k, _)| *k == key) {
            Some((_, g)) => g.push(e.clone()),
            None => grouped.push((key, vec![e.clone()])),
        }
    }
    let mut out: Vec<Event> = Vec::new();
    let mut explained: HashSet<String> = HashSet::new();
    for (key, group) in grouped {
        let txid = if key.starts_with("ki:") { String::new() } else { key.clone() };
        let chain = if txid.is_empty() { None } else { chain_of(&txid) };
        let received: u64 = group.iter().map(|e| e.amount_pxmr).sum();
        let height = group.iter().map(|e| e.height).min().unwrap_or(0);
        let ts = group.iter().map(|e| e.timestamp).max().unwrap_or(0);
        let consumed_kis: Vec<String> = chain.as_ref().map(|c| c.key_images.iter().filter(|k| our_kis.contains(k.as_str())).cloned().collect()).unwrap_or_default();
        if !consumed_kis.is_empty() {
            let consumed: Vec<WalletEntry> = consumed_kis.iter().filter_map(|k| by_ki.get(k.as_str()).map(|e| (*e).clone())).collect();
            explained.extend(consumed_kis.iter().cloned());
            let spent_total: u64 = consumed.iter().map(|e| e.amount_pxmr).sum();
            let fee = chain.as_ref().map_or(0, |c| c.fee_pxmr);
            let paid = spent_total.saturating_sub(received).saturating_sub(fee);
            let rec = sends.get(&txid).copied().or_else(|| {
                consumed_kis.iter().find_map(|k| recovered_by_ki.get(k).copied()).map(|r| {
                    paired.insert(r.txid_hex.clone() + &r.ts.to_string());
                    r
                })
            });
            let mut e = Event::blank(Direction::Sent);
            e.txid = txid.clone();
            e.height = height;
            e.timestamp = if ts > 0 { ts } else { rec.map_or(0, |r| r.ts) };
            e.amount_pxmr = paid;
            e.fee_pxmr = fee;
            e.net_pxmr = received as i64 - spent_total as i64;
            e.counterparty = name_of(rec.and_then(|r| r.contact.as_deref()));
            e.address = rec.map(|r| r.to_address.clone());
            e.source = if rec.is_some() { Source::OurRecord } else { Source::Unknown };
            e.note = rec.and_then(|r| r.note.clone());
            e.donation = rec.map_or(false, |r| r.donate);
            e.contact_hex = rec.and_then(|r| r.contact.clone());
            e.ours = group;
            e.consumed = consumed;
            e.sort_height = height;
            out.push(e);
        } else {
            let named = announced.get(&txid);
            let mut e = Event::blank(Direction::Received);
            e.txid = txid.clone();
            e.height = height;
            e.timestamp = ts;
            e.amount_pxmr = received;
            e.net_pxmr = received as i64;
            e.counterparty = named.map(|n| n.0.clone());
            e.source = if named.is_some() { Source::Notice } else { Source::Unknown };
            e.note = named.and_then(|n| n.1.clone());
            e.locked = tip > 0 && height > 0 && height + LOCK_BLOCKS > tip;
            e.unlocks_in_blocks = (height + LOCK_BLOCKS).saturating_sub(tip);
            e.provisional = !txid.is_empty() && chain.is_none() && sends.contains_key(&txid);
            e.ours = group;
            e.sort_height = height;
            out.push(e);
        }
    }
    for e in entries.iter().filter(|e| e.spent && !explained.contains(&e.key_image)) {
        let rec = recovered_by_ki.get(&e.key_image).copied();
        if let Some(r) = rec {
            paired.insert(r.txid_hex.clone() + &r.ts.to_string());
        }
        let mut ev = Event::blank(Direction::Sent);
        ev.timestamp = rec.map_or(0, |r| r.ts);
        ev.amount_pxmr = rec.map_or(e.amount_pxmr, |r| r.amount_pxmr);
        ev.fee_pxmr = rec.map_or(0, |r| r.fee);
        ev.net_pxmr = -(e.amount_pxmr as i64);
        ev.counterparty = name_of(rec.and_then(|r| r.contact.as_deref()));
        ev.address = rec.map(|r| r.to_address.clone());
        ev.source = if rec.is_some() { Source::OurRecord } else { Source::Unknown };
        ev.note = rec.and_then(|r| r.note.clone());
        ev.donation = rec.map_or(false, |r| r.donate);
        ev.contact_hex = rec.and_then(|r| r.contact.clone());
        ev.consumed = vec![e.clone()];
        ev.unexplained = rec.is_none();
        ev.sort_height = e.height + 1;
        out.push(ev);
    }
    let on_chain: HashSet<String> = out.iter().filter(|e| !e.txid.is_empty()).map(|e| e.txid.clone()).collect();
    let mut pending: Vec<&SentPayment> = send_records.iter().filter(|s| !on_chain.contains(&s.txid_hex.to_lowercase()) && !paired.contains(&(s.txid_hex.clone() + &s.ts.to_string()))).collect();
    pending.sort_by(|a, b| b.ts.cmp(&a.ts));
    let mut unattributed: Vec<usize> = (0..out.len()).filter(|i| out[*i].unexplained).collect();
    unattributed.sort_by(|a, b| out[*b].sort_height.cmp(&out[*a].sort_height));
    let mut leftover: Vec<&SentPayment> = Vec::new();
    for s in pending {
        match unattributed.iter().position(|i| out[*i].amount_pxmr >= s.amount_pxmr + s.fee) {
            None => leftover.push(s),
            Some(slot) => {
                let i = unattributed.remove(slot);
                let e = &mut out[i];
                e.txid = s.txid_hex.clone();
                e.timestamp = s.ts;
                e.amount_pxmr = s.amount_pxmr;
                e.fee_pxmr = s.fee;
                e.counterparty = name_of(s.contact.as_deref());
                e.address = Some(s.to_address.clone());
                e.source = Source::OurRecord;
                e.note = s.note.clone();
                e.contact_hex = s.contact.clone();
                e.donation = s.donate;
                e.unexplained = false;
                e.pending = true;
            }
        }
    }
    for s in leftover {
        let mut e = Event::blank(Direction::Sent);
        e.txid = s.txid_hex.clone();
        e.timestamp = s.ts;
        e.amount_pxmr = s.amount_pxmr;
        e.fee_pxmr = s.fee;
        e.counterparty = name_of(s.contact.as_deref());
        e.address = Some(s.to_address.clone());
        e.source = Source::OurRecord;
        e.note = s.note.clone();
        e.donation = s.donate;
        e.contact_hex = s.contact.clone();
        e.pending = true;
        e.sort_height = u64::MAX;
        out.push(e);
    }
    out.sort_by(|a, b| a.sort_height.cmp(&b.sort_height).then_with(|| a.timestamp.cmp(&b.timestamp)));
    let mut running: i64 = 0;
    for e in out.iter_mut() {
        running += e.net_pxmr;
        e.balance_after_pxmr = running;
    }
    out.reverse();
    out
}

pub fn summarize(events: &[Event], from_ts: u64, to_ts: u64) -> Summary {
    let mut s = Summary::default();
    for e in events {
        if e.pending || e.provisional || e.timestamp < from_ts || e.timestamp >= to_ts {
            continue;
        }
        if e.direction == Direction::Received {
            s.in_pxmr += e.amount_pxmr;
            s.in_count += 1;
            s.tax_collected_pxmr += e.tax_pxmr.unwrap_or(0);
        } else {
            s.out_pxmr += e.amount_pxmr;
            s.out_count += 1;
            s.fees_pxmr += e.fee_pxmr;
            if e.donation {
                s.donations_pxmr += e.amount_pxmr;
            }
        }
        s.net_pxmr += e.net_pxmr;
    }
    s
}

pub fn summarize_business(tabs: &[RunningTab], from_ts: u64, to_ts: u64) -> BusinessSummary {
    let mut by: Vec<(String, DoorTake)> = Vec::new();
    let mut out = BusinessSummary::default();
    for t in tabs {
        match t.state.as_str() {
            "paid" | "settled" => {
                let at = if t.settled_at > 0 { t.settled_at / 1000 } else { t.opened_at / 1000 };
                if at < from_ts || at >= to_ts {
                    continue;
                }
                let d = match by.iter_mut().find(|(o, _)| *o == t.origin) {
                    Some((_, d)) => d,
                    None => {
                        by.push((t.origin.clone(), DoorTake::default()));
                        &mut by.last_mut().unwrap().1
                    }
                };
                d.count += 1;
                d.take_pxmr += t.take_pxmr();
                d.tip_pxmr += t.tip_pxmr();
                out.tax_collected_pxmr += t.tax.unwrap_or(0);
            }
            "open" if t.bill_seq >= 0 => {
                out.outstanding_count += 1;
                out.outstanding_pxmr += t.total_pxmr();
            }
            _ => {}
        }
    }
    out.sales_count = by.iter().map(|(_, d)| d.count).sum();
    out.sales_pxmr = by.iter().map(|(_, d)| d.take_pxmr).sum();
    out.by_origin = by;
    out
}

/// A CSV cell that cannot start a formula and cannot break a row.
pub fn csv_cell(v: &str) -> String {
    let safe = if v.starts_with(['=', '+', '-', '@', '\t', '\r']) { format!("'{v}") } else { v.to_string() };
    if safe.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", safe.replace('"', "\"\""))
    } else {
        safe
    }
}

fn xmr(pxmr: i64) -> String {
    let neg = pxmr < 0;
    let a = pxmr.unsigned_abs();
    format!("{}{}.{:012}", if neg { "-" } else { "" }, a / 1_000_000_000_000, a % 1_000_000_000_000)
}

fn iso_utc(secs: u64) -> String {
    // Civil date from days since the epoch (Howard Hinnant), no calendar crate.
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", rem / 3600, (rem % 3600) / 60, rem % 60)
}

impl App {
    fn chain_tx(&self, txid: &str) -> Option<ChainTx> {
        self.store(CONTACTS).get(&format!("tx_{}", txid.to_lowercase()))
    }

    fn put_chain_tx(&self, tx: &ChainTx) {
        let _ = self.store(CONTACTS).put(&format!("tx_{}", tx.txid.to_lowercase()), tx);
    }

    /// Every event, newest first, with receipts and notices paired in.
    pub fn ledger(&self) -> Vec<Event> {
        let everyone = self.contacts();
        let mut announced: HashMap<String, (String, Option<String>)> = HashMap::new();
        let mut announced_hex: HashMap<String, String> = HashMap::new();
        for c in &everyone {
            for m in self.thread(&c.persona_hex) {
                if let Some(id) = m.txid_hex.as_deref().map(|t| t.to_lowercase()) {
                    if !m.outgoing && m.kind == 2 {
                        announced.insert(id.clone(), (c.display_name(), Some(m.body.clone()).filter(|b| !b.trim().is_empty())));
                        announced_hex.insert(id, c.persona_hex.clone());
                    }
                }
            }
        }
        let receipts = self.receipts();
        let mut papered: HashMap<String, ReceiptRecord> = HashMap::new();
        let mut loose: Vec<(ReceiptRecord, bool)> = Vec::new();
        for r in receipts {
            match r.txid.as_deref().map(|t| t.to_lowercase()) {
                Some(id) => {
                    papered.insert(id, r);
                }
                None if r.amount_pxmr > 0 && !r.oob => loose.push((r, false)),
                None => {}
            }
        }
        let sends = self.sends();
        let sends_by_tx: HashMap<String, SentPayment> = sends.iter().map(|s| (s.txid_hex.to_lowercase(), s.clone())).collect();
        let name_of = |h: Option<&str>| -> Option<String> { h.and_then(|h| everyone.iter().find(|c| c.persona_hex == h).map(|c| c.display_name())) };
        let chain_of = |t: &str| self.chain_tx(t);
        let mut built = assemble(&self.entries(), self.tip(), &chain_of, &sends, &name_of, &announced);
        // A kiosk order paid by any wallet has no notice to name it; the
        // order does — its noisy total matched the note.
        let orders: HashMap<String, u32> = self
            .orders()
            .into_iter()
            .filter_map(|o| o.seen_tx.as_deref().map(|t| (t.to_lowercase(), o.number)))
            .collect();
        for e in built.iter_mut() {
            if e.counterparty.is_none() && e.direction == Direction::Received {
                if let Some(n) = orders.get(&e.txid.to_lowercase()) {
                    e.counterparty = Some(format!("Kiosk order #{n}"));
                    e.note = Some("paid at the counter".into());
                    e.source = Source::Order;
                }
            }
        }
        built
            .into_iter()
            .map(|mut e| {
                let id = e.txid.to_lowercase();
                let known_hex = match e.direction {
                    Direction::Sent => sends_by_tx.get(&id).and_then(|s| s.contact.clone()),
                    Direction::Received => announced_hex.get(&id).cloned(),
                };
                let mut paper = papered.get(&id).cloned();
                if paper.is_none() {
                    let want_mine = e.direction == Direction::Received;
                    let pick = loose
                        .iter_mut()
                        .filter(|(r, spent)| !*spent && r.amount_pxmr == e.amount_pxmr && r.mine == want_mine && known_hex.as_deref().map_or(true, |k| r.contact_hex == k))
                        .filter(|(r, _)| {
                            let d = (r.timestamp as i64 - e.timestamp as i64).unsigned_abs();
                            if known_hex.is_some() {
                                e.timestamp == 0 || r.timestamp == 0 || d <= 86_400
                            } else {
                                e.timestamp != 0 && r.timestamp != 0 && d <= 86_400
                            }
                        })
                        .min_by_key(|(r, _)| (r.timestamp as i64 - e.timestamp as i64).unsigned_abs());
                    if let Some(l) = pick {
                        l.1 = true;
                        paper = Some(l.0.clone());
                    }
                }
                let hex = known_hex.or_else(|| paper.as_ref().map(|p| p.contact_hex.clone()));
                if let Some(p) = &paper {
                    e.items = p.items.clone();
                    e.tax_pxmr = p.tax;
                    e.receipted = true;
                    e.receipt_by = Some(if p.mine { "you".to_string() } else { p.counterparty.clone() });
                    e.receipt_at = p.timestamp;
                }
                if hex.is_some() {
                    e.contact_hex = hex;
                }
                e
            })
            .collect()
    }

    /// Fill in what the ledger cannot know from the notes alone: transaction
    /// ids from stored outputs, chain details, block times — a few lookups
    /// per turn. True when anything changed.
    pub fn enrich_ledger(&self, node: &str, budget: usize) -> bool {
        let mut changed = false;
        let entries = self.entries();
        if entries.iter().any(|e| e.tx_hash_hex.is_empty() && !e.blob.is_empty()) {
            let mut got = 0;
            let filled: Vec<WalletEntry> = entries
                .iter()
                .cloned()
                .map(|mut e| {
                    if e.tx_hash_hex.is_empty() && !e.blob.is_empty() {
                        if let Ok(meta) = monero_output_meta(e.blob.clone()) {
                            if !meta.tx_hash_hex.is_empty() {
                                got += 1;
                                e.tx_hash_hex = meta.tx_hash_hex;
                            }
                        }
                    }
                    e
                })
                .collect();
            if got > 0 {
                let _ = self.store(CONTACTS).put("wallet_outputs", &filled);
                log::info(TAG, format!("recovered {got} transaction id(s) from stored outputs"));
                changed = true;
            }
        }
        let mut spent = 0;
        let mut seen: HashSet<String> = HashSet::new();
        for txid in self.entries().iter().map(|e| e.tx_hash_hex.to_lowercase()) {
            if spent >= budget {
                break;
            }
            if txid.is_empty() || !seen.insert(txid.clone()) || self.chain_tx(&txid).is_some() {
                continue;
            }
            spent += 1;
            match monero_tx_details(node.to_string(), txid.clone()) {
                Ok(d) => {
                    self.put_chain_tx(&ChainTx {
                        txid: d.tx_hash_hex.to_lowercase(),
                        version: d.version,
                        fee_pxmr: d.fee_pxmr,
                        key_images: d.key_images_hex,
                        input_count: d.input_count,
                        output_count: d.output_count,
                        ring_size: d.ring_size,
                        additional_timelock: d.additional_timelock,
                        extra_len: d.extra_len,
                        coinbase: d.coinbase,
                    });
                    changed = true;
                }
                Err(e) => log::warn(TAG, format!("tx {txid}: {e}")),
            }
        }
        let mut need: Vec<u64> = self.entries().iter().filter(|e| e.timestamp == 0 && e.height > 0).map(|e| e.height).collect();
        need.sort();
        need.dedup();
        need.truncate(budget.saturating_sub(spent));
        if !need.is_empty() {
            let mut times: HashMap<u64, u64> = HashMap::new();
            for h in need {
                match monero_block_time(node.to_string(), h) {
                    Ok(t) => {
                        times.insert(h, t);
                    }
                    Err(e) => log::warn(TAG, format!("block {h} time: {e}")),
                }
            }
            if !times.is_empty() {
                let filled: Vec<WalletEntry> = self
                    .entries()
                    .into_iter()
                    .map(|mut e| {
                        if e.timestamp == 0 {
                            if let Some(t) = times.get(&e.height) {
                                e.timestamp = *t;
                            }
                        }
                        e
                    })
                    .collect();
                let _ = self.store(CONTACTS).put("wallet_outputs", &filled);
                log::info(TAG, format!("filled in {} block time(s)", times.len()));
                changed = true;
            }
        }
        if changed {
            bump();
        }
        changed
    }

    pub fn export_ledger_csv(&self) -> String {
        let mut sb = String::from("date_utc,direction,counterparty,note,items,amount_xmr,fee_xmr,net_xmr,tax_xmr,donation,txid,height,balance_after_xmr\n");
        let mut events = self.ledger();
        events.reverse();
        for e in events.iter().filter(|e| !e.pending) {
            let items = e.items.iter().map(|i| format!("{} {}", i.description, xmr(i.amount_pxmr as i64))).collect::<Vec<_>>().join("; ");
            sb.push_str(&format!(
                "{},{},{},{},{},{},{},{},{},{},{},{},{}\n",
                csv_cell(&iso_utc(e.timestamp)),
                if e.direction == Direction::Sent { "out" } else { "in" },
                csv_cell(e.counterparty.as_deref().unwrap_or("")),
                csv_cell(e.note.as_deref().unwrap_or("")),
                csv_cell(&items),
                xmr(e.amount_pxmr as i64),
                xmr(e.fee_pxmr as i64),
                xmr(e.net_pxmr),
                xmr(e.tax_pxmr.unwrap_or(0) as i64),
                if e.donation { "yes" } else { "" },
                csv_cell(&e.txid),
                e.height,
                xmr(e.balance_after_pxmr)
            ));
        }
        sb
    }

    pub fn export_ledger_json(&self) -> String {
        let mut events = self.ledger();
        events.reverse();
        let rows: Vec<serde_json::Value> = events
            .iter()
            .filter(|e| !e.pending)
            .map(|e| {
                let mut o = serde_json::json!({
                    "date_utc": iso_utc(e.timestamp),
                    "direction": if e.direction == Direction::Sent { "out" } else { "in" },
                    "amount_xmr": xmr(e.amount_pxmr as i64),
                    "fee_xmr": xmr(e.fee_pxmr as i64),
                    "net_xmr": xmr(e.net_pxmr),
                    "txid": e.txid,
                    "height": e.height,
                    "balance_after_xmr": xmr(e.balance_after_pxmr),
                });
                if let Some(c) = &e.counterparty {
                    o["counterparty"] = serde_json::Value::from(c.clone());
                }
                if let Some(n) = &e.note {
                    o["note"] = serde_json::Value::from(n.clone());
                }
                if !e.items.is_empty() {
                    o["items"] = serde_json::Value::Array(e.items.iter().map(|i| serde_json::json!({ "description": i.description, "amount_xmr": xmr(i.amount_pxmr as i64) })).collect());
                }
                if let Some(t) = e.tax_pxmr {
                    o["tax_xmr"] = serde_json::Value::from(xmr(t as i64));
                }
                if e.donation {
                    o["donation"] = serde_json::Value::from(true);
                }
                if e.receipted {
                    o["receipted"] = serde_json::Value::from(true);
                    if let Some(b) = &e.receipt_by {
                        o["receipt_by"] = serde_json::Value::from(b.clone());
                    }
                    if e.receipt_at > 0 {
                        o["receipt_at_utc"] = serde_json::Value::from(iso_utc(e.receipt_at));
                    }
                }
                if e.locked {
                    o["locked"] = serde_json::Value::from(true);
                }
                o
            })
            .collect();
        serde_json::to_string_pretty(&serde_json::json!({ "format": "ducat-ledger", "version": 1, "generated_utc": iso_utc(App::now()), "events": rows })).unwrap_or_default()
    }

    pub fn export_ledger_to(&self, path: &std::path::Path, json: bool) -> Result<u64, Error> {
        let text = if json { self.export_ledger_json() } else { self.export_ledger_csv() };
        std::fs::write(path, text.as_bytes())?;
        Ok(text.len() as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(ki: &str, amt: u64, h: u64, tx: &str, spent: bool) -> WalletEntry {
        WalletEntry { amount_pxmr: amt, height: h, spent, key_image: ki.into(), blob: vec![1], tx_hash_hex: tx.into(), timestamp: h * 100, minor: 0 }
    }

    #[test]
    fn a_send_pairs_its_consumed_notes_with_its_change_and_the_balance_runs() {
        let entries = vec![note("a", 100, 10, "tx1", true), note("b", 30, 20, "tx2", false)];
        let chain = |t: &str| (t == "tx2").then(|| ChainTx { txid: "tx2".into(), version: 2, fee_pxmr: 5, key_images: vec!["a".into()], input_count: 1, output_count: 2, ring_size: 16, additional_timelock: 0, extra_len: 0, coinbase: false });
        let sends = vec![SentPayment { txid_hex: "tx2".into(), amount_pxmr: 65, fee: 5, to_address: "4x".into(), contact: Some("pat".into()), note: Some("lunch".into()), ts: 2000, donate: false, recovered: false, key_images: vec![] }];
        let name = |h: Option<&str>| h.map(|_| "Pat".to_string());
        let ev = assemble(&entries, 100, &chain, &sends, &name, &HashMap::new());
        assert_eq!(ev.len(), 2);
        // Newest first: the send, then the receipt of a.
        assert_eq!(ev[0].direction, Direction::Sent);
        assert_eq!(ev[0].amount_pxmr, 65);
        assert_eq!(ev[0].fee_pxmr, 5);
        assert_eq!(ev[0].net_pxmr, -70);
        assert_eq!(ev[0].counterparty.as_deref(), Some("Pat"));
        assert_eq!(ev[0].balance_after_pxmr, 30);
        assert_eq!(ev[1].direction, Direction::Received);
        assert_eq!(ev[1].balance_after_pxmr, 100);
        let s = summarize(&ev, 0, u64::MAX);
        assert_eq!(s.in_pxmr, 100);
        assert_eq!(s.out_pxmr, 65);
        assert_eq!(s.fees_pxmr, 5);
    }

    #[test]
    fn a_locked_note_and_a_pending_send_are_marked_not_counted() {
        let entries = vec![note("a", 100, 95, "tx1", false)];
        let chain = |_: &str| None;
        let sends = vec![SentPayment { txid_hex: "txp".into(), amount_pxmr: 10, fee: 1, to_address: "4y".into(), contact: None, note: None, ts: 3000, donate: true, recovered: false, key_images: vec![] }];
        let name = |_: Option<&str>| None;
        let ev = assemble(&entries, 100, &chain, &sends, &name, &HashMap::new());
        let recv = ev.iter().find(|e| e.direction == Direction::Received).unwrap();
        assert!(recv.locked);
        assert_eq!(recv.unlocks_in_blocks, 5);
        let pend = ev.iter().find(|e| e.pending).unwrap();
        assert_eq!(pend.txid, "txp");
        let s = summarize(&ev, 0, u64::MAX);
        assert_eq!(s.out_count, 0);
    }

    #[test]
    fn csv_cells_cannot_run_formulas_or_break_rows() {
        assert_eq!(csv_cell("=1+1"), "'=1+1");
        assert_eq!(csv_cell("a,b"), "\"a,b\"");
        assert_eq!(csv_cell("say \"hi\""), "\"say \"\"hi\"\"\"");
        assert_eq!(csv_cell("plain"), "plain");
        assert_eq!(iso_utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(iso_utc(1_788_609_224), "2026-09-05T11:53:44Z");
        assert_eq!(xmr(-1_500_000_000_000), "-1.500000000000");
    }
}
