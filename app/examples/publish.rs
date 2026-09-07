//! The publishing half of `ducat-app` against the live network — the same
//! calls the desk's commands make, without the window.
//!
//!   DUCAT_DESK_STATE=<dir> cargo run -p ducat-app --example publish -- share <file>
//!   DUCAT_DESK_STATE=<dir> cargo run -p ducat-app --example publish -- site <folder> <title>
//!   DUCAT_DESK_STATE=<dir> cargo run -p ducat-app --example publish -- get <ducat:file/... | ducat:site/...>
//!   DUCAT_DESK_STATE=<dir> cargo run -p ducat-app --example publish -- serve
//!
//! `share` and `site` print the address and then keep serving until killed,
//! so a second identity can `get` it. Markers: PUB_ADDR, PUB_OK, PUB_FAIL.

use std::time::{Duration, Instant};

use ducat_app::{releases, sites, App};

fn ready(app: &App) {
    app.start_node().expect("PUB_FAIL node start");
    let t0 = Instant::now();
    while t0.elapsed() < Duration::from_secs(240) {
        let s = app.node_status();
        if s.public_internet_ready {
            eprintln!("node ready — {} peers", s.peers);
            return;
        }
        std::thread::sleep(Duration::from_secs(2));
    }
    panic!("PUB_FAIL node never became ready");
}

fn veilid_view() {
    for cmd in ["nodeinfo", "route list"] {
        match ducat_mobile::node::node_debug(cmd.into()) {
            Ok(out) => {
                for line in out.lines() {
                    println!("PUB_DEBUG {cmd}: {line}");
                }
            }
            Err(e) => println!("PUB_DEBUG {cmd} failed: {e}"),
        }
    }
}

fn serve_forever() -> ! {
    veilid_view();
    // The swarm's own notes while serving, so a seeder that lost its
    // route says so — and the routing table's health every five minutes,
    // because a host node's table decays where a phone's does not, and
    // the figures over an hour are what tell the two apart.
    let mut ticks = 0u64;
    loop {
        std::thread::sleep(Duration::from_secs(2));
        for line in ducat_mobile::node::node_logs() {
            println!("PUB_NOTE {line}");
        }
        ticks += 1;
        if ticks % 150 == 0 {
            let h = ducat_mobile::node::node_routing_health();
            let s = ducat_mobile::node::node_status();
            println!("PUB_HEALTH t+{}m state={} peers={} total={} live={} dead={} find={}ms", ticks / 30, s.state, s.peers, h.total, h.live, h.dead, h.find_node_ms);
            // The node's own traffic and message counters, so a bare node's
            // baseline can be told from what a client adds on top of it.
            if let Ok(stats) = ducat_mobile::node::node_debug("network stats".into()) {
                for line in stats.lines() {
                    let l = line.trim();
                    if l.starts_with("Down:") || l.starts_with("Up:") || l.starts_with("Cache hits:") {
                        println!("PUB_NET t+{}m {l}", ticks / 30);
                    }
                }
            }
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let app = App::open_default().expect("PUB_FAIL open");
    eprintln!("state: {}", app.root().display());
    ready(&app);

    match args.first().map(String::as_str) {
        Some("share") => {
            let path = args.get(1).expect("PUB_FAIL share <file>");
            let r = app.share_file(std::path::Path::new(path), "").expect("PUB_FAIL share");
            println!("PUB_ADDR {}", releases::uri_of(&r.share_key, &r.digest_hex));
            println!("PUB_OK shared {} bytes as '{}'", r.bytes, r.title);
            serve_forever()
        }
        Some("site") => {
            let dir = args.get(1).expect("PUB_FAIL site <folder> <title>");
            let title = args.get(2).cloned().unwrap_or_else(|| "Untitled".into());
            let s = app
                .publish_site(std::path::Path::new(dir), &title, None, None)
                .expect("PUB_FAIL publish");
            println!("PUB_ADDR {}", sites::uri_of(&s.record_key));
            println!("PUB_OK published '{}' share={} digest={}", s.title, s.share, s.digest_hex);
            veilid_view();
            serve_forever()
        }
        Some("get") => {
            let uri = args.get(1).expect("PUB_FAIL get <address>");
            let t0 = Instant::now();
            veilid_view();
            // The swarm's own notes, so a stall says where it stalled.
            std::thread::spawn(|| loop {
                std::thread::sleep(Duration::from_secs(2));
                for line in ducat_mobile::node::node_logs() {
                    println!("PUB_NOTE {line}");
                }
            });
            if releases::parse(uri).is_some() {
                let r = app.add_release(uri, "").expect("PUB_FAIL add");
                let dir = app.fetch_release(&r.digest_hex).expect("PUB_FAIL fetch");
                let bytes = app.releases().into_iter().find(|x| x.digest_hex == r.digest_hex).map(|x| x.bytes).unwrap_or(0);
                println!("PUB_OK release {} bytes in {:.1}s at {}", bytes, t0.elapsed().as_secs_f64(), dir.display());
            } else if let Some(key) = sites::parse_uri(uri) {
                let s = app.add_site(&key).expect("PUB_FAIL add site (head unreadable)");
                println!("head: '{}' digest={} updated={}", s.title, s.digest_hex, s.updated);
                let dir = app.fetch_site_bundle(&key).expect("PUB_FAIL fetch bundle");
                let index = dir.join("index.html");
                println!(
                    "PUB_OK site '{}' in {:.1}s, index.html {} bytes at {}",
                    s.title,
                    t0.elapsed().as_secs_f64(),
                    std::fs::metadata(&index).map(|m| m.len()).unwrap_or(0),
                    dir.display()
                );
            } else {
                panic!("PUB_FAIL not an address: {uri}");
            }
        }
        Some("serve") => {
            app.reseed_all_sites();
            app.reseed_all_releases();
            println!("PUB_OK serving {} site(s), {} release(s)", app.sites().len(), app.releases().len());
            serve_forever()
        }
        _ => panic!("PUB_FAIL usage: share <file> | site <folder> <title> | get <address> | serve"),
    }
}
