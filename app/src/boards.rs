//! Boards: the stands notices are posted on, named by place and week, and
//! the block beacon every notice is stamped with — the phone's
//! `Hailing.standNow`/`standStale` and `Beacons.kt`.
//!
//! A stand is a DHT record named for a cell and a weekly epoch (§16.18):
//! `local:<geohash>` becomes `local:<geohash>@<epoch>-<shard>`. A notice
//! carries the height and hash of a recent Monero block (§16.18.1), so a
//! board for next week cannot be filled this afternoon: the reader checks
//! the hash against the chain, a lookup at a time, a few per board.

use std::collections::HashMap;
use std::sync::Mutex;

use ducat_mobile::contacts::{standEpoch, standEpochName, standShardName};
use ducat_mobile::monero::monero_block_ref;

use crate::contacts::{now_ms, CONTACTS};
use crate::{log, App};

const TAG: &str = "Beacons";
const TIP_FRESH_MS: u64 = 3 * 60 * 1000;
const STAMP_SLACK_MS: i64 = 60_000;
pub const LOOKUPS_PER_BOARD: u32 = 8;

static TIP: Mutex<(u64, u64)> = Mutex::new((0, 0));
static HASHES: Mutex<Option<HashMap<u64, String>>> = Mutex::new(None);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    Confirmed,
    Unknown,
    Wrong,
}

#[derive(Clone, Debug)]
pub struct Stamp {
    pub height: u64,
    pub hash_hex: String,
}

/// A board's name for this week. A name that already carries its epoch
/// is returned as it is.
pub fn stand_now(base: &str) -> String {
    if base.contains('@') {
        return base.to_string();
    }
    let epoch = standEpoch(App::now());
    standEpochName(base.to_string(), epoch).unwrap_or_else(|_| base.to_string())
}

pub fn stand_shard(base_now: &str, shard: u32) -> Option<String> {
    standShardName(base_now.to_string(), shard).ok()
}

/// Whether a board name belongs to a week that has passed.
pub fn stand_stale(board: &str) -> bool {
    if !board.contains('@') {
        return true;
    }
    let base = board.split('@').next().unwrap_or("");
    let generation = board.split('-').next().unwrap_or("");
    stand_now(base) != generation
}

pub fn max_stand_shards() -> u32 {
    ducat_mobile::contacts::maxStandShards()
}

pub fn max_notice_ttl_secs() -> u64 {
    ducat_mobile::contacts::maxNoticeTtlSecs()
}

/// A lookup allowance for one board's worth of notices.
pub struct Budget {
    left: u32,
}

pub fn budget() -> Budget {
    Budget { left: LOOKUPS_PER_BOARD }
}

fn remember(height: u64, hash_hex: &str) {
    let mut g = HASHES.lock().unwrap_or_else(|e| e.into_inner());
    let m = g.get_or_insert_with(HashMap::new);
    if m.len() > 4 * 720 {
        m.clear();
    }
    m.insert(height, hash_hex.to_string());
}

fn usable_tip(stored: u64, stored_at: u64, now: u64) -> u64 {
    if stored == 0 || stored_at == 0 {
        return 0;
    }
    let age = now as i64 - stored_at as i64;
    if age < TIP_FRESH_MS as i64 && age > -STAMP_SLACK_MS {
        stored
    } else {
        0
    }
}

impl App {
    /// The chain's height as recently seen — cached three minutes, kept
    /// across restarts, refreshed from the node when older.
    pub fn beacon_tip(&self) -> u64 {
        let now = now_ms();
        {
            let g = TIP.lock().unwrap_or_else(|e| e.into_inner());
            if g.0 > 0 && now.saturating_sub(g.1) < TIP_FRESH_MS {
                return g.0;
            }
        }
        let store = self.store(CONTACTS);
        let stored_at: u64 = store.get("beacon_tip_at").unwrap_or(0);
        let stored = usable_tip(store.get("beacon_tip").unwrap_or(0), stored_at, now);
        if stored > 0 {
            *TIP.lock().unwrap_or_else(|e| e.into_inner()) = (stored, stored_at);
            return stored;
        }
        let Some(url) = self.last_good_node() else { return 0 };
        let Ok(got) = monero_block_ref(url, 0) else { return 0 };
        if got.tip_height == 0 {
            return 0;
        }
        *TIP.lock().unwrap_or_else(|e| e.into_inner()) = (got.tip_height, now);
        if !got.hash_hex.is_empty() {
            remember(got.tip_height, &got.hash_hex);
        }
        let _ = store.update(|m| {
            m.insert("beacon_tip".into(), serde_json::Value::from(got.tip_height));
            m.insert("beacon_tip_at".into(), serde_json::Value::from(now));
        });
        got.tip_height
    }

    /// A fresh stamp for a notice about to be posted; None without a
    /// chain view — there is nothing honest to put there.
    pub fn stamp_now(&self) -> Option<Stamp> {
        let url = self.last_good_node()?;
        let at = monero_block_ref(url, 0).ok()?;
        if at.tip_height == 0 || at.hash_hex.is_empty() {
            return None;
        }
        let now = now_ms();
        *TIP.lock().unwrap_or_else(|e| e.into_inner()) = (at.tip_height, now);
        remember(at.tip_height, &at.hash_hex);
        let _ = self.store(CONTACTS).update(|m| {
            m.insert("beacon_tip".into(), serde_json::Value::from(at.tip_height));
            m.insert("beacon_tip_at".into(), serde_json::Value::from(now));
        });
        Some(Stamp { height: at.tip_height, hash_hex: at.hash_hex })
    }

    /// Does the chain have this block with this hash? Cached answers are
    /// free; a lookup spends the budget.
    pub fn confirm_beacon(&self, height: u64, hash_hex: &str, budget: &mut Budget) -> Verdict {
        if height == 0 || hash_hex.is_empty() {
            return Verdict::Unknown;
        }
        if let Some(known) = HASHES.lock().unwrap_or_else(|e| e.into_inner()).as_ref().and_then(|m| m.get(&height).cloned()) {
            return if known.eq_ignore_ascii_case(hash_hex) { Verdict::Confirmed } else { Verdict::Wrong };
        }
        if height > self.beacon_tip() {
            return Verdict::Unknown;
        }
        let Some(url) = self.last_good_node() else { return Verdict::Unknown };
        if budget.left == 0 {
            return Verdict::Unknown;
        }
        budget.left -= 1;
        let Ok(got) = monero_block_ref(url, height) else { return Verdict::Unknown };
        if got.hash_hex.is_empty() {
            return Verdict::Unknown;
        }
        remember(height, &got.hash_hex);
        if got.hash_hex.eq_ignore_ascii_case(hash_hex) {
            Verdict::Confirmed
        } else {
            log::warn(TAG, format!("a notice claims block {height} with a hash that block does not have"));
            Verdict::Wrong
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_board_name_carries_this_weeks_epoch_and_goes_stale_with_it() {
        let now = stand_now("local:dqche");
        assert!(now.starts_with("local:dqche@"));
        assert_eq!(stand_now(&now), now);
        let shard = stand_shard(&now, 0).unwrap();
        assert!(!stand_stale(&shard));
        assert!(stand_stale("local:dqche"));
        assert!(stand_stale("local:dqche@1-0"));
    }

    #[test]
    fn a_stored_tip_is_usable_only_while_fresh_and_not_from_the_future() {
        assert_eq!(usable_tip(100, 1_000, 1_000 + TIP_FRESH_MS - 1), 100);
        assert_eq!(usable_tip(100, 1_000, 1_000 + TIP_FRESH_MS + 1), 0);
        assert_eq!(usable_tip(100, 100_000, 1_000), 0);
        assert_eq!(usable_tip(0, 1_000, 1_000), 0);
    }
}
