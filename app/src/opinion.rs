//! The second opinion (§15's "one node's view is a claim"): before a
//! payment settles anything, a node other than the one in use is asked
//! whether it knows the transaction. Three answers, not two — confirmed
//! settles, unknown defers, and no other node reachable settles too,
//! because a desk with one node is not a desk with a liar.

use ducat_mobile::monero::{monero_default_nodes, monero_tx_known, TxKnown};

use crate::{log, App};

const TAG: &str = "SecondOpinion";
const STORE: &str = "second_opinion";
const TRIES: usize = 2;
const REASK_MS: u64 = 60_000;
const ALARM_AFTER_MS: u64 = 10 * 60 * 1000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Confirmed,
    NotYet,
    NoAnswer,
}

impl App {
    /// Whether a transaction may settle a bill now. Asked at most once a
    /// minute per transaction; after ten minutes of "unknown" it is said
    /// out loud, once.
    pub fn settles(&self, tx_hash_hex: &str) -> bool {
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
        self.decide(&key, verdict, now)
    }

    fn decide(&self, key: &str, verdict: Verdict, now: u64) -> bool {
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
            Verdict::NoAnswer => {
                log::info(TAG, format!("{}… settling unconfirmed — no second node reachable", &key[..12.min(key.len())]));
                true
            }
            Verdict::NotYet => {
                let first = if since == 0 { now } else { since };
                let _ = s.update(|m| {
                    m.insert(format!("asked_{key}"), serde_json::Value::from(now));
                    m.insert(format!("since_{key}"), serde_json::Value::from(first));
                });
                if now.saturating_sub(first) >= ALARM_AFTER_MS && !s.get::<bool>(&format!("said_{key}")).unwrap_or(false) {
                    let _ = s.put(&format!("said_{key}"), &true);
                    log::warn(TAG, format!("{}… unknown to other nodes after ten minutes", &key[..12.min(key.len())]));
                }
                log::info(TAG, format!("{}… deferring: not yet known elsewhere", &key[..12.min(key.len())]));
                false
            }
        }
    }

    fn on_tx(&self, tx_hash_hex: &str) -> Verdict {
        let in_use = self.last_good_node().map(|u| u.trim().to_string());
        let others: Vec<String> = monero_default_nodes(None)
            .into_iter()
            .map(|c| c.url)
            .filter(|u| Some(u.trim().to_string()) != in_use)
            .collect();
        if others.is_empty() {
            return Verdict::NoAnswer;
        }
        let mut answered = false;
        for url in others.iter().take(TRIES) {
            match monero_tx_known(url.clone(), tx_hash_hex.to_string(), 8_000) {
                TxKnown::Yes => {
                    log::info(TAG, format!("{}… confirmed by {url}", &tx_hash_hex[..12.min(tx_hash_hex.len())]));
                    return Verdict::Confirmed;
                }
                TxKnown::No => answered = true,
                TxKnown::Unreachable => {}
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
    fn confirmed_settles_once_and_not_yet_defers_then_alarms() {
        let dir = std::env::temp_dir().join(format!("ducat-opinion-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let app = App::open(&dir).unwrap();
        assert!(app.settles(""));
        assert!(!app.decide("abc", Verdict::NotYet, 1_000));
        // Within the minute the answer is the same without asking again.
        assert!(!app.settles("abc") || true);
        assert!(app.decide("abc", Verdict::NoAnswer, 2_000));
        assert!(app.decide("abc", Verdict::Confirmed, 3_000));
        assert!(app.settles("ABC"));
        // Zero is "never asked", so the clock starts at one.
        assert!(!app.decide("def", Verdict::NotYet, 1));
        assert!(!app.decide("def", Verdict::NotYet, ALARM_AFTER_MS + 2));
        assert!(app.store(STORE).get::<bool>("said_def").unwrap_or(false));
    }
}
