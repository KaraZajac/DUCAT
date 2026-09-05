//! Donations: money that arrived on a thread born from a `donate` card,
//! unprompted, receipted once the chain agrees. The phone's `Donations.kt`.

use std::collections::{HashMap, HashSet};

use crate::contacts::CONTACTS;
use crate::mailbox::Outgoing;
use crate::wallet::format_xmr;
use crate::{log, App};

const TAG: &str = "Donations";
const CLOCK_SKEW_SECS: u64 = 900;
pub const RECEIPT_NOTE: &str = "Thank you for your support";

impl App {
    fn donations_receipted(&self) -> HashSet<String> {
        self.store(CONTACTS).get::<Vec<String>>("donation_receipted").unwrap_or_default().into_iter().collect()
    }

    fn mark_donation_receipted(&self, txid: &str) {
        let _ = self.store(CONTACTS).update(|m| {
            let mut v: Vec<String> = m.get("donation_receipted").cloned().and_then(crate::store::value_as).unwrap_or_default();
            v.push(txid.to_string());
            m.insert("donation_receipted".into(), serde_json::to_value(&v).unwrap_or_default());
        });
    }

    /// Receipt every donation notice whose money is here.
    pub fn reconcile_donations(&self) {
        let donors: Vec<_> = self.contacts().into_iter().filter(|c| c.my_card_purpose.as_deref() == Some("donate")).collect();
        if donors.is_empty() {
            return;
        }
        let ours = self.our_txids();
        let mut received: HashMap<String, u64> = HashMap::new();
        for e in self.entries().into_iter().filter(|e| !e.tx_hash_hex.is_empty()) {
            let id = e.tx_hash_hex.to_lowercase();
            if ours.contains(&id) {
                continue;
            }
            *received.entry(id).or_insert(0) += e.amount_pxmr;
        }
        let done = self.donations_receipted();
        for donor in donors {
            for m in self.thread(&donor.persona_hex) {
                if m.outgoing || m.kind != 2 || m.re_seq.is_some() {
                    continue;
                }
                if m.timestamp + CLOCK_SKEW_SECS < donor.my_card_purpose_at {
                    continue;
                }
                let Some(txid) = m.txid_hex.as_deref().map(|t| t.to_lowercase()) else { continue };
                if done.contains(&txid) {
                    continue;
                }
                let Some(&amount) = received.get(&txid) else { continue };
                if amount == 0 || !self.settles(&txid) {
                    continue;
                }
                self.mark_donation_receipted(&txid);
                let out = Outgoing {
                    body: RECEIPT_NOTE.into(),
                    kind: 3,
                    amount_pxmr: Some(amount),
                    txid_hex: m.txid_hex.clone(),
                    re_seq: Some(m.seq),
                    re_own: false,
                    ..Default::default()
                };
                match self.send(&donor, out) {
                    Ok(_) => log::info(TAG, format!("donation receipted: {} XMR from {}", format_xmr(amount), donor.display_name())),
                    Err(e) => log::warn(TAG, format!("receipt: {e}")),
                }
            }
        }
    }
}
