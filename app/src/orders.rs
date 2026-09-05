//! Orders at a counter (the kiosk): a numbered order with its own
//! subaddress and a noise-tagged total, so a payment from *any* wallet is
//! recognised by amount and address — or, when the customer answers the
//! order's card, a bill in a thread with a receipt to follow. The phone's
//! `Orders.kt`.

use serde::{Deserialize, Serialize};

use crate::contacts::{bump, BillItem, CONTACTS};
use crate::mailbox::Outgoing;
use crate::tabs::ORIGIN_KIOSK;
use crate::wallet::{exact_xmr, format_xmr};
use crate::{log, App, Error};

const TAG: &str = "Orders";
const STORE: &str = "ducat_orders";
/// The last six digits of a total are noise, so two orders for the same
/// menu never share an amount.
const TAG_RANGE: u64 = 1_000_000;
const ADDRESS_SLOTS: u32 = 64;
const ABANDON_AFTER_SECS: u64 = 30 * 60;
const KEEP: usize = 500;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum OrderState {
    Awaiting,
    Seen,
    Confirmed,
    Abandoned,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Order {
    pub id: String,
    pub number: u32,
    #[serde(default)]
    pub lines: Vec<BillItem>,
    #[serde(rename = "total")]
    pub total_pxmr: u64,
    #[serde(default)]
    pub tax: Option<u64>,
    #[serde(default)]
    pub address: String,
    pub state: OrderState,
    #[serde(rename = "at")]
    pub placed_at: u64,
    #[serde(rename = "seen", default)]
    pub seen_tx: Option<String>,
    #[serde(rename = "tab", default)]
    pub tab_id: Option<String>,
    #[serde(rename = "who", default)]
    pub persona_hex: Option<String>,
    #[serde(rename = "ready", default)]
    pub ready_at: u64,
    #[serde(rename = "minor", default)]
    pub billed_minor: Option<u32>,
    /// The sale card this order shows, until somebody answers it.
    #[serde(default)]
    pub card: Option<String>,
    #[serde(default)]
    pub card_inbox: Option<String>,
}

impl Order {
    pub fn unpaired(&self) -> bool {
        self.tab_id.is_none() && self.address.is_empty()
    }

    /// A `monero:` code any wallet can pay.
    pub fn pay_uri(&self) -> String {
        format!("monero:{}?tx_amount={}", self.address, exact_xmr(self.total_pxmr))
    }
}

fn new_id() -> String {
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    format!("o-{:x}-{:x}", crate::contacts::now_ms(), N.fetch_add(1, std::sync::atomic::Ordering::Relaxed))
}

impl App {
    pub fn orders(&self) -> Vec<Order> {
        let mut v: Vec<Order> = self.store(STORE).get("orders").unwrap_or_default();
        v.sort_by(|a, b| b.placed_at.cmp(&a.placed_at));
        v
    }

    pub fn order(&self, id: &str) -> Option<Order> {
        self.orders().into_iter().find(|o| o.id == id)
    }

    fn save_orders(&self, orders: &[Order]) -> Result<(), Error> {
        let mut v = orders.to_vec();
        v.sort_by(|a, b| b.placed_at.cmp(&a.placed_at));
        v.truncate(KEEP);
        self.store(STORE).put("orders", &v)?;
        bump();
        Ok(())
    }

    pub fn update_order(&self, order: Order) -> Result<(), Error> {
        let mut all: Vec<Order> = self.orders().into_iter().filter(|o| o.id != order.id).collect();
        all.push(order);
        self.save_orders(&all)
    }

    fn next_number(&self) -> u32 {
        self.orders().iter().map(|o| o.number).max().unwrap_or(0) % 999 + 1
    }

    /// An order any wallet can pay: its own subaddress from a ring of
    /// slots, and a total with six digits of noise so it is recognisable.
    pub fn place_order(&self, lines: Vec<BillItem>, tax: Option<u64>) -> Result<Order, Error> {
        if lines.is_empty() {
            return Err(Error::Refused("nothing on the order".into()));
        }
        let plain: u64 = lines.iter().map(|l| l.amount_pxmr).sum::<u64>() + tax.unwrap_or(0);
        let noise = u64::from_le_bytes(ducat_mobile::create_persona_secret()[..8].try_into().unwrap_or([0; 8])) % TAG_RANGE;
        let slot = self
            .store(STORE)
            .update(|m| {
                let n = m.get("slot_next").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                m.insert("slot_next".into(), serde_json::Value::from((n + 1) % ADDRESS_SLOTS));
                n
            })
            .unwrap_or(0);
        let who = format!("order_slot_{slot}");
        let payto = self.address_for(&who).ok_or_else(|| Error::Refused("no wallet address to be paid at".into()))?;
        let main = self.wallet_address();
        let order = Order {
            id: new_id(),
            number: self.next_number(),
            lines,
            total_pxmr: plain + noise,
            tax: tax.filter(|t| *t > 0),
            address: payto.clone(),
            state: OrderState::Awaiting,
            placed_at: App::now(),
            seen_tx: None,
            tab_id: None,
            persona_hex: None,
            ready_at: 0,
            billed_minor: self.minor_of(&who).filter(|_| Some(&payto) != main.as_ref()),
            card: None,
            card_inbox: None,
        };
        self.update_order(order.clone())?;
        log::info(TAG, format!("order #{}: {} XMR", order.number, format_xmr(order.total_pxmr)));
        Ok(order)
    }

    /// Give an order a card too: a DUCAT customer answers it, gets a bill
    /// in a thread, and a receipt after.
    pub fn order_card(&self, id: &str) -> Result<Order, Error> {
        let o = self.order(id).ok_or_else(|| Error::Refused("no such order".into()))?;
        if let Some(uri) = o.card.clone() {
            return Ok(Order { card: Some(uri), ..o });
        }
        let name = self.my_name(None)?;
        let h = self.issue_card(name.as_deref(), 60 * 60 * 2, "sale", None)?;
        self.store(CONTACTS).update(|m| {
            let mut map: std::collections::BTreeMap<String, String> = m.get("order_cards").cloned().and_then(crate::store::value_as).unwrap_or_default();
            map.insert(h.inbox_key.clone(), id.to_string());
            m.insert("order_cards".into(), serde_json::to_value(&map).unwrap_or_default());
        })?;
        let next = Order { card: Some(h.uri), card_inbox: Some(h.inbox_key), ..o };
        self.update_order(next.clone())?;
        Ok(next)
    }

    /// An order's card was answered: the customer gets the bill through a
    /// kiosk tab; the order follows that tab from here on.
    pub fn bind_order(&self, id: &str, persona_hex: &str) -> Result<Order, Error> {
        let o = self.order(id).ok_or_else(|| Error::Refused("no such order".into()))?;
        let opened = self.open_tab(persona_hex, ORIGIN_KIOSK)?;
        self.update_order(Order { tab_id: Some(opened.id.clone()), persona_hex: Some(persona_hex.to_string()), state: OrderState::Awaiting, ..o.clone() })?;
        let (lines, tax) = (o.lines.clone(), o.tax);
        let lined = self
            .mutate_tab(&opened.id, move |mut t| {
                t.lines = lines;
                t.tax = tax;
                t
            })?
            .ok_or_else(|| Error::Refused("the tab vanished".into()))?;
        let settled = self.settle_tab(&lined)?;
        let bound = Order { tab_id: Some(opened.id), persona_hex: Some(persona_hex.to_string()), total_pxmr: settled.settled_total, state: OrderState::Awaiting, ..o };
        self.update_order(bound.clone())?;
        log::info(TAG, format!("order #{} billed to {}…", bound.number, &persona_hex[..8.min(persona_hex.len())]));
        Ok(bound)
    }

    /// The claim sweep's hook: a card bound to an order.
    pub fn on_order_card_claimed(&self, inbox_key: &str, persona_hex: &str) -> bool {
        let map: std::collections::BTreeMap<String, String> = self.store(CONTACTS).get("order_cards").unwrap_or_default();
        let Some(id) = map.get(inbox_key).cloned() else { return false };
        match self.bind_order(&id, persona_hex) {
            Ok(_) => {}
            Err(e) => log::warn(TAG, format!("bind: {e}")),
        }
        true
    }

    pub fn abandon_order(&self, id: &str) -> Result<(), Error> {
        if let Some(o) = self.order(id) {
            if o.state == OrderState::Awaiting {
                if let Some(tab) = o.tab_id.as_deref().and_then(|t| self.tab(t)) {
                    let _ = self.cancel_tab(&tab);
                }
                self.update_order(Order { state: OrderState::Abandoned, ..o })?;
            }
        }
        Ok(())
    }

    /// Call an order as ready: a line in the customer's thread.
    pub fn say_ready(&self, id: &str) -> Result<(), Error> {
        let o = self.order(id).ok_or_else(|| Error::Refused("no such order".into()))?;
        let hex = o.persona_hex.clone().ok_or_else(|| Error::Refused("nobody claimed this order".into()))?;
        let c = self.contact(&hex).ok_or_else(|| Error::Refused("that customer is gone".into()))?;
        self.send(&c, Outgoing::text(&format!("Order #{} is ready", o.number)))?;
        self.update_order(Order { ready_at: App::now(), ..o })?;
        log::info(TAG, format!("order #{} called as ready", self.order(id).map_or(0, |o| o.number)));
        Ok(())
    }

    /// Where an order stands: its tab's word when it has one.
    pub fn order_state(&self, o: &Order) -> OrderState {
        let Some(tab) = o.tab_id.as_deref().and_then(|t| self.tab(t)) else { return o.state };
        match tab.state.as_str() {
            "paid" | "paid_oob" => OrderState::Confirmed,
            "cancelled" => OrderState::Abandoned,
            _ if tab.seen_tx.is_some() => OrderState::Seen,
            _ => OrderState::Awaiting,
        }
    }

    /// The mempool, for orders paid by address.
    pub fn orders_pool_sight(&self, node: &str) {
        let all = self.orders();
        let waiting: Vec<&Order> = all.iter().filter(|o| o.state == OrderState::Awaiting && o.tab_id.is_none() && !o.address.is_empty()).collect();
        if waiting.is_empty() {
            return;
        }
        let Some(spend) = self.spend_key_hex() else { return };
        let Ok(hits) = ducat_mobile::monero::monero_scan_pool(node.to_string(), spend, 40, self.subaddress_count()) else { return };
        if hits.is_empty() {
            return;
        }
        let ours = self.our_txids();
        let mut claimed: Vec<String> = all.iter().filter_map(|o| o.seen_tx.clone()).collect();
        for o in waiting {
            let Some(hit) = hits.iter().find(|h| h.amount_pxmr == o.total_pxmr && !claimed.contains(&h.tx_hash_hex) && !ours.contains(&h.tx_hash_hex.to_lowercase()) && o.billed_minor.map_or(true, |m| h.minor == m)) else { continue };
            claimed.push(hit.tx_hash_hex.clone());
            let _ = self.update_order(Order { state: OrderState::Seen, seen_tx: Some(hit.tx_hash_hex.clone()), ..o.clone() });
            crate::notify::post(format!("Order #{}", o.number), format!("{} XMR seen — settling", format_xmr(o.total_pxmr)), None);
            log::info(TAG, format!("order #{} seen — {}…", o.number, &hit.tx_hash_hex[..16.min(hit.tx_hash_hex.len())]));
        }
    }

    /// Half an hour unpaid and unpaired is abandoned.
    pub fn expire_orders(&self) {
        let cutoff = App::now().saturating_sub(ABANDON_AFTER_SECS);
        for o in self.orders() {
            if o.state == OrderState::Awaiting && o.tab_id.is_none() && o.placed_at > 0 && o.placed_at < cutoff {
                let _ = self.update_order(Order { state: OrderState::Abandoned, ..o.clone() });
                log::info(TAG, format!("order #{} abandoned — nobody paid", o.number));
            }
        }
    }

    /// Chain matches for address-paid orders; sightings confirmed on
    /// landing.
    pub fn reconcile_orders(&self) {
        let all = self.orders();
        let entries = self.entries();
        let landed: std::collections::HashSet<String> = entries.iter().map(|e| e.tx_hash_hex.to_lowercase()).collect();
        for o in all.iter().filter(|o| o.state == OrderState::Seen) {
            if o.seen_tx.as_deref().map_or(false, |t| landed.contains(&t.to_lowercase())) {
                let _ = self.update_order(Order { state: OrderState::Confirmed, ..o.clone() });
                log::info(TAG, format!("order #{} confirmed on chain", o.number));
            }
        }
        let mut waiting: Vec<&Order> = all.iter().filter(|o| o.state == OrderState::Awaiting && o.tab_id.is_none() && !o.address.is_empty()).collect();
        if waiting.is_empty() {
            return;
        }
        waiting.sort_by_key(|o| o.placed_at);
        let ours = self.our_txids();
        let mut claimed: Vec<String> = all.iter().filter_map(|o| o.seen_tx.clone()).collect();
        for o in waiting {
            let Some(hit) = entries.iter().find(|e| e.amount_pxmr == o.total_pxmr && !e.tx_hash_hex.is_empty() && !claimed.contains(&e.tx_hash_hex) && !ours.contains(&e.tx_hash_hex.to_lowercase()) && o.billed_minor.map_or(true, |m| e.minor == m)) else { continue };
            if !self.settles(&hit.tx_hash_hex) {
                continue;
            }
            claimed.push(hit.tx_hash_hex.clone());
            let _ = self.update_order(Order { state: OrderState::Confirmed, seen_tx: Some(hit.tx_hash_hex.clone()), ..o.clone() });
            crate::notify::post(format!("Order #{}", o.number), format!("{} XMR paid", format_xmr(o.total_pxmr)), None);
            log::info(TAG, format!("order #{} paid on chain, never sighted — {}…", o.number, &hit.tx_hash_hex[..16.min(hit.tx_hash_hex.len())]));
        }
    }

    pub fn orders_lap(&self, node: &str) {
        self.reconcile_orders();
        self.orders_pool_sight(node);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_order_carries_noise_and_a_payable_code() {
        let dir = std::env::temp_dir().join(format!("ducat-orders-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let app = App::open(&dir).unwrap();
        let w = ducat_mobile::create_wallet(1, true);
        app.wallet_save(&w.address, &w.spend_key_hex, 1, true).unwrap();
        let o = app.place_order(vec![BillItem { description: "Soup".into(), amount_pxmr: 5_000_000_000 }], None).unwrap();
        assert_eq!(o.number, 1);
        assert!(o.total_pxmr >= 5_000_000_000 && o.total_pxmr < 5_001_000_000);
        assert!(o.address.starts_with('7'), "a subaddress: {}", o.address);
        assert!(o.pay_uri().starts_with("monero:7"));
        assert!(o.pay_uri().contains("tx_amount=0.00500"));
        assert_eq!(o.billed_minor, Some(1));
        let p = app.place_order(vec![BillItem { description: "Tea".into(), amount_pxmr: 1 }], None).unwrap();
        assert_eq!(p.number, 2);
        assert_ne!(p.address, o.address);
        assert_eq!(app.order_state(&o), OrderState::Awaiting);
        app.abandon_order(&o.id).unwrap();
        assert_eq!(app.order(&o.id).unwrap().state, OrderState::Abandoned);
    }
}
