//! The swarm's first live proof: two processes, one file, real Veilid.
//!
//!   cargo run --example swarmtest -- seed  <state-dir> <file>
//!   cargo run --example swarmtest -- fetch <state-dir> <share-key> <digest> <out-dir>
//!
//! The seeder prints `SWARMTEST_SHARE <key> <index-digest>` and the
//! payload's own BLAKE3 (`SWARMTEST_PAYLOAD <hex>`), then serves until
//! killed. The fetcher pulls the share, then hashes what landed on disk
//! and prints `SWARMTEST_OK <bytes> <secs> <payload-hex>` — the runner
//! compares the two payload hashes, which is the end-to-end fact no
//! internal check can fake: the bytes that arrived are the bytes that
//! were shared, moved by the vendored engine over routes on the SAME
//! node the mailbox machinery runs on.

use ducat_mobile::node::{node_start, node_status};
use ducat_mobile::swarm::{swarm_fetch, swarm_seed};

fn wait_ready() {
    for i in 0..120 {
        let s = node_status();
        if s.public_internet_ready {
            eprintln!("node ready after ~{}s", i * 2);
            return;
        }
        std::thread::sleep(std::time::Duration::from_secs(2));
    }
    panic!("node never became route-capable");
}

fn blake3_file(path: &std::path::Path) -> String {
    let bytes = std::fs::read(path).expect("read for hashing");
    blake3::hash(&bytes).to_hex().to_string()
}

fn main() {
    // Diagnosis runs set RUST_LOG (e.g. stigmerge_peer=debug); silence
    // otherwise, exactly as before.
    if std::env::var("RUST_LOG").is_ok() {
        let _ = tracing_subscriber::fmt()
            .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
            .with_writer(std::io::stderr)
            .try_init();
    }
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("seed") => {
            let state = args[2].clone();
            let file = std::path::PathBuf::from(&args[3]);
            node_start(state, true).expect("node start");
            wait_ready();
            eprintln!("payload hashing…");
            let payload = blake3_file(&file);
            eprintln!("indexing + announcing…");
            let share = swarm_seed(file.to_string_lossy().into()).expect("seed");
            println!("SWARMTEST_SHARE {} {}", share.share_key, share.index_digest_hex);
            println!("SWARMTEST_PAYLOAD {payload}");
            eprintln!("serving; kill me when the fetch is done");
            loop {
                std::thread::sleep(std::time::Duration::from_secs(5));
            }
        }
        // Diagnosis stances: a bare node, and a seed that stops serving —
        // for finding where the idle-serve CPU actually lives.
        Some("idle") => {
            node_start(args[2].clone(), true).expect("node start");
            wait_ready();
            println!("SWARMTEST_IDLE");
            loop {
                std::thread::sleep(std::time::Duration::from_secs(5));
            }
        }
        // Held-open record vs closed record: where does an idle node's
        // extra burn live?
        Some("recidle") | Some("recclosed") => {
            let close = args[1] == "recclosed";
            node_start(args[2].clone(), true).expect("node start");
            wait_ready();
            let rec = ducat_mobile::node::node_dht_create(32).expect("create");
            for i in 0..8u32 {
                ducat_mobile::node::node_dht_set(rec.key.clone(), i, vec![0xAB; 32_000])
                    .expect("set");
            }
            if close {
                ducat_mobile::node::node_dht_close(rec.key.clone()).expect("close");
            }
            println!("SWARMTEST_REC {} closed={close}", rec.key);
            loop {
                std::thread::sleep(std::time::Duration::from_secs(5));
            }
        }
        Some("seedstop") => {
            let state = args[2].clone();
            let file = std::path::PathBuf::from(&args[3]);
            node_start(state, true).expect("node start");
            wait_ready();
            let share = swarm_seed(file.to_string_lossy().into()).expect("seed");
            println!("SWARMTEST_SHARE {} {}", share.share_key, share.index_digest_hex);
            std::thread::sleep(std::time::Duration::from_secs(5));
            ducat_mobile::swarm::swarm_stop();
            println!("SWARMTEST_STOPPED");
            loop {
                std::thread::sleep(std::time::Duration::from_secs(5));
            }
        }
        Some("fetch") => {
            let state = args[2].clone();
            let key = args[3].clone();
            let digest = args[4].clone();
            let out = std::path::PathBuf::from(&args[5]);
            std::fs::create_dir_all(&out).expect("out dir");
            node_start(state, true).expect("node start");
            wait_ready();
            let t0 = std::time::Instant::now();
            // Not staying to seed: the proof is the fetch, and the process
            // exits on the hash.
            let bytes = swarm_fetch(key, digest, out.to_string_lossy().into(), false).expect("fetch");
            let secs = t0.elapsed().as_secs_f64();
            // One file in the out dir is the share; hash it.
            let fetched = std::fs::read_dir(&out)
                .expect("read out dir")
                .filter_map(|e| e.ok())
                .map(|e| e.path())
                .find(|p| p.is_file())
                .expect("a fetched file");
            let payload = blake3_file(&fetched);
            println!("SWARMTEST_OK {bytes} {secs:.1} {payload}");
        }
        _ => {
            eprintln!("usage: swarmtest seed <state> <file> | fetch <state> <key> <digest> <out>");
            std::process::exit(2);
        }
    }
}
