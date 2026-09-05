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
    let last = thread.iter().rev().find(|r| r.surfaces());
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
}

fn message_row(m: StoredMessage) -> MessageRow {
    MessageRow {
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
    }
}

#[tauri::command]
fn thread(persona_hex: String) -> Result<Vec<MessageRow>, String> {
    let a = app()?;
    Ok(a.thread(&persona_hex).into_iter().filter(|m| m.surfaces()).map(message_row).collect())
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
        name: a.contact(&t.persona_hex).map(|c| c.display_name()).unwrap_or_else(|| "(gone)".into()),
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

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
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
        ])
        .run(tauri::generate_context!())
        .expect("error while running the desk");
}
