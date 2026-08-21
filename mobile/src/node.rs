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
    changed_keys()
        .lock()
        .map(|mut q| q.drain(..).collect())
        .unwrap_or_default()
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
    logs().lock().unwrap().drain(..).collect()
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
    let guard = slot().lock().unwrap();
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
    let mut guard = slot().lock().unwrap();
    if guard.is_some() {
        return Ok(()); // already running; starting twice would fight over the store
    }

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
        // Seen on the emulator on 2026-08-21 and worth writing down, because
        // it is invisible from the outside — the app simply has no network,
        // every board read comes back empty, and the search offers to try
        // again. It lasted about fifteen minutes across several process
        // restarts and two reinstalls, then cleared on its own. Transient,
        // then, not fatal, and nothing about the app provoked it.
        //
        // The fallback was *not* shown to help. It was switched on during
        // that window and the node did come up, but no `protected_store`
        // directory was ever written — so the secure keyring had simply
        // started working again and the fallback path never ran. Whether it
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
                    let mut q = inbox().lock().unwrap();
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
                        if let Ok(mut w) = watched().lock() {
                            w.remove(&key);
                        }
                    } else if let Ok(mut q) = changed_keys().lock() {
                        if q.len() >= MAX_CHANGED {
                            q.pop_front();
                        }
                        q.push_back(key);
                    }
                    let (flag, cond) = change_signal();
                    *flag.lock().unwrap() = true;
                    cond.notify_all();
                }
                VeilidUpdate::Log(l) => {
                    let mut q = logs().lock().unwrap();
                    if q.len() >= MAX_LOGS {
                        q.pop_front();
                    }
                    q.push_back(format!("{} {}", l.log_level, l.message));
                }
                _ => {}
            }
        });
        let api = api_startup_json(cb, cfg.to_string())
            .await
            .map_err(|e| format!("startup: {e}"))?;
        api.attach().await.map_err(|e| format!("attach: {e}"))?;
        Ok::<VeilidAPI, String>(api)
    })
    .map_err(NodeError::Failed)?;

    *guard = Some(Node { api, runtime });
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
            attached: !matches!(s.attachment.state, AttachmentState::Detached),
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
    let guard = slot().lock().unwrap();
    let node = guard.as_ref().ok_or_else(|| NodeError::Failed("node not started".into()))?;
    node.runtime.block_on(async {
        let r = node
            .api
            .new_custom_private_route(PrivateSpec::default())
            .await
            .map_err(|e| NodeError::Failed(e.to_string()))?;
        let len = r.blob.len() as u32;
        let _ = node.api.release_private_route(r.route_id);
        Ok(len)
    })
}

#[uniffi::export]
pub fn node_stop() {
    let mut guard = slot().lock().unwrap();
    if let Some(node) = guard.take() {
        node.runtime.block_on(node.api.shutdown());
    }
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
    inbox()
        .lock()
        .unwrap()
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
#[uniffi::export]
pub fn node_dht_set(key: String, subkey: u32, data: Vec<u8>) -> Result<(), NodeError> {
    let (api, rt) = handles()?;
    rt.block_on(async {
        let rc = api
            .routing_context()
            .map_err(|e| NodeError::Failed(format!("routing context: {e}")))?;
        rc.set_dht_value(parse_key(&key)?, subkey, data, None)
            .await
            .map_err(|e| NodeError::Failed(format!("set: {e}")))?;
        Ok(())
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
    let guard = flag.lock().unwrap();
    let (mut guard, _timeout) = cond
        .wait_timeout_while(
            guard,
            std::time::Duration::from_millis(timeout_ms as u64),
            |changed| !*changed,
        )
        .unwrap();
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
    watched()
        .lock()
        .map(|w| w.contains(&key.to_string()))
        .unwrap_or(false)
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
            if let Ok(mut w) = watched().lock() {
                w.insert(key.to_string());
            }
        }
        Ok(armed)
    })
}

/// The board's record key, computed locally. Costs no network round trip.
#[uniffi::export]
pub fn stand_record_key(cell: String) -> Result<String, NodeError> {
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
