//! A Veilid node inside the app.
//!
//! The node runs on its own runtime on a background thread and outlives any
//! screen: a route takes seconds to build and a payment cannot wait for it, so
//! starting one when the user opens a tap screen would put the transport's
//! latency directly into §15.3's three-second budget.
//!
//! Everything the UI sees is a snapshot it polls. Nothing here blocks a caller
//! on the network, because the caller is the main thread.

use std::collections::VecDeque;
use std::sync::{Mutex, OnceLock};

use veilid_core::*;

struct Handles {
    api: VeilidAPI,
    rt: tokio::runtime::Handle,
}

struct Node {
    api: VeilidAPI,
    runtime: tokio::runtime::Runtime,
}

static NODE: OnceLock<Mutex<Option<Node>>> = OnceLock::new();
/// Set while a node is coming up or going down — work the slot's lock does
/// not cover any more (see [`node_start`]). A start that meets it returns
/// as if the node were already running; the caller polls status either way.
static BUSY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Clears [`BUSY`] on every way out.
struct Busy;
impl Busy {
    fn take() -> Option<Busy> {
        (!BUSY.swap(true, std::sync::atomic::Ordering::SeqCst)).then_some(Busy)
    }
}
impl Drop for Busy {
    fn drop(&mut self) {
        BUSY.store(false, std::sync::atomic::Ordering::SeqCst);
    }
}

/// One bit and a doorbell: "something you watch has changed".
static CHANGE: OnceLock<(Mutex<bool>, std::sync::Condvar)> = OnceLock::new();

/// *Which* records changed, alongside the bit — so a listener that watches
/// many records can read the one that moved instead of all of them.
///
/// Bounded for the same reason the inbox is: a peer that can reach us can
/// change a record as fast as it likes, and the drain is on somebody else's
/// clock. Overflow drops the oldest; the sweep behind it is the guarantee.
const MAX_CHANGED: usize = 64;

static CHANGED: OnceLock<Mutex<VecDeque<String>>> = OnceLock::new();

fn changed_keys() -> &'static Mutex<VecDeque<String>> {
    CHANGED.get_or_init(|| Mutex::new(VecDeque::new()))
}

/// Take the record keys that have changed since the last call.
///
/// Draining, because these are events: whoever asks gets them, and asking
/// twice does not get them twice. Pairs with [`node_wait_change`], which
/// consumes the flag the same way and for the same reason.
#[uniffi::export]
pub fn node_changed_keys() -> Vec<String> {
    crate::lock(changed_keys()).drain(..).collect()
}

fn change_signal() -> &'static (Mutex<bool>, std::sync::Condvar) {
    CHANGE.get_or_init(|| (Mutex::new(false), std::sync::Condvar::new()))
}

fn slot() -> &'static Mutex<Option<Node>> {
    NODE.get_or_init(|| Mutex::new(None))
}

/// Inbound `app_call`s waiting for the UI to answer them.
///
/// Bounded, and the bound is not decoration: a peer that can reach our route
/// can queue as fast as it likes, and an unbounded queue behind a UI that polls
/// on a timer is a memory exhaustion bug with a network trigger. Overflow drops
/// the *oldest*, because a caller who has already waited past the deadline is
/// not going to be helped by an answer.
const MAX_PENDING: usize = 64;

static INBOX: OnceLock<Mutex<VecDeque<(u64, Vec<u8>)>>> = OnceLock::new();

// --- the swarm's share of the node (post-1.0 1.3) --------------------------
//
// stigmerge rides this node through veilnet's borrowed connection. The
// update callback below is the ONE place Veilid speaks to this process, so
// the swarm's view of the network is fed from here: every non-AppCall
// update is forwarded to its feeder, and AppCalls are demultiplexed by the
// route they arrived on — the seeder answers block requests on routes the
// announcer registered, the mailbox answers everything else, and neither
// can steal the other's single reply slot.
type Feeder = std::sync::Arc<dyn Fn(VeilidUpdate) + Send + Sync>;
static SWARM_FEEDER: OnceLock<Mutex<Option<Feeder>>> = OnceLock::new();
static SWARM_ROUTES: OnceLock<Mutex<std::collections::HashSet<RouteId>>> = OnceLock::new();

fn swarm_feeder() -> &'static Mutex<Option<Feeder>> {
    SWARM_FEEDER.get_or_init(|| Mutex::new(None))
}

fn swarm_routes() -> &'static Mutex<std::collections::HashSet<RouteId>> {
    SWARM_ROUTES.get_or_init(|| Mutex::new(std::collections::HashSet::new()))
}

pub(crate) fn swarm_install_feeder(f: Box<dyn Fn(VeilidUpdate) + Send + Sync>) {
    *crate::lock(swarm_feeder()) = Some(std::sync::Arc::from(f));
}

pub(crate) fn swarm_route_changed(route_id: &RouteId, added: bool) {
    let mut r = crate::lock(swarm_routes());
    if added {
        r.insert(route_id.clone());
    } else {
        r.remove(route_id);
    }
}

// ---------------------------------------------------------------------------
// Live calls (§16.21): media rides app messages on call-only routes.
//
// The same demux discipline as the swarm's AppCalls, for the same reason:
// the node has ONE update stream, and a voice frame must never be mistaken
// for a mailbox event. Frames land in a bounded ring the client drains;
// voice is real-time, so when the ring is full the OLDEST frame drops —
// late audio is worse than lost audio.
static CALL_ROUTES: OnceLock<Mutex<std::collections::HashSet<RouteId>>> = OnceLock::new();
/// blob -> id for routes THIS node allocated, so one can be released
/// mid-call (a RENEW retires its predecessor deliberately in tests).
static CALL_MINE: OnceLock<Mutex<std::collections::HashMap<Vec<u8>, RouteId>>> = OnceLock::new();
static CALL_RX: OnceLock<Mutex<VecDeque<Vec<u8>>>> = OnceLock::new();
static CALL_TARGETS: OnceLock<Mutex<std::collections::HashMap<Vec<u8>, RouteId>>> =
    OnceLock::new();
const CALL_RING_CAP: usize = 256;

fn call_routes() -> &'static Mutex<std::collections::HashSet<RouteId>> {
    CALL_ROUTES.get_or_init(|| Mutex::new(std::collections::HashSet::new()))
}
fn call_rx() -> &'static Mutex<VecDeque<Vec<u8>>> {
    CALL_RX.get_or_init(|| Mutex::new(VecDeque::new()))
}
fn call_targets() -> &'static Mutex<std::collections::HashMap<Vec<u8>, RouteId>> {
    CALL_TARGETS.get_or_init(|| Mutex::new(std::collections::HashMap::new()))
}
fn call_mine() -> &'static Mutex<std::collections::HashMap<Vec<u8>, RouteId>> {
    CALL_MINE.get_or_init(|| Mutex::new(std::collections::HashMap::new()))
}

/// Cloned out, then called: the feeder is stigmerge's handler chain, and
/// it is fed from the update callback — which the node runs on its own
/// thread with no catch_unwind over it. Holding the slot's lock while a
/// handler ran meant a handler that reached back for the node (a route
/// rebuild does) deadlocked against `ensure_conn`, and one that panicked
/// took the feeder's lock down with it.
fn feed_swarm(update: VeilidUpdate) {
    let f = crate::lock(swarm_feeder()).clone();
    if let Some(f) = f {
        f(update);
    }
}

/// The running API and its runtime handle, for the swarm module.
pub(crate) fn swarm_handles() -> Option<(VeilidAPI, tokio::runtime::Handle)> {
    let guard = crate::lock(slot());
    guard.as_ref().map(|n| (n.api.clone(), n.runtime.handle().clone()))
}

fn inbox() -> &'static Mutex<VecDeque<(u64, Vec<u8>)>> {
    INBOX.get_or_init(|| Mutex::new(VecDeque::new()))
}

/// Veilid's own words, waiting for the app log to drain them.
///
/// The node's tracing has nowhere to go on Android — no terminal, no
/// subscriber — which made "attaching forever with zero peers" a black box on
/// the second emulated phone. With `logging.api` on, veilid narrates through
/// the update callback; this ring keeps the tail and `node_logs` hands it to
/// whoever polls (the Kotlin poller writes it into ducat.log). Bounded for the
/// same reason the inbox is.
const MAX_LOGS: usize = 256;

static LOGS: OnceLock<Mutex<VecDeque<String>>> = OnceLock::new();

fn logs() -> &'static Mutex<VecDeque<String>> {
    LOGS.get_or_init(|| Mutex::new(VecDeque::new()))
}

/// Drain the node's buffered log lines, oldest first.
#[uniffi::export]
pub fn node_logs() -> Vec<String> {
    crate::lock(logs()).drain(..).collect()
}

/// A line of our own into the same ring — the swarm's fetch loop lives and
/// dies entirely between two FFI calls, and on a phone that death is
/// invisible without this.
pub(crate) fn note(line: String) {
    let mut q = crate::lock(logs());
    if q.len() >= MAX_LOGS {
        q.pop_front();
    }
    q.push_back(line);
}

/// Take a clone of what a call needs, and **release the lock before doing any
/// network work**.
///
/// Every function here used to hold the global node mutex across its
/// `block_on`. Building a private route takes seconds, and `node_status` is
/// polled from a Compose recomposition — so a status poll during a route build
/// blocked the main thread until the route finished, which Android reports as
/// the app not responding. It reads to a user as a crash while building a card,
/// and there is nothing in the log to say a lock was the reason.
fn handles() -> Result<(VeilidAPI, tokio::runtime::Handle), NodeError> {
    let guard = crate::lock(slot());
    let node = guard.as_ref().ok_or(NodeError::NotRunning)?;
    Ok((node.api.clone(), node.runtime.handle().clone()))
}

/// What the UI can show and a person can troubleshoot from.
#[derive(uniffi::Record, Clone, Default)]
pub struct NodeStatus {
    pub running: bool,
    /// Attached to the network at all.
    pub attached: bool,
    /// **The one that matters for routes.** A node that has not determined its
    /// network class cannot allocate a private route, and every DUCAT reach mode
    /// depends on one. Attachment alone is not enough and looks identical from
    /// a status line that only reports "connected".
    pub public_internet_ready: bool,
    pub peers: u32,
    pub reliable_peers: u32,
    pub state: String,
    /// Present when startup failed, because a node that silently did not start
    /// is indistinguishable from a network with no peers.
    pub error: Option<String>,
}

#[derive(uniffi::Error, Debug, thiserror::Error)]
pub enum NodeError {
    #[error("{0}")]
    Failed(String),
    /// Distinct from `Failed` because the UI's response differs: this one means
    /// "wait for the node", not "something went wrong".
    #[error("the Veilid node is not running")]
    NotRunning,
}

/// Start the node, storing its state under `storage_dir`.
///
/// Returns as soon as startup is under way rather than when the network is
/// usable: readiness takes seconds to minutes and a UI that blocks on it is a UI
/// that appears frozen. Poll [`node_status`].
#[uniffi::export]
pub fn node_start(storage_dir: String, udp: bool) -> Result<(), NodeError> {
    // "Already running" and "already starting" both mean don't: starting
    // twice would fight over the store. The flag, not the slot's lock, is
    // what holds the second caller off — startup takes seconds (the keyring,
    // the table store, attach), and the slot's lock used to be held across
    // all of it, so a `node_status` poll from a recomposition stood behind
    // it for that long, which Android reports as the app not responding.
    let _busy = {
        let guard = crate::lock(slot());
        if guard.is_some() {
            return Ok(());
        }
        match Busy::take() {
            Some(b) => b,
            None => return Ok(()),
        }
    };

    // The swarm's own narration, into the same ring the node's goes to.
    // stigmerge speaks through `tracing`, and on a phone that had no
    // subscriber — a resolver refusing a watch, a route import failing,
    // every reason a fetch dies, all said clearly and heard by no one.
    static TRACE: std::sync::Once = std::sync::Once::new();
    TRACE.call_once(|| {
        use tracing_subscriber::{fmt, layer::SubscriberExt, EnvFilter};
        struct Ring;
        impl std::io::Write for Ring {
            fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                let line = String::from_utf8_lossy(buf);
                let line = line.trim();
                if !line.is_empty() {
                    note(line.to_string());
                }
                Ok(buf.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let filter = EnvFilter::try_from_env("DUCAT_TRACE").unwrap_or_else(|_| {
            EnvFilter::new("off,stigmerge_peer=debug,stigmerge_fileindex=info")
        });
        let layer = fmt::layer()
            .with_writer(|| Ring)
            .with_ansi(false)
            .without_time()
            .with_target(false);
        let _ = tracing::subscriber::set_global_default(
            tracing_subscriber::registry().with(filter).with(layer),
        );
    });

    // Four, because discovery is nine boards at once.
    //
    // Two workers was sized for a node that mostly waits. Reading a ring of
    // boards asks the runtime for seventy-two concurrent lookups, and with two
    // workers those queue behind each other — nine boards took nine boards'
    // worth of time however many threads called in. This is still small enough
    // to be polite on a phone.
    //
    // Not the search's bottleneck, though it looks like it. A ring of boards
    // comes back in about forty-eight seconds where one empty board costs a
    // flat twenty-one, which reads as an effective width of four and invites
    // exactly the fix you are thinking of. It was tried: sixteen workers,
    // measured against the live network on the same boards, gave 63s where
    // four gave 66s — noise. Whatever bounds concurrent `get_dht_value` calls
    // is inside veilid-core, not here, and more threads only park.
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()
        .map_err(|e| NodeError::Failed(format!("runtime: {e}")))?;

    let api = runtime.block_on(async {
        let mut cfg: serde_json::Value = serde_json::from_str(&default_veilid_config())
            .map_err(|e| format!("config: {e}"))?;
        cfg["program_name"] = serde_json::json!("ducat");
        cfg["namespace"] = serde_json::json!("ducat");
        for store in ["protected_store", "table_store", "block_store"] {
            cfg[store]["directory"] = serde_json::json!(format!("{storage_dir}/{store}"));
        }
        // `protected_store.allow_insecure_fallback` is deliberately left at
        // its default of false.
        //
        // Veilid's protected store holds the device encryption key for the
        // table store. On Android it opens through the OS keyring, and when
        // that will not open there is no second path: startup fails with
        // "Could not initialize the protected store" and the node never
        // starts.
        //
        // The one cause established here (2026-08-21, then found on 08-22):
        // an APK built for the wrong CPU. The emulator is x86_64 and the
        // arm64-v8a build had been installed on it by hand; the keyring goes
        // through JNI into `androidx.security.crypto`, and under binary
        // translation that call fails. Installing the x86_64 build fixed it
        // in one attempt, having survived restarts, reinstalls and a full
        // reboot before that. A keystore entry written under the translated
        // build is then unusable by the native one, so the ABI switch also
        // wants app data cleared.
        //
        // Earlier notes here read the same failure as intermittent and
        // blamed repeated reinstalls. That was wrong: the two "it cleared
        // itself" episodes were installs that happened to land the right
        // ABI. Nothing has shown this failing on a correctly-built install.
        // DUCAT is sideloaded, though, so somebody picking a file off a
        // releases page can reach it the same way — which is what the status
        // screen now suggests checking.
        //
        // The fallback was *not* shown to help. It was switched on during
        // that window and the node did come up, but no `protected_store`
        // directory was ever written — so the right-ABI install had simply
        // started working and the fallback path never ran. Whether it
        // rescues this failure is untested.
        //
        // It stays false regardless. Moving that key from the keyring to a
        // file is a decision about what this app promises, not a way to get a
        // test phone back. What the failure gets instead is somewhere to be
        // read: the start error is logged (see DucatApplication), the poller
        // retries and says so, and the status screen names the reason.
        // An emulator's SLIRP user-networking silently eats sustained UDP —
        // reads worked, every DHT set died on the way out, and the app
        // believed its own local copy (observed 2026-08-15: an hour of
        // "posted" hails the network never held). TCP and WSS survive SLIRP;
        // a real phone keeps UDP.
        if !udp {
            cfg["network"]["protocol"]["udp"]["enabled"] = serde_json::json!(false);
        }
        // Diagnosis knob (2026-08-31, load-shedding hunt): the consensus bar
        // a DHT set must clear before veilid stops re-fanning it out every
        // second from the offline-subkey-write queue. Env-gated, harness use
        // only; unset means veilid's default.
        if let Some(n) = std::env::var("DUCAT_SET_VALUE_COUNT")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
        {
            cfg["network"]["dht"]["set_value_count"] = serde_json::json!(n);
        }
        // The pipe a board wave flows through (2026-09-01). A ring read is
        // 9 boards x 8 shard subkeys = 72 get operations, and veilid gates
        // outbound DHT operations behind a 16-permit semaphore
        // (storage_manager operation_concurrency) - so the wave drains in
        // ~4.5 batches of the flat 10-second empty-board timeout, which is
        // the measured 48 seconds that read as "an effective width of four"
        // (see the worker-thread note above; the workers were never the
        // bottleneck). 72 permits lets the whole wave fly at once; an idle
        // permit costs nothing. The fanout under each operation (5 nodes,
        // quorum 3) is unchanged - this widens how many questions we ask
        // together, not how hard each question hits the network.
        let dht_ops = std::env::var("DUCAT_DHT_OPS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(72);
        cfg["network"]["dht"]["max_concurrent_operations"] = serde_json::json!(dht_ops);
        // Probe knob for the residual gate (bench use): veilid's RPC worker
        // count, 0 = automatic. Unset means leave the default.
        if let Some(n) = std::env::var("DUCAT_RPC_CONCURRENCY")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
        {
            cfg["network"]["rpc"]["concurrency"] = serde_json::json!(n);
        }
        // A wallet is a client, not a backbone (2026-08-31, the load-shedding
        // find): with inbound reachability veilid volunteers this device as a
        // relay, DHT host, route hop, signaler and dial-info validator for
        // strangers — measured on an idle desk as ~200 messages/second of
        // other people's keepalives, every one crypto-verified, ~half a core
        // for ever. Shedding the server roles keeps everything the app itself
        // does (APPM stays: our mailbox answers calls; watches, reads and
        // writes are outbound and unaffected). A deliberately run
        // infrastructure node re-enables with DUCAT_FULL_NODE=1.
        // The env var serves a desk; a phone has no shell environment, so
        // the same choice rides a marker file in the node's storage dir
        // (`adb shell run-as … touch files/veilid/full_node` to flip one).
        let full_node = std::env::var("DUCAT_FULL_NODE").ok().as_deref() == Some("1")
            || std::path::Path::new(&storage_dir).join("full_node").exists();
        if !full_node {
            cfg["capabilities"]["disable"] =
                serde_json::json!(["ROUT", "TUNL", "RLAY", "DHTV"]);
        }
        // NOTE (2026-08-16): veilid-core 0.5.7's config has no "logging"
        // section — api-level logging is wired through a tracing layer, not
        // the JSON config — so the `node_logs` ring below stays empty until
        // that layer is installed. The ring and its export are kept: the
        // two-phone debugging session proved a silent transport is the worst
        // kind of black box on a phone.

        // `AppCall` is now consumed: it is how a contact's message reaches us
        // (§16.11). Everything else is still dropped, and deliberately — a
        // callback that pretended to handle updates no protocol reads would be
        // a place for a half-implemented flow to hide.
        let cb: UpdateCallback = std::sync::Arc::new(|update| {
            match update {
                VeilidUpdate::AppCall(call) => {
                    // The swarm's block requests arrive on routes its
                    // announcer registered; those calls are the seeder's
                    // EXCLUSIVELY — a call has one reply slot, and two
                    // answerers means whoever loses answered nothing.
                    let to_swarm = call
                        .route_id()
                        .map(|r| crate::lock(swarm_routes()).contains(r))
                        .unwrap_or(false);
                    if to_swarm {
                        feed_swarm(VeilidUpdate::AppCall(call));
                        return;
                    }
                    let mut q = crate::lock(inbox());
                    if q.len() >= MAX_PENDING {
                        q.pop_front();
                    }
                    q.push_back((call.id().as_u64(), call.message().to_vec()));
                }
                // A watched record moved (§16.12). The event carries which
                // record and what value, and we deliberately keep none of it:
                // the poller's read path is the one place values enter the
                // app, and an event that merely *wakes* it cannot introduce a
                // second, subtly different way for a message to arrive.
                VeilidUpdate::ValueChange(vc) => {
                    // The swarm's watches see it too — this arm consumes the
                    // update for the mailbox's doorbell, and a consumed
                    // update the swarm never saw would be a watch that never
                    // fires over there.
                    feed_swarm(VeilidUpdate::ValueChange(vc.clone()));
                    // Which record, not merely that something moved. A driver
                    // watching eighteen boards used to be told only "one of
                    // them changed" and had to read all eighteen to find out
                    // which — a lap, for a fare that arrived on one board.
                    let key = vc.key.to_string();
                    // An empty subkey range or a zero count is veilid saying
                    // the watch itself has died, not that a value changed.
                    // Forget it, so the next arming pass puts it back and
                    // reads resume closing the record.
                    if vc.count == 0 || vc.subkeys.is_empty() {
                        crate::lock(watched()).remove(&key);
                    } else {
                        let mut q = crate::lock(changed_keys());
                        if q.len() >= MAX_CHANGED {
                            q.pop_front();
                        }
                        q.push_back(key);
                    }
                    let (flag, cond) = change_signal();
                    *crate::lock(flag) = true;
                    cond.notify_all();
                }
                VeilidUpdate::Log(l) => {
                    let mut q = crate::lock(logs());
                    if q.len() >= MAX_LOGS {
                        q.pop_front();
                    }
                    q.push_back(format!("{} {}", l.log_level, l.message));
                }
                // Everything that is not an AppCall also goes to the swarm's
                // feeder: its connection needs attachment state to know the
                // network is up, route changes to rebuild dead routes, and
                // value changes for the records it watches. The feeder is a
                // handler chain that ignores what it has no handler for.
                VeilidUpdate::AppMessage(msg) => {
                    let to_call = msg
                        .route_id()
                        .map(|r| crate::lock(call_routes()).contains(r))
                        .unwrap_or(false);
                    if to_call {
                        let mut ring = crate::lock(call_rx());
                        if ring.len() >= CALL_RING_CAP {
                            ring.pop_front();
                        }
                        ring.push_back(msg.message().to_vec());
                    } else {
                        feed_swarm(VeilidUpdate::AppMessage(msg));
                    }
                }
                other => feed_swarm(other),
            }
        });
        let api = api_startup_json(cb, cfg.to_string())
            .await
            .map_err(|e| format!("startup: {e}"))?;
        api.attach().await.map_err(|e| format!("attach: {e}"))?;
        Ok::<VeilidAPI, String>(api)
    })
    .map_err(NodeError::Failed)?;

    *crate::lock(slot()) = Some(Node { api, runtime });
    Ok(())
}

/// A snapshot. Cheap, and safe to call from a recomposition.
#[uniffi::export]
pub fn node_status() -> NodeStatus {
    let Ok((api, rt)) = handles() else {
        return NodeStatus::default();
    };
    let node = Handles { api, rt };
    match node.rt.block_on(node.api.get_state()) {
        Ok(s) => NodeStatus {
            running: true,
            // veilid's own test, not "anything but Detached". Attaching is
            // not attached — its own doc says "not yet able to perform
            // network operations" — and it is the state a node sits in when
            // it cannot reach the network at all. Reported as attached, the
            // status screen put a tick beside the one line whose whole job
            // is to say whether the transport is up, on a phone showing
            // "0 live, 0 reliable" two rows below it. is_attached() excludes
            // Detached, Attaching and Detaching, and cannot drift from the
            // enum the way a hand-written match can.
            attached: s.attachment.state.is_attached(),
            public_internet_ready: s.attachment.public_internet_ready,
            peers: u64::from(s.attachment.live_peer_count) as u32,
            reliable_peers: u64::from(s.attachment.reliable_peer_count) as u32,
            state: format!("{:?}", s.attachment.state),
            error: None,
        },
        Err(e) => NodeStatus {
            running: true,
            state: "unknown".into(),
            error: Some(e.to_string()),
            ..Default::default()
        },
    }
}

/// Allocate a private route and return its blob size.
///
/// The blob is what §15.3 puts inside a tap, so its size is the tap's size — and
/// a route that will not allocate is the difference between a client that can
/// transact and one that can only receive.
#[uniffi::export]
pub fn node_test_route() -> Result<u32, NodeError> {
    let (api, rt) = handles()?;
    rt.block_on(async {
        let r = api
            .new_custom_private_route(PrivateSpec::default())
            .await
            .map_err(|e| NodeError::Failed(e.to_string()))?;
        let len = r.blob.len() as u32;
        let _ = api.release_private_route(r.route_id);
        Ok(len)
    })
}

/// Stop the node and forget everything that was only true of it.
///
/// Every map in this module holds handles into the node that just went
/// away: routes it allocated, routes imported through it, watches it armed,
/// the swarm's feeder installed for its connection, and the call sender's
/// "up" flag — for a task that ran on its runtime and died with it. A
/// restart (the service coming back after Android reclaimed it) used to
/// find the flag still set and never spawn a sender: the next call
/// connected and carried no audio, until the process was killed.
#[uniffi::export]
pub fn node_stop() {
    // Busy for the shutdown too, or a start arriving mid-way finds the slot
    // empty and opens the store the old node is still closing.
    let node = crate::lock(slot()).take();
    if let Some(node) = node {
        let busy = loop {
            if let Some(b) = Busy::take() {
                break b;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        };
        let Node { api, runtime } = node;
        runtime.block_on(api.shutdown());
        drop(runtime);
        drop(busy);
    }
    crate::callcodec::reset();
    crate::lock(call_routes()).clear();
    crate::lock(call_mine()).clear();
    crate::lock(call_targets()).clear();
    crate::lock(call_rx()).clear();
    crate::lock(call_queue()).clear();
    call_sender_up().store(false, std::sync::atomic::Ordering::SeqCst);
    crate::lock(watched()).clear();
    crate::lock(inbox()).clear();
    *crate::lock(swarm_feeder()) = None;
    crate::lock(swarm_routes()).clear();
    crate::swarm::node_stopped();
}

// ---------------------------------------------------------------------------
// Android: handing Veilid the JavaVM and Context
// ---------------------------------------------------------------------------

/// Register the app's JNI environment with veilid-core.
///
/// **Must run before any node starts.** Without it startup fails with
/// `Internal: Android globals are not set up` — which names the cause exactly
/// and still reads, from a Kotlin stack, like the library is broken rather than
/// uninitialised.
///
/// Not a UniFFI export: an Android `Context` is a Java object rather than a
/// value, so it cannot cross that boundary. This is the one hand-written JNI
/// function in the crate, and its name encodes the Kotlin class that calls it —
/// rename `VeilidInit` and this stops being found, at runtime, with
/// `UnsatisfiedLinkError`.
#[cfg(target_os = "android")]
#[no_mangle]
#[allow(non_snake_case)]
pub extern "system" fn Java_org_ducatproject_ducat_VeilidInit_setupAndroid(
    env: jni::EnvUnowned,
    _class: jni::objects::JClass,
    ctx: jni::objects::JObject,
) {
    veilid_core::veilid_core_setup_android(env, ctx);
    ANDROID_READY.store(true, std::sync::atomic::Ordering::SeqCst);
}

/// Tracked here rather than asked of veilid-core: it exports the setup function
/// but not its `is_android_ready` companion, and a flag set at the one call site
/// is more honest than inferring readiness from a later failure.
static ANDROID_READY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Whether the JNI handshake has happened.
///
/// Exposed so the UI can distinguish "not set up" from "no peers yet". They look
/// the same in a status line and have nothing to do with each other.
#[uniffi::export]
pub fn android_ready() -> bool {
    #[cfg(target_os = "android")]
    {
        ANDROID_READY.load(std::sync::atomic::Ordering::SeqCst)
    }
    #[cfg(not(target_os = "android"))]
    {
        true
    }
}


/// A private route this node can be reached on.
///
/// **Not for messaging.** Contact cards carry a DHT record key now (§16.12),
/// because a route dies with the process that made it and a card must not.
/// What remains correct for is the **tap** (§15.3), where both parties are
/// standing together and a live round trip is the right shape — a payment is a
/// conversation with a person in front of you, not a letter.
///
/// Each call builds a *new* route. That is the expensive, correct default: a
/// route reused across cards links every holder of those cards to one another,
/// which is the linkability §16.6 accounts for and does not want handed out for
/// free.
#[uniffi::export]
pub fn node_route_blob() -> Result<Vec<u8>, NodeError> {
    let (api, rt) = handles()?;
    rt.block_on(async {
        // Allocation genuinely fails sometimes — veilid returns `TryAgain:
        // allocated route failed to test`, which is a transient result of the
        // hops it picked rather than a permanent condition. Retrying is what
        // the error name asks for, and one failure should not be a dead end in
        // front of a user who just pressed a button.
        let mut last = String::new();
        for attempt in 1..=4 {
            match api.new_custom_private_route(PrivateSpec::default()).await {
                Ok(r) => return Ok(r.blob),
                Err(e) => {
                    last = format!("{e}");
                    if attempt < 4 {
                        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                    }
                }
            }
        }
        Err(NodeError::Failed(format!("route: {last} (4 attempts)")))
    })
}

/// One request/response exchange with a peer reached by its route blob.
///
/// For the tap (§15.3). Messaging goes through records — see the note on
/// [`node_route_blob`] for why using this as a mailbox was the mistake §16.12
/// documents.
///
/// Blocking, with a caller-supplied timeout. Kotlin must call this off the main
/// thread; a route round trip is tens to hundreds of milliseconds on a good day
/// and §8.7.2 measured far worse.
#[uniffi::export]
pub fn node_app_call(
    route_blob: Vec<u8>,
    message: Vec<u8>,
    timeout_ms: u32,
) -> Result<Vec<u8>, NodeError> {
    let (api, rt) = handles()?;
    rt.block_on(async {
        let route = api
            .import_remote_private_route(route_blob)
            .map_err(|e| NodeError::Failed(format!("import route: {e}")))?;
        let rc = api
            .routing_context()
            .map_err(|e| NodeError::Failed(format!("routing context: {e}")))?;
        let fut = rc.app_call(Target::RouteId(route), message);
        match tokio::time::timeout(std::time::Duration::from_millis(timeout_ms as u64), fut).await {
            Ok(Ok(reply)) => Ok(reply),
            Ok(Err(e)) => Err(NodeError::Failed(format!("app_call: {e}"))),
            Err(_) => Err(NodeError::Failed("timed out".into())),
        }
    })
}

/// An inbound call the UI has not answered yet.
#[derive(uniffi::Record, Clone)]
pub struct InboundCall {
    pub id: u64,
    pub message: Vec<u8>,
}

/// Take the next inbound call, if any. Non-blocking, safe on any thread.
#[uniffi::export]
pub fn node_poll_call() -> Option<InboundCall> {
    crate::lock(inbox())
        .pop_front()
        .map(|(id, message)| InboundCall { id, message })
}

/// Answer one. Veilid allows a single reply per call, so a second attempt is an
/// error rather than an overwrite.
#[uniffi::export]
pub fn node_reply(id: u64, message: Vec<u8>) -> Result<(), NodeError> {
    let (api, rt) = handles()?;
    rt.block_on(async {
        api.app_call_reply(OperationId::new(id), message)
            .await
            .map_err(|e| NodeError::Failed(format!("reply: {e}")))
    })
}

/// Allocate this end's door for one live call (§16.21): a fresh private
/// route whose inbound app messages land in the call ring, not the
/// mailbox. Returns the blob the offer or answer carries.
#[uniffi::export]
pub fn node_call_route() -> Result<Vec<u8>, NodeError> {
    let (api, rt) = handles()?;
    rt.block_on(async {
        // A node that only just attached refuses allocation with TryAgain
        // ("allocated route failed to test") — the same young-node reflex
        // the swarm meets. A ring is worth forty patient seconds.
        let mut waited = 0u32;
        loop {
            match api.new_private_route().await {
                Ok(rb) => {
                    crate::lock(call_routes()).insert(rb.route_id.clone());
                    crate::lock(call_mine()).insert(rb.blob.clone(), rb.route_id);
                    return Ok(rb.blob);
                }
                Err(e) if format!("{e}").contains("TryAgain") && waited < 40 => {
                    waited += 2;
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                }
                Err(e) => return Err(NodeError::Failed(format!("call route: {e}"))),
            }
        }
    })
}

/// One media frame out the far door — without blocking the microphone and
/// without flooding the route. The frame goes on a short queue served by a
/// single sender task: one app-message in flight at a time, which a phone's
/// relayed, NAT-shadowed connection can actually sustain (32 concurrent
/// sends thrashed route resolution — "could not get remote private route" —
/// and delivered 2%). On a slow route the queue keeps the freshest
/// [CALL_QUEUE_MAX] frames and the receiver's concealment bridges the gaps;
/// a blocked capture thread was the original sin (it capped a phone at
/// ~14 fps and got blamed on the microphone).
#[uniffi::export]
pub fn node_call_send(route_blob: Vec<u8>, frame: Vec<u8>) -> Result<(), NodeError> {
    let (api, rt) = handles()?;
    {
        let mut q = crate::lock(call_queue());
        while q.len() >= CALL_QUEUE_MAX {
            q.pop_front(); // freshest wins; voice never waits for the past
        }
        q.push_back((route_blob, frame));
    }
    if !call_sender_up().swap(true, std::sync::atomic::Ordering::SeqCst) {
        rt.spawn(async move {
            // Ticks with nothing to send. Two seconds of them and the call
            // is over: the task hands the flag back and goes, instead of
            // waking two hundred times a second for the life of the
            // process. The next frame spawns a fresh one.
            const IDLE_TICKS: u32 = 400;
            let mut idle = 0u32;
            loop {
                let next = crate::lock(call_queue()).pop_front();
                let Some((blob, frame)) = next else {
                    idle += 1;
                    if idle >= IDLE_TICKS {
                        call_sender_up().store(false, std::sync::atomic::Ordering::SeqCst);
                        // A frame queued between the pop and the store has
                        // no task yet unless its sender spawned one — in
                        // which case the flag is taken again and we go.
                        if crate::lock(call_queue()).is_empty()
                            || call_sender_up().swap(true, std::sync::atomic::Ordering::SeqCst)
                        {
                            return;
                        }
                        idle = 0;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
                    continue;
                };
                idle = 0;
                let route = {
                    let cached = crate::lock(call_targets()).get(&blob).cloned();
                    match cached {
                        Some(r) => r,
                        None => match api.import_remote_private_route(blob.clone()) {
                            Ok(r) => {
                                crate::lock(call_targets()).insert(blob.clone(), r.clone());
                                r
                            }
                            Err(e) => {
                                call_send_errs()
                                    .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                                *crate::lock(call_send_last()) = format!("import: {e}");
                                continue;
                            }
                        },
                    }
                };
                let Ok(rc) = api.routing_context() else { continue };
                match rc.app_message(Target::RouteId(route), frame).await {
                    Ok(()) => {
                        call_send_oks().fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    }
                    Err(e) => {
                        call_send_errs().fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        let msg = format!("{e}");
                        // A route that stopped resolving may have rotated
                        // under us: forget it so the next frame re-imports.
                        if msg.contains("private route") {
                            crate::lock(call_targets()).remove(&blob);
                        }
                        *crate::lock(call_send_last()) = msg;
                    }
                }
            }
        });
    }
    Ok(())
}

fn call_send_oks() -> &'static std::sync::atomic::AtomicI32 {
    static N: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);
    &N
}

fn call_send_errs() -> &'static std::sync::atomic::AtomicI32 {
    static N: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);
    &N
}

fn call_send_last() -> &'static Mutex<String> {
    static S: OnceLock<Mutex<String>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(String::new()))
}

/// What became of the fire-and-forget frames: "confirmed/failed last-error".
/// Confirmation is veilid's send completing, not the far end hearing it.
#[uniffi::export]
pub fn node_call_send_report() -> String {
    format!(
        "{}/{} {}",
        call_send_oks().load(std::sync::atomic::Ordering::SeqCst),
        call_send_errs().load(std::sync::atomic::Ordering::SeqCst),
        crate::lock(call_send_last())
    )
}

/// Release ONE of our own call doors by its blob — what a RENEW's test
/// harness does to its predecessor, proving the far side really moved.
#[uniffi::export]
pub fn node_call_release(route_blob: Vec<u8>) {
    let id = crate::lock(call_mine()).remove(&route_blob);
    if let Some(id) = id {
        crate::lock(call_routes()).remove(&id);
        if let Ok((api, _rt)) = handles() {
            let _ = api.release_private_route(id);
        }
    }
}

const CALL_QUEUE_MAX: usize = 8;

fn call_queue() -> &'static Mutex<VecDeque<(Vec<u8>, Vec<u8>)>> {
    static Q: OnceLock<Mutex<VecDeque<(Vec<u8>, Vec<u8>)>>> = OnceLock::new();
    Q.get_or_init(|| Mutex::new(VecDeque::new()))
}

fn call_sender_up() -> &'static std::sync::atomic::AtomicBool {
    static B: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    &B
}

/// The next inbound frame, or None after `timeout_ms` of silence. Simple
/// short-poll under the hood — a 20 ms cadence needs nothing cleverer.
#[uniffi::export]
pub fn node_call_recv(timeout_ms: u32) -> Option<Vec<u8>> {
    let deadline =
        std::time::Instant::now() + std::time::Duration::from_millis(timeout_ms as u64);
    loop {
        if let Some(f) = crate::lock(call_rx()).pop_front() {
            return Some(f);
        }
        if std::time::Instant::now() >= deadline {
            return None;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
}

/// Hang up: release every call route this node allocated, drop the ring
/// and the import cache. A call's routes never outlive the call.
#[uniffi::export]
pub fn node_call_close() {
    crate::callcodec::reset();
    let routes: Vec<RouteId> = crate::lock(call_routes()).drain().collect();
    if let Ok((api, rt)) = handles() {
        rt.block_on(async {
            for r in routes {
                let _ = api.release_private_route(r);
            }
        });
    }
    crate::lock(call_rx()).clear();
    crate::lock(call_targets()).clear();
    crate::lock(call_queue()).clear();
    crate::lock(call_mine()).clear();
}

// ---------------------------------------------------------------------------
// DHT records (§16.12)
// ---------------------------------------------------------------------------
//
// `app_call` is a live RPC and we had been using it as a mailbox. Every failure
// in the first messaging build traced to that: a private route dies with the
// process, so a card went stale the moment the app restarted, and both parties
// had to be online at the same instant for anything to move.
//
// A DHT record key is **permanent**. The writer publishes into their own record
// whenever they are online; the reader collects whenever *they* are. Neither
// needs the other present, which is also what makes a payment request that
// waits until someone reads it expressible at all.

/// A record this node owns, and the credentials to write it.
#[derive(uniffi::Record, Clone)]
pub struct DhtRecord {
    /// The permanent address. This is what goes in a contact card, in place of
    /// a route blob that outlives nothing.
    pub key: String,
    pub owner_public: Vec<u8>,
    pub owner_secret: Vec<u8>,
    pub subkey_count: u32,
}

/// Create a record only we can write.
///
/// `subkey_count` bounds the log: subkey 0 is a head, the rest carry messages
/// as a ring. Veilid caps a record's subkeys, so a conversation is a bounded
/// buffer rather than an archive — which matches §16.11 anyway, where a message
/// is meant to become unreadable rather than accumulate.
#[uniffi::export]
pub fn node_dht_create(subkey_count: u32) -> Result<DhtRecord, NodeError> {
    let (api, rt) = handles()?;
    rt.block_on(async {
        let rc = api
            .routing_context()
            .map_err(|e| NodeError::Failed(format!("routing context: {e}")))?;
        let schema = DHTSchema::dflt(subkey_count as u16)
            .map_err(|e| NodeError::Failed(format!("schema: {e}")))?;
        let desc = rc
            .create_dht_record(CRYPTO_KIND_VLD0, schema, None)
            .await
            .map_err(|e| NodeError::Failed(format!("create: {e}")))?;
        let owner = desc
            .owner_secret()
            .ok_or_else(|| NodeError::Failed("record has no owner secret".into()))?;
        Ok(DhtRecord {
            key: desc.key().to_string(),
            owner_public: desc.owner().value().bytes().to_vec(),
            owner_secret: owner.value().bytes().to_vec(),
            subkey_count,
        })
    })
}

/// Create a record we own and **one other party may also write**.
///
/// This is the contact-request inbox: subkey 0 is ours, subkey 1 is theirs. The
/// card carries the key and the writer secret, so whoever holds the card — and
/// only they — can answer in place, without either side needing a live route.
#[uniffi::export]
pub fn node_dht_create_shared(
    writer_public: Vec<u8>,
) -> Result<DhtRecord, NodeError> {
    let (api, rt) = handles()?;
    rt.block_on(async {
        let rc = api
            .routing_context()
            .map_err(|e| NodeError::Failed(format!("routing context: {e}")))?;
        // A member id is the same bytes as the signing key; the type differs
        // because the schema names writers rather than verifies signatures.
        let member = BareMemberId::new(&writer_public);
        let schema = DHTSchema::smpl(1, vec![DHTSchemaSMPLMember { m_key: member, m_cnt: 1 }])
            .map_err(|e| NodeError::Failed(format!("schema: {e}")))?;
        let desc = rc
            .create_dht_record(CRYPTO_KIND_VLD0, schema, None)
            .await
            .map_err(|e| NodeError::Failed(format!("create: {e}")))?;
        let owner = desc
            .owner_secret()
            .ok_or_else(|| NodeError::Failed("record has no owner secret".into()))?;
        Ok(DhtRecord {
            key: desc.key().to_string(),
            owner_public: desc.owner().value().bytes().to_vec(),
            owner_secret: owner.value().bytes().to_vec(),
            subkey_count: 2,
        })
    })
}

/// Open a record, optionally as a writer.
///
/// **A record must be open before `set` or `get` will work, and creating one
/// leaves it open only for the life of this process.** After a restart the app
/// must re-open every record it intends to use — its own outbox included.
/// Forgetting this produces a failure that looks like the network (a set that
/// goes nowhere) and is bookkeeping, which is the same shape of bug that cost a
/// night on the `app_call` build.
///
/// Opening an already-open record is harmless, so callers should re-open rather
/// than track whether they have.
#[uniffi::export]
pub fn node_dht_open(
    key: String,
    writer_public: Option<Vec<u8>>,
    writer_secret: Option<Vec<u8>>,
) -> Result<u32, NodeError> {
    let (api, rt) = handles()?;
    rt.block_on(async {
        let rc = api
            .routing_context()
            .map_err(|e| NodeError::Failed(format!("routing context: {e}")))?;
        let rk = parse_key(&key)?;
        let writer = match (writer_public, writer_secret) {
            (Some(p), Some(s)) => Some(KeyPair::new(
                CRYPTO_KIND_VLD0,
                BareKeyPair::new(BarePublicKey::new(&p), BareSecretKey::new(&s)),
            )),
            _ => None,
        };
        let desc = rc
            .open_dht_record(rk, writer)
            .await
            .map_err(|e| NodeError::Failed(format!("open: {e}")))?;
        Ok(desc.schema().max_subkey())
    })
}

/// Write one subkey. The record must be open (see [`node_dht_open`]), and this
/// node must be the owner or a named writer for that subkey.
///
/// Returning `Ok` means the network holds these bytes. It did not always: a set
/// answers `Ok(None)` when the value was stored and `Ok(Some(theirs))` when it
/// was refused for being older than what the network already has, and the
/// `Some` used to be dropped on the floor here. Every caller reads `Ok` as
/// delivered, so a refused write travelled all the way up as a sent message.
///
/// Refusal is not an edge case, because the sequence number a write is signed
/// with comes from `handle_get_single_local_value` — *local* state, never the
/// network. A phone with no local copy of a record signs seq 0. Restore a
/// backup and that is every record it owns: the keys come back from the file,
/// veilid's table store does not, and so every message, every hail, every board
/// post is signed seq 0 against a network holding seq N, refused, and reported
/// as delivered. Found on exactly that path (2026-08-22) — two phones restored,
/// both attached, "delivered seq 4 to Sam" logged in 900 ms, and nothing on
/// Sam's screen, twice.
///
/// The retry works because a refusal is not inert: veilid stores the network's
/// value locally on its way out (`process_outbound_set_value_result_locked`),
/// which is the priming the first attempt was missing. So the second signs from
/// their seq and lands. One retry is the whole ladder — a third would mean
/// somebody else is writing this subkey in the same breath, and for a log slot
/// only its owner writes, that is a real conflict and not ours to paper over.
#[uniffi::export]
pub fn node_dht_set(key: String, subkey: u32, data: Vec<u8>) -> Result<(), NodeError> {
    let (api, rt) = handles()?;
    rt.block_on(async {
        let rc = api
            .routing_context()
            .map_err(|e| NodeError::Failed(format!("routing context: {e}")))?;
        let rk = parse_key(&key)?;
        let refused = rc
            .set_dht_value(rk.clone(), subkey, data.clone(), None)
            .await
            .map_err(|e| NodeError::Failed(format!("set: {e}")))?;
        if refused.is_none() {
            return Ok(());
        }
        // Their value is in our local store now. Sign against it and go again.
        if rc
            .set_dht_value(rk, subkey, data, None)
            .await
            .map_err(|e| NodeError::Failed(format!("set (retry): {e}")))?
            .is_none()
        {
            return Ok(());
        }
        Err(NodeError::Failed(format!(
            "set: subkey {subkey} refused twice — the network holds a newer value"
        )))
    })
}

/// Read a subkey. `force_refresh` goes to the network rather than the local
/// copy, which is what a poll for new messages needs and what a re-read of your
/// own writes does not.
/// Ask the network to tell us when a record changes (§16.12).
///
/// The record must already be open in this process. Watches are best-effort
/// and expire — the network promises a wake-up, not delivery — which is why
/// the poller keeps its sweep: the watch buys latency, the sweep keeps the
/// correctness it always had.
#[uniffi::export]
pub fn node_dht_watch(key: String) -> Result<bool, NodeError> {
    let (api, rt) = handles()?;
    rt.block_on(async {
        let rc = api
            .routing_context()
            .map_err(|e| NodeError::Failed(format!("routing context: {e}")))?;
        let rk = parse_key(&key)?;
        rc.watch_dht_values(rk, None, None, None)
            .await
            .map_err(|e| NodeError::Failed(format!("watch: {e}")))
    })
}

/// Block until any watched record changes, or the timeout passes.
///
/// Returns true on a change. The flag is level- rather than edge-triggered on
/// purpose: a change that lands between the poller's sweep and its next wait
/// is caught by the flag still being up, not lost in the gap.
#[uniffi::export]
pub fn node_wait_change(timeout_ms: u32) -> bool {
    let (flag, cond) = change_signal();
    let guard = crate::lock(flag);
    // Poison-tolerant like every other lock here: the flag is a bool, and a
    // poisoned one is still a bool.
    let (mut guard, _timeout) = cond
        .wait_timeout_while(
            guard,
            std::time::Duration::from_millis(timeout_ms as u64),
            |changed| !*changed,
        )
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let fired = *guard;
    *guard = false;
    fired
}

/// Forget a record this device is done with (§18.7's stewardship).
///
/// Local, and honestly so: the network's copies expire by their own TTL and
/// nothing a client says can hasten that. What this does is stop *us* being a
/// long-lived origin for a record whose purpose is spent — an answered
/// handshake inbox, a fetched attachment — and free the local storage. A good
/// tenant cleans its own unit; the building handles the rest.
#[uniffi::export]
pub fn node_dht_delete(key: String) -> Result<(), NodeError> {
    let (api, rt) = handles()?;
    rt.block_on(async {
        let rc = api
            .routing_context()
            .map_err(|e| NodeError::Failed(format!("routing context: {e}")))?;
        let rk = parse_key(&key)?;
        rc.delete_dht_record(rk)
            .await
            .map_err(|e| NodeError::Failed(format!("delete: {e}")))
    })
}

#[uniffi::export]
pub fn node_dht_get(
    key: String,
    subkey: u32,
    force_refresh: bool,
) -> Result<Option<Vec<u8>>, NodeError> {
    let (api, rt) = handles()?;
    rt.block_on(async {
        let rc = api
            .routing_context()
            .map_err(|e| NodeError::Failed(format!("routing context: {e}")))?;
        let v = rc
            .get_dht_value(parse_key(&key)?, subkey, force_refresh)
            .await
            .map_err(|e| NodeError::Failed(format!("get: {e}")))?;
        Ok(v.map(|d| d.data().to_vec()))
    })
}

/// A subkey's bytes together with the sequence they were written at.
///
/// A DHT subkey is a *mutable slot*. `SMPL(1, [writer])` bounds how many
/// subkeys a member may write, not how many times, and [node_dht_set]
/// deliberately retries against the network's sequence so a later write wins.
/// The sequence is therefore the only thing that tells "written once" from
/// "written over" — and on a card's reply subkey that is the difference
/// between the person who answered and somebody who read the same public board
/// and answered after them.
#[derive(uniffi::Record)]
pub struct DhtRead {
    pub data: Vec<u8>,
    /// `Some(0)` for a slot written exactly once, `Some(n)` after n+1 writes,
    /// and `None` for one never written at all.
    pub seq: Option<u32>,
}

/// [node_dht_get], with the sequence kept.
#[uniffi::export]
pub fn node_dht_get_versioned(
    key: String,
    subkey: u32,
    force_refresh: bool,
) -> Result<Option<DhtRead>, NodeError> {
    let (api, rt) = handles()?;
    rt.block_on(async {
        let rc = api
            .routing_context()
            .map_err(|e| NodeError::Failed(format!("routing context: {e}")))?;
        let v = rc
            .get_dht_value(parse_key(&key)?, subkey, force_refresh)
            .await
            .map_err(|e| NodeError::Failed(format!("get: {e}")))?;
        Ok(v.map(|d| DhtRead { data: d.data().to_vec(), seq: d.seq().to_option() }))
    })
}

#[uniffi::export]
pub fn node_dht_close(key: String) -> Result<(), NodeError> {
    let (api, rt) = handles()?;
    rt.block_on(async {
        let rc = api
            .routing_context()
            .map_err(|e| NodeError::Failed(format!("routing context: {e}")))?;
        rc.close_dht_record(parse_key(&key)?)
            .await
            .map_err(|e| NodeError::Failed(format!("close: {e}")))?;
        Ok(())
    })
}

fn parse_key(s: &str) -> Result<RecordKey, NodeError> {
    use std::str::FromStr;
    RecordKey::from_str(s).map_err(|e| NodeError::Failed(format!("bad record key: {e}")))
}

// ---------------------------------------------------------------------------
// Stands (§15.12): rendezvous by convention.
// ---------------------------------------------------------------------------

/// Derive the stand's owner keypair and encryption key from the cell name.
///
/// Both halves are the convention, pinned by §15.12: the opaque record key
/// derives from the owner public key, and the *values* are encrypted under a
/// key that rides the record-key handle — so a public board derives that too,
/// or readers compute the right record and cannot open its values.
/// Refuse a board name that does not name a generation (§15.12).
///
/// The epoch lives in the *name*, so that a name written down stays resolvable
/// to the record it was written to — a board key derived from the clock would
/// silently repoint every stored name at rollover. The cost of that choice is
/// that every place forming a board name has to stamp it, and a place that
/// forgets would read and write a board nobody else computes: a feature that
/// fails only in the field, only against other people, and looks like the
/// network being quiet.
///
/// So it is not allowed to be quiet. Every entry point funnels through
/// `stand_material`, and this is the one check in front of it.
fn require_generation(cell: &str) -> Result<(), NodeError> {
    if cell.contains('@') {
        return Ok(());
    }
    Err(NodeError::Failed(format!(
        "board name {cell:?} names no generation — stamp it with standEpochName first"
    )))
}

fn stand_material(cell: &str) -> (BareKeyPair, BareSharedSecret) {
    use sha2::{Digest, Sha256};
    let seed: [u8; 32] = Sha256::new()
        .chain_update(b"DUCAT-STAND-v0")
        .chain_update([0u8])
        .chain_update(cell.as_bytes())
        .finalize()
        .into();
    let sk = ed25519_dalek::SigningKey::from_bytes(&seed);
    let pk = sk.verifying_key();
    let enc: [u8; 32] = Sha256::new()
        .chain_update(b"DUCAT-STAND-v0-ENC")
        .chain_update([0u8])
        .chain_update(cell.as_bytes())
        .finalize()
        .into();
    (
        BareKeyPair::new(BarePublicKey::new(pk.as_bytes()), BareSecretKey::new(&seed)),
        BareSharedSecret::new(&enc),
    )
}

fn stand_schema() -> DHTSchema {
    DHTSchema::dflt(8).expect("static schema")
}

/// Boards this process is watching, and must therefore stop closing.
///
/// Closing a record cancels its watch, and every board read opens and closes
/// the record it reads. So a watch armed on a board would be cancelled by the
/// very next sweep over that board — the two mechanisms undoing each other,
/// silently, once a lap.
static WATCHED: OnceLock<Mutex<std::collections::HashSet<String>>> = OnceLock::new();

fn watched() -> &'static Mutex<std::collections::HashSet<String>> {
    WATCHED.get_or_init(|| Mutex::new(std::collections::HashSet::new()))
}

fn is_watched(key: &RecordKey) -> bool {
    crate::lock(watched()).contains(&key.to_string())
}

/// Ask the network to tell us when this *board* changes.
///
/// `node_dht_watch` cannot do this on its own: watching requires the record to
/// be open in this process, and a board is never open — every reader opens it,
/// reads, and closes again. Armed through that function, the watch was refused
/// with "record not open", the caller discarded the result, and nothing said
/// so. Measured on the live network (`:desktop:watchtest`): a driver watching
/// a cell, a fare posted onto it, and four minutes of silence. The sweep was
/// finding every fare, which is why a hail took a lap to appear instead of
/// seconds.
///
/// So this opens the board the way a reader does — creating it first if nobody
/// has pinned that corner yet, since a watch on a record that does not exist
/// is refused too — arms the watch, and deliberately leaves the record open.
#[uniffi::export]
pub fn stand_watch(cell: String) -> Result<bool, NodeError> {
    // Before the node, deliberately: this is a complaint about the argument,
    // and a stopped node must not be able to answer in its place.
    require_generation(&cell)?;
    let (api, rt) = handles()?;
    let (kp, enc) = stand_material(&cell);
    rt.block_on(async {
        let rc = api
            .routing_context()
            .map_err(|e| NodeError::Failed(format!("routing context: {e}")))?;
        let key = api
            .get_dht_record_key(
                stand_schema(),
                PublicKey::new(CRYPTO_KIND_VLD0, kp.key()),
                Some(SharedSecret::new(CRYPTO_KIND_VLD0, enc)),
            )
            .await
            .map_err(|e| NodeError::Failed(format!("record key: {e}")))?;
        // Same convention as a read: first one to the corner pins the board.
        if rc.open_dht_record(key.clone(), None).await.is_err() {
            if let Ok(desc) = rc
                .create_dht_record(
                    CRYPTO_KIND_VLD0,
                    stand_schema(),
                    Some(KeyPair::new(CRYPTO_KIND_VLD0, kp.clone())),
                )
                .await
            {
                let _ = rc.close_dht_record(desc.key().clone()).await;
            }
            rc.open_dht_record(key.clone(), None)
                .await
                .map_err(|e| NodeError::Failed(format!("open: {e}")))?;
        }
        let armed = rc
            .watch_dht_values(key.clone(), None, None, None)
            .await
            .map_err(|e| NodeError::Failed(format!("watch: {e}")))?;
        if armed {
            crate::lock(watched()).insert(key.to_string());
        }
        Ok(armed)
    })
}

/// The board's record key, computed locally. Costs no network round trip.
#[uniffi::export]
pub fn stand_record_key(cell: String) -> Result<String, NodeError> {
    // Before the node, deliberately: this is a complaint about the argument,
    // and a stopped node must not be able to answer in its place.
    require_generation(&cell)?;
    let (api, rt) = handles()?;
    let (kp, enc) = stand_material(&cell);
    rt.block_on(async {
        let key = api
            .get_dht_record_key(
                stand_schema(),
                PublicKey::new(CRYPTO_KIND_VLD0, kp.key()),
                Some(SharedSecret::new(CRYPTO_KIND_VLD0, enc)),
            )
            .await
            .map_err(|e| NodeError::Failed(format!("record key: {e}")))?;
        Ok(key.to_string())
    })
}

/// Post a notice onto the board at `subkey`. Creates the board if this is the
/// first pin; opens under the conventional key so what is written decrypts
/// for anyone who can derive the cell.
#[uniffi::export]
pub fn stand_post(cell: String, subkey: u32, data: Vec<u8>) -> Result<(), NodeError> {
    // Before the node, deliberately: this is a complaint about the argument,
    // and a stopped node must not be able to answer in its place.
    require_generation(&cell)?;
    let (api, rt) = handles()?;
    let (kp, enc) = stand_material(&cell);
    rt.block_on(async {
        let rc = api
            .routing_context()
            .map_err(|e| NodeError::Failed(format!("routing context: {e}")))?;
        let owner = KeyPair::new(CRYPTO_KIND_VLD0, kp.clone());
        let key = api
            .get_dht_record_key(
                stand_schema(),
                PublicKey::new(CRYPTO_KIND_VLD0, kp.key()),
                Some(SharedSecret::new(CRYPTO_KIND_VLD0, enc)),
            )
            .await
            .map_err(|e| NodeError::Failed(format!("record key: {e}")))?;
        // The create-time random encryption key is deliberately unused
        // (§15.12): descriptor published, handle discarded, reopened under
        // the conventional key.
        if let Ok(desc) = rc
            .create_dht_record(CRYPTO_KIND_VLD0, stand_schema(), Some(owner.clone()))
            .await
        {
            let _ = rc.close_dht_record(desc.key().clone()).await;
        }
        rc.open_dht_record(key.clone(), Some(owner))
            .await
            .map_err(|e| NodeError::Failed(format!("open: {e}")))?;
        // Prime the local value_seq with the slot's current tenant first: a
        // write from a store that never saw the slot goes out at seq 0 and
        // the network silently keeps whatever is already there (§16.12's
        // read-before-write, learned the hard way on the mailbox ring).
        // Best-effort: retry a failed prime once, then write anyway — a slot
        // nobody has written yet has nothing to fetch.
        if rc.get_dht_value(key.clone(), subkey, true).await.is_err() {
            let _ = rc.get_dht_value(key.clone(), subkey, true).await;
        }
        // The set itself reports a lost race: Some(newer) back means the
        // network already holds a value newer than ours and kept it.
        let refused = rc
            .set_dht_value(key.clone(), subkey, data.clone(), None)
            .await
            .map_err(|e| NodeError::Failed(format!("post: {e}")))?;
        if refused.is_some() {
            // Closing cancels any watch on it; a board this process is
            // watching stays open (see stand_watch).
            if !is_watched(&key) {
                let _ = rc.close_dht_record(key).await;
            }
            return Err(NodeError::Failed("slot taken by a concurrent writer".into()));
        }
        // Verify against the *local* record store, not the network: seconds
        // after a set, a force-refreshed read races propagation and loses —
        // which read as "every shard is full" on a nearly-empty cell, the
        // first bug the emulated phone ever caught. The local copy reflects
        // exactly what the set call accepted.
        let echoed = rc
            .get_dht_value(key.clone(), subkey, false)
            .await
            .map_err(|e| NodeError::Failed(format!("verify: {e}")))?;
        let held = echoed.as_ref().map(|v| v.data()).unwrap_or(&[]);
        if held != data.as_slice() {
            // Closing cancels any watch on it; a board this process is
            // watching stays open (see stand_watch).
            if !is_watched(&key) {
                let _ = rc.close_dht_record(key).await;
            }
            return Err(NodeError::Failed(
                "slot taken by a concurrent writer".into(),
            ));
        }
        // Closing cancels any watch on it; a board this process is
        // watching stays open (see stand_watch).
        if !is_watched(&key) {
            let _ = rc.close_dht_record(key).await;
        }
        Ok(())
    })
}

/// Everything currently pinned to the board: (subkey, bytes) pairs, freshly
/// fetched. Empty values are cleared slots and are skipped.
#[uniffi::export]
pub fn stand_read(cell: String) -> Result<Vec<StandNotice>, NodeError> {
    // Before the node, deliberately: this is a complaint about the argument,
    // and a stopped node must not be able to answer in its place.
    require_generation(&cell)?;
    let (api, rt) = handles()?;
    let (kp, enc) = stand_material(&cell);
    rt.block_on(async {
        let rc = api
            .routing_context()
            .map_err(|e| NodeError::Failed(format!("routing context: {e}")))?;
        let key = api
            .get_dht_record_key(
                stand_schema(),
                PublicKey::new(CRYPTO_KIND_VLD0, kp.key()),
                Some(SharedSecret::new(CRYPTO_KIND_VLD0, enc)),
            )
            .await
            .map_err(|e| NodeError::Failed(format!("record key: {e}")))?;
        // First one to the corner pins the board: a reader arriving before
        // any writer would otherwise get KeyNotFound, and the convention
        // means the reader holds the keypair to fix that itself.
        if rc.open_dht_record(key.clone(), None).await.is_err() {
            if let Ok(desc) = rc
                .create_dht_record(
                    CRYPTO_KIND_VLD0,
                    stand_schema(),
                    Some(KeyPair::new(CRYPTO_KIND_VLD0, kp.clone())),
                )
                .await
            {
                let _ = rc.close_dht_record(desc.key().clone()).await;
            }
            rc.open_dht_record(key.clone(), None)
                .await
                .map_err(|e| NodeError::Failed(format!("open: {e}")))?;
        }
        // All eight slots at once.
        //
        // A force-refreshed get is a network round trip, and proving a slot is
        // *empty* is the slow kind: the node has to hear back from the peers
        // that would hold it rather than stopping at the first copy. Asked one
        // after another, a board that nobody has posted to cost about fifty
        // seconds to come back empty — and a search reads nine boards, which
        // is where "looking for a car near you" turned into seven minutes of
        // spinner. The eight slots have nothing to do with each other, so the
        // wait is one round trip's worth, not eight.
        let mut tasks = Vec::with_capacity(8);
        for subkey in 0..8u32 {
            let rc = rc.clone();
            let key = key.clone();
            tasks.push(tokio::spawn(async move {
                (subkey, rc.get_dht_value(key, subkey, true).await)
            }));
        }
        let mut out = Vec::new();
        for t in tasks {
            if let Ok((subkey, Ok(Some(v)))) = t.await {
                if !v.data().is_empty() {
                    out.push(StandNotice { subkey, data: v.data().to_vec() });
                }
            }
        }
        // Slot order is the board's order, and a caller that fills the lowest
        // free slot first depends on it; finishing order is the network's.
        out.sort_by_key(|n| n.subkey);
        // Closing cancels any watch on it; a board this process is
        // watching stays open (see stand_watch).
        if !is_watched(&key) {
            let _ = rc.close_dht_record(key).await;
        }
        Ok(out)
    })
}

/// One pinned notice, as raw bytes the caller decodes (§16.17).
#[derive(uniffi::Record)]
pub struct StandNotice {
    pub subkey: u32,
    pub data: Vec<u8>,
}

#[cfg(test)]
mod stand_tests {
    use super::require_generation;

    /// The guard that makes a forgotten stamp loud.
    ///
    /// The epoch lives in the board *name*, so every site that forms one has
    /// to stamp it. A site that forgot would derive a different record key
    /// from everyone else's and quietly read and write a board of its own —
    /// working perfectly against itself, failing only against other people,
    /// and looking exactly like a network with nobody on it. That is the
    /// worst shape a bug can have here, so an unstamped name is refused
    /// rather than derived from.
    #[test]
    fn a_board_name_must_name_a_generation() {
        assert!(require_generation("geo:u4pruy@3021").is_ok());
        assert!(require_generation("geo:u4pruy@3021-7").is_ok());
        assert!(require_generation("local:u4pru@0").is_ok());

        for bare in ["geo:u4pruy", "geo:u4pruy-7", "local:u4pru", ""] {
            let e = require_generation(bare).unwrap_err();
            let msg = format!("{e}");
            assert!(
                msg.contains("generation"),
                "an unstamped name was accepted or refused unhelpfully: {msg}",
            );
            // The complaint has to say what to do about it — this is read by
            // whoever is looking at a board that mysteriously has nobody on it.
            assert!(msg.contains("standEpochName"), "the refusal names no remedy: {msg}");
        }
    }
}
