//! The swarm, riding the node the mailbox already runs (post-1.0, 1.3).
//!
//! stigmerge (vendored, credited, BLAKE3 — see mobile/vendor/) moves the
//! bytes; this module is only the marriage: one borrowed veilnet
//! connection over the running [`crate::node`] API, the route registry
//! feeding the node's AppCall demux, and two verbs — seed and fetch.
//!
//! Exposed over uniffi since the two-process proof (100 MiB, ~3 Mbit/s,
//! BLAKE3-identical — see STIGMERGE-NOTICE.md): the clients speak these
//! verbs now, and mobile/examples/swarmtest.rs stays as the harness.

use std::sync::{Mutex, OnceLock};

use stigmerge_peer::share::{Event, Mode, Share};
use tokio_util::sync::CancellationToken;
use veilnet::connection::veilid::connection::Connection as VeilidConnection;

#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum SwarmError {
    #[error("swarm: {0}")]
    Failed(String),
}

fn fail<E: std::fmt::Display>(e: E) -> SwarmError {
    SwarmError::Failed(e.to_string())
}

/// The one borrowed connection, shared by every seed and fetch — a second
/// one would be a second handler chain reading the same feeder.
static CONN: OnceLock<Mutex<Option<VeilidConnection>>> = OnceLock::new();

/// A running seed, kept alive until stopped: the Share's tasks serve
/// block requests for as long as this holds their cancellation token.
static SEEDING: OnceLock<Mutex<std::collections::HashMap<String, CancellationToken>>> = OnceLock::new();

fn conn_slot() -> &'static Mutex<Option<VeilidConnection>> {
    CONN.get_or_init(|| Mutex::new(None))
}

fn seeding_slot() -> &'static Mutex<std::collections::HashMap<String, CancellationToken>> {
    SEEDING.get_or_init(|| Mutex::new(std::collections::HashMap::new()))
}

/// One seed per key. A second seed of the same share (a re-fetch that
/// stays, a library re-seeding what it already serves) replaces the first
/// and cancels it — an overwritten token was a Share still serving, with
/// nothing left that could ever stop it.
fn register_seed(key: String, cancel: CancellationToken) {
    let old = seeding_slot()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .insert(key, cancel);
    if let Some(old) = old {
        old.cancel();
    }
}

/// The node under this module went away (see `node_stop`). The borrowed
/// connection, the feeder installed for it and every seed's tasks belonged
/// to that node's runtime and died with it; a cached connection to a dead
/// node made every later swarm call fail with a stale handle until the
/// process was killed. Cleared here, the next call borrows the new node.
pub(crate) fn node_stopped() {
    for (_, c) in seeding_slot()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .drain()
    {
        c.cancel();
    }
    conn_slot()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .take();
    progress_map()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clear();
}

/// The borrowed connection, made on first use.
///
/// Order matters and is subtle: the feeder must be installed BEFORE
/// `from_api` returns to anyone, or an update arriving in the gap is
/// dropped on the floor — so the whole dance happens under the slot's
/// lock, and the route observer goes in at the same time.
fn ensure_conn() -> Result<VeilidConnection, SwarmError> {
    let mut slot = crate::lock(conn_slot());
    if let Some(c) = slot.as_ref() {
        return Ok(c.clone());
    }
    let (api, rt) = crate::node::swarm_handles()
        .ok_or_else(|| SwarmError::Failed("the node is not running".into()))?;
    let (conn, feeder) = rt
        .block_on(VeilidConnection::from_api(api))
        .map_err(fail)?;
    crate::node::swarm_install_feeder(feeder);
    stigmerge_peer::route_registry::set_observer(Box::new(|route_id, added| {
        crate::node::swarm_route_changed(route_id, added);
    }));
    *slot = Some(conn.clone());
    Ok(conn)
}

/// What a seed hands out: the share key a fetcher bootstraps from, and
/// the index digest that authenticates what they will be handed. These
/// two travel together on the §16.20 thread — a key without its digest
/// bootstraps into whatever answers, which is not a fetch, it is an ask.
#[derive(uniffi::Record, Clone)]
pub struct SwarmShare {
    pub share_key: String,
    pub index_digest_hex: String,
}

/// Where a fetch is, for a screen that polls: bytes landed, bytes
/// wanted. Zero/zero before the first status arrives.
#[derive(uniffi::Record, Clone, Default)]
pub struct SwarmProgress {
    pub position: i64,
    pub length: u64,
    pub done: bool,
}

static PROGRESS: OnceLock<Mutex<std::collections::HashMap<String, SwarmProgress>>> =
    OnceLock::new();

fn progress_map() -> &'static Mutex<std::collections::HashMap<String, SwarmProgress>> {
    PROGRESS.get_or_init(|| Mutex::new(std::collections::HashMap::new()))
}

/// The current fetch's progress. One fetch at a time is the client
/// contract for now; a screen polls this the way wallet sync is polled.
#[uniffi::export]
pub fn swarm_fetch_progress(share_key: String) -> SwarmProgress {
    crate::lock(progress_map())
        .get(&share_key)
        .cloned()
        .unwrap_or(SwarmProgress {
            position: 0,
            length: 0,
            done: false,
        })
}

/// Seed a file into the swarm. Returns once the share is announced and
/// every local piece is verified available; serving continues in the
/// background until [`swarm_stop`].
#[uniffi::export]
pub fn swarm_seed(path: String) -> Result<SwarmShare, SwarmError> {
    let conn = ensure_conn()?;
    let (_, rt) = crate::node::swarm_handles()
        .ok_or_else(|| SwarmError::Failed("the node is not running".into()))?;
    rt.block_on(async move {
        let mut share = Share::new(
            conn,
            Mode::Seed {
                path: std::path::PathBuf::from(path),
            },
        )
        .map_err(fail)?;
        let mut events = share.subscribe_events();
        let cancel = CancellationToken::new();
        share.start(cancel.clone()).await.map_err(fail)?;

        // The events channel answers "announced where" and "ready to serve".
        let mut out: Option<SwarmShare> = None;
        let mut available = false;
        while out.is_none() || !available {
            match events.recv().await.map_err(fail)? {
                Event::ShareInfo(info) => {
                    out = Some(SwarmShare {
                        share_key: info.key.to_string(),
                        index_digest_hex: hex_of(&info.want_index_digest),
                    });
                }
                Event::SeederAvailable => available = true,
                _ => {}
            }
        }
        // Registered by share key: every seed is individually stoppable,
        // and older seeds no longer become unstoppable orphans when a new
        // one starts (the old single slot dropped their tokens).
        if let Some(share) = &out {
            register_seed(share.share_key.clone(), cancel);
        }
        // The Share's tasks keep serving; their JoinSet lives inside it, so
        // park the whole thing on the runtime for the life of the seed.
        tokio::spawn(async move {
            let _ = share.join().await;
        });
        Ok(out.expect("loop exits with it set"))
    })
}

/// Stop serving. A fetcher mid-download loses this source and keeps any
/// other peer it has met — every peer is a seeder, which is the shape's
/// whole point.
#[uniffi::export]
pub fn swarm_stop() {
    for (_, c) in crate::lock(seeding_slot()).drain() {
        c.cancel();
    }
}

/// Stop seeding one share, leaving the rest serving.
#[uniffi::export]
pub fn swarm_stop_share(share_key: String) {
    if let Some(c) = crate::lock(seeding_slot()).remove(&share_key) {
        c.cancel();
    }
}

/// Fetch a share into `root`, blocking until every piece has verified.
/// Returns the byte count. The caller supplies the digest it was promised
/// (it rode the same message as the share key, §16.20's manifest rule) —
/// a share whose index does not match is not the content, whatever its
/// key says. Blocking by design: the Kotlin side calls it on IO, the way
/// attachment chunk fetches already block there.
#[uniffi::export]
pub fn swarm_fetch(
    share_key: String,
    index_digest_hex: String,
    root: String,
    // stay_seeding: keep serving after the last piece verifies. The
    // fetching share already answers block requests (all peers are
    // seeders); with this set it is parked in the seed registry under its
    // share key instead of torn down - the reader becomes a mirror. Also
    // how a restart re-seeds finished content: a fetch over complete
    // files verifies, downloads nothing, and stays.
    stay_seeding: bool,
) -> Result<u64, SwarmError> {
    let conn = ensure_conn()?;
    let (_, rt) = crate::node::swarm_handles()
        .ok_or_else(|| SwarmError::Failed("the node is not running".into()))?;
    let want: [u8; 32] = {
        let v = un_hex(&index_digest_hex)
            .ok_or_else(|| SwarmError::Failed("digest is 64 hex chars".into()))?;
        v.try_into()
            .map_err(|_| SwarmError::Failed("digest is 32 bytes".into()))?
    };
    let key: veilid_core::RecordKey = share_key
        .parse()
        .map_err(|_| SwarmError::Failed("that is not a share key".into()))?;
    rt.block_on(async move {
        // Two live-met failures, retried rather than surfaced:
        //
        // TryAgain — a node that only just attached refuses route allocation
        // ("allocated route failed to test"); veilid means exactly what it
        // says, so say it back with a retry rather than a failure the person
        // has to be for us.
        //
        // A stall — the block stream going quiet mid-transfer (a seeder's
        // route rotating under it, first seen 5.2 MB into a phone fetch).
        // The fetcher waits politely for ever; we do not. Tear down and
        // re-bootstrap: the share record names the seeder's current route,
        // and the pieces already on disk verify rather than re-download.
        let mut waited = 0u64;
        let mut stalls = 0u32;
        loop {
            crate::node::note(format!(
                "swarm: fetch attempt (stalls {stalls}, waited {waited}s) for {}…",
                &share_key[..share_key.len().min(20)]
            ));
            let before = crate::lock(progress_map())
                .get(&share_key)
                .map(|p| p.position)
                .unwrap_or(0);
            let outcome = fetch_once(
                conn.clone(),
                root.clone(),
                want,
                key.clone(),
                share_key.clone(),
                stay_seeding,
            )
            .await;
            let after = crate::lock(progress_map())
                .get(&share_key)
                .map(|p| p.position)
                .unwrap_or(0);
            // An attempt that moved bytes buys the next one a clean slate:
            // a swarm seeded through a dead origin's frozen peer list deals
            // pieces by lottery among live and dead candidates, and each
            // re-bootstrap redraws. Progress means somebody is serving —
            // only six DRY windows in a row mean nobody is.
            if after > before {
                stalls = 0;
            }
            match outcome {
                Err(SwarmError::Failed(e)) if e.contains("TryAgain") && waited < 40 => {
                    crate::node::note(format!("swarm: route not ready, retrying — {e}"));
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                    waited += 5;
                }
                Err(SwarmError::Failed(e)) if e.contains("went quiet") && stalls < 6 => {
                    crate::node::note("swarm: went quiet, re-bootstrapping".into());
                    stalls += 1;
                }
                other => {
                    if let Err(SwarmError::Failed(e)) = &other {
                        crate::node::note(format!("swarm: giving up — {e}"));
                    }
                    return other;
                }
            }
        }
    })
}

async fn fetch_once(
    conn: VeilidConnection,
    root: String,
    want: [u8; 32],
    key: veilid_core::RecordKey,
    progress_key: String,
    stay_seeding: bool,
) -> Result<u64, SwarmError> {
    let mut share = Share::new(
        conn,
        Mode::Fetch {
            root: std::path::PathBuf::from(root),
            want_index_digest: Some(want),
            share_keys: vec![key],
        },
    )
    .map_err(fail)?;
    let mut events = share.subscribe_events();
    let cancel = CancellationToken::new();
    if let Err(e) = share.start(cancel.clone()).await {
        // Half-started tasks die with the token; the join is parked so a
        // slow shutdown cannot hold the retry hostage.
        cancel.cancel();
        tokio::spawn(async move {
            let _ = share.join().await;
        });
        crate::node::note(format!("swarm: bootstrap refused — {e}"));
        return Err(fail(e));
    }
    crate::node::note("swarm: bootstrapped, waiting on the stream".into());

    crate::lock(progress_map()).insert(
        progress_key.clone(),
        SwarmProgress {
            position: 0,
            length: 0,
            done: false,
        },
    );
    let mut total: u64 = 0;
    let mut seen: u32 = 0;
    let mut baseline: i64 = 0;
    let mut advanced = false;
    let mut quiet_windows = 0u32;
    let born = std::time::Instant::now();
    loop {
        // The watchdog: verification of what is already on disk emits
        // progress, so a healthy fetch — resumed or fresh — always has
        // something to say inside this window. Silence means the stream
        // died under a peer that will wait for ever, and the caller's
        // answer to that is a re-bootstrap, not more waiting.
        //
        // Unless bytes have already moved THIS attempt: then somebody is
        // serving, the pool is mid-lottery between live peers and the
        // frozen peer list's corpses, and a teardown would erase the
        // failure scores it just paid for. A progressing attempt gets
        // three quiet windows before the axe; a dry one still gets one.
        let ev = tokio::time::timeout(
            std::time::Duration::from_secs(90),
            events.recv(),
        )
        .await;
        let ev = match ev {
            Err(_) => {
                if advanced && quiet_windows < 2 {
                    quiet_windows += 1;
                    crate::node::note(format!(
                        "swarm: quiet window {quiet_windows} on a moving fetch, holding"
                    ));
                    continue;
                }
                crate::node::note(format!(
                    "swarm: watchdog fired after {seen} event(s)"
                ));
                cancel.cancel();
                tokio::spawn(async move {
                    let _ = share.join().await;
                });
                return Err(SwarmError::Failed("the swarm went quiet".into()));
            }
            Ok(r) => r.map_err(fail)?,
        };
        quiet_windows = 0;
        seen += 1;
        if seen <= 12 || seen % 64 == 0 {
            let line: String = format!("{ev:?}").chars().take(110).collect();
            crate::node::note(format!("swarm: ev {seen}: {line}"));
        }
        match ev {
            Event::FetcherStatus(stigmerge_peer::fetcher::Status::Done) => {
                if let Some(p) = crate::lock(progress_map()).get_mut(&progress_key) {
                    p.done = true;
                }
                break;
            }
            Event::FetcherStatus(stigmerge_peer::fetcher::Status::FetchProgress {
                fetch_position,
                fetch_length,
                ..
            }) => {
                total = fetch_length;
                // Early reports are the resume point — what disk already
                // held, re-verified in a burst; only movement past that,
                // later than the burst, is the network actually serving.
                if born.elapsed() < std::time::Duration::from_secs(10) {
                    baseline = baseline.max(fetch_position);
                } else if fetch_position > baseline {
                    advanced = true;
                }
                if let Some(p) = crate::lock(progress_map()).get_mut(&progress_key) {
                    p.position = fetch_position;
                    p.length = fetch_length;
                }
            }
            _ => {}
        }
    }
    if stay_seeding {
        // The share already served every verified piece on the way down;
        // staying is just not leaving. Registered like any seed, so it is
        // individually stoppable and dies with the process otherwise.
        register_seed(progress_key, cancel);
        tokio::spawn(async move {
            let _ = share.join().await;
        });
        return Ok(total);
    }
    // Done: stop our tasks and let them wind down without holding the
    // fetch hostage to their shutdown order.
    cancel.cancel();
    tokio::spawn(async move {
        let _ = share.join().await;
    });
    Ok(total)
}

fn hex_of(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn un_hex(s: &str) -> Option<Vec<u8>> {
    crate::hex_to_bytes(s)
}
