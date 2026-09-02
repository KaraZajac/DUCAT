//! What a live call would feel like: RTT and jitter through real private
//! routes, measured with the app's own primitives (`node_app_call` — the
//! same import-route-and-call path every mailbox send takes).
//!
//!   cargo run --release --example pingtest -- listen <state-dir>
//!   cargo run --release --example pingtest -- ping   <state-dir> <blob-hex>
//!
//! The listener prints `PINGTEST_ROUTE <hex>` and echoes every call. The
//! pinger runs two phases and prints a machine-readable summary of each:
//!
//!   rtt    — 30 sequential 200-byte calls: the conversational floor.
//!   stream — 20 ms-paced calls for 10 s (Opus frame cadence, 160-byte
//!            payloads), each on its own thread with a 3 s deadline: what
//!            a one-way voice leg would ride, loss included.

use ducat_mobile::node::{node_app_call, node_poll_call, node_reply, node_route_blob, node_start, node_status};

fn wait_ready() {
    for i in 0..120 {
        if node_status().public_internet_ready {
            eprintln!("node ready after ~{}s", i * 2);
            return;
        }
        std::thread::sleep(std::time::Duration::from_secs(2));
    }
    panic!("node never became route-capable");
}

fn hex_of(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn un_hex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).expect("hex"))
        .collect()
}

fn stats(mut ms: Vec<f64>) -> (f64, f64, f64, f64) {
    ms.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let pick = |p: f64| ms[((ms.len() - 1) as f64 * p) as usize];
    (ms[0], pick(0.5), pick(0.9), ms[ms.len() - 1])
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("listen") => {
            node_start(args[2].clone(), true).expect("node start");
            wait_ready();
            // The same route a contact card would carry.
            let blob = loop {
                match node_route_blob() {
                    Ok(b) => break b,
                    Err(e) => {
                        eprintln!("route not ready ({e}); retrying");
                        std::thread::sleep(std::time::Duration::from_secs(3));
                    }
                }
            };
            println!("PINGTEST_ROUTE {}", hex_of(&blob));
            eprintln!("echoing; kill me when the pinger is done");
            loop {
                match node_poll_call() {
                    Some(call) => {
                        let _ = node_reply(call.id, call.message);
                    }
                    None => std::thread::sleep(std::time::Duration::from_millis(2)),
                }
            }
        }
        Some("ping") => {
            node_start(args[2].clone(), true).expect("node start");
            wait_ready();
            let blob = un_hex(&args[3]);

            // Phase 1: sequential round trips, the conversational floor.
            eprintln!("phase rtt: 30 sequential 200 B calls");
            let mut rtts = Vec::new();
            let mut first = None;
            for i in 0..30u32 {
                let t0 = std::time::Instant::now();
                match node_app_call(blob.clone(), vec![0xA5; 200], 8_000) {
                    Ok(_) => {
                        let ms = t0.elapsed().as_secs_f64() * 1e3;
                        if first.is_none() {
                            first = Some(ms);
                        } else {
                            rtts.push(ms);
                        }
                        eprintln!("  ping {i}: {ms:.0} ms");
                    }
                    Err(e) => eprintln!("  ping {i}: LOST ({e})"),
                }
            }
            if let Some(f) = first {
                println!("PINGTEST_FIRST {f:.0}");
            }
            if !rtts.is_empty() {
                let n = rtts.len();
                let (min, p50, p90, max) = stats(rtts);
                println!(
                    "PINGTEST_RTT n={n} min={min:.0} p50={p50:.0} p90={p90:.0} max={max:.0}"
                );
            }

            // Phase 2: the voice cadence. 20 ms pacing, one thread per call
            // so a slow reply never delays the next frame leaving.
            eprintln!("phase stream: 500 calls at 50 Hz, 160 B");
            let results = std::sync::Arc::new(std::sync::Mutex::new(Vec::<Option<f64>>::new()));
            let mut handles = Vec::new();
            for _ in 0..500u32 {
                let blob = blob.clone();
                let results = results.clone();
                handles.push(std::thread::spawn(move || {
                    let t0 = std::time::Instant::now();
                    let out = node_app_call(blob, vec![0x5A; 160], 3_000)
                        .ok()
                        .map(|_| t0.elapsed().as_secs_f64() * 1e3);
                    results.lock().unwrap().push(out);
                }));
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            for h in handles {
                let _ = h.join();
            }
            let results = results.lock().unwrap();
            let lost = results.iter().filter(|r| r.is_none()).count();
            let ok: Vec<f64> = results.iter().filter_map(|r| *r).collect();
            if !ok.is_empty() {
                let n = ok.len();
                let (min, p50, p90, max) = stats(ok.clone());
                // Jitter the way RFC 3550 thinks about it: mean absolute
                // delta between consecutive round trips.
                let jitter = ok.windows(2).map(|w| (w[1] - w[0]).abs()).sum::<f64>()
                    / (ok.len() - 1).max(1) as f64;
                println!(
                    "PINGTEST_STREAM n={n} lost={lost} min={min:.0} p50={p50:.0} \
                     p90={p90:.0} max={max:.0} jitter={jitter:.0}"
                );
            } else {
                println!("PINGTEST_STREAM n=0 lost={lost}");
            }
        }
        _ => {
            eprintln!("usage: pingtest listen <state> | ping <state> <blob-hex>");
            std::process::exit(2);
        }
    }
}
