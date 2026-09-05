//! The desk's commands: one thin call each into `ducat-app`.
//!
//! Anything that waits on the network runs on a blocking thread through
//! `spawn_blocking`, so the window keeps painting while a bundle arrives.
//! Errors cross as strings — the UI shows them, it does not branch on them.

use std::sync::OnceLock;

use ducat_app::contacts::{Contact, StoredMessage};
use ducat_app::mailbox::{Claim, Outgoing};
use ducat_app::{App, CardProblem, Error};
use serde::Serialize;
use tauri::Manager;

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


// ----- who we are ------------------------------------------------------------

/// A refusal in the reader's terms. Card problems are typed on the app
/// side so this is the one place they become sentences.
fn said(e: Error) -> String {
    match e {
        Error::Card(CardProblem::AlreadyUsed) => "That card has already been answered — ask them for a fresh one.".into(),
        Error::Card(CardProblem::Own) => "That card is your own.".into(),
        Error::Card(CardProblem::NotPublished) => "The card's details have not reached the network yet — try again in a minute.".into(),
        Error::Card(CardProblem::Expired) => "That card has expired — ask them for a fresh one.".into(),
        e => e.to_string(),
    }
}

#[derive(Serialize)]
struct PersonaRow {
    hex: String,
    name: String,
    color: i64,
    primary: bool,
    worn: bool,
    /// The name this persona asserts on its cards.
    my_name: Option<String>,
}

#[tauri::command]
fn personas() -> Result<Vec<PersonaRow>, String> {
    let a = app()?;
    let worn = a.worn().map_err(said)?;
    a.personas()
        .map_err(said)?
        .into_iter()
        .map(|p| {
            Ok(PersonaRow {
                my_name: a.my_name(Some(&p.hex)).map_err(said)?,
                worn: p.hex == worn,
                hex: p.hex,
                name: p.name,
                color: p.color,
                primary: p.primary,
            })
        })
        .collect()
}

#[tauri::command]
fn wear(hex: String) -> Result<(), String> {
    app()?.wear(&hex).map_err(said)
}

#[tauri::command]
fn create_persona(name: String, color: i64) -> Result<Option<PersonaRow>, String> {
    let a = app()?;
    Ok(a.create_persona(&name, color).map_err(said)?.map(|p| PersonaRow {
        hex: p.hex,
        name: p.name,
        color: p.color,
        primary: p.primary,
        worn: false,
        my_name: None,
    }))
}

#[tauri::command]
fn set_my_name(name: String, persona_hex: Option<String>) -> Result<(), String> {
    app()?.set_my_name(persona_hex.as_deref(), &name).map_err(said)
}

#[derive(Serialize)]
struct Code {
    uri: String,
    inbox_key: String,
    svg: String,
}

fn qr_svg(text: &str) -> String {
    use qrcode::render::svg;
    match qrcode::QrCode::new(text.as_bytes()) {
        Ok(code) => code
            .render::<svg::Color>()
            .min_dimensions(240, 240)
            .quiet_zone(true)
            .dark_color(svg::Color("#000000"))
            .light_color(svg::Color("#ffffff"))
            .build(),
        Err(_) => String::new(),
    }
}

/// The standing profile code for the worn persona, cut if none is
/// outstanding — a network round trip when it is.
#[tauri::command]
async fn profile_code() -> Result<Code, String> {
    let a = app()?;
    tauri::async_runtime::spawn_blocking(move || {
        let h = a.profile_code(None).map_err(said)?;
        Ok(Code { svg: qr_svg(&h.uri), uri: h.uri, inbox_key: h.inbox_key })
    })
    .await
    .map_err(s)?
}

// ----- contacts and threads ----------------------------------------------------

#[derive(Serialize)]
struct ContactRow {
    persona_hex: String,
    name: String,
    named: bool,
    petname: Option<String>,
    asserted_name: Option<String>,
    unread: bool,
    last_body: Option<String>,
    last_at: u64,
    last_outgoing: bool,
    chat_visible: bool,
    has_keys: bool,
    owner: String,
    their_address: Option<String>,
    pending_address: Option<String>,
    card_purpose: Option<String>,
    email: Option<String>,
    phone: Option<String>,
    signal: Option<String>,
}

fn contact_row(a: &App, c: Contact) -> ContactRow {
    let thread = a.thread(&c.persona_hex);
    // The preview is the last thing said, not the last thing done to it.
    let last = thread.iter().rev().find(|r| r.surfaces() && !matches!(r.kind, 4 | 5 | 14 | 15));
    ContactRow {
        name: c.display_name(),
        named: c.named(),
        unread: c.in_seq > a.chat_seen(&c),
        last_body: last.map(|r| r.body.clone()),
        last_at: last.map_or(0, |r| r.timestamp),
        last_outgoing: last.map_or(false, |r| r.outgoing),
        has_keys: c.their_bundle.is_some(),
        persona_hex: c.persona_hex,
        petname: c.petname,
        asserted_name: c.asserted_name,
        chat_visible: c.chat_visible,
        owner: c.owner,
        their_address: c.their_address,
        pending_address: c.pending_address,
        card_purpose: c.card_purpose,
        email: c.email,
        phone: c.phone,
        signal: c.signal,
    }
}

#[tauri::command]
fn contacts() -> Result<Vec<ContactRow>, String> {
    let a = app()?;
    let mut rows: Vec<ContactRow> = a.contacts().into_iter().map(|c| contact_row(a, c)).collect();
    rows.sort_by(|x, y| y.last_at.cmp(&x.last_at).then_with(|| x.name.cmp(&y.name)));
    Ok(rows)
}

#[derive(Serialize)]
struct ClaimOut {
    contact: ContactRow,
    known: bool,
}

#[tauri::command]
async fn claim_card(uri: String, petname: Option<String>) -> Result<ClaimOut, String> {
    let a = app()?;
    tauri::async_runtime::spawn_blocking(move || {
        let petname = petname.as_deref().map(str::trim).filter(|p| !p.is_empty());
        match a.claim_card(&uri, petname, false, None).map_err(said)? {
            Claim::New(c) => Ok(ClaimOut { contact: contact_row(a, c), known: false }),
            Claim::Known(c) => Ok(ClaimOut { contact: contact_row(a, c), known: true }),
        }
    })
    .await
    .map_err(s)?
}

#[derive(Serialize)]
struct MessageRow {
    outgoing: bool,
    seq: u64,
    body: String,
    timestamp: u64,
    kind: u32,
    amount_pxmr: u64,
    delivered: bool,
    forward_secret: bool,
    dead_letter: bool,
    read_by_them: Option<bool>,
    att_name: Option<String>,
    att_mime: Option<String>,
    att_len: u64,
    att_hash: Option<String>,
    att_on_swarm: bool,
    att_here: bool,
    items: Vec<(String, u64)>,
    tax_pxmr: Option<u64>,
    payto: Option<String>,
    txid_hex: Option<String>,
    re_seq: Option<u64>,
    re_own: bool,
    oob: bool,
    group_id: Option<String>,
    pub_wanted: Option<String>,
    pub_period_id: Option<String>,
    /// Reactions on this row: ours, theirs.
    react_mine: Option<String>,
    react_theirs: Option<String>,
    /// A plain message of ours we took back, or a bill withdrawn/refused.
    unsent: bool,
    withdrawn: bool,
    refused: bool,
}

fn message_row(m: StoredMessage) -> MessageRow {
    let att_here = m.att_hash.as_deref().map_or(false, |h| APP.get().map_or(false, |a| a.attachment_file(h).exists()));
    MessageRow {
        att_hash: m.att_hash.clone(),
        att_on_swarm: m.att_swarm.is_some(),
        att_here,
        outgoing: m.outgoing,
        seq: m.seq,
        body: m.body,
        timestamp: m.timestamp,
        kind: m.kind,
        amount_pxmr: m.amount_pxmr,
        delivered: m.delivered,
        forward_secret: m.forward_secret,
        dead_letter: m.dead_letter,
        read_by_them: m.read_by_them,
        att_name: m.att_name,
        att_mime: m.att_mime,
        att_len: m.att_len,
        items: m.items.into_iter().map(|i| (i.description, i.amount_pxmr)).collect(),
        tax_pxmr: m.tax_pxmr,
        payto: m.payto,
        txid_hex: m.txid_hex,
        re_seq: m.re_seq,
        re_own: m.re_own,
        oob: m.oob,
        group_id: m.group_id,
        pub_wanted: m.pub_wanted,
        pub_period_id: m.pub_period_id,
        react_mine: None,
        react_theirs: None,
        unsent: false,
        withdrawn: false,
        refused: false,
    }
}

#[tauri::command]
fn thread(persona_hex: String) -> Result<Vec<MessageRow>, String> {
    let a = app()?;
    let all = a.thread(&persona_hex);
    let reactions = ducat_app::contacts::reactions_on(&all);
    let marks = ducat_app::contacts::retractions(&all);
    Ok(all
        .iter()
        .filter(|m| m.surfaces() && m.kind != 4 && !marks.quiet.contains(&(m.seq, m.timestamp)))
        .map(|m| {
            let key = (m.seq, m.timestamp);
            let mut row = message_row(m.clone());
            if let Some((mine, theirs)) = reactions.get(&key) {
                row.react_mine = mine.clone();
                row.react_theirs = theirs.clone();
            }
            row.unsent = marks.unsent.contains(&key);
            row.withdrawn = marks.withdrawn.contains(&key);
            row.refused = marks.refused.contains(&key);
            row
        })
        .collect())
}

#[tauri::command]
async fn react(persona_hex: String, seq: u64, re_own: bool, emoji: String) -> Result<(), String> {
    let a = app()?;
    tauri::async_runtime::spawn_blocking(move || a.react(&persona_hex, seq, re_own, &emoji).map_err(said)).await.map_err(s)?
}

#[tauri::command]
async fn retract_message(persona_hex: String, seq: u64) -> Result<(), String> {
    let a = app()?;
    tauri::async_runtime::spawn_blocking(move || a.retract(&persona_hex, seq).map_err(said)).await.map_err(s)?
}

#[tauri::command]
fn delete_message(persona_hex: String, seq: u64, outgoing: bool, timestamp: u64) -> Result<(), String> {
    app()?.delete_message(&persona_hex, seq, outgoing, Some(timestamp)).map_err(said)
}

#[tauri::command]
fn delete_thread(persona_hex: String) -> Result<(), String> {
    app()?.delete_thread(&persona_hex).map_err(said)
}

#[tauri::command]
fn disappear_after(persona_hex: String) -> Result<u64, String> {
    Ok(app()?.disappear_after(&persona_hex))
}

#[tauri::command]
fn set_disappear_after(persona_hex: String, secs: u64) -> Result<(), String> {
    app()?.set_disappear_after(&persona_hex, secs).map_err(said)
}

#[tauri::command]
fn draft(persona_hex: String) -> Result<String, String> {
    Ok(app()?.draft_of(&persona_hex))
}

#[tauri::command]
fn save_draft(persona_hex: String, text: String) -> Result<(), String> {
    app()?.save_draft(&persona_hex, &text).map_err(said)
}

#[tauri::command]
fn set_chat_visible(persona_hex: String, visible: bool) -> Result<(), String> {
    app()?.set_chat_visible(&persona_hex, visible).map_err(said)
}

#[tauri::command]
async fn send_text(persona_hex: String, body: String) -> Result<(), String> {
    let a = app()?;
    tauri::async_runtime::spawn_blocking(move || {
        let c = a.contact(&persona_hex).ok_or("no such contact")?;
        a.send(&c, Outgoing::text(&body)).map(|_| ()).map_err(said)
    })
    .await
    .map_err(s)?
}

#[tauri::command]
fn mark_seen(persona_hex: String) -> Result<(), String> {
    let a = app()?;
    if let Some(c) = a.contact(&persona_hex) {
        a.set_chat_seen(&c).map_err(said)?;
    }
    Ok(())
}

#[tauri::command]
fn set_petname(persona_hex: String, name: Option<String>) -> Result<(), String> {
    app()?.set_petname(&persona_hex, name.as_deref()).map_err(said)
}

#[tauri::command]
fn remove_contact(persona_hex: String) -> Result<(), String> {
    app()?.remove_contact(&persona_hex).map_err(said)
}

#[tauri::command]
fn unread_threads() -> Result<usize, String> {
    Ok(app()?.unread_threads())
}

/// Bumped on every write to the tables; a screen that sees it move
/// re-reads what it shows.
#[tauri::command]
fn generation() -> u64 {
    ducat_app::contacts::generation()
}

/// Read the logs now rather than at the lap's next turn — after a send,
/// so the reply is not fifteen seconds late.
#[tauri::command]
async fn poll_now(persona_hex: Option<String>) -> Result<usize, String> {
    let a = app()?;
    tauri::async_runtime::spawn_blocking(move || match persona_hex {
        Some(h) => a.poll_contact(&h),
        None => {
            a.collect_claims(None);
            a.poll()
        }
    })
    .await
    .map_err(s)
}


// ----- the wallet ----------------------------------------------------------------

#[derive(Serialize)]
struct WalletView {
    address: Option<String>,
    balances: ducat_app::wallet::Balances,
    blocker: ducat_app::wallet::SyncBlocker,
    node: Option<String>,
    own_node: Option<String>,
    stagenet: bool,
    fiat_spendable: Option<ducat_app::wallet::FiatView>,
    currency: String,
    address_svg: String,
}

#[tauri::command]
fn wallet_status() -> Result<WalletView, String> {
    let a = app()?;
    let b = a.balances();
    let address = a.wallet_address();
    Ok(WalletView {
        fiat_spendable: a.rate_view(b.spendable_pxmr),
        address_svg: address.as_deref().map(qr_svg).unwrap_or_default(),
        address,
        balances: b,
        blocker: a.blocker(),
        node: a.last_good_node(),
        own_node: a.monero_own_url(),
        stagenet: a.wallet_stagenet(),
        currency: a.rate_currency(),
    })
}

#[derive(Serialize)]
struct NoteRow {
    amount_pxmr: u64,
    height: u64,
    spent: bool,
    tx_hash_hex: String,
    timestamp: u64,
    minor: u32,
    unlocked: bool,
    from: Option<String>,
}

#[tauri::command]
fn wallet_notes() -> Result<Vec<NoteRow>, String> {
    let a = app()?;
    let tip = a.tip();
    let mut rows: Vec<NoteRow> = a
        .entries()
        .into_iter()
        .map(|e| NoteRow {
            unlocked: tip > 0 && e.height + ducat_app::wallet::LOCK_BLOCKS <= tip,
            from: a.persona_for_minor(e.minor).and_then(|h| a.contact(&h)).map(|c| c.display_name()),
            amount_pxmr: e.amount_pxmr,
            height: e.height,
            spent: e.spent,
            tx_hash_hex: e.tx_hash_hex,
            timestamp: e.timestamp,
            minor: e.minor,
        })
        .collect();
    rows.sort_by(|x, y| y.height.cmp(&x.height));
    Ok(rows)
}

#[derive(Serialize)]
struct SentRow {
    txid_hex: String,
    amount_pxmr: u64,
    fee_pxmr: u64,
    to_address: String,
    contact: Option<String>,
    contact_name: Option<String>,
    note: Option<String>,
    timestamp: u64,
    donation: bool,
    recovered: bool,
}

#[tauri::command]
fn wallet_sends() -> Result<Vec<SentRow>, String> {
    let a = app()?;
    let mut rows: Vec<SentRow> = a
        .sends()
        .into_iter()
        .map(|s| SentRow {
            contact_name: s.contact.as_deref().and_then(|h| a.contact(h)).map(|c| c.display_name()),
            txid_hex: s.txid_hex,
            amount_pxmr: s.amount_pxmr,
            fee_pxmr: s.fee,
            to_address: s.to_address,
            contact: s.contact,
            note: s.note,
            timestamp: s.ts,
            donation: s.donate,
            recovered: s.recovered,
        })
        .collect();
    rows.sort_by(|x, y| y.timestamp.cmp(&x.timestamp));
    Ok(rows)
}

#[tauri::command]
async fn wallet_quote(amount_xmr: String, priority: u32) -> Result<ducat_app::wallet::Quote, String> {
    let a = app()?;
    let amount = ducat_app::wallet::parse_xmr(&amount_xmr).ok_or("that is not an amount")?;
    tauri::async_runtime::spawn_blocking(move || a.quote(amount, priority)).await.map_err(s)
}

#[tauri::command]
async fn wallet_send(to: String, amount_xmr: String, note: Option<String>, priority: u32, contact_hex: Option<String>) -> Result<String, String> {
    let a = app()?;
    let amount = ducat_app::wallet::parse_xmr(&amount_xmr).ok_or("that is not an amount")?;
    tauri::async_runtime::spawn_blocking(move || {
        a.send_xmr(&to, amount, contact_hex.as_deref(), note.as_deref(), priority, false).map(|r| r.txid_hex).map_err(said)
    })
    .await
    .map_err(s)?
}

#[tauri::command]
async fn wallet_max(priority: u32) -> Result<u64, String> {
    let a = app()?;
    tauri::async_runtime::spawn_blocking(move || a.max_sendable(priority)).await.map_err(s)
}

#[tauri::command]
async fn set_own_node(url: Option<String>) -> Result<(), String> {
    let a = app()?;
    a.set_monero_own_url(url.as_deref()).map_err(said)?;
    tauri::async_runtime::spawn_blocking(move || {
        a.pick_node();
    })
    .await
    .map_err(s)
}

#[tauri::command]
fn wallet_rescan(height: u64) -> Result<(), String> {
    app()?.rescan_from(height).map_err(said)
}

/// A scan step now, for a screen that is being watched.
#[tauri::command]
async fn wallet_step() -> Result<bool, String> {
    let a = app()?;
    tauri::async_runtime::spawn_blocking(move || {
        let Some(node) = a.last_good_node().or_else(|| a.pick_node()) else { return false };
        if a.wallet_address().is_none() {
            let _ = a.ensure_wallet();
        }
        let moved = a.scan_step(&node);
        if moved {
            a.refresh_spent(&node);
        }
        moved
    })
    .await
    .map_err(s)
}


// ----- the till: tabs, sales, the catalogue -----------------------------------

#[derive(Serialize)]
struct TabRow {
    id: String,
    origin: String,
    persona_hex: String,
    name: String,
    opened_at: u64,
    lines: Vec<(String, u64)>,
    tax_pxmr: Option<u64>,
    state: String,
    total_pxmr: u64,
    settled_total: u64,
    settled_at: u64,
    paid_pxmr: u64,
    tip_pxmr: u64,
    seen_tx: Option<String>,
    receipt_owed: bool,
    shown: ducat_app::wallet::Shown,
}

fn tab_row(a: &App, t: ducat_app::tabs::RunningTab) -> TabRow {
    TabRow {
        name: if t.persona_hex.starts_with("pending:") { "Waiting for a scan".into() } else { a.contact(&t.persona_hex).map(|c| c.display_name()).unwrap_or_else(|| "(gone)".into()) },
        total_pxmr: t.total_pxmr(),
        tip_pxmr: t.tip_pxmr(),
        receipt_owed: t.word_seq == ducat_app::tabs::WORD_UNSENT,
        shown: a.show_amount(t.take_pxmr()),
        lines: t.lines.iter().map(|l| (l.description.clone(), l.amount_pxmr)).collect(),
        id: t.id,
        origin: t.origin,
        persona_hex: t.persona_hex,
        opened_at: t.opened_at,
        tax_pxmr: t.tax,
        state: t.state,
        settled_total: t.settled_total,
        settled_at: t.settled_at,
        paid_pxmr: t.paid_pxmr,
        seen_tx: t.seen_tx,
    }
}

#[tauri::command]
fn tabs() -> Result<Vec<TabRow>, String> {
    let a = app()?;
    let mut rows: Vec<TabRow> = a.tabs().into_iter().map(|t| tab_row(a, t)).collect();
    rows.sort_by(|x, y| y.opened_at.cmp(&x.opened_at));
    Ok(rows)
}

#[tauri::command]
fn open_tab(persona_hex: String, origin: String) -> Result<TabRow, String> {
    let a = app()?;
    a.open_or_resume_tab(&persona_hex, &origin).map(|t| tab_row(a, t)).map_err(said)
}

#[tauri::command]
fn tab_add_line(id: String, description: String, amount_pxmr: u64) -> Result<TabRow, String> {
    let a = app()?;
    let d = ducat_mobile::contacts::clean_display_text(description.trim().to_string());
    if d.is_empty() || amount_pxmr == 0 {
        return Err("a line needs a name and an amount".into());
    }
    a.mutate_tab(&id, |mut t| {
        t.lines.push(ducat_app::contacts::BillItem { description: d, amount_pxmr });
        t
    })
    .map_err(said)?
    .map(|t| tab_row(a, t))
    .ok_or_else(|| "that tab is gone".into())
}

#[tauri::command]
fn tab_remove_line(id: String, index: usize) -> Result<TabRow, String> {
    let a = app()?;
    a.mutate_tab(&id, |mut t| {
        if index < t.lines.len() {
            t.lines.remove(index);
        }
        t
    })
    .map_err(said)?
    .map(|t| tab_row(a, t))
    .ok_or_else(|| "that tab is gone".into())
}

#[tauri::command]
fn tab_set_tax(id: String, tax_pxmr: Option<u64>) -> Result<TabRow, String> {
    let a = app()?;
    a.mutate_tab(&id, |mut t| {
        t.tax = tax_pxmr.filter(|v| *v > 0);
        t
    })
    .map_err(said)?
    .map(|t| tab_row(a, t))
    .ok_or_else(|| "that tab is gone".into())
}

#[tauri::command]
async fn settle_tab(id: String) -> Result<TabRow, String> {
    let a = app()?;
    tauri::async_runtime::spawn_blocking(move || {
        let t = a.tab(&id).ok_or("that tab is gone")?;
        if t.lines.is_empty() {
            return Err("nothing on the tab yet".to_string());
        }
        a.settle_tab(&t).map(|t| tab_row(a, t)).map_err(said)
    })
    .await
    .map_err(s)?
}

#[tauri::command]
async fn cancel_tab(id: String) -> Result<Option<TabRow>, String> {
    let a = app()?;
    tauri::async_runtime::spawn_blocking(move || {
        let t = a.tab(&id).ok_or("that tab is gone")?;
        a.cancel_tab(&t).map(|o| o.map(|t| tab_row(a, t))).map_err(said)
    })
    .await
    .map_err(s)?
}

#[tauri::command]
async fn tab_paid_outside(id: String) -> Result<Option<TabRow>, String> {
    let a = app()?;
    tauri::async_runtime::spawn_blocking(move || {
        let t = a.tab(&id).ok_or("that tab is gone")?;
        a.mark_tab_paid_outside(&t).map(|o| o.map(|t| tab_row(a, t))).map_err(said)
    })
    .await
    .map_err(s)?
}

#[tauri::command]
async fn tab_send_receipt(id: String) -> Result<Option<TabRow>, String> {
    let a = app()?;
    tauri::async_runtime::spawn_blocking(move || {
        let t = a.tab(&id).ok_or("that tab is gone")?;
        let r = if t.state == "paid_oob" { a.send_oob_receipt(&t) } else { a.send_chain_receipt(&t) };
        r.map(|o| o.map(|t| tab_row(a, t))).map_err(said)
    })
    .await
    .map_err(s)?
}

#[tauri::command]
fn delete_tab(id: String) -> Result<(), String> {
    app()?.delete_tab(&id).map_err(said)
}

/// A sale's handshake: a card good for two hours, purpose "sale", so the
/// customer who answers it gets a bill and nothing else about us.
#[tauri::command]
async fn sale_card() -> Result<Code, String> {
    let a = app()?;
    tauri::async_runtime::spawn_blocking(move || {
        let name = a.my_name(None).map_err(said)?;
        let h = a.issue_card(name.as_deref(), 60 * 60 * 2, "sale", None).map_err(said)?;
        Ok(Code { svg: qr_svg(&h.uri), uri: h.uri, inbox_key: h.inbox_key })
    })
    .await
    .map_err(s)?
}

/// Whoever answered this card, once they have.
#[tauri::command]
async fn card_claimant(inbox_key: String) -> Result<Option<ContactRow>, String> {
    let a = app()?;
    tauri::async_runtime::spawn_blocking(move || {
        a.collect_claims(Some(&inbox_key));
        let hex = a.issued_cards().into_iter().find(|c| c.inbox_key == inbox_key).and_then(|c| c.answered_by);
        Ok(hex.and_then(|h| a.contact(&h)).map(|c| contact_row(a, c)))
    })
    .await
    .map_err(s)?
}

#[derive(Serialize)]
struct ItemRow {
    id: String,
    name: String,
    price: String,
    currency: String,
    category: String,
    sold_out: bool,
    pxmr: Option<u64>,
    snag: Option<ducat_app::catalogue::Snag>,
}

#[tauri::command]
fn catalogue() -> Result<Vec<ItemRow>, String> {
    let a = app()?;
    Ok(a.catalogue_live()
        .into_iter()
        .map(|i| {
            let priced = a.price_item(&i);
            ItemRow {
                pxmr: priced.as_ref().ok().map(|p| p.pxmr),
                snag: priced.err(),
                id: i.id,
                name: i.name,
                price: i.price,
                currency: i.currency,
                category: i.category,
                sold_out: i.sold_out,
            }
        })
        .collect())
}

#[tauri::command]
fn put_item(id: Option<String>, name: String, price: String, sold_out: bool) -> Result<(), String> {
    let a = app()?;
    let mut item = match id.and_then(|id| a.catalogue().into_iter().find(|i| i.id == id)) {
        Some(mut i) => {
            i.name = ducat_mobile::contacts::clean_display_text(name.trim().to_string());
            i.price = price.trim().to_string();
            i
        }
        None => a.draft_item(&name, &price),
    };
    item.sold_out = sold_out;
    if item.name.is_empty() || ducat_app::catalogue::parse_money(&item.price).is_none() {
        return Err("an item needs a name and a price".into());
    }
    a.put_item(item).map_err(said)
}

#[tauri::command]
fn remove_item(id: String) -> Result<(), String> {
    app()?.remove_item(&id).map_err(said)
}

/// Fiat text → pXMR at the cached rate, for the screens that let a
/// person type a price in money.
#[tauri::command]
fn fiat_to_pxmr(text: String) -> Result<Option<u64>, String> {
    let a = app()?;
    Ok(a.rate_cached_pair().and_then(|(rate, _)| a.fiat_to_pxmr(&text, rate)))
}

#[tauri::command]
fn show_amount(pxmr: u64) -> Result<ducat_app::wallet::Shown, String> {
    Ok(app()?.show_amount(pxmr))
}


/// Pay a bill in a thread (or send money unprompted): the wallet first,
/// then the notice. Returns the transaction id.
#[tauri::command]
async fn pay_bill(persona_hex: String, answers_seq: Option<u64>, amount_pxmr: u64, memo: Option<String>, priority: u32) -> Result<String, String> {
    let a = app()?;
    tauri::async_runtime::spawn_blocking(move || a.pay_bill(&persona_hex, answers_seq, amount_pxmr, memo.as_deref(), priority).map_err(said))
        .await
        .map_err(s)?
}


// ----- the library: press and reading room ------------------------------------

#[derive(Serialize)]
struct IssueRow {
    period: String,
    on_shelf: bool,
    on_swarm: bool,
    bytes: u64,
    sent: Vec<String>,
    billed: Vec<String>,
    file: String,
}

#[derive(Serialize)]
struct PublicationRow {
    id: String,
    title: String,
    price_pxmr: u64,
    subscribers: Vec<ContactRow>,
    issues: Vec<IssueRow>,
    has_shelf: bool,
    press_code: Option<String>,
    created: u64,
}

fn publication_row(a: &App, id: String, p: ducat_app::publications::Publication) -> PublicationRow {
    PublicationRow {
        subscribers: p.subs.iter().filter_map(|h| a.contact(h)).map(|c| contact_row(a, c)).collect(),
        issues: p
            .issues_sorted()
            .into_iter()
            .map(|(period, i)| IssueRow {
                period,
                on_shelf: i.on_shelf(),
                on_swarm: i.on_swarm(),
                bytes: if i.rec_bytes > 0 { i.rec_bytes } else { std::fs::metadata(&i.file).map(|m| m.len()).unwrap_or(0) },
                sent: i.sent.clone(),
                billed: i.billed.keys().cloned().collect(),
                file: i.file.clone(),
            })
            .collect(),
        has_shelf: p.root_rec.is_some(),
        press_code: p.press_code.clone().filter(|_| p.press_code_exp > App::now()),
        id,
        title: p.title,
        price_pxmr: p.price,
        created: p.created,
    }
}

#[tauri::command]
fn publications() -> Result<Vec<PublicationRow>, String> {
    let a = app()?;
    let mut rows: Vec<PublicationRow> = a.publications().into_iter().map(|(id, p)| publication_row(a, id, p)).collect();
    rows.sort_by(|x, y| y.created.cmp(&x.created));
    Ok(rows)
}

#[tauri::command]
fn create_publication(title: String) -> Result<String, String> {
    if title.trim().is_empty() {
        return Err("a publication needs a title".into());
    }
    app()?.create_publication(&title).map_err(said)
}

#[tauri::command]
fn delete_publication(id: String) -> Result<(), String> {
    app()?.delete_publication(&id).map_err(said)
}

#[tauri::command]
fn set_publication_price(id: String, price_pxmr: u64) -> Result<(), String> {
    app()?.set_price(&id, price_pxmr).map_err(said)
}

#[tauri::command]
fn set_subscriber(id: String, persona_hex: String, on: bool) -> Result<(), String> {
    app()?.set_subscriber(&id, &persona_hex, on).map_err(said)
}

/// Put a file under a period: on the shelf when it fits, on the swarm
/// otherwise (or when asked), and then to the subscribers.
#[tauri::command]
async fn publish_issue(id: String, period: String, path: String, prefer_swarm: bool, note: String) -> Result<usize, String> {
    let a = app()?;
    tauri::async_runtime::spawn_blocking(move || {
        let period = period.trim().to_string();
        if !ducat_app::publications::is_safe_period_id(&period) {
            return Err("a period is a short name — no slashes".to_string());
        }
        let file = std::path::Path::new(&path);
        let size = std::fs::metadata(file).map(|m| m.len()).map_err(s)?;
        if !prefer_swarm && size <= ducat_app::publications::SHELF_MULTI_CAP_BYTES {
            a.shelve_issue(&id, &period, file).map_err(said)?;
        } else {
            a.ship_issue(&id, &period, file).map_err(said)?;
        }
        a.release_issue(&id, &period, &note).map_err(said)
    })
    .await
    .map_err(s)?
}

#[tauri::command]
async fn press_code(id: String) -> Result<Code, String> {
    let a = app()?;
    tauri::async_runtime::spawn_blocking(move || {
        let uri = a.press_code(&id).map_err(said)?;
        Ok(Code { svg: qr_svg(&uri), uri, inbox_key: String::new() })
    })
    .await
    .map_err(s)?
}

#[derive(Serialize)]
struct ShelfRow {
    period: String,
    has_key: bool,
    on_shelf: bool,
    shelf_bytes: u64,
    on_swarm: bool,
    fetched_bytes: Option<u64>,
    asked: bool,
    dir: String,
}

#[derive(Serialize)]
struct SubscriptionRow {
    publisher_hex: String,
    name: String,
    price_known: bool,
    mirror: bool,
    muted: bool,
    has_shelf: bool,
    shelf_seen_at: u64,
    periods: Vec<ShelfRow>,
}

#[tauri::command]
fn subscriptions() -> Result<Vec<SubscriptionRow>, String> {
    let a = app()?;
    let mut rows = Vec::new();
    for (publisher, sub) in a.subscriptions() {
        let shelved = a.shelved_periods(&publisher);
        let mut periods: std::collections::BTreeSet<String> = sub.periods.keys().cloned().collect();
        periods.extend(shelved.keys().cloned());
        let mut prows: Vec<ShelfRow> = periods
            .into_iter()
            .map(|period| ShelfRow {
                has_key: sub.periods.contains_key(&period),
                on_shelf: shelved.contains_key(&period),
                shelf_bytes: shelved.get(&period).copied().unwrap_or(0),
                on_swarm: sub.ships.get(&period).map_or(false, |s| !s.key.is_empty()),
                fetched_bytes: a.fetched_bytes(&publisher, &period),
                asked: a.asked_for(&publisher, &period),
                dir: a.issue_dir(&publisher, &period).to_string_lossy().into_owned(),
                period,
            })
            .collect();
        prows.sort_by(|x, y| y.period.cmp(&x.period));
        rows.push(SubscriptionRow {
            name: a.contact(&publisher).map(|c| c.display_name()).unwrap_or_else(|| format!("{}…", &publisher[..8.min(publisher.len())])),
            price_known: false,
            mirror: sub.mirror,
            muted: sub.muted,
            has_shelf: sub.record.is_some(),
            shelf_seen_at: a.shelf_seen_at(&publisher),
            periods: prows,
            publisher_hex: publisher,
        });
    }
    Ok(rows)
}

#[tauri::command]
async fn fetch_issue(publisher_hex: String, period: String) -> Result<String, String> {
    let a = app()?;
    tauri::async_runtime::spawn_blocking(move || a.fetch_issue(&publisher_hex, &period).map(|p| p.to_string_lossy().into_owned()).map_err(said))
        .await
        .map_err(s)?
}

#[tauri::command]
async fn refresh_shelf(publisher_hex: String) -> Result<i64, String> {
    let a = app()?;
    tauri::async_runtime::spawn_blocking(move || a.refresh_shelf(&publisher_hex)).await.map_err(s)
}

#[tauri::command]
async fn ask_for_period(publisher_hex: String, period: String) -> Result<(), String> {
    let a = app()?;
    tauri::async_runtime::spawn_blocking(move || {
        let c = a.contact(&publisher_hex).ok_or("no such contact")?;
        a.ask_for_period(&c, &period).map_err(said)
    })
    .await
    .map_err(s)?
}

#[tauri::command]
fn set_mirroring(publisher_hex: String, on: bool) -> Result<(), String> {
    app()?.set_mirroring(&publisher_hex, on).map_err(said)
}

#[tauri::command]
fn set_muted(publisher_hex: String, muted: bool) -> Result<(), String> {
    app()?.set_muted(&publisher_hex, muted).map_err(said)
}


// ----- groups ----------------------------------------------------------------------

#[derive(Serialize)]
struct GroupRowOut {
    id_hex: String,
    name: String,
    members: Vec<ContactRow>,
    missing: Vec<String>,
    mine: String,
    unread: bool,
    last_body: Option<String>,
    last_at: u64,
}

fn group_row(a: &App, g: ducat_app::groups::Group) -> GroupRowOut {
    let rows = a.group_thread(&g.id_hex);
    let last = rows.last();
    GroupRowOut {
        members: g.members.iter().filter_map(|h| a.contact(h)).map(|c| contact_row(a, c)).collect(),
        missing: a.group_missing(&g.id_hex),
        mine: a.mine_in(&g.members),
        unread: App::group_unread(&a.group_seen(&g.id_hex), &a.look_at(&rows)),
        last_body: last.map(|r| r.message.body.clone()),
        last_at: last.map_or(0, |r| r.message.timestamp),
        id_hex: g.id_hex,
        name: g.name,
    }
}

#[tauri::command]
fn groups() -> Result<Vec<GroupRowOut>, String> {
    let a = app()?;
    let mut rows: Vec<GroupRowOut> = a.groups().into_iter().map(|g| group_row(a, g)).collect();
    rows.sort_by(|x, y| y.last_at.cmp(&x.last_at));
    Ok(rows)
}

#[tauri::command]
async fn create_group(name: String, members: Vec<String>) -> Result<GroupRowOut, String> {
    let a = app()?;
    tauri::async_runtime::spawn_blocking(move || a.create_group(&name, &members).map(|g| group_row(a, g)).map_err(said))
        .await
        .map_err(s)?
}

#[tauri::command]
async fn add_to_group(id_hex: String, persona_hex: String) -> Result<(), String> {
    let a = app()?;
    tauri::async_runtime::spawn_blocking(move || a.add_to_group(&id_hex, &persona_hex).map_err(said)).await.map_err(s)?
}

#[derive(Serialize)]
struct GroupMessage {
    sender_hex: String,
    sender_name: String,
    mine: bool,
    message: MessageRow,
}

#[tauri::command]
fn group_thread(id_hex: String) -> Result<Vec<GroupMessage>, String> {
    let a = app()?;
    let ours = a.persona_hexes();
    Ok(a.group_thread(&id_hex)
        .into_iter()
        .map(|r| GroupMessage {
            mine: ours.contains(&r.sender_hex),
            sender_name: if ours.contains(&r.sender_hex) { "You".into() } else { a.contact(&r.sender_hex).map(|c| c.display_name()).unwrap_or_else(|| format!("{}…", &r.sender_hex[..8.min(r.sender_hex.len())])) },
            sender_hex: r.sender_hex,
            message: message_row(r.message),
        })
        .collect())
}

#[tauri::command]
async fn send_group(id_hex: String, body: String) -> Result<bool, String> {
    let a = app()?;
    tauri::async_runtime::spawn_blocking(move || a.send_group(&id_hex, &body, 0, None, None).map_err(said)).await.map_err(s)?
}

#[tauri::command]
fn mark_group_seen(id_hex: String) -> Result<(), String> {
    let a = app()?;
    let rows = a.group_thread(&id_hex);
    let look = a.look_at(&rows);
    a.mark_group_seen(&id_hex, &look).map_err(said)
}


// ----- backup ------------------------------------------------------------------

#[derive(Serialize)]
struct BackupInfo {
    exported_at: u64,
    has_wallet: bool,
}

#[tauri::command]
fn backup_info() -> Result<BackupInfo, String> {
    let a = app()?;
    Ok(BackupInfo { exported_at: a.backup_exported_at(), has_wallet: a.spend_key_hex().is_some() })
}

#[tauri::command]
async fn export_backup(path: String, passphrase: String) -> Result<u64, String> {
    let a = app()?;
    if passphrase.chars().count() < 8 {
        return Err("a passphrase is eight characters at least".into());
    }
    tauri::async_runtime::spawn_blocking(move || a.export_backup_to(std::path::Path::new(&path), &passphrase).map_err(said))
        .await
        .map_err(s)?
}

#[tauri::command]
async fn import_backup(path: String, passphrase: String) -> Result<ducat_app::backup::Restored, String> {
    let a = app()?;
    tauri::async_runtime::spawn_blocking(move || a.import_backup_from(std::path::Path::new(&path), &passphrase).map_err(said))
        .await
        .map_err(s)?
}


// ----- the board: listings and browsing -------------------------------------------

#[derive(Serialize)]
struct ListingRow {
    id: String,
    kind: u32,
    kind_name: String,
    title: String,
    area: String,
    cell: String,
    price_pxmr: u64,
    deposit_pxmr: u64,
    specs: serde_json::Map<String, serde_json::Value>,
    private_details: String,
    quantity: u64,
    thumb_data_url: Option<String>,
    photos: Vec<String>,
    posted: bool,
    board: Option<String>,
    posted_at: u64,
    wanted: bool,
    price_typed: Option<String>,
    price_currency: Option<String>,
    shown: ducat_app::wallet::Shown,
}

fn data_url(b64: &str, mime: &str) -> String {
    format!("data:{mime};base64,{b64}")
}

fn listing_row(a: &App, l: ducat_app::listings::Listing) -> ListingRow {
    ListingRow {
        kind_name: ducat_app::listings::kind_name(l.kind).into(),
        thumb_data_url: l.thumb.as_deref().filter(|t| !t.is_empty()).map(|t| data_url(t, "image/jpeg")),
        photos: a.photos(&l.id).into_iter().map(|p| p.to_string_lossy().into_owned()).collect(),
        posted: l.posted(),
        shown: a.show_amount(l.price_pxmr),
        id: l.id,
        kind: l.kind,
        title: l.title,
        area: l.area,
        cell: l.cell,
        price_pxmr: l.price_pxmr,
        deposit_pxmr: l.deposit_pxmr,
        specs: l.specs,
        private_details: l.private_details,
        quantity: l.quantity,
        board: l.board,
        posted_at: l.posted_at,
        wanted: l.wanted,
        price_typed: l.price_typed,
        price_currency: l.price_currency,
    }
}

#[tauri::command]
fn listings() -> Result<Vec<ListingRow>, String> {
    let a = app()?;
    let mut rows: Vec<ListingRow> = a.listings().into_iter().map(|l| listing_row(a, l)).collect();
    rows.sort_by(|x, y| y.posted_at.cmp(&x.posted_at));
    Ok(rows)
}

#[derive(serde::Deserialize)]
struct ListingDraft {
    id: Option<String>,
    kind: u32,
    title: String,
    area: String,
    cell: String,
    price_text: String,
    price_is_fiat: bool,
    specs: serde_json::Map<String, serde_json::Value>,
    private_details: String,
    quantity: u64,
}

#[tauri::command]
fn save_listing(draft: ListingDraft) -> Result<ListingRow, String> {
    let a = app()?;
    if draft.title.trim().is_empty() {
        return Err("a listing needs a title".into());
    }
    let cell = draft.cell.trim().to_lowercase();
    if cell.len() < 4 || !cell.chars().all(|c| "0123456789bcdefghjkmnpqrstuvwxyz".contains(c)) {
        return Err("the area is a geohash cell, e.g. dqche".into());
    }
    let (price_pxmr, typed, currency) = if draft.price_is_fiat {
        let (rate, _) = a.rate_cached_pair().ok_or("no exchange rate yet — price in XMR, or wait for the wallet")?;
        (a.fiat_to_pxmr(&draft.price_text, rate).ok_or("that is not a price")?, Some(draft.price_text.trim().to_string()), Some(a.rate_currency()))
    } else {
        (ducat_app::wallet::parse_xmr(&draft.price_text).ok_or("that is not an amount of XMR")?, None, None)
    };
    let mut l = a
        .draft_listing(draft.kind, &draft.title, &draft.area, price_pxmr, &cell, draft.specs, &draft.private_details, typed.as_deref(), currency.as_deref(), draft.quantity, None)
        .map_err(said)?;
    if let Some(id) = draft.id.filter(|i| !i.is_empty()) {
        if let Some(old) = a.listing(&id) {
            l.id = id;
            l.thumb = old.thumb;
            l.created = old.created;
            l.gallery = old.gallery;
            l.gallery_dig = old.gallery_dig;
        }
    }
    a.put_draft(l.clone()).map_err(said)?;
    Ok(listing_row(a, a.listing(&l.id).unwrap_or(l)))
}

#[tauri::command]
fn remove_listing(id: String) -> Result<(), String> {
    app()?.remove_listing(&id).map_err(said)
}

#[tauri::command]
async fn post_listing(id: String) -> Result<bool, String> {
    let a = app()?;
    tauri::async_runtime::spawn_blocking(move || a.post_listing(&id).map_err(said)).await.map_err(s)?
}

#[tauri::command]
async fn unpost_listing(id: String) -> Result<(), String> {
    let a = app()?;
    tauri::async_runtime::spawn_blocking(move || a.unpost_listing(&id).map_err(said)).await.map_err(s)?
}

#[tauri::command]
fn add_listing_photo(id: String, path: String) -> Result<usize, String> {
    let a = app()?;
    let n = a.add_photo(&id, std::path::Path::new(&path)).map_err(said)?;
    // The first picture is the board's picture until another is chosen.
    if n == 1 {
        a.set_thumb_from_photo(&id, 0).map_err(said)?;
    }
    Ok(n)
}

#[tauri::command]
fn remove_listing_photo(id: String, index: usize) -> Result<(), String> {
    app()?.remove_photo(&id, index).map_err(said)
}

#[tauri::command]
fn set_listing_cover(id: String, index: usize) -> Result<bool, String> {
    app()?.set_thumb_from_photo(&id, index).map_err(said)
}

#[derive(Serialize)]
struct FoundRow {
    card: String,
    poster: String,
    kind: u64,
    kind_name: String,
    title: String,
    area: String,
    cell: Option<String>,
    price_pxmr: u64,
    deposit_pxmr: u64,
    expiry: u64,
    specs: serde_json::Map<String, serde_json::Value>,
    features: Vec<String>,
    quantity: u64,
    thumb_data_url: Option<String>,
    gallery: Option<String>,
    gallery_dig: Option<String>,
    mine: bool,
    shown: ducat_app::wallet::Shown,
}

fn found_row(a: &App, f: ducat_app::listings::Found) -> FoundRow {
    let ours = a.persona_hexes();
    FoundRow {
        kind_name: ducat_app::listings::kind_name(f.kind as u32).into(),
        thumb_data_url: f.thumb.as_deref().map(|t| data_url(t, "image/jpeg")),
        mine: ours.contains(&f.poster),
        shown: a.show_amount(f.price),
        card: f.card,
        poster: f.poster,
        kind: f.kind,
        title: f.title,
        area: f.area,
        cell: f.cell,
        price_pxmr: f.price,
        deposit_pxmr: f.deposit,
        expiry: f.expiry,
        specs: f.specs,
        features: f.features,
        quantity: f.quantity,
        gallery: f.gallery,
        gallery_dig: f.gallery_dig,
    }
}

#[tauri::command]
fn browse_cached(cell: String, kind: Option<u32>) -> Result<Vec<FoundRow>, String> {
    let a = app()?;
    Ok(a.browse_cached(&cell.trim().to_lowercase(), kind).into_iter().map(|f| found_row(a, f)).collect())
}

#[tauri::command]
async fn browse(cell: String, kind: Option<u32>) -> Result<Vec<FoundRow>, String> {
    let a = app()?;
    let cell = cell.trim().to_lowercase();
    if cell.len() < 4 {
        return Err("the area is a geohash cell, e.g. dqche".into());
    }
    tauri::async_runtime::spawn_blocking(move || a.browse(&cell, kind).into_iter().map(|f| found_row(a, f)).collect()).await.map_err(s)
}

#[tauri::command]
async fn fetch_gallery(share: String, digest_hex: String) -> Result<Vec<String>, String> {
    let a = app()?;
    tauri::async_runtime::spawn_blocking(move || {
        let dir = a.fetch_gallery(&share, &digest_hex).map_err(said)?;
        let mut files: Vec<String> = std::fs::read_dir(&dir).map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.is_file()).map(|p| p.to_string_lossy().into_owned()).collect()).unwrap_or_default();
        files.sort();
        Ok(files)
    })
    .await
    .map_err(s)?
}

/// A picture off disk as a data URL, for the webview which cannot read
/// files itself.
#[tauri::command]
fn picture_data_url(path: String) -> Result<String, String> {
    let bytes = std::fs::read(&path).map_err(s)?;
    let mime = match std::path::Path::new(&path).extension().and_then(|e| e.to_str()).map(|e| e.to_lowercase()).as_deref() {
        Some("png") => "image/png",
        Some("webp") => "image/webp",
        Some("gif") => "image/gif",
        _ => "image/jpeg",
    };
    use base64::Engine;
    Ok(data_url(&base64::engine::general_purpose::STANDARD.encode(&bytes), mime))
}

#[tauri::command]
fn enquiry_about(persona_hex: String) -> Result<Option<ducat_app::listings::Enquiry>, String> {
    Ok(app()?.enquiry(&persona_hex))
}


// ----- activity: the ledger --------------------------------------------------------

#[derive(Serialize)]
struct LedgerOut {
    events: Vec<ducat_app::ledger::Event>,
    summary: ducat_app::ledger::Summary,
    business: ducat_app::ledger::BusinessSummary,
}

#[tauri::command]
fn ledger(from_ts: u64, to_ts: u64) -> Result<LedgerOut, String> {
    let a = app()?;
    let events = a.ledger();
    Ok(LedgerOut {
        summary: ducat_app::ledger::summarize(&events, from_ts, to_ts),
        business: ducat_app::ledger::summarize_business(&a.tabs(), from_ts, to_ts),
        events,
    })
}

#[tauri::command]
async fn export_ledger(path: String, json: bool) -> Result<u64, String> {
    let a = app()?;
    tauri::async_runtime::spawn_blocking(move || a.export_ledger_to(std::path::Path::new(&path), json).map_err(said)).await.map_err(s)?
}


// ----- requests, standing bills, donations ------------------------------------

#[tauri::command]
async fn request_payment(persona_hex: String, amount_pxmr: u64, note: String) -> Result<(), String> {
    let a = app()?;
    tauri::async_runtime::spawn_blocking(move || a.request_payment(&persona_hex, amount_pxmr, &note).map_err(said)).await.map_err(s)?
}

#[derive(Serialize)]
struct StandingRow {
    id: String,
    persona_hex: String,
    name: String,
    amount_pxmr: u64,
    note: String,
    monthly: bool,
    next_at: u64,
}

#[tauri::command]
fn standing_bills() -> Result<Vec<StandingRow>, String> {
    let a = app()?;
    Ok(a.standing_bills()
        .into_iter()
        .map(|b| StandingRow {
            name: a.contact(&b.persona_hex).map(|c| c.display_name()).unwrap_or_else(|| "(gone)".into()),
            id: b.id,
            persona_hex: b.persona_hex,
            amount_pxmr: b.amount_pxmr,
            note: b.note,
            monthly: b.monthly,
            next_at: b.next_at,
        })
        .collect())
}

#[tauri::command]
fn add_standing_bill(persona_hex: String, amount_pxmr: u64, note: String, monthly: bool) -> Result<(), String> {
    if amount_pxmr == 0 {
        return Err("a standing bill needs an amount".into());
    }
    app()?.add_standing_bill(&persona_hex, amount_pxmr, &note, monthly).map(|_| ()).map_err(said)
}

#[tauri::command]
fn stop_standing_bill(id: String) -> Result<(), String> {
    app()?.stop_standing_bill(&id).map_err(said)
}

/// A code whose thread takes unprompted payments as donations (§16.13).
#[tauri::command]
async fn donate_code() -> Result<Code, String> {
    let a = app()?;
    tauri::async_runtime::spawn_blocking(move || {
        let owner = a.worn().map_err(said)?;
        if let Some(c) = a.current_card(&owner, "donate") {
            return Ok(Code { svg: qr_svg(&c.uri), uri: c.uri, inbox_key: c.inbox_key });
        }
        let name = a.my_name(Some(&owner)).map_err(said)?;
        let h = a.issue_card(name.as_deref(), 60 * 60 * 24 * 30, "donate", Some(&owner)).map_err(said)?;
        Ok(Code { svg: qr_svg(&h.uri), uri: h.uri, inbox_key: h.inbox_key })
    })
    .await
    .map_err(s)?
}


// ----- attachments ---------------------------------------------------------------

#[tauri::command]
async fn send_attachment(persona_hex: String, path: String, caption: Option<String>) -> Result<(), String> {
    let a = app()?;
    tauri::async_runtime::spawn_blocking(move || a.send_attachment(&persona_hex, std::path::Path::new(&path), caption.as_deref()).map_err(said))
        .await
        .map_err(s)?
}

/// Where an attachment's plaintext is, if it has arrived.
#[tauri::command]
fn attachment_path(ct_hash_hex: String) -> Result<Option<String>, String> {
    let a = app()?;
    let p = a.attachment_file(&ct_hash_hex);
    Ok(p.exists().then(|| p.to_string_lossy().into_owned()))
}

#[tauri::command]
async fn fetch_swarm_attachment(persona_hex: String, seq: u64, outgoing: bool) -> Result<String, String> {
    let a = app()?;
    tauri::async_runtime::spawn_blocking(move || a.fetch_swarm_attachment(&persona_hex, seq, outgoing).map(|p| p.to_string_lossy().into_owned()).map_err(said))
        .await
        .map_err(s)?
}


#[derive(Serialize)]
struct Presented {
    code: Code,
    tab: TabRow,
}

/// A sale: the tab first, bound to the card it shows; the lap bills
/// whoever answers, whatever the screen is doing by then.
#[tauri::command]
async fn present_sale(lines: Vec<(String, u64)>, tax_pxmr: Option<u64>) -> Result<Presented, String> {
    let a = app()?;
    tauri::async_runtime::spawn_blocking(move || {
        let items: Vec<ducat_app::contacts::BillItem> = lines.into_iter().map(|(d, a)| ducat_app::contacts::BillItem { description: d, amount_pxmr: a }).collect();
        let (h, tab) = a.present_sale(items, tax_pxmr).map_err(said)?;
        Ok(Presented { code: Code { svg: qr_svg(&h.uri), uri: h.uri, inbox_key: h.inbox_key }, tab: tab_row(a, tab) })
    })
    .await
    .map_err(s)?
}

#[tauri::command]
fn sales_in_progress() -> Result<Vec<(String, TabRow)>, String> {
    let a = app()?;
    Ok(a.sales_in_progress().into_iter().map(|(i, t)| (i, tab_row(a, t))).collect())
}


// ----- calls -------------------------------------------------------------------

#[derive(Serialize)]
struct CallView {
    state: ducat_app::calls::CallState,
    contact_name: Option<String>,
    rx_frames: u64,
    tx_frames: u64,
    has_audio: bool,
}

#[tauri::command]
fn call_state() -> Result<CallView, String> {
    let a = app()?;
    let cs = ducat_app::calls::calls();
    let state = cs.state();
    Ok(CallView {
        contact_name: state.contact_hex().and_then(|h| a.contact(h)).map(|c| c.display_name()),
        state,
        rx_frames: cs.rx_frames.load(std::sync::atomic::Ordering::Relaxed),
        tx_frames: cs.tx_frames.load(std::sync::atomic::Ordering::Relaxed),
        has_audio: cs.has_audio(),
    })
}

#[tauri::command]
async fn place_call(persona_hex: String) -> Result<(), String> {
    let a = app()?;
    tauri::async_runtime::spawn_blocking(move || a.place_call(&persona_hex).map_err(said)).await.map_err(s)?
}

#[tauri::command]
async fn answer_call() -> Result<(), String> {
    let a = app()?;
    tauri::async_runtime::spawn_blocking(move || a.answer_call().map_err(said)).await.map_err(s)?
}

#[tauri::command]
async fn decline_call() -> Result<(), String> {
    let a = app()?;
    tauri::async_runtime::spawn_blocking(move || a.decline_call().map_err(said)).await.map_err(s)?
}

#[tauri::command]
async fn hang_up() -> Result<(), String> {
    let a = app()?;
    tauri::async_runtime::spawn_blocking(move || a.hang_up()).await.map_err(s)
}

#[tauri::command]
fn dismiss_call() -> Result<(), String> {
    app()?.dismiss_call();
    Ok(())
}

/// The sound devices a call uses: the machine's when the desk was built
/// with sound, a test tone when asked for one (debug runs), none
/// otherwise — and then calls say so.
fn wire_audio() {
    #[cfg(debug_assertions)]
    if std::env::var_os("DUCAT_DESK_TONE_AUDIO").is_some() {
        ducat_app::calls::calls().set_audio(Box::new(ducat_app::calls::ToneAudio::new(440.0)));
        ducat_app::log::info("Desk", "calls will use a test tone, not the microphone");
        return;
    }
    #[cfg(feature = "sound")]
    {
        ducat_app::calls::calls().set_audio(Box::new(ducat_app::audio::DeviceAudio::new()));
        ducat_app::log::info("Desk", "calls use the machine's sound devices");
        return;
    }
    #[allow(unreachable_code)]
    match ducat_app::audio_proc::ProcessAudio::detect() {
        Some(a) => {
            ducat_app::log::info("Desk", &format!("calls use {} through its command-line tools", a.tool().name()));
            ducat_app::calls::calls().set_audio(Box::new(a));
        }
        None => ducat_app::log::warn("Desk", "no sound tools found; calls will connect without audio"),
    }
}


/// Notices queued by the app since the last take: a message, money, a
/// call. The window shows them as the platform does.
#[tauri::command]
fn take_notices() -> Vec<ducat_app::notify::Notice> {
    ducat_app::notify::take()
}


// ----- the kiosk: orders --------------------------------------------------------

#[derive(Serialize)]
struct OrderRow {
    id: String,
    number: u32,
    lines: Vec<(String, u64)>,
    total_pxmr: u64,
    tax_pxmr: Option<u64>,
    address: String,
    pay_uri: String,
    pay_svg: String,
    state: ducat_app::orders::OrderState,
    placed_at: u64,
    ready_at: u64,
    customer: Option<String>,
    card: Option<String>,
    card_svg: Option<String>,
    shown: ducat_app::wallet::Shown,
}

fn order_row(a: &App, o: ducat_app::orders::Order) -> OrderRow {
    OrderRow {
        state: a.order_state(&o),
        customer: o.persona_hex.as_deref().and_then(|h| a.contact(h)).map(|c| c.display_name()),
        pay_uri: o.pay_uri(),
        pay_svg: if o.address.is_empty() { String::new() } else { qr_svg(&o.pay_uri()) },
        card_svg: o.card.as_deref().map(qr_svg),
        shown: a.show_amount(o.total_pxmr),
        lines: o.lines.iter().map(|l| (l.description.clone(), l.amount_pxmr)).collect(),
        id: o.id,
        number: o.number,
        total_pxmr: o.total_pxmr,
        tax_pxmr: o.tax,
        address: o.address,
        placed_at: o.placed_at,
        ready_at: o.ready_at,
        card: o.card,
    }
}

#[tauri::command]
fn orders() -> Result<Vec<OrderRow>, String> {
    let a = app()?;
    Ok(a.orders().into_iter().map(|o| order_row(a, o)).collect())
}

#[tauri::command]
async fn place_order(lines: Vec<(String, u64)>, tax_pxmr: Option<u64>, with_card: bool) -> Result<OrderRow, String> {
    let a = app()?;
    tauri::async_runtime::spawn_blocking(move || {
        let items: Vec<ducat_app::contacts::BillItem> = lines.into_iter().map(|(d, a)| ducat_app::contacts::BillItem { description: d, amount_pxmr: a }).collect();
        let mut o = a.place_order(items, tax_pxmr).map_err(said)?;
        if with_card {
            o = a.order_card(&o.id).map_err(said)?;
        }
        Ok(order_row(a, o))
    })
    .await
    .map_err(s)?
}

#[tauri::command]
async fn order_card(id: String) -> Result<OrderRow, String> {
    let a = app()?;
    tauri::async_runtime::spawn_blocking(move || a.order_card(&id).map(|o| order_row(a, o)).map_err(said)).await.map_err(s)?
}

#[tauri::command]
async fn abandon_order(id: String) -> Result<(), String> {
    let a = app()?;
    tauri::async_runtime::spawn_blocking(move || a.abandon_order(&id).map_err(said)).await.map_err(s)?
}

#[tauri::command]
async fn say_ready(id: String) -> Result<(), String> {
    let a = app()?;
    tauri::async_runtime::spawn_blocking(move || a.say_ready(&id).map_err(said)).await.map_err(s)?
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

// ----- the drive: a dev-only hand on the window -----------------------------
//
// With DUCAT_DESK_DRIVE=<dir> set, a debug build evaluates every `*.js`
// file dropped into that directory inside the page, in name order, and
// deletes it; `drive_report` lets the page write what it saw back to
// `<dir>/report.txt`. It exists because the window cannot be driven from
// outside — X11 pointer warps are ignored under a Wayland session — and a
// walk of every screen after every change is how the desk is tested here.
// Compiled out of release builds: a shipped desk has no such door.

#[tauri::command]
fn drive_report(text: String) {
    let Ok(dir) = std::env::var("DUCAT_DESK_DRIVE") else { return };
    use std::io::Write;
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(std::path::Path::new(&dir).join("report.txt"))
    {
        let _ = writeln!(f, "{ms}|{text}");
    }
}

#[cfg(debug_assertions)]
fn start_drive(handle: tauri::AppHandle) {
    let Ok(dir) = std::env::var("DUCAT_DESK_DRIVE") else { return };
    ducat_app::log::info("Drive", format!("watching {dir}"));
    std::thread::Builder::new()
        .name("desk-drive".into())
        .spawn(move || loop {
            let mut files: Vec<std::path::PathBuf> = std::fs::read_dir(&dir)
                .map(|rd| {
                    rd.flatten()
                        .map(|e| e.path())
                        .filter(|p| p.extension().map_or(false, |x| x == "js"))
                        .collect()
                })
                .unwrap_or_default();
            files.sort();
            // The page knows it is being driven, so a screen may offer a
            // typed path where a person would get the file picker.
            if let Some(w) = handle.get_webview_window("main") {
                let _ = w.eval("window.__DUCAT_DRIVE = true;");
            }
            for p in files {
                let js = std::fs::read_to_string(&p).unwrap_or_default();
                let _ = std::fs::remove_file(&p);
                match handle.get_webview_window("main") {
                    Some(w) => {
                        if let Err(e) = w.eval(&js) {
                            ducat_app::log::warn("Drive", format!("{}: {e}", p.display()));
                        }
                    }
                    None => ducat_app::log::warn("Drive", "no main window yet"),
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(250));
        })
        .ok();
}

#[cfg(not(debug_assertions))]
fn start_drive(_: tauri::AppHandle) {}

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

    // The lap: answers to our cards, everyone's log, slot insurance, and
    // once an hour every kept site and release put back on the network —
    // a desk that stopped serving what it promised on every reboot is not
    // a mirror.
    a.start_lap();
    wire_audio();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .setup(|app| {
            start_drive(app.handle().clone());
            Ok(())
        })
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
            drive_report,
            personas,
            wear,
            create_persona,
            set_my_name,
            profile_code,
            contacts,
            claim_card,
            thread,
            send_text,
            mark_seen,
            set_petname,
            remove_contact,
            unread_threads,
            generation,
            poll_now,
            wallet_status,
            wallet_notes,
            wallet_sends,
            wallet_quote,
            wallet_send,
            wallet_max,
            set_own_node,
            wallet_rescan,
            wallet_step,
            tabs,
            open_tab,
            tab_add_line,
            tab_remove_line,
            tab_set_tax,
            settle_tab,
            cancel_tab,
            tab_paid_outside,
            tab_send_receipt,
            delete_tab,
            sale_card,
            card_claimant,
            catalogue,
            put_item,
            remove_item,
            fiat_to_pxmr,
            show_amount,
            pay_bill,
            publications,
            create_publication,
            delete_publication,
            set_publication_price,
            set_subscriber,
            publish_issue,
            press_code,
            subscriptions,
            fetch_issue,
            refresh_shelf,
            ask_for_period,
            set_mirroring,
            set_muted,
            groups,
            create_group,
            add_to_group,
            group_thread,
            send_group,
            mark_group_seen,
            backup_info,
            export_backup,
            import_backup,
            listings,
            save_listing,
            remove_listing,
            post_listing,
            unpost_listing,
            add_listing_photo,
            remove_listing_photo,
            set_listing_cover,
            browse_cached,
            browse,
            fetch_gallery,
            picture_data_url,
            enquiry_about,
            ledger,
            export_ledger,
            request_payment,
            standing_bills,
            add_standing_bill,
            stop_standing_bill,
            donate_code,
            send_attachment,
            attachment_path,
            fetch_swarm_attachment,
            present_sale,
            sales_in_progress,
            call_state,
            place_call,
            answer_call,
            decline_call,
            hang_up,
            dismiss_call,
            react,
            retract_message,
            delete_message,
            delete_thread,
            disappear_after,
            set_disappear_after,
            draft,
            save_draft,
            set_chat_visible,
            take_notices,
            orders,
            place_order,
            order_card,
            abandon_order,
            say_ready,
        ])
        .run(tauri::generate_context!())
        .expect("error while running the desk");
}
