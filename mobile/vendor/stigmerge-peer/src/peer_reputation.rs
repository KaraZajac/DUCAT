// DUCAT modification (see ../../STIGMERGE-NOTICE.md): peer reputation.
//
// The fetcher respawns an exited pool immediately and unconditionally —
// upstream's own TODO calls this out — so one dead peer is redialed hot
// for ever, and a share whose origin died keeps every corpse in its
// frozen peer list in the piece lottery. Requests to the dead then burn
// most of every window while the one live mirror waits its turn.
//
// This is the standard cure, process-global like `route_registry`: the
// libp2p peerstore's dial backoff joined with BitTorrent's snub. A peer
// that fails waits out an exponential backoff (30 s doubling, capped at
// ten minutes) before it is dialed again; one success clears it; memory
// older than an hour is forgotten, because a verdict that cannot decay
// turns a transient failure into a permanent partition. Entries a
// frozen peer record last refreshed over an hour ago start with one
// minute on the bench rather than a hot dial — staleness is a priority
// signal, never a filter, since a dead origin freezes the live mirror's
// timestamp along with the corpses'.
//
// The fetcher guarantees the optimistic slot (BitTorrent's optimistic
// unchoke): when nothing is admissible, the least-recently-tried peer
// is dialed anyway, so a swarm of benched peers can still be probed and
// a recovered one rediscovered.

use std::collections::HashMap;
use std::sync::{OnceLock, RwLock};
use std::time::{Duration, Instant};

use veilid_core::RecordKey;

const BASE: Duration = Duration::from_secs(30);
const CAP: Duration = Duration::from_secs(600);
const FORGET: Duration = Duration::from_secs(3600);
const STALE_SEED: Duration = Duration::from_secs(60);

#[derive(Clone, Copy)]
struct Rep {
    failures: u32,
    last_attempt: Instant,
    admissible_at: Instant,
}

static REP: OnceLock<RwLock<HashMap<RecordKey, Rep>>> = OnceLock::new();

fn map() -> &'static RwLock<HashMap<RecordKey, Rep>> {
    REP.get_or_init(|| RwLock::new(HashMap::new()))
}

fn fresh(now: Instant) -> Rep {
    Rep {
        failures: 0,
        last_attempt: now,
        admissible_at: now,
    }
}

/// The peer is being dialed now.
pub fn note_attempt(key: &RecordKey) {
    let now = Instant::now();
    let mut m = map().write().unwrap();
    m.entry(key.clone()).or_insert_with(|| fresh(now)).last_attempt = now;
}

/// The peer served a whole verified piece: cleared.
pub fn note_success(key: &RecordKey) {
    let now = Instant::now();
    map().write().unwrap().insert(key.clone(), fresh(now));
}

/// The peer failed a dial or a piece: bench it, twice as long each time.
pub fn note_failure(key: &RecordKey) {
    let now = Instant::now();
    let mut m = map().write().unwrap();
    let r = m.entry(key.clone()).or_insert_with(|| fresh(now));
    r.failures = r.failures.saturating_add(1);
    r.last_attempt = now;
    let wait = BASE
        .checked_mul(1u32 << (r.failures - 1).min(20))
        .map(|d| d.min(CAP))
        .unwrap_or(CAP);
    r.admissible_at = now + wait;
}

/// A peer learned from an entry nobody has refreshed in a long time —
/// a frozen record's whole roster, typically. Benched briefly on first
/// sight so fresh entries dial first; an already-known peer keeps its
/// earned record.
pub fn note_stale(key: &RecordKey) {
    let now = Instant::now();
    let mut m = map().write().unwrap();
    m.entry(key.clone()).or_insert(Rep {
        failures: 0,
        last_attempt: now,
        admissible_at: now + STALE_SEED,
    });
}

/// How much bench time remains — `None` means dial away. Memory idle
/// past the horizon is forgotten on the way out.
pub fn wait_remaining(key: &RecordKey) -> Option<Duration> {
    let now = Instant::now();
    let mut m = map().write().unwrap();
    if let Some(r) = m.get(key).copied() {
        if now.duration_since(r.last_attempt) > FORGET {
            m.remove(key);
            return None;
        }
        if r.admissible_at > now {
            return Some(r.admissible_at - now);
        }
    }
    None
}

/// Of the given peers, the one least recently tried — the optimistic
/// slot's pick when nothing is admissible.
pub fn least_recently_tried(keys: &[RecordKey]) -> Option<RecordKey> {
    let m = map().read().unwrap();
    keys.iter()
        .min_by_key(|k| m.get(*k).map(|r| r.last_attempt))
        .cloned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use veilid_core::{BareOpaqueRecordKey, BareRecordKey, RecordKey, CRYPTO_KIND_VLD0};

    fn key(b: u8) -> RecordKey {
        RecordKey::new(
            CRYPTO_KIND_VLD0,
            BareRecordKey::new(BareOpaqueRecordKey::new(&[b; 32]), None),
        )
    }

    #[test]
    fn failure_benches_and_success_clears() {
        let k = key(1);
        assert!(wait_remaining(&k).is_none());
        note_failure(&k);
        let w = wait_remaining(&k).expect("benched");
        assert!(w <= BASE && w > BASE / 2);
        note_failure(&k);
        let w2 = wait_remaining(&k).expect("benched longer");
        assert!(w2 > w, "doubles: {w2:?} vs {w:?}");
        note_success(&k);
        assert!(wait_remaining(&k).is_none(), "success clears the bench");
    }

    #[test]
    fn backoff_caps() {
        let k = key(2);
        for _ in 0..30 {
            note_failure(&k);
        }
        let w = wait_remaining(&k).expect("benched");
        assert!(w <= CAP, "capped: {w:?}");
    }

    #[test]
    fn stale_seed_is_gentle_and_not_an_overwrite() {
        let k = key(3);
        note_stale(&k);
        let w = wait_remaining(&k).expect("benched briefly");
        assert!(w <= STALE_SEED);
        // A peer with an earned clean record is not re-benched by
        // being rediscovered in somebody's stale roster.
        let k2 = key(4);
        note_success(&k2);
        note_stale(&k2);
        assert!(wait_remaining(&k2).is_none());
    }

    #[test]
    fn optimistic_pick_is_least_recently_tried() {
        let a = key(5);
        let b = key(6);
        note_failure(&a);
        std::thread::sleep(Duration::from_millis(5));
        note_failure(&b);
        // `a` was tried longer ago; an unknown key is older than both.
        assert_eq!(least_recently_tried(&[a.clone(), b.clone()]), Some(a));
        let c = key(7);
        assert_eq!(
            least_recently_tried(&[b.clone(), c.clone()]),
            Some(c),
            "never-tried sorts oldest"
        );
    }
}
