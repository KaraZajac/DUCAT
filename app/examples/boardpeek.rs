//! Read one group board from the outside: the desk's group store names the
//! record and the key, a fresh node opens the record read-only, inspects
//! it, and opens every written page. Markers: PEEK_*.
//!
//!   DUCAT_DESK_STATE=<fresh dir> cargo run -p ducat-app --example boardpeek -- <deskstate prefs/ducat_groups.json> <group name>

use std::time::{Duration, Instant};

use ducat_app::App;
use ducat_mobile::contacts::{group_board_nameplate, group_page_decode, group_page_open, GroupPageSeal};
use ducat_mobile::node::{node_dht_board_key, node_dht_get_versioned, node_dht_inspect, node_dht_open, GroupBoardSpec};

fn hexb(s: &str) -> Vec<u8> {
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let store: serde_json::Value = serde_json::from_slice(&std::fs::read(&args[0]).expect("PEEK_FAIL read store")).unwrap();
    let g = store["groups"].as_array().unwrap().iter().find(|g| g["name"] == args[1]).expect("PEEK_FAIL no such group").clone();
    let b = &g["board"];
    let owner = b["owner"].as_str().unwrap().to_string();
    let gen = b["gen"].as_u64().unwrap();
    let pages = b["pages"].as_u64().unwrap() as u32;
    let gkey = hexb(b["gkey"].as_str().unwrap());
    let id_hex = g["id"].as_str().unwrap();
    let mut others: Vec<Vec<u8>> = g["members"].as_array().unwrap().iter().map(|m| m.as_str().unwrap()).filter(|m| *m != owner).map(hexb).collect();
    others.sort();
    let spec = GroupBoardSpec { owner_public: hexb(&owner), nameplate: group_board_nameplate(hexb(id_hex), gen), members: others.clone(), pages };
    let app = App::open_default().expect("PEEK_FAIL open");
    app.start_node().expect("PEEK_FAIL node");
    let t0 = Instant::now();
    while !app.node_status().public_internet_ready && t0.elapsed() < Duration::from_secs(240) {
        std::thread::sleep(Duration::from_secs(2));
    }
    // PEEK_SEED=1 parks every seed the desk would, then waits a minute:
    // the desk's shape, to see whether the swarm's traffic is what makes
    // its reads time out.
    if std::env::var("PEEK_SEED").ok().as_deref() == Some("1") {
        app.reseed_all_sites();
        app.reseed_all_releases();
        app.reseed_issues();
        app.reseed_library();
        app.reseed_galleries();
        app.reseed_attachments();
        println!("PEEK_SEEDING {} site(s), {} release(s) — waiting 60 s", app.sites().len(), app.releases().len());
        std::thread::sleep(Duration::from_secs(60));
    }
    if let Ok(info) = ducat_mobile::node::node_debug("nodeinfo".into()) {
        for l in info.lines().filter(|l| l.contains("total=") || l.contains("live:") || l.contains("responsive:") || l.contains("Down:") || l.contains("Up:")).take(8) {
            println!("PEEK_HEALTH {}", l.trim());
        }
    }
    let key = node_dht_board_key(spec.clone()).expect("PEEK_FAIL key");
    println!("PEEK_KEY {key} (store says {})", b["key"].as_str().unwrap_or(""));
    // PEEK_OWNER_SECRET_B64 opens the record as its owner, the way the desk
    // does; without it the record is opened read-only, like a stranger.
    match std::env::var("PEEK_OWNER_SECRET_B64") {
        Ok(b64) => {
            use base64::Engine;
            let secret = base64::engine::general_purpose::STANDARD.decode(b64.trim()).expect("PEEK_FAIL secret b64");
            node_dht_open(key.clone(), Some(spec.owner_public.clone()), Some(secret)).expect("PEEK_FAIL open as owner");
            println!("PEEK_OPEN as owner");
        }
        Err(_) => {
            node_dht_open(key.clone(), None, None).expect("PEEK_FAIL open record");
            println!("PEEK_OPEN read-only");
        }
    }
    let seqs = node_dht_inspect(key.clone()).expect("PEEK_FAIL inspect");
    println!("PEEK_SEQS {:?}", seqs.iter().map(|s| if *s == u32::MAX { -1 } else { *s as i64 }).collect::<Vec<_>>());
    for (subkey, seq) in seqs.iter().enumerate() {
        if *seq == u32::MAX {
            continue;
        }
        let t = Instant::now();
        let got = node_dht_get_versioned(key.clone(), subkey as u32, true);
        println!("PEEK_GET subkey {subkey} took {:.1}s", t.elapsed().as_secs_f64());
        match got {
            Ok(Some(read)) => {
                let who = if (subkey as u32) < pages { "owner".to_string() } else if subkey as u32 == pages { "nameplate".to_string() } else { format!("member {}", (subkey as u32 - pages - 1) / pages) };
                match group_page_open(GroupPageSeal { group_key: gkey.clone(), record_key: key.clone(), subkey: subkey as u32, bytes: read.data.clone() }) {
                    Ok(plain) => match group_page_decode(plain) {
                        Ok(p) => println!("PEEK_PAGE subkey {subkey} ({who}) seq {:?} gen {} entries {:?}", read.seq, p.generation, p.entries.iter().map(|e| (e.seq, e.kind, e.body.clone().unwrap_or_default())).collect::<Vec<_>>()),
                        Err(e) => println!("PEEK_PAGE subkey {subkey} ({who}) refused: {e}"),
                    },
                    Err(e) => println!("PEEK_PAGE subkey {subkey} ({who}) {} bytes does not open: {e}", read.data.len()),
                }
            }
            Ok(None) => println!("PEEK_PAGE subkey {subkey} empty"),
            Err(e) => println!("PEEK_PAGE subkey {subkey} get failed: {e}"),
        }
    }
    println!("PEEK_OK");
}
