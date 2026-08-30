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
static SEEDING: OnceLock<Mutex<Option<CancellationToken>>> = OnceLock::new();

fn conn_slot() -> &'static Mutex<Option<VeilidConnection>> {
    CONN.get_or_init(|| Mutex::new(None))
}

fn seeding_slot() -> &'static Mutex<Option<CancellationToken>> {
    SEEDING.get_or_init(|| Mutex::new(None))
}

/// The borrowed connection, made on first use.
///
/// Order matters and is subtle: the feeder must be installed BEFORE
/// `from_api` returns to anyone, or an update arriving in the gap is
/// dropped on the floor — so the whole dance happens under the slot's
/// lock, and the route observer goes in at the same time.
fn ensure_conn() -> Result<VeilidConnection, SwarmError> {
    let mut slot = conn_slot().lock().unwrap();
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

static PROGRESS: Mutex<SwarmProgress> = Mutex::new(SwarmProgress {
    position: 0,
    length: 0,
    done: false,
});

/// The current fetch's progress. One fetch at a time is the client
/// contract for now; a screen polls this the way wallet sync is polled.
#[uniffi::export]
pub fn swarm_fetch_progress() -> SwarmProgress {
    PROGRESS.lock().unwrap().clone()
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
        *seeding_slot().lock().unwrap() = Some(cancel);
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
    if let Some(c) = seeding_slot().lock().unwrap().take() {
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
        share.start(cancel.clone()).await.map_err(fail)?;

        *PROGRESS.lock().unwrap() = SwarmProgress::default();
        let mut total: u64 = 0;
        loop {
            match events.recv().await.map_err(fail)? {
                Event::FetcherStatus(stigmerge_peer::fetcher::Status::Done) => {
                    let mut p = PROGRESS.lock().unwrap();
                    p.done = true;
                    break;
                }
                Event::FetcherStatus(stigmerge_peer::fetcher::Status::FetchProgress {
                    fetch_position,
                    fetch_length,
                    ..
                }) => {
                    total = fetch_length;
                    let mut p = PROGRESS.lock().unwrap();
                    p.position = fetch_position;
                    p.length = fetch_length;
                }
                _ => {}
            }
        }
        // Done: stop our tasks (we are not staying to seed in this call —
        // the caller decides that with a seed of its own) and let them wind
        // down without holding the fetch hostage to their shutdown order.
        cancel.cancel();
        tokio::spawn(async move {
            let _ = share.join().await;
        });
        Ok(total)
    })
}

fn hex_of(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn un_hex(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
        .collect()
}
