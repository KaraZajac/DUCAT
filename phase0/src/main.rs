//! DUCAT Phase 0 — empirical measurements that gate Part II and §8.7.
//!
//! 0a  Veilid private-route blob size vs. media budgets (§15.3.2, O11)
//! 0b  Veilid throughput at block-sync volumes (§8.7.1, O14)
//!
//! 0c (status of Veilid issue #395) is answered out-of-band; see REPORT.md.

use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use veilid_core::*;

// ---- Media budgets, from §15.3.2 -------------------------------------------
const NTAG213: usize = 144;
const NTAG215: usize = 504;
const NTAG216: usize = 888;
const QR_L: usize = 2953;
const QR_M: usize = 2331;
const QR_Q: usize = 1663;
const QR_H: usize = 1273;
const HCE_COMFORT: usize = 1024; // ~1 KB per ~300 ms contact window

/// Fixed portion of a TapPresent, per §15.3.1 (everything but `route`).
const TAPPRESENT_FIXED: usize = 158;

fn budget_row(label: &str, tap_size: usize) -> String {
    let media: [(&str, usize); 8] = [
        ("NTAG213", NTAG213),
        ("NTAG215", NTAG215),
        ("NTAG216", NTAG216),
        ("QR-H", QR_H),
        ("QR-Q", QR_Q),
        ("QR-M", QR_M),
        ("QR-L", QR_L),
        ("HCE~1KB", HCE_COMFORT),
    ];
    let marks: Vec<String> = media
        .iter()
        .map(|(name, cap)| {
            format!("{} {}", name, if tap_size <= *cap { "PASS" } else { "fail" })
        })
        .collect();
    format!("  {:<28} TapPresent={:>5} B   {}", label, tap_size, marks.join(" | "))
}

/// Route allocation requires a determined PublicInternet network class, which
/// arrives well after the first "attached" signal. Poll `public_internet_ready`.
async fn wait_ready(api: &VeilidAPI, timeout: Duration) -> Result<bool, VeilidAPIError> {
    let start = Instant::now();
    let mut last = String::new();
    while start.elapsed() < timeout {
        let a = api.get_state().await?.attachment;
        let s = format!(
            "{:?} pub_ready={} peers={}/{} est_net={}",
            a.state, a.public_internet_ready, a.reliable_peer_count, a.live_peer_count,
            a.estimated_network_size
        );
        if s != last {
            println!("    [{:>3}s] {}", start.elapsed().as_secs(), s);
            last = s;
        }
        if a.public_internet_ready {
            return Ok(true);
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
    Ok(false)
}

/// 0a — allocate routes across hop counts and measure the encoded blob.
async fn phase_0a(api: &VeilidAPI) {
    println!("\n== 0a  Private-route blob size vs. media budgets ==\n");
    println!(
        "  TapPresent fixed portion = {} B (§15.3.1); token mode = {} B\n",
        TAPPRESENT_FIXED,
        TAPPRESENT_FIXED + 32
    );
    println!("{}", budget_row("token mode (32 B pointer)", TAPPRESENT_FIXED + 32));
    println!();

    let mut any = false;
    for hops in [0usize, 1, 2, 3, 4] {
        let spec = PrivateSpec {
            hop_count: hops,
            ..Default::default()
        };
        match api.new_custom_private_route(spec).await {
            Ok(rb) => {
                any = true;
                let blob = rb.blob.len();
                let tap = TAPPRESENT_FIXED + blob;
                let label = if hops == 0 {
                    "inline, default hops".to_string()
                } else {
                    format!("inline, {} hop(s)", hops)
                };
                println!("{}", budget_row(&label, tap));
                println!("      route blob alone = {} B", blob);
                let _ = api.release_private_route(rb.route_id);
            }
            Err(e) => {
                println!("  inline, {} hop(s): ALLOCATION FAILED — {}", hops, e);
            }
        }
    }

    if !any {
        println!("\n  No routes allocated. Node likely has too few peers to build a route.");
    }
}

/// 0b — round-trip throughput over a private route, at sync-like payload sizes.
async fn phase_0b(api: &VeilidAPI, mut calls: mpsc::UnboundedReceiver<(OperationId, Vec<u8>)>) {
    println!("\n== 0b  Throughput over a private route (block-sync volumes) ==\n");

    let rb = match api.new_private_route().await {
        Ok(rb) => rb,
        Err(e) => {
            println!("  Could not allocate a route to measure against: {}", e);
            return;
        }
    };
    let target = match api.import_remote_private_route(rb.blob.clone()) {
        Ok(t) => t,
        Err(e) => {
            println!("  Could not import own route: {}", e);
            return;
        }
    };
    let rc = match api.routing_context() {
        Ok(rc) => rc,
        Err(e) => {
            println!("  No routing context: {}", e);
            return;
        }
    };

    // Answer our own app_calls so the round trip completes.
    let api2 = api.clone();
    tokio::spawn(async move {
        while let Some((op_id, _msg)) = calls.recv().await {
            let _ = api2.app_call_reply(op_id, b"ok".to_vec()).await;
        }
    });

    println!("  {:<12} {:>10} {:>12} {:>14}", "payload", "rtt", "throughput", "1 GB would take");
    for size in [1024usize, 4096, 16384, 32768] {
        let payload = vec![0u8; size];
        let t0 = Instant::now();
        let res = tokio::time::timeout(
            Duration::from_secs(30),
            rc.app_call(Target::RouteId(target.clone()), payload),
        )
        .await;
        match res {
            Ok(Ok(_)) => {
                let dt = t0.elapsed();
                let bps = size as f64 / dt.as_secs_f64();
                let gb_secs = 1_073_741_824.0 / bps;
                println!(
                    "  {:<12} {:>8.0}ms {:>10.1} KB/s {:>12.1} h",
                    format!("{} B", size),
                    dt.as_millis(),
                    bps / 1024.0,
                    gb_secs / 3600.0
                );
            }
            Ok(Err(e)) => println!("  {:<12} error: {}", format!("{} B", size), e),
            Err(_) => println!("  {:<12} TIMEOUT (>30s)", format!("{} B", size)),
        }
    }

    let _ = api.release_private_route(rb.route_id);
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("DUCAT Phase 0 harness — veilid-core {}", veilid_version_string());

    let dir = std::env::temp_dir().join(format!("ducat-phase0-{}", std::process::id()));
    std::fs::create_dir_all(&dir)?;
    println!("state dir: {}", dir.display());

    // Start from the shipped default config and redirect storage to a temp dir.
    let mut cfg: serde_json::Value = serde_json::from_str(&default_veilid_config())?;
    cfg["program_name"] = serde_json::json!("ducat-phase0");
    cfg["namespace"] = serde_json::json!("phase0");
    for store in ["protected_store", "table_store", "block_store"] {
        cfg[store]["directory"] = serde_json::json!(dir.join(store).to_string_lossy());
    }

    let (tx, rx) = mpsc::unbounded_channel();
    let update_cb: UpdateCallback = Arc::new(move |u: VeilidUpdate| {
        if let VeilidUpdate::AppCall(ac) = u {
            let _ = tx.send((ac.id(), ac.message().to_vec()));
        }
    });

    println!("\nstarting node...");
    let api = api_startup_json(update_cb, cfg.to_string()).await?;
    api.attach().await?;

    let budget: u64 = std::env::var("DUCAT_WAIT_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(300);
    println!("waiting for PublicInternet readiness (up to {}s)...", budget);
    let ready = wait_ready(&api, Duration::from_secs(budget)).await?;
    let a = api.get_state().await?.attachment;
    if !ready {
        println!("\n!! public_internet_ready never became true.");
        println!("   final: {:?} peers={}/{} est_net={}",
            a.state, a.reliable_peer_count, a.live_peer_count, a.estimated_network_size);
        println!("   Route allocation is impossible without it, so 0a/0b cannot");
        println!("   produce numbers here. Most likely inbound UDP is unreachable,");
        println!("   so the node cannot determine its network class.\n");
        api.shutdown().await;
        return Ok(());
    }
    println!("ready. peers={}/{} est_net={}\n",
        a.reliable_peer_count, a.live_peer_count, a.estimated_network_size);

    phase_0a(&api).await;
    phase_0b(&api, rx).await;

    println!("\nshutting down.");
    api.shutdown().await;
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}
