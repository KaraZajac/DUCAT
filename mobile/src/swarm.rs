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
    /// The share's index declares more bytes than this fetch was allowed
    /// (research/security, N4/D2). Its own variant so a screen can say the
    /// two numbers to the person instead of showing them a failure.
    #[error("{0}")]
    TooLarge(String),
    /// The transfer is alive but delivering nothing (N18). Distinct from
    /// "went quiet", which is silence: this one is a peer answering slowly
    /// enough to hold an unattended fetch open for ever.
    #[error("{0}")]
    TooSlow(String),
}

fn fail<E: std::fmt::Display>(e: E) -> SwarmError {
    SwarmError::Failed(e.to_string())
}

/// Per-kind fetch ceilings, in bytes (research/security phase 1: N4/D2, N26).
///
/// The index says how big a share is and, until these, the fetcher believed
/// it: a hearted home fetched on the lap, with nobody watching, could be any
/// size its publisher liked. The engine now refuses an index that declares
/// more than the caller will take, before it creates a single file — so
/// every caller must say what kind of thing it asked for, because "a photo
/// of a bicycle" and "a film" are not the same promise.
///
/// The same table is spelled out for the phone in Swarm.kt; keep them
/// together.
pub mod caps {
    /// A listing's pictures (§16.18.3). Photographs, thumbnailed.
    pub const GALLERY: u64 = 64 * 1024 * 1024;
    /// Somebody's home page and feed, fetched unattended when hearted.
    pub const HOME: u64 = 256 * 1024 * 1024;
    /// A kept site's bundle, also fetched unattended.
    pub const SITE: u64 = 256 * 1024 * 1024;
    /// One issue of a publication.
    pub const ISSUE: u64 = 256 * 1024 * 1024;
    /// A release whose entry does not name its own size. A release is the
    /// one thing here that is legitimately huge, and its address carries no
    /// length until somebody has fetched it once.
    pub const RELEASE: u64 = 4 * 1024 * 1024 * 1024;
    /// Slack over a sealed attachment's declared length: the AEAD tag and
    /// whatever the blob is wrapped in. The room check does the real work.
    pub const ATTACHMENT_SLACK: u64 = 1024 * 1024;
    /// What an uncapped caller gets. Not a budget — a ceiling, so that the
    /// "any size at all" shape cannot come back through a caller nobody
    /// updated.
    pub const DEFAULT: u64 = 4 * 1024 * 1024 * 1024;
}

/// Bytes as a person would say them, for an error they will read.
fn bytes_human(n: u64) -> String {
    const UNITS: [(&str, u64); 4] = [
        ("GB", 1_000_000_000),
        ("MB", 1_000_000),
        ("kB", 1_000),
        ("bytes", 1),
    ];
    for (unit, scale) in UNITS {
        if n >= scale {
            if scale == 1 {
                return format!("{n} bytes");
            }
            return format!("{:.1} {unit}", n as f64 / scale as f64);
        }
    }
    format!("{n} bytes")
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
    /// Pieces verified, and how many there are. Bytes alone cannot say
    /// whether a transfer is moving: a share whose peers all refuse sits
    /// at "0 B of 1.0 kB" and looks identical to one that has not started.
    /// The index knows the piece count before a single byte arrives, so a
    /// reader can be shown the shape of the thing and watch it fill.
    pub pieces_done: u64,
    pub pieces_total: u64,
    /// Which pieces are verified, one bit each, little-endian within a
    /// byte. Pieces land scattered — the lease manager pops them out of a
    /// HashSet of what is wanted, filtered by what each peer holds — so a
    /// count says far less than the pattern does.
    pub pieces: Vec<u8>,
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
            pieces_done: 0,
            pieces_total: 0,
            pieces: Vec::new(),
            done: false,
        })
}

/// Seed a file into the swarm. Returns once the share is announced and
/// every local piece is verified available; serving continues in the
/// background until [`swarm_stop`].
#[uniffi::export]
pub fn swarm_seed(path: String) -> Result<SwarmShare, SwarmError> {
    // A node that only just attached refuses route allocation for a while
    // ("allocated route failed to test"); the fetch side already waits it
    // out, and a publish that fails hard in that window was the seed side
    // not doing the same.
    let mut waited = 0u64;
    loop {
        match seed_once(path.clone()) {
            Err(SwarmError::Failed(e)) if e.contains("TryAgain") && waited < 90 => {
                crate::node::note(format!("swarm: seed route not ready, retrying — {e}"));
                std::thread::sleep(std::time::Duration::from_secs(5));
                waited += 5;
            }
            other => return other,
        }
    }
}

fn seed_once(path: String) -> Result<SwarmShare, SwarmError> {
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

/// Whether this process is serving a share right now — a seed that is
/// already up is not something to tear down and rebuild.
#[uniffi::export]
pub fn swarm_seeding(share_key: String) -> bool {
    crate::lock(seeding_slot()).contains_key(&share_key)
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
    swarm_fetch_capped(
        share_key,
        index_digest_hex,
        root,
        stay_seeding,
        caps::DEFAULT,
    )
}

/// The same fetch, with a ceiling on how big the share may say it is.
///
/// The index is the publisher's claim and the fetcher used to believe it:
/// every file it named was created and sized before a byte was verified.
/// `max_bytes` is what the caller will actually take — 64 MiB for a
/// listing's pictures, 256 MiB for somebody's home — and a share that
/// declares more is refused before anything is created, with
/// [`SwarmError::TooLarge`] so the caller can say both numbers out loud.
///
/// See [`caps`] for the table both clients use.
#[uniffi::export]
pub fn swarm_fetch_capped(
    share_key: String,
    index_digest_hex: String,
    root: String,
    stay_seeding: bool,
    max_bytes: u64,
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
    // Never our own (N12). A share key arrives in a message somebody else
    // wrote, and this one names a record *we* announce: opening it as a
    // stranger's re-opens our own record with no writer, and veilid then
    // refuses our own writes to it — a seeder silenced by a share key
    // handed back to it. Nothing legitimate asks a node to fetch what it
    // is already serving.
    if crate::lock(seeding_slot()).contains_key(&share_key) {
        return Err(SwarmError::Failed("that share is one of ours — fetching it would disarm the seed".into()));
    }
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
            let outcome = fetch_once(
                conn.clone(),
                root.clone(),
                want,
                key.clone(),
                share_key.clone(),
                stay_seeding,
                max_bytes,
            )
            .await;
            // The stall budget is NOT reset by an attempt that moved bytes
            // (N18). It used to be: any progress at all bought the next
            // attempt a clean slate, so a peer that served one block per
            // re-bootstrap could keep an unattended fetch — a hearted home,
            // a kept site — bootstrapping for ever. Six attempts is the
            // whole budget now, whatever happens inside them; a fetch that
            // is genuinely moving finishes inside one, and the minimum
            // throughput rule inside `fetch_once` ends the ones that are
            // not.
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
                Err(SwarmError::TooSlow(e)) if stalls < 6 => {
                    crate::node::note(format!("swarm: {e} — re-bootstrapping"));
                    stalls += 1;
                }
                other => {
                    match &other {
                        Err(SwarmError::Failed(e)) | Err(SwarmError::TooSlow(e)) => {
                            crate::node::note(format!("swarm: giving up — {e}"));
                        }
                        // Not a failure to retry: the share is what it is,
                        // and it is bigger than this caller will take.
                        Err(SwarmError::TooLarge(e)) => {
                            crate::node::note(format!("swarm: refused — {e}"));
                        }
                        Ok(_) => {}
                    }
                    return other;
                }
            }
        }
    })
}

/// The rule for a fetch that is alive but not arriving (N18).
///
/// A peer that answers just often enough keeps the engine's own watchdog
/// happy for ever, which on an unattended fetch — a hearted home, a kept
/// site — means a fetch that never ends and a node that never rests. Fewer
/// than one verified piece (1 MiB) a minute, over three minutes, with a
/// bootstrapped swarm to ask, is not a slow link: it is nobody serving.
const THROUGHPUT_WINDOW: std::time::Duration = std::time::Duration::from_secs(180);
const MIN_PIECES_PER_WINDOW: u64 = 3;

async fn fetch_once(
    conn: VeilidConnection,
    root: String,
    want: [u8; 32],
    key: veilid_core::RecordKey,
    progress_key: String,
    stay_seeding: bool,
    max_bytes: u64,
) -> Result<u64, SwarmError> {
    let mut share = Share::new(
        conn,
        Mode::Fetch {
            root: std::path::PathBuf::from(root),
            want_index_digest: Some(want),
            share_keys: vec![key],
            // The ceiling, enforced inside the engine before it creates or
            // sizes a single file of the publisher's choosing.
            max_bytes: Some(max_bytes),
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
        // Refused for its size: both numbers, in words the caller can put
        // in front of a person.
        if let Some(t) = stigmerge_peer::share::too_large(&e) {
            let said = format!(
                "this bundle says it is {}; the most this will take is {}",
                bytes_human(t.declared),
                bytes_human(t.max)
            );
            crate::node::note(format!("swarm: refused — {said}"));
            return Err(SwarmError::TooLarge(said));
        }
        crate::node::note(format!("swarm: bootstrap refused — {e}"));
        return Err(fail(e));
    }
    crate::node::note("swarm: bootstrapped, waiting on the stream".into());

    crate::lock(progress_map()).insert(
        progress_key.clone(),
        SwarmProgress {
            position: 0,
            length: 0,
            pieces_done: 0,
            pieces_total: 0,
            pieces: Vec::new(),
            done: false,
        },
    );
    let mut total: u64 = 0;
    let mut seen: u32 = 0;
    let mut baseline: i64 = 0;
    let mut advanced = false;
    let mut quiet_windows = 0u32;
    let born = std::time::Instant::now();
    // Minimum throughput (N18): pieces verified, and the window they are
    // counted over. The window opens at the first status the fetcher sends,
    // not at birth — before that the share is still being resolved, which
    // is DHT work and can honestly take minutes.
    let mut pieces_seen: u64 = 0;
    let mut window: Option<(std::time::Instant, u64)> = None;
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
        //
        // Measured end to end on two phones, 2026-09-02, fetching a site
        // whose only seeder could not be routed to: ShareInfo, one
        // FetchProgress at zero, then nothing. Watchdog at 90s, six
        // attempts, "giving up — the swarm went quiet" about nine minutes
        // after the tap. Worth writing down because veilid's own tracing
        // is busy throughout — LeaseRejected, advertise failures, a
        // `resuming delay=11.4s` every few seconds — and reads like a loop
        // that never returns. Those are its internal logs, not events on
        // this stream; the stream really does go quiet and this really
        // does terminate. Do not add a second timer on the strength of
        // the log alone.
        // Before the first event the stream is resolving the share and
        // announcing itself — several DHT operations, each of which has
        // been measured at ten to sixty seconds on a slow day. That phase
        // gets a longer first window; once the stream has spoken, silence
        // means what it always meant.
        // The throughput rule, checked on every pass — an attempt that is
        // producing events but no verified pieces must end too.
        if let Some((started, base)) = window {
            if started.elapsed() >= THROUGHPUT_WINDOW {
                let gained = pieces_seen.saturating_sub(base);
                if gained < MIN_PIECES_PER_WINDOW {
                    crate::node::note(format!(
                        "swarm: {gained} piece(s) in {}s — too slow to be serving",
                        THROUGHPUT_WINDOW.as_secs()
                    ));
                    cancel.cancel();
                    tokio::spawn(async move {
                        let _ = share.join().await;
                    });
                    return Err(SwarmError::TooSlow(
                        "the swarm is answering but not delivering".into(),
                    ));
                }
                window = Some((std::time::Instant::now(), pieces_seen));
            }
        }
        let watchdog = if seen == 0 { 240 } else { 90 };
        let ev = tokio::time::timeout(
            std::time::Duration::from_secs(watchdog),
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
                verify_position,
                verify_length,
            }) => {
                total = fetch_length;
                pieces_seen = pieces_seen.max(verify_position);
                if window.is_none() {
                    window = Some((std::time::Instant::now(), pieces_seen));
                }
                // Early reports are the resume point — what disk already
                // held, re-verified in a burst; only movement past that,
                // later than the burst, is the network actually serving.
                if born.elapsed() < std::time::Duration::from_secs(10) {
                    baseline = baseline.max(fetch_position);
                } else if fetch_position > baseline {
                    advanced = true;
                }
                let bitmap = share.verified_bitmap().await;
                if let Some(p) = crate::lock(progress_map()).get_mut(&progress_key) {
                    p.position = fetch_position;
                    p.length = fetch_length;
                    p.pieces_done = verify_position;
                    p.pieces_total = verify_length;
                    if let Some((bits, n)) = bitmap {
                        p.pieces = bits;
                        // The verifier knows the real count; verify_length
                        // has been seen to lag it early on.
                        if n > 0 {
                            p.pieces_total = n as u64;
                        }
                    }
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
