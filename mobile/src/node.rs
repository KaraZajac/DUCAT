//! A Veilid node inside the app.
//!
//! The node runs on its own runtime on a background thread and outlives any
//! screen: a route takes seconds to build and a payment cannot wait for it, so
//! starting one when the user opens a tap screen would put the transport's
//! latency directly into §15.3's three-second budget.
//!
//! Everything the UI sees is a snapshot it polls. Nothing here blocks a caller
//! on the network, because the caller is the main thread.

use std::sync::{Mutex, OnceLock};

use veilid_core::*;

struct Node {
    api: VeilidAPI,
    runtime: tokio::runtime::Runtime,
}

static NODE: OnceLock<Mutex<Option<Node>>> = OnceLock::new();

fn slot() -> &'static Mutex<Option<Node>> {
    NODE.get_or_init(|| Mutex::new(None))
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
}

/// Start the node, storing its state under `storage_dir`.
///
/// Returns as soon as startup is under way rather than when the network is
/// usable: readiness takes seconds to minutes and a UI that blocks on it is a UI
/// that appears frozen. Poll [`node_status`].
#[uniffi::export]
pub fn node_start(storage_dir: String) -> Result<(), NodeError> {
    let mut guard = slot().lock().unwrap();
    if guard.is_some() {
        return Ok(()); // already running; starting twice would fight over the store
    }

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
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

        // Updates are dropped for now. Nothing in the app consumes them yet, and
        // a callback that pretends to handle AppCall while no protocol runs
        // would be a place for a half-implemented tap to hide.
        let cb: UpdateCallback = std::sync::Arc::new(|_| {});
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
    let guard = slot().lock().unwrap();
    let Some(node) = guard.as_ref() else {
        return NodeStatus::default();
    };
    match node.runtime.block_on(node.api.get_state()) {
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
