//! The desk's commands: one thin call each into `ducat-app`.
//!
//! Anything that waits on the network runs on a blocking thread through
//! `spawn_blocking`, so the window keeps painting while a bundle arrives.
//! Errors cross as strings — the UI shows them, it does not branch on them.

use std::sync::OnceLock;

use ducat_app::App;
use serde::Serialize;

static APP: OnceLock<App> = OnceLock::new();

fn app() -> Result<&'static App, String> {
    APP.get().ok_or_else(|| "the app has not opened its directory yet".to_string())
}

fn s<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

#[derive(Serialize)]
struct Status {
    running: bool,
    attached: bool,
    ready: bool,
    peers: u32,
    reliable_peers: u32,
    state: String,
    error: Option<String>,
    data_dir: String,
}

#[tauri::command]
fn status() -> Result<Status, String> {
    let a = app()?;
    let n = a.node_status();
    Ok(Status {
        running: n.running,
        attached: n.attached,
        ready: n.public_internet_ready,
        peers: n.peers,
        reliable_peers: n.reliable_peers,
        state: n.state,
        error: n.error,
        data_dir: a.root().to_string_lossy().into_owned(),
    })
}

#[derive(Serialize)]
struct Progress {
    position: i64,
    length: u64,
    done: bool,
    pieces_done: u64,
    pieces_total: u64,
    /// One byte per piece, 0 or 1, for the scattered-dots bar.
    pieces: Vec<u8>,
}

#[tauri::command]
fn fetch_progress(share_key: String) -> Progress {
    let p = ducat_mobile::swarm::swarm_fetch_progress(share_key);
    Progress {
        position: p.position,
        length: p.length,
        done: p.done,
        pieces_done: p.pieces_done,
        pieces_total: p.pieces_total,
        pieces: p.pieces,
    }
}

// ----- releases: a file at an address that cannot change --------------------

#[derive(Serialize)]
struct ReleaseRow {
    share_key: String,
    digest_hex: String,
    title: String,
    added_at: u64,
    bytes: u64,
    keep_alive: bool,
    mine: bool,
    here: bool,
    uri: String,
    dir: String,
}

fn release_row(a: &App, r: ducat_app::releases::Release) -> ReleaseRow {
    ReleaseRow {
        here: a.release_is_here(&r.digest_hex),
        uri: ducat_app::releases::uri_of(&r.share_key, &r.digest_hex),
        dir: a.release_dir(&r.digest_hex).to_string_lossy().into_owned(),
        share_key: r.share_key,
        digest_hex: r.digest_hex,
        title: r.title,
        added_at: r.added_at,
        bytes: r.bytes,
        keep_alive: r.keep_alive,
        mine: r.mine,
    }
}

#[tauri::command]
fn releases() -> Result<Vec<ReleaseRow>, String> {
    let a = app()?;
    Ok(a.releases().into_iter().map(|r| release_row(a, r)).collect())
}

#[tauri::command]
async fn share_file(path: String, title: String) -> Result<ReleaseRow, String> {
    let a = app()?;
    tauri::async_runtime::spawn_blocking(move || {
        a.share_file(std::path::Path::new(&path), &title).map(|r| release_row(a, r)).map_err(s)
    })
    .await
    .map_err(s)?
}

#[tauri::command]
fn add_release(uri: String, title: String) -> Result<ReleaseRow, String> {
    let a = app()?;
    a.add_release(&uri, &title).map(|r| release_row(a, r)).map_err(s)
}

#[tauri::command]
async fn fetch_release(digest_hex: String) -> Result<String, String> {
    let a = app()?;
    tauri::async_runtime::spawn_blocking(move || {
        a.fetch_release(&digest_hex).map(|p| p.to_string_lossy().into_owned()).map_err(s)
    })
    .await
    .map_err(s)?
}

#[tauri::command]
fn set_release_keep(digest_hex: String, keep: bool) -> Result<(), String> {
    app()?.set_release_keep_alive(&digest_hex, keep).map_err(s)
}

#[tauri::command]
fn remove_release(digest_hex: String) -> Result<(), String> {
    app()?.remove_release(&digest_hex).map_err(s)
}

// ----- sites: one mutable head at a stable key ------------------------------

#[derive(Serialize)]
struct SiteRow {
    record_key: String,
    title: String,
    share: String,
    digest_hex: String,
    updated: u64,
    added_at: u64,
    keep_alive: bool,
    mine: bool,
    cached: bool,
    /// The cached copy is the edition the head names.
    current: bool,
    uri: String,
    dir: String,
}

fn site_row(a: &App, x: ducat_app::sites::Site) -> SiteRow {
    SiteRow {
        mine: x.mine(),
        cached: a.site_is_cached(&x.record_key),
        current: x.fetched_digest_hex.as_deref() == Some(x.digest_hex.as_str()),
        uri: ducat_app::sites::uri_of(&x.record_key),
        dir: a.site_bundle_dir(&x.record_key).to_string_lossy().into_owned(),
        record_key: x.record_key,
        title: x.title,
        share: x.share,
        digest_hex: x.digest_hex,
        updated: x.updated,
        added_at: x.added_at,
        keep_alive: x.keep_alive,
    }
}

#[tauri::command]
fn sites() -> Result<Vec<SiteRow>, String> {
    let a = app()?;
    Ok(a.sites().into_iter().map(|x| site_row(a, x)).collect())
}

#[tauri::command]
async fn publish_site(dir: String, title: String, record_key: Option<String>) -> Result<SiteRow, String> {
    let a = app()?;
    tauri::async_runtime::spawn_blocking(move || {
        a.publish_site(std::path::Path::new(&dir), &title, record_key.as_deref(), None)
            .map(|x| site_row(a, x))
            .map_err(s)
    })
    .await
    .map_err(s)?
}

#[tauri::command]
async fn add_site(uri: String) -> Result<SiteRow, String> {
    let a = app()?;
    let key = ducat_app::sites::parse_uri(&uri).ok_or("that is not a ducat:site/ address")?;
    tauri::async_runtime::spawn_blocking(move || a.add_site(&key).map(|x| site_row(a, x)).map_err(s))
        .await
        .map_err(s)?
}

#[tauri::command]
async fn fetch_site(record_key: String) -> Result<String, String> {
    let a = app()?;
    tauri::async_runtime::spawn_blocking(move || {
        // Re-read the head first, so "open" always means the current edition.
        a.add_site(&record_key).map_err(s)?;
        a.fetch_site_bundle(&record_key).map(|p| p.to_string_lossy().into_owned()).map_err(s)
    })
    .await
    .map_err(s)?
}

#[tauri::command]
fn set_site_keep(record_key: String, keep: bool) -> Result<(), String> {
    app()?.set_site_keep_alive(&record_key, keep).map_err(s)
}

#[tauri::command]
fn remove_site(record_key: String) -> Result<(), String> {
    app()?.remove_site(&record_key).map_err(s)
}

#[tauri::command]
fn lint_site(dir: String) -> Option<String> {
    ducat_app::sites::clearnet_in(std::path::Path::new(&dir))
}

// ----- the log ---------------------------------------------------------------

#[tauri::command]
fn log_tail(lines: usize) -> Result<Vec<String>, String> {
    let a = app()?;
    let text = std::fs::read_to_string(a.root().join("ducat.log")).unwrap_or_default();
    let all: Vec<&str> = text.lines().collect();
    let start = all.len().saturating_sub(lines);
    Ok(all[start..].iter().map(|l| l.to_string()).collect())
}

pub fn run() {
    let a = App::open_default().expect("could not open the data directory");
    // The same first line the phone writes, so a log read cold says what
    // it was looking at before it says anything else.
    ducat_app::log::info(
        "App",
        format!(
            "started — desk v{}, {} {}, state {}",
            env!("CARGO_PKG_VERSION"),
            std::env::consts::OS,
            std::env::consts::ARCH,
            a.root().display()
        ),
    );
    // Start the node before the window: the first thing the screen asks is
    // "are we attached", and the answer should already be on its way.
    if let Err(e) = a.start_node() {
        ducat_app::log::error("Desk", format!("node: {e}"));
    }
    let _ = APP.set(a.clone());

    // Once the node is up, put every kept site and release back on the
    // network — a restart drops the seed registry, and a desk that stopped
    // serving what it promised on every reboot is not a mirror.
    std::thread::Builder::new()
        .name("desk-reseed".into())
        .spawn(move || {
            for _ in 0..120 {
                if a.node_status().public_internet_ready {
                    a.reseed_all_sites();
                    a.reseed_all_releases();
                    a.sweep_site_orphans();
                    a.sweep_release_orphans();
                    return;
                }
                std::thread::sleep(std::time::Duration::from_secs(2));
            }
            ducat_app::log::warn("Desk", "node never became ready; nothing re-parked");
        })
        .ok();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            status,
            fetch_progress,
            releases,
            share_file,
            add_release,
            fetch_release,
            set_release_keep,
            remove_release,
            sites,
            publish_site,
            add_site,
            fetch_site,
            set_site_keep,
            remove_site,
            lint_site,
            log_tail,
        ])
        .run(tauri::generate_context!())
        .expect("error while running the desk");
}
