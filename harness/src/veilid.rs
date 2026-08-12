//! Node startup, shared by both roles.

use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::mpsc;
use veilid_core::*;

pub type Calls = mpsc::UnboundedReceiver<(OperationId, Vec<u8>)>;

/// Start a node with its own storage, so two roles can run side by side.
pub async fn start(role: &str) -> Result<(VeilidAPI, Calls), Box<dyn std::error::Error>> {
    let dir = std::env::temp_dir().join(format!("ducat-harness-{role}-{}", std::process::id()));
    std::fs::create_dir_all(&dir)?;

    let mut cfg: serde_json::Value = serde_json::from_str(&default_veilid_config())?;
    cfg["program_name"] = serde_json::json!("ducat-harness");
    cfg["namespace"] = serde_json::json!(role);
    for store in ["protected_store", "table_store", "block_store"] {
        cfg[store]["directory"] = serde_json::json!(dir.join(store).to_string_lossy());
    }

    let (tx, rx) = mpsc::unbounded_channel();
    let cb: UpdateCallback = Arc::new(move |u: VeilidUpdate| {
        if let VeilidUpdate::AppCall(ac) = u {
            let _ = tx.send((ac.id(), ac.message().to_vec()));
        }
    });

    println!("  [{role}] starting node (veilid-core {})", veilid_version_string());
    let api = api_startup_json(cb, cfg.to_string()).await?;
    api.attach().await?;

    // Route allocation is impossible before the node knows its network class,
    // so this wait is not optional politeness — it is the difference between a
    // route and an error nobody can act on.
    let budget = std::env::var("DUCAT_WAIT_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(300u64);
    let started = Instant::now();
    let mut ready = false;
    while started.elapsed() < Duration::from_secs(budget) {
        let s = api.get_state().await?;
        if s.attachment.public_internet_ready {
            ready = true;
            break;
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
    let a = api.get_state().await?.attachment;
    println!(
        "  [{role}] {} after {:.0}s — peers {}/{}",
        if ready { "ready" } else { "NOT READY" },
        started.elapsed().as_secs_f64(),
        a.reliable_peer_count,
        a.live_peer_count
    );
    if !ready {
        return Err("public internet never became ready; a route cannot be built".into());
    }
    Ok((api, rx))
}
