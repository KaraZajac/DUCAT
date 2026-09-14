//! The second opinion (§15's "one node's view is a claim"): before a
//! payment settles anything, a node other than the one in use is asked
//! where it stands. Three answers, not two — **in a block** elsewhere
//! settles, "in the pool" or "never heard of it" defers, and nobody
//! reachable defers too, unless the sale is small enough that the operator
//! said a lone node's word will do. Deferring costs nothing irreversible:
//! the tab stays billed, the next lap asks again, and after ten minutes
//! the operator is told and offered "settle anyway".
//!
//! It used to settle on silence ("a desk with one node is not a desk with a
//! liar") and on a pool sighting (`get_transactions` returns pool entries
//! too). The first meant a network position — every default node is plain
//! http — defeated the check by dropping the second node's packets; the
//! second meant a payment still replaceable in the mempool corroborated
//! itself. Only `InBlock` is cached: a yes from the pool is not a fact yet.

use ducat_mobile::monero::{monero_second_opinion_nodes, monero_tx_status, TxStatus};

use crate::{log, App, Error};

const TAG: &str = "SecondOpinion";
const STORE: &str = "second_opinion";
const REASK_MS: u64 = 60_000;
const ALARM_AFTER_MS: u64 = 10 * 60 * 1000;
/// The operator's small-sale floor, in pXMR. Zero — the default — means
/// no sale is small: nothing settles on silence, and every sale waits for
/// the confirmations its size needs.
const FLOOR_KEY: &str = "small_sale_floor";
pub const ONE_XMR: u64 = 1_000_000_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Another node has it in a block.
    Confirmed,
    /// Another node answered — pool, or never heard of it — so wait.
    NotYet,
    /// Nobody else could be reached.
    NoAnswer,
}

impl App {
    /// Sales at or under this amount settle on one block, and on our own
    /// node's word alone when no second node can be reached. The operator
    /// sets it; it is their risk to size.
    pub fn small_sale_floor_pxmr(&self) -> u64 {
        self.store(STORE).get(FLOOR_KEY).unwrap_or(0)
    }

    pub fn set_small_sale_floor_pxmr(&self, pxmr: u64) -> Result<(), Error> {
        self.store(STORE).put(FLOOR_KEY, &pxmr)?;
        crate::contacts::bump();
        Ok(())
    }

    /// How many blocks a payment of this size must have before it settles
    /// a sale: one under the operator's floor, three up to a monero, ten
    /// above that — Monero's own lock. A block is two minutes; a reorg
    /// deeper than three has not been seen in years, and nothing worth
    /// more than a monero should ride on fewer.
    pub fn confirmations_needed(&self, amount_pxmr: u64) -> u64 {
        if amount_pxmr > 0 && amount_pxmr <= self.small_sale_floor_pxmr() {
            1
        } else if amount_pxmr <= ONE_XMR {
            3
        } else {
            10
        }
    }

    /// Blocks on top of a note at `height` when the tip is `tip`; zero
    /// while it is not in a block.
    pub fn confirmations_of(height: u64, tip: u64) -> u64 {
        if height > 0 && tip >= height {
            tip - height + 1
        } else {
            0
        }
    }

    /// Whether a transaction may settle a sale of `amount_pxmr` now.
    /// Asked at most once a minute per transaction; after ten minutes
    /// without corroboration it is said out loud, once, and the screens
    /// offer the operator the choice.
    pub fn settles(&self, tx_hash_hex: &str, amount_pxmr: u64) -> bool {
        if tx_hash_hex.trim().is_empty() {
            return true;
        }
        let key = tx_hash_hex.to_lowercase();
        let s = self.store(STORE);
        if s.get::<bool>(&format!("ok_{key}")).unwrap_or(false) {
            return true;
        }
        let now = crate::contacts::now_ms();
        let asked = s.get::<u64>(&format!("asked_{key}")).unwrap_or(0);
        if asked != 0 && now.saturating_sub(asked) < REASK_MS {
            return false;
        }
        let verdict = self.on_tx(&key);
        self.decide(&key, verdict, now, amount_pxmr)
    }

    /// Asked for ten minutes and still not corroborated — the state the
    /// screens show "could not corroborate on a second node" for.
    pub fn opinion_stalled(&self, tx_hash_hex: &str) -> bool {
        self.stalled_at(&tx_hash_hex.to_lowercase(), crate::contacts::now_ms())
    }

    /// `opinion_stalled` with the clock passed in, so a test can walk a
    /// timeline instead of waiting ten minutes.
    fn stalled_at(&self, key: &str, now: u64) -> bool {
        let s = self.store(STORE);
        if s.get::<bool>(&format!("ok_{key}")).unwrap_or(false) {
            return false;
        }
        let since = s.get::<u64>(&format!("since_{key}")).unwrap_or(0);
        since != 0 && now.saturating_sub(since) >= ALARM_AFTER_MS
    }

    /// The operator's word in place of the missing corroboration. Recorded
    /// as such, so the log says who decided.
    pub fn settle_anyway(&self, tx_hash_hex: &str) -> Result<(), Error> {
        let key = tx_hash_hex.to_lowercase();
        self.store(STORE).update(|m| {
            m.insert(format!("ok_{key}"), serde_json::Value::from(true));
            m.insert(format!("forced_{key}"), serde_json::Value::from(crate::contacts::now_ms()));
            for k in ["asked_", "since_", "said_"] {
                m.remove(&format!("{k}{key}"));
            }
        })?;
        log::warn(TAG, format!("{}… settled on the operator's word, uncorroborated", &key[..12.min(key.len())]));
        Ok(())
    }

    fn decide(&self, key: &str, verdict: Verdict, now: u64, amount_pxmr: u64) -> bool {
        let s = self.store(STORE);
        let since = s.get::<u64>(&format!("since_{key}")).unwrap_or(0);
        match verdict {
            Verdict::Confirmed => {
                let _ = s.update(|m| {
                    m.insert(format!("ok_{key}"), serde_json::Value::from(true));
                    for k in ["asked_", "since_", "said_"] {
                        m.remove(&format!("{k}{key}"));
                    }
                });
                true
            }
            Verdict::NoAnswer if amount_pxmr > 0 && amount_pxmr <= self.small_sale_floor_pxmr() => {
                log::info(TAG, format!("{}… settling under the small-sale floor — no second node reachable", &key[..12.min(key.len())]));
                true
            }
            Verdict::NoAnswer | Verdict::NotYet => {
                let first = if since == 0 { now } else { since };
                let _ = s.update(|m| {
                    m.insert(format!("asked_{key}"), serde_json::Value::from(now));
                    m.insert(format!("since_{key}"), serde_json::Value::from(first));
                });
                if now.saturating_sub(first) >= ALARM_AFTER_MS && !s.get::<bool>(&format!("said_{key}")).unwrap_or(false) {
                    let _ = s.put(&format!("said_{key}"), &true);
                    let short = &key[..12.min(key.len())];
                    match verdict {
                        Verdict::NoAnswer => {
                            log::warn(TAG, format!("{short}… no second node reachable for ten minutes"));
                            crate::notify::post("Payment not corroborated", "No second node could be reached to confirm it. The sale stays billed until you settle it.", None);
                        }
                        _ => {
                            log::warn(TAG, format!("{short}… unknown to other nodes after ten minutes"));
                            crate::notify::post("Payment not confirmed", "Another node has no record of it. This sale stays unpaid.", None);
                        }
                    }
                }
                log::info(
                    TAG,
                    format!(
                        "{}… deferring: {}",
                        &key[..12.min(key.len())],
                        if verdict == Verdict::NoAnswer { "no second node reachable" } else { "not in a block elsewhere yet" }
                    ),
                );
                false
            }
        }
    }

    fn on_tx(&self, tx_hash_hex: &str) -> Verdict {
        let others = monero_second_opinion_nodes(self.last_good_node(), self.monero_own_url());
        if others.is_empty() {
            return Verdict::NoAnswer;
        }
        let short = &tx_hash_hex[..12.min(tx_hash_hex.len())];
        let mut answered = false;
        for n in others {
            match monero_tx_status(n.url.clone(), tx_hash_hex.to_string(), 8_000) {
                TxStatus::InBlock { height } => {
                    log::info(TAG, format!("{short}… in block {height} at {}", n.url));
                    return Verdict::Confirmed;
                }
                TxStatus::InPool => {
                    // Seen elsewhere, which rules out a forgery by our node —
                    // but not settled: a pool transaction can still be
                    // replaced. Wait for the block.
                    log::info(TAG, format!("{short}… in the pool at {} — not settled yet", n.url));
                    answered = true;
                }
                TxStatus::Unknown => answered = true,
                TxStatus::Unreachable => {}
            }
        }
        if answered {
            Verdict::NotYet
        } else {
            Verdict::NoAnswer
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_block_elsewhere_settles_and_silence_defers_above_the_floor() {
        let dir = std::env::temp_dir().join(format!("ducat-opinion-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let app = App::open(&dir).unwrap();
        assert!(app.settles("", 5));
        assert!(!app.decide("abc", Verdict::NotYet, 1_000, 5));
        // Silence no longer settles: the tab stays billed.
        assert!(!app.decide("abc", Verdict::NoAnswer, 2_000, 5));
        assert!(!app.stalled_at("abc", 2_500));
        assert!(app.stalled_at("abc", 1_000 + ALARM_AFTER_MS));
        assert!(app.decide("abc", Verdict::Confirmed, 3_000, 5));
        assert!(app.settles("ABC", 5));
        // Zero is "never asked", so the clock starts at one.
        assert!(!app.decide("def", Verdict::NotYet, 1, 5));
        assert!(!app.decide("def", Verdict::NoAnswer, ALARM_AFTER_MS + 2, 5));
        assert!(app.store(STORE).get::<bool>("said_def").unwrap_or(false));
        // The operator's word ends the wait.
        app.settle_anyway("def").unwrap();
        assert!(app.settles("def", 5));
        assert!(!app.opinion_stalled("def"));
    }

    #[test]
    fn the_small_sale_floor_settles_on_silence_and_needs_one_block() {
        let dir = std::env::temp_dir().join(format!("ducat-opinion-floor-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let app = App::open(&dir).unwrap();
        assert_eq!(app.confirmations_needed(1), 3);
        assert_eq!(app.confirmations_needed(ONE_XMR), 3);
        assert_eq!(app.confirmations_needed(ONE_XMR + 1), 10);
        // Unknown amounts are never small.
        assert_eq!(app.confirmations_needed(0), 3);
        app.set_small_sale_floor_pxmr(1_000).unwrap();
        assert_eq!(app.confirmations_needed(1_000), 1);
        assert_eq!(app.confirmations_needed(1_001), 3);
        assert!(app.decide("ghi", Verdict::NoAnswer, 2_000, 1_000));
        assert!(!app.decide("jkl", Verdict::NoAnswer, 2_000, 1_001));
        // A yes from the pool is not cached as a yes.
        assert!(!app.decide("mno", Verdict::NotYet, 2_000, 1_000));
        assert!(!app.store(STORE).get::<bool>("ok_mno").unwrap_or(false));
        assert_eq!(App::confirmations_of(100, 102), 3);
        assert_eq!(App::confirmations_of(0, 102), 0);
        assert_eq!(App::confirmations_of(103, 102), 0);
    }
}
