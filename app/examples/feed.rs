//! §16.23 against the live network, without a window: a stranger who holds
//! nothing but a persona's key reads that persona's feed.
//!
//!   DUCAT_DESK_STATE=<dir> cargo run -p ducat-app --example feed -- read <persona_hex>
//!   DUCAT_DESK_STATE=<dir> cargo run -p ducat-app --example feed -- post "<text>"
//!
//! Markers: FEED_KEY, FEED_HEAD, FEED_POST, FEED_OK, FEED_FAIL.

use std::time::{Duration, Instant};

use ducat_app::App;

fn ready(app: &App) {
    app.start_node().expect("FEED_FAIL node start");
    let t0 = Instant::now();
    while t0.elapsed() < Duration::from_secs(240) {
        if app.node_status().public_internet_ready {
            return;
        }
        std::thread::sleep(Duration::from_secs(2));
    }
    panic!("FEED_FAIL node never became ready");
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let app = App::open_default().expect("FEED_FAIL open");
    ready(&app);
    match args.first().map(String::as_str) {
        Some("read") => {
            let hex = args.get(1).expect("FEED_FAIL read <persona_hex>");
            let key = app.home_key_of(hex).expect("FEED_FAIL key");
            println!("FEED_KEY {key}");
            let t0 = Instant::now();
            let site = match app.add_site(&key) {
                Ok(s) => s,
                Err(e) => {
                    println!("FEED_FAIL head: {e}");
                    return;
                }
            };
            println!("FEED_HEAD '{}' digest={} updated={} in {:.1}s", site.title, &site.digest_hex[..12.min(site.digest_hex.len())], site.updated, t0.elapsed().as_secs_f64());
            let dir = app.fetch_site_bundle(&key).expect("FEED_FAIL fetch");
            let text = std::fs::read_to_string(dir.join("feed.json")).expect("FEED_FAIL no feed.json in the bundle");
            let doc = ducat_mobile::feed::feed_parse(text).expect("FEED_FAIL feed unreadable");
            for p in &doc.posts {
                println!("FEED_POST {} at={} media={} files={} text={:?}", p.id, p.at, p.media.len(), p.files.len(), p.text);
            }
            let pages = doc.posts.iter().filter(|p| dir.join("posts").join(format!("{}.html", p.id)).is_file()).count();
            println!("FEED_OK {} post(s) by '{}', {} page(s) in the bundle, {:.1}s", doc.posts.len(), doc.name, pages, t0.elapsed().as_secs_f64());
        }
        Some("post") => {
            let text = args.get(1).cloned().unwrap_or_else(|| "hello from the example".into());
            let p = app.post_to_feed(&text, &[], &[]).expect("FEED_FAIL post");
            println!("FEED_POST {} at={}", p.id, p.at);
            let v = app.home_view(&app.worn().unwrap()).expect("FEED_FAIL view");
            println!("FEED_OK home {} has {} post(s), digest={}", v.record_key, v.posts, &v.digest_hex[..12.min(v.digest_hex.len())]);
        }
        _ => panic!("FEED_FAIL usage: read <persona_hex> | post <text>"),
    }
}
