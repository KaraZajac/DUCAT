//! Listings (§16.18): things for rent, for sale, or for hire, posted as
//! notices on the local boards for a day at a time, found by anyone
//! browsing that part of the map. The phone's `Listings.kt` and
//! `Enquiries.kt`.
//!
//! A listing is private until posted. Posting cuts a card whose purpose
//! is "rental", seals a notice against a fresh block, and takes a free
//! slot on this week's board for the listing's cell — climbing shards when
//! the first is full. Browsing reads the home cell and its eight
//! neighbours, paints from what it remembered while it looks, and climbs
//! the ladder on boards that were full.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Mutex;

use ducat_mobile::contacts::{geohashEncode, geohashNeighbors, rental_decode, rental_encode, RentalInfo};
use ducat_mobile::node::{stand_post, stand_read};
use ducat_mobile::swarm;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::boards::{budget, max_notice_ttl_secs, max_stand_shards, stand_now, stand_shard, stand_stale, Verdict};
use crate::contacts::{b64, bump, now_ms, unb64};
use crate::thumbs::THUMB_BYTES;
use crate::{log, App, Error};

const TAG: &str = "Listings";
const STORE: &str = "ducat_listings";
const CACHE_STORE: &str = "ducat_board_cache";
pub const TTL_SECONDS: u64 = 24 * 60 * 60;
pub const REFRESH_SECONDS: u64 = 6 * 60 * 60;
pub const RETRY_SECONDS: u64 = 30 * 60;
pub const CELL_PRECISION: u32 = 5;
const LADDER_RUNG: u32 = 3;
const CACHE_TTL_MS: u64 = 6 * 60 * 60 * 1000;
const CACHE_KEEP: usize = 48;
pub const MAX_PHOTOS: usize = 8;
pub const MAX_QUANTITY: u64 = 999;

pub const KIND_PLACE: u32 = 1;
pub const KIND_VEHICLE: u32 = 2;
pub const KIND_SALE: u32 = 3;
pub const KIND_GEAR: u32 = 4;
pub const KIND_SKILL: u32 = 5;
pub const KINDS: [u32; 5] = [KIND_PLACE, KIND_VEHICLE, KIND_GEAR, KIND_SALE, KIND_SKILL];

/// §15.13's stakes: what a counterparty locks, by the kind of deal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Deal {
    Ride,
    Stay,
    Vehicle,
    Sale,
    Labour,
}

impl Deal {
    pub fn percent(self) -> u64 {
        match self {
            Deal::Ride => 10,
            Deal::Stay => 20,
            Deal::Vehicle => 30,
            Deal::Sale => 10,
            Deal::Labour => 10,
        }
    }
}

pub const STAKE_FLOOR_PXMR: u64 = 400_000_000;
const MAX_STAKE_PERCENT: u64 = 50;

pub fn stake_for(deal: Deal, amount_pxmr: u64) -> u64 {
    let p = deal.percent().min(MAX_STAKE_PERCENT);
    if amount_pxmr == 0 || p == 0 {
        return 0;
    }
    let raw = amount_pxmr / 100 * p + (amount_pxmr % 100) * p / 100;
    if raw >= STAKE_FLOOR_PXMR {
        raw
    } else if amount_pxmr >= STAKE_FLOOR_PXMR * 2 {
        STAKE_FLOOR_PXMR
    } else {
        0
    }
}

pub fn deal_for(kind: u32) -> Deal {
    match kind {
        KIND_VEHICLE | KIND_GEAR => Deal::Vehicle,
        KIND_SALE => Deal::Sale,
        KIND_SKILL => Deal::Labour,
        _ => Deal::Stay,
    }
}

pub fn subtype_top(kind: u32) -> u64 {
    match kind {
        KIND_PLACE => 2,
        KIND_VEHICLE => 3,
        KIND_SALE => 9,
        KIND_GEAR => 5,
        KIND_SKILL => 12,
        _ => 0,
    }
}

pub fn kind_name(kind: u32) -> &'static str {
    match kind {
        KIND_PLACE => "place",
        KIND_VEHICLE => "vehicle",
        KIND_SALE => "for sale",
        KIND_GEAR => "gear",
        KIND_SKILL => "skill",
        _ => "listing",
    }
}

fn board_name(cell: &str) -> String {
    format!("local:{cell}")
}

/// A listing as kept — the phone's JSON keys.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Listing {
    pub id: String,
    #[serde(default)]
    pub kind: u32,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub area: String,
    #[serde(default)]
    pub cell: String,
    #[serde(rename = "pricePxmr", default)]
    pub price_pxmr: u64,
    #[serde(rename = "depositPxmr", default)]
    pub deposit_pxmr: u64,
    #[serde(default)]
    pub specs: Map<String, Value>,
    #[serde(rename = "private", default)]
    pub private_details: String,
    #[serde(default = "one")]
    pub quantity: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thumb: Option<String>,
    #[serde(default)]
    pub created: u64,
    #[serde(default)]
    pub owner: String,
    #[serde(rename = "priceTyped", default, skip_serializing_if = "Option::is_none")]
    pub price_typed: Option<String>,
    #[serde(rename = "priceCurrency", default, skip_serializing_if = "Option::is_none")]
    pub price_currency: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gallery: Option<String>,
    #[serde(rename = "gallery_dig", default, skip_serializing_if = "Option::is_none")]
    pub gallery_dig: Option<String>,
    // Tenancy on the board.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub board: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subkey: Option<u32>,
    #[serde(rename = "postedAt", default)]
    pub posted_at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub card: Option<String>,
    #[serde(default)]
    pub cards: Vec<String>,
    #[serde(default)]
    pub wanted: bool,
    #[serde(rename = "triedAt", default)]
    pub tried_at: u64,
}

fn one() -> u64 {
    1
}

impl Listing {
    pub fn posted(&self) -> bool {
        self.board.as_deref().map_or(false, |b| !b.is_empty())
    }

    fn still_held(&self, now: u64) -> bool {
        now.saturating_sub(self.posted_at) < TTL_SECONDS - 3600
    }

    pub fn thumb_bytes(&self) -> Option<Vec<u8>> {
        let raw = self.thumb.as_deref().filter(|s| !s.is_empty())?;
        let bytes = unb64(raw)?;
        if bytes.is_empty() || bytes.len() > THUMB_BYTES {
            log::warn(TAG, format!("dropping a {}-byte thumbnail: over the board's cap", bytes.len()));
            return None;
        }
        Some(bytes)
    }

    /// What the board gets: the public half, with the card that answers.
    pub fn public_notice(&self, card: &str) -> RentalInfo {
        let txt = |k: &str| self.specs.get(k).and_then(Value::as_str).filter(|s| !s.trim().is_empty()).map(String::from);
        let num = |k: &str| self.specs.get(k).and_then(Value::as_u64).filter(|n| *n > 0);
        let vehicle = self.kind == KIND_VEHICLE;
        let place = self.kind == KIND_PLACE;
        RentalInfo {
            poster: String::new(),
            beacon_height: 0,
            beacon_hash: String::new(),
            card: card.to_string(),
            kind: self.kind as u64,
            title: self.title.clone(),
            area: self.area.clone(),
            cell: Some(self.cell.clone()).filter(|c| !c.is_empty()),
            price_pxmr: self.price_pxmr,
            deposit_pxmr: self.deposit_pxmr,
            expiry: App::now() + TTL_SECONDS,
            make: if vehicle { txt("make") } else { None },
            model: if vehicle { txt("model") } else { None },
            year: if vehicle { num("year") } else { None },
            gearbox: if vehicle { num("gearbox") } else { None },
            fuel: if vehicle { num("fuel") } else { None },
            seats: if vehicle { num("seats") } else { None },
            color: if vehicle { txt("color") } else { None },
            trim: if vehicle { txt("trim") } else { None },
            rooms: if place { num("rooms") } else { None },
            sleeps: if place { num("sleeps") } else { None },
            size_m2: if place { num("size_m2") } else { None },
            subtype: num("subtype"),
            features: self.specs.get("features").and_then(Value::as_array).map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect()).unwrap_or_default(),
            quantity: if self.kind == KIND_SKILL { 1 } else { self.quantity.clamp(1, MAX_QUANTITY) },
            thumb: self.thumb_bytes(),
            gallery_share: self.gallery.clone().filter(|g| !g.is_empty() && self.gallery_dig.as_deref().map_or(false, |d| !d.is_empty())),
            gallery_digest: self.gallery_dig.clone().filter(|d| !d.is_empty() && self.gallery.as_deref().map_or(false, |g| !g.is_empty())),
        }
    }
}

/// What a browse found: one notice, kept as the phone caches it.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Found {
    pub poster: String,
    pub card: String,
    pub kind: u64,
    pub title: String,
    #[serde(default)]
    pub area: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cell: Option<String>,
    pub price: u64,
    #[serde(default)]
    pub deposit: u64,
    pub expiry: u64,
    #[serde(default)]
    pub specs: Map<String, Value>,
    #[serde(default)]
    pub features: Vec<String>,
    #[serde(default = "one")]
    pub quantity: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thumb: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gallery: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gallery_dig: Option<String>,
}

impl From<RentalInfo> for Found {
    fn from(r: RentalInfo) -> Self {
        let mut specs = Map::new();
        let mut put_s = |k: &str, v: &Option<String>| {
            if let Some(v) = v {
                specs.insert(k.into(), Value::from(v.clone()));
            }
        };
        put_s("make", &r.make);
        put_s("model", &r.model);
        put_s("color", &r.color);
        put_s("trim", &r.trim);
        let mut put_n = |k: &str, v: Option<u64>| {
            if let Some(v) = v {
                specs.insert(k.into(), Value::from(v));
            }
        };
        put_n("year", r.year);
        put_n("gearbox", r.gearbox);
        put_n("fuel", r.fuel);
        put_n("seats", r.seats);
        put_n("rooms", r.rooms);
        put_n("sleeps", r.sleeps);
        put_n("size_m2", r.size_m2);
        put_n("subtype", r.subtype);
        Found {
            poster: r.poster,
            card: r.card,
            kind: r.kind,
            title: r.title,
            area: r.area,
            cell: r.cell,
            price: r.price_pxmr,
            deposit: r.deposit_pxmr,
            expiry: r.expiry,
            specs,
            features: r.features,
            quantity: r.quantity,
            thumb: r.thumb.as_deref().map(b64),
            gallery: r.gallery_share,
            gallery_dig: r.gallery_digest,
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct CachedCell {
    at: u64,
    rows: Vec<Found>,
}

/// What a listing's card answered to — kept beside the contact, so a
/// thread born from a board knows what it is about.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Enquiry {
    pub title: String,
    #[serde(rename = "price")]
    pub price_pxmr: u64,
    #[serde(rename = "deposit", default)]
    pub deposit_pxmr: u64,
    pub kind: u32,
    #[serde(rename = "listing", default)]
    pub listing_id: String,
}

static LISTINGS: Mutex<()> = Mutex::new(());
static POSTING: Mutex<Option<HashSet<String>>> = Mutex::new(None);

fn new_id() -> String {
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    format!("{:x}-{:x}-{:x}", now_ms(), std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::Relaxed))
}

impl App {
    // ----- the store ----------------------------------------------------------------

    pub fn listings(&self) -> Vec<Listing> {
        self.store(STORE).get("listings").unwrap_or_default()
    }

    pub fn listing(&self, id: &str) -> Option<Listing> {
        self.listings().into_iter().find(|l| l.id == id)
    }

    fn save_listings(&self, items: &[Listing]) -> Result<(), Error> {
        self.store(STORE).put("listings", &items)?;
        bump();
        Ok(())
    }

    pub fn put_listing(&self, l: Listing) -> Result<(), Error> {
        let _g = LISTINGS.lock().unwrap_or_else(|e| e.into_inner());
        let mut cur = self.listings();
        match cur.iter_mut().find(|x| x.id == l.id) {
            Some(slot) => *slot = l,
            None => cur.push(l),
        }
        self.save_listings(&cur)
    }

    fn edit_listing<F: FnOnce(&mut Listing)>(&self, id: &str, f: F) -> Result<Option<Listing>, Error> {
        let _g = LISTINGS.lock().unwrap_or_else(|e| e.into_inner());
        let mut cur = self.listings();
        let Some(l) = cur.iter_mut().find(|x| x.id == id) else { return Ok(None) };
        f(l);
        let out = l.clone();
        self.save_listings(&cur)?;
        Ok(Some(out))
    }

    /// Keep a draft, carrying the board tenancy of the listing it replaces.
    pub fn put_draft(&self, mut draft: Listing) -> Result<(), Error> {
        if let Some(cur) = self.listing(&draft.id) {
            draft.board = draft.board.or(cur.board);
            draft.subkey = draft.subkey.or(cur.subkey);
            if draft.posted_at == 0 {
                draft.posted_at = cur.posted_at;
            }
            draft.card = draft.card.or(cur.card);
            if draft.cards.is_empty() {
                draft.cards = cur.cards;
            }
            draft.wanted = draft.wanted || cur.wanted;
            if draft.tried_at == 0 {
                draft.tried_at = cur.tried_at;
            }
            if draft.owner.is_empty() {
                draft.owner = cur.owner;
            }
        }
        self.put_listing(draft)
    }

    pub fn remove_listing(&self, id: &str) -> Result<(), Error> {
        self.stop_gallery(id);
        let _ = std::fs::remove_dir_all(self.photo_dir(id));
        let _g = LISTINGS.lock().unwrap_or_else(|e| e.into_inner());
        let cur: Vec<Listing> = self.listings().into_iter().filter(|l| l.id != id).collect();
        self.save_listings(&cur)
    }

    /// A fresh draft. `lat_e7`/`lon_e7` place it on the map; a desk with no
    /// GPS takes them typed, or a geohash cell directly.
    pub fn draft_listing(&self, kind: u32, title: &str, area: &str, price_pxmr: u64, cell: &str, specs: Map<String, Value>, private_details: &str, price_typed: Option<&str>, price_currency: Option<&str>, quantity: u64, thumb: Option<&[u8]>) -> Result<Listing, Error> {
        let clean = |s: &str| ducat_mobile::contacts::clean_display_text(s.trim().to_string());
        Ok(Listing {
            id: new_id(),
            kind,
            title: clean(title),
            area: clean(area),
            cell: cell.trim().to_lowercase(),
            price_pxmr,
            deposit_pxmr: stake_for(deal_for(kind), price_pxmr),
            specs,
            private_details: private_details.to_string(),
            quantity: quantity.clamp(1, MAX_QUANTITY),
            thumb: thumb.map(b64),
            created: App::now(),
            owner: self.worn()?,
            price_typed: price_typed.map(String::from).filter(|_| price_currency.is_some()),
            price_currency: price_currency.map(String::from).filter(|_| price_typed.is_some()),
            gallery: None,
            gallery_dig: None,
            board: None,
            subkey: None,
            posted_at: 0,
            card: None,
            cards: Vec::new(),
            wanted: false,
            tried_at: 0,
        })
    }

    pub fn cell_of(lat_e7: i64, lon_e7: i64) -> Option<String> {
        geohashEncode(lat_e7, lon_e7, CELL_PRECISION).ok()
    }

    pub fn set_listing_quantity(&self, id: &str, n: u64) -> Result<(), Error> {
        self.edit_listing(id, |l| l.quantity = n.clamp(1, MAX_QUANTITY)).map(|_| ())
    }

    // ----- photos and the gallery -------------------------------------------------------

    pub fn photo_dir(&self, listing_id: &str) -> PathBuf {
        let safe: String = listing_id.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_').take(64).collect();
        self.files_dir().join("listing_photos").join(if safe.is_empty() { "unnamed".into() } else { safe })
    }

    pub fn photos(&self, listing_id: &str) -> Vec<PathBuf> {
        let mut v: Vec<PathBuf> = std::fs::read_dir(self.photo_dir(listing_id)).map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.is_file()).collect()).unwrap_or_default();
        v.sort();
        v
    }

    /// Add a picture to a listing: copied in under a sortable name, at most
    /// [`MAX_PHOTOS`]. Returns the new count.
    pub fn add_photo(&self, listing_id: &str, source: &std::path::Path) -> Result<usize, Error> {
        let dir = self.photo_dir(listing_id);
        std::fs::create_dir_all(&dir)?;
        let have = self.photos(listing_id);
        if have.len() >= MAX_PHOTOS {
            return Err(Error::Refused(format!("{MAX_PHOTOS} pictures is the most a listing carries")));
        }
        let ext = source.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_else(|| "jpg".into());
        let next = have.len();
        std::fs::copy(source, dir.join(format!("{next:02}.{ext}")))?;
        bump();
        Ok(next + 1)
    }

    pub fn remove_photo(&self, listing_id: &str, index: usize) -> Result<(), Error> {
        let have = self.photos(listing_id);
        if let Some(p) = have.get(index) {
            std::fs::remove_file(p)?;
        }
        bump();
        Ok(())
    }

    /// The board's picture: a thumbnail of one of the listing's photos.
    pub fn set_thumb_from_photo(&self, listing_id: &str, index: usize) -> Result<bool, Error> {
        let have = self.photos(listing_id);
        let Some(p) = have.get(index) else { return Ok(false) };
        let bytes = std::fs::read(p)?;
        let Some(t) = crate::thumbs::thumbnail(&bytes, THUMB_BYTES) else {
            return Err(Error::Refused("that picture could not be shrunk to a thumbnail".into()));
        };
        self.edit_listing(listing_id, |l| l.thumb = Some(b64(&t)))?;
        Ok(true)
    }

    fn seed_gallery(&self, listing_id: &str) -> Option<(String, String)> {
        let dir = self.photo_dir(listing_id);
        if self.photos(listing_id).is_empty() {
            return None;
        }
        match swarm::swarm_seed(dir.to_string_lossy().into_owned()) {
            Ok(share) => {
                log::info(TAG, format!("gallery of {}… serving at {} digest {}", &listing_id[..8.min(listing_id.len())], share.share_key, share.index_digest_hex));
                Some((share.share_key, share.index_digest_hex))
            }
            Err(e) => {
                log::warn(TAG, format!("gallery of {}…: {e}", &listing_id[..8.min(listing_id.len())]));
                None
            }
        }
    }

    /// Put a gallery back on the network after a restart: stop the share
    /// and re-park it with the seeder kept, never a one-shot seed.
    pub fn reseed_gallery(&self, listing_id: &str) {
        let Some(l) = self.listing(listing_id) else { return };
        let (Some(share), Some(digest)) = (l.gallery.clone(), l.gallery_dig.clone()) else { return };
        if self.photos(listing_id).is_empty() {
            log::info(TAG, format!("gallery of {}… has no pictures left to serve", &listing_id[..8.min(listing_id.len())]));
            return;
        }
        let dir = self.photo_dir(listing_id);
        swarm::swarm_stop_share(share.clone());
        match swarm::swarm_fetch(share, digest, dir.to_string_lossy().into_owned(), true) {
            Ok(_) => log::info(TAG, format!("gallery of {}… serving again", &listing_id[..8.min(listing_id.len())])),
            Err(e) => log::warn(TAG, format!("gallery of {}…: {e}", &listing_id[..8.min(listing_id.len())])),
        }
    }

    fn stop_gallery(&self, listing_id: &str) {
        if let Some(share) = self.listing(listing_id).and_then(|l| l.gallery).filter(|g| !g.is_empty()) {
            swarm::swarm_stop_share(share);
        }
    }

    /// Where a browsed listing's gallery lands, and the fetch itself.
    pub fn gallery_dir(&self, digest_hex: &str) -> PathBuf {
        self.files_dir().join("galleries").join(digest_hex)
    }

    pub fn fetch_gallery(&self, share: &str, digest_hex: &str) -> Result<PathBuf, Error> {
        let dir = self.gallery_dir(digest_hex);
        if crate::has_any_file(&dir) {
            return Ok(dir);
        }
        let part = dir.with_extension("part");
        let _ = std::fs::remove_dir_all(&part);
        std::fs::create_dir_all(&part)?;
        swarm::swarm_fetch(share.to_string(), digest_hex.to_string(), part.to_string_lossy().into_owned(), false)?;
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::rename(&part, &dir)?;
        Ok(dir)
    }

    // ----- pricing ---------------------------------------------------------------------

    /// A listing priced in fiat follows the rate: re-derive the XMR price
    /// before it is posted.
    fn reprice(&self, l: Listing) -> Listing {
        let (Some(typed), Some(cur)) = (l.price_typed.clone(), l.price_currency.clone()) else { return l };
        if !self.rate_enabled() || !self.rate_currency().eq_ignore_ascii_case(&cur) {
            return l;
        }
        let Some((rate, _)) = self.rate_cached_pair().filter(|(r, _)| *r > 0.0) else { return l };
        let Some(pxmr) = self.fiat_to_pxmr(&typed, rate) else { return l };
        if pxmr == l.price_pxmr {
            return l;
        }
        let mut next = l.clone();
        next.price_pxmr = pxmr;
        next.deposit_pxmr = stake_for(deal_for(l.kind), pxmr);
        let _ = self.put_listing(next.clone());
        log::info(TAG, format!("{}: {typed} {cur} is now {} XMR", l.title, crate::wallet::format_xmr(pxmr)));
        next
    }

    // ----- posting ------------------------------------------------------------------------

    /// Put a listing on this week's board for its cell. True when it took
    /// a slot; false when every shard was full.
    pub fn post_listing(&self, id: &str) -> Result<bool, Error> {
        {
            let mut g = POSTING.lock().unwrap_or_else(|e| e.into_inner());
            let set = g.get_or_insert_with(HashSet::new);
            if !set.insert(id.to_string()) {
                return Ok(false);
            }
        }
        let r = self.post_locked(id);
        if let Some(set) = POSTING.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
            set.remove(id);
        }
        r
    }

    fn post_locked(&self, id: &str) -> Result<bool, Error> {
        let now = App::now();
        self.edit_listing(id, |l| {
            l.wanted = true;
            l.tried_at = now;
        })?;
        let Some(l) = self.listing(id) else { return Ok(false) };
        let mut o = self.reprice(l);
        if o.gallery.as_deref().map_or(true, str::is_empty) {
            if let Some((share, digest)) = self.seed_gallery(id) {
                self.edit_listing(id, |l| {
                    l.gallery = Some(share.clone());
                    l.gallery_dig = Some(digest.clone());
                })?;
                o.gallery = Some(share);
                o.gallery_dig = Some(digest);
            }
        }
        if o.cell.is_empty() {
            return Err(Error::Refused("this listing has no area yet".into()));
        }
        let owner_hex = if o.owner.is_empty() {
            let h = self.primary_hex()?;
            self.edit_listing(id, |l| l.owner = h.clone())?;
            h
        } else {
            o.owner.clone()
        };
        let name = self.my_name(Some(&owner_hex))?;
        let card = self.issue_card(name.as_deref(), TTL_SECONDS, "rental", Some(&owner_hex))?;
        let persona = match self.persona_secret(&owner_hex)? {
            Some(s) => s,
            None => self.primary_secret()?,
        };
        let beacon = self.stamp_now().ok_or_else(|| Error::Refused("no recent Monero block to stamp a notice against — the wallet's node has not answered yet".into()))?;
        // The notice is rebuilt per seal: the bridge's record does not clone.
        let seal = |board: &str, slot: u32| -> Result<Vec<u8>, Error> {
            Ok(rental_encode(o.public_notice(&card.uri), persona.clone(), id.to_string(), board.to_string(), slot, beacon.height, beacon.hash_hex.clone())?)
        };
        let mut placed: Option<(String, u32)> = None;
        if let (Some(existing), Some(slot)) = (o.board.clone().filter(|b| !b.is_empty() && !stand_stale(b) && o.still_held(now)), o.subkey) {
            if stand_post(existing.clone(), slot, seal(&existing, slot)?).is_ok() {
                placed = Some((existing, slot));
            }
        }
        if placed.is_none() {
            let tip = self.beacon_tip();
            'ladder: for shard in 0..max_stand_shards() {
                let Some(name) = stand_shard(&stand_now(&board_name(&o.cell)), shard) else { continue };
                let taken: HashSet<u32> = stand_read(name.clone())
                    .unwrap_or_default()
                    .into_iter()
                    .filter_map(|n| rental_decode(n.data, name.clone(), n.subkey, tip).ok().filter(|d| d.expiry > now).map(|_| n.subkey))
                    .collect();
                for free in 0..8u32 {
                    if taken.contains(&free) {
                        continue;
                    }
                    if stand_post(name.clone(), free, seal(&name, free)?).is_ok() {
                        placed = Some((name, free));
                        break 'ladder;
                    }
                }
            }
        }
        let Some((board, slot)) = placed else {
            log::warn(TAG, format!("every shard of {} is full", board_name(&o.cell)));
            return Ok(false);
        };
        let uri = card.uri.clone();
        let updated = self.edit_listing(id, |l| {
            l.card = Some(uri.clone());
            l.cards.push(uri.clone());
            while l.cards.len() > 8 {
                l.cards.remove(0);
            }
            l.board = Some(board.clone());
            l.subkey = Some(slot);
            l.posted_at = now;
        })?;
        if updated.is_none() {
            let _ = stand_post(board.clone(), slot, Vec::new());
            log::info(TAG, format!("listing {id} was removed while posting; slot cleared"));
            return Ok(false);
        }
        log::info(TAG, format!("listing {} posted to {board}/{slot}", o.title));
        Ok(true)
    }

    /// Take a listing off the board (the slot is cleared while it is ours).
    pub fn unpost_listing(&self, id: &str) -> Result<(), Error> {
        let Some(o) = self.listing(id) else { return Ok(()) };
        if let (Some(board), Some(slot)) = (o.board.clone().filter(|b| !b.is_empty()), o.subkey) {
            if !stand_stale(&board) && o.still_held(App::now()) {
                if let Err(e) = stand_post(board, slot, Vec::new()) {
                    log::warn(TAG, format!("clearing slot: {e:?}"));
                }
            }
        }
        self.edit_listing(id, |l| {
            l.board = None;
            l.subkey = None;
            l.posted_at = 0;
            l.card = None;
            l.wanted = false;
            l.tried_at = 0;
        })?;
        Ok(())
    }

    /// Listings whose notice is due again: a posted one every six hours or
    /// when its week ended, a wanted one every half hour until it lands.
    pub fn listings_needing_refresh(&self) -> Vec<Listing> {
        let now = App::now();
        self.listings()
            .into_iter()
            .filter(|l| match l.board.as_deref().filter(|b| !b.is_empty()) {
                None => l.wanted && now.saturating_sub(l.tried_at) >= RETRY_SECONDS,
                Some(b) => now.saturating_sub(l.posted_at) >= REFRESH_SECONDS || stand_stale(b),
            })
            .collect()
    }

    /// The listings' turn on the lap: refresh what is due, link the cards
    /// that were answered to the threads they opened.
    pub fn listings_lap(&self) {
        for l in self.listings_needing_refresh() {
            match self.post_listing(&l.id) {
                Ok(true) => {}
                Ok(false) => log::info(TAG, format!("{}: no slot this time", l.title)),
                Err(e) => log::warn(TAG, format!("refresh {}: {e}", l.title)),
            }
        }
        self.link_claims();
    }

    pub fn reseed_galleries(&self) {
        for l in self.listings() {
            if l.gallery.is_some() && l.wanted {
                self.reseed_gallery(&l.id);
            }
        }
    }

    // ----- enquiries ------------------------------------------------------------------

    pub fn enquiry(&self, persona_hex: &str) -> Option<Enquiry> {
        self.store("ducat_enquiries").get::<Enquiry>(&format!("about_{persona_hex}")).filter(|e| !e.title.is_empty())
    }

    pub fn forget_enquiry(&self, persona_hex: &str) -> Result<(), Error> {
        self.store("ducat_enquiries").remove(&format!("about_{persona_hex}"))?;
        Ok(())
    }

    fn remember_enquiry(&self, persona_hex: &str, about: Enquiry) -> Result<(), Error> {
        if persona_hex.is_empty() {
            return Ok(());
        }
        if let Some(e) = self.enquiry(persona_hex) {
            if e.title == about.title && e.price_pxmr == about.price_pxmr && e.kind == about.kind {
                return Ok(());
            }
        }
        self.store("ducat_enquiries").put(&format!("about_{persona_hex}"), &about)?;
        Ok(())
    }

    /// A rental card that was answered: the listing gets a fresh card on
    /// the board (its old one is spent), and the new contact is remembered
    /// as an enquiry about that listing.
    pub fn link_claims(&self) {
        let store = self.store("ducat_enquiries");
        let answered: Vec<crate::contacts::IssuedCard> = self
            .issued_cards()
            .into_iter()
            .filter(|c| c.purpose == "rental" && c.answered_by.is_some() && !store.get::<bool>(&format!("card:{}", c.uri)).unwrap_or(false))
            .collect();
        if answered.is_empty() {
            return;
        }
        let mut by_card: HashMap<String, Listing> = HashMap::new();
        for l in self.listings() {
            if let Some(c) = l.card.clone().filter(|c| !c.is_empty()) {
                by_card.insert(c, l.clone());
            }
            for c in &l.cards {
                by_card.insert(c.clone(), l.clone());
            }
        }
        for issued in answered {
            let Some(l) = by_card.get(&issued.uri) else { continue };
            if l.card.as_deref() == Some(issued.uri.as_str()) && l.posted() {
                match self.post_listing(&l.id) {
                    Ok(_) => log::info(TAG, format!("{}: fresh card after an enquiry", l.title)),
                    Err(e) => log::warn(TAG, format!("re-post after enquiry: {e}")),
                }
            }
            let _ = store.put(&format!("card:{}", issued.uri), &true);
            if let Some(who) = issued.answered_by.as_deref() {
                let _ = self.remember_enquiry(who, Enquiry { title: l.title.clone(), price_pxmr: l.price_pxmr, deposit_pxmr: l.deposit_pxmr, kind: l.kind, listing_id: l.id.clone() });
            }
        }
    }

    // ----- browsing -------------------------------------------------------------------------

    fn cache_key(cell: &str, kind: Option<u32>) -> String {
        format!("{cell}|{}", kind.map_or(-1, |k| k as i64))
    }

    fn cached_cell(&self, cell: &str, kind: Option<u32>) -> Option<Vec<Found>> {
        let all: BTreeMap<String, CachedCell> = self.store(CACHE_STORE).get("cells").unwrap_or_default();
        let e = all.get(&App::cache_key(cell, kind))?;
        if now_ms().saturating_sub(e.at) >= CACHE_TTL_MS {
            return None;
        }
        let now = App::now();
        Some(e.rows.iter().filter(|r| r.expiry > now).cloned().collect())
    }

    fn remember_cell(&self, cell: &str, kind: Option<u32>, rows: &[Found]) {
        let _ = self.store(CACHE_STORE).update(|m| {
            let mut all: BTreeMap<String, CachedCell> = m.get("cells").cloned().and_then(crate::store::value_as).unwrap_or_default();
            all.insert(App::cache_key(cell, kind), CachedCell { at: now_ms(), rows: rows.to_vec() });
            if all.len() > CACHE_KEEP {
                let mut by_age: Vec<(String, u64)> = all.iter().map(|(k, v)| (k.clone(), v.at)).collect();
                by_age.sort_by(|a, b| a.1.cmp(&b.1));
                for (k, _) in by_age.into_iter().take(all.len() - CACHE_KEEP) {
                    all.remove(&k);
                }
            }
            m.insert("cells".into(), serde_json::to_value(&all).unwrap_or(Value::Null));
        });
    }

    fn read_shard(&self, name: &str, kind: Option<u32>) -> Option<(Vec<Found>, usize)> {
        let now = App::now();
        let ttl_cap = max_notice_ttl_secs();
        let raw = stand_read(name.to_string()).ok()?;
        let slots = raw.len();
        let tip = self.beacon_tip();
        let mut budget = budget();
        let rows: Vec<Found> = raw
            .into_iter()
            .filter_map(|n| rental_decode(n.data, name.to_string(), n.subkey, tip).ok())
            .filter(|d| tip == 0 || self.confirm_beacon(d.beacon_height, &d.beacon_hash, &mut budget) == Verdict::Confirmed)
            .filter(|d| d.expiry > now && d.expiry <= now + ttl_cap)
            .filter(|d| kind.map_or(true, |k| d.kind == k as u64))
            .map(Found::from)
            .collect();
        Some((rows, slots))
    }

    /// One cell's board: the first shard, and when `deep`, up the ladder
    /// three shards at a time until a rung is empty.
    fn read_cell(&self, cell: &str, kind: Option<u32>, deep: bool) -> Option<(Vec<Found>, bool)> {
        let base = stand_now(&board_name(cell));
        let (first, slots) = self.read_shard(&base, kind)?;
        let full = slots >= 8;
        if !deep {
            return Some((first, full));
        }
        let mut out = first;
        let top = max_stand_shards();
        let mut shard = 1;
        while shard < top {
            let mut anything = false;
            for s in shard..(shard + LADDER_RUNG).min(top) {
                let Some(name) = stand_shard(&base, s) else { continue };
                if let Some((live, _)) = self.read_shard(&name, kind) {
                    if !live.is_empty() {
                        anything = true;
                        out.extend(live);
                    }
                }
            }
            if !anything {
                break;
            }
            shard += LADDER_RUNG;
        }
        Some((out, full))
    }

    /// What the cache remembers around a cell — painted while the sweep runs.
    pub fn browse_cached(&self, home: &str, kind: Option<u32>) -> Vec<Found> {
        let mut cells = vec![home.to_string()];
        cells.extend(geohashNeighbors(home.to_string()).unwrap_or_default());
        let mut merged: BTreeMap<String, Found> = BTreeMap::new();
        for c in cells {
            for f in self.cached_cell(&c, kind).unwrap_or_default() {
                merged.entry(f.card.clone()).or_insert(f);
            }
        }
        merged.into_values().collect()
    }

    /// Sweep the home cell and its ring, one thread per cell, climbing the
    /// ladder afterwards on boards that were full. Returns every listing
    /// found, one per card.
    pub fn browse(&self, home: &str, kind: Option<u32>) -> Vec<Found> {
        let started = std::time::Instant::now();
        let mut cells = vec![home.to_string()];
        cells.extend(geohashNeighbors(home.to_string()).unwrap_or_default());
        let attached = self.node_status().public_internet_ready;
        let results: Mutex<Vec<(String, Vec<Found>, bool)>> = Mutex::new(Vec::new());
        std::thread::scope(|s| {
            for cell in cells.iter().cloned() {
                let results = &results;
                let app = self;
                s.spawn(move || {
                    if let Some((rows, full)) = app.read_cell(&cell, kind, false) {
                        results.lock().unwrap_or_else(|e| e.into_inner()).push((cell, rows, full));
                    }
                });
            }
        });
        let mut got = results.into_inner().unwrap_or_else(|e| e.into_inner());
        let crowded: Vec<String> = got.iter().filter(|(_, _, full)| *full).map(|(c, _, _)| c.clone()).collect();
        if !crowded.is_empty() {
            log::info(TAG, format!("climbing the ladder on {} full board(s)", crowded.len()));
            let deeper: Mutex<Vec<(String, Vec<Found>)>> = Mutex::new(Vec::new());
            std::thread::scope(|s| {
                for cell in crowded.iter().cloned() {
                    let deeper = &deeper;
                    let app = self;
                    s.spawn(move || {
                        if let Some((rows, _)) = app.read_cell(&cell, kind, true) {
                            deeper.lock().unwrap_or_else(|e| e.into_inner()).push((cell, rows));
                        }
                    });
                }
            });
            for (cell, rows) in deeper.into_inner().unwrap_or_else(|e| e.into_inner()) {
                if let Some(slot) = got.iter_mut().find(|(c, _, _)| *c == cell) {
                    slot.1 = rows;
                }
            }
        }
        let mut merged: BTreeMap<String, Found> = BTreeMap::new();
        let replied = got.len();
        for (cell, rows, _) in got {
            if !rows.is_empty() || attached {
                self.remember_cell(&cell, kind, &rows);
            }
            for f in rows {
                merged.entry(f.card.clone()).or_insert(f);
            }
        }
        log::info(TAG, format!("search near {home}: {} listing(s) from {replied} board(s) in {}s", merged.len(), started.elapsed().as_secs()));
        merged.into_values().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stakes_follow_the_deal_with_a_floor() {
        assert_eq!(stake_for(Deal::Stay, 10_000_000_000_000), 2_000_000_000_000);
        assert_eq!(stake_for(Deal::Sale, 10_000_000_000_000), 1_000_000_000_000);
        // Below the floor but the amount can carry the floor.
        assert_eq!(stake_for(Deal::Sale, 1_000_000_000), STAKE_FLOOR_PXMR);
        // Too small to stake at all.
        assert_eq!(stake_for(Deal::Sale, 100), 0);
        assert_eq!(stake_for(Deal::Sale, 0), 0);
    }

    #[test]
    fn a_draft_becomes_a_notice_with_only_its_kinds_specs() {
        let dir = std::env::temp_dir().join(format!("ducat-listings-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let app = App::open(&dir).unwrap();
        let mut specs = Map::new();
        specs.insert("make".into(), Value::from("Volvo"));
        specs.insert("rooms".into(), Value::from(3));
        specs.insert("features".into(), serde_json::json!(["roof rack"]));
        let l = app.draft_listing(KIND_VEHICLE, "  An estate ", "Uptown", 5_000_000_000_000, "DQCHE", specs, "keys under the mat", Some("500"), Some("USD"), 2, None).unwrap();
        assert_eq!(l.cell, "dqche");
        assert_eq!(l.deposit_pxmr, stake_for(Deal::Vehicle, 5_000_000_000_000));
        let n = l.public_notice("ducat:card/x");
        assert_eq!(n.make.as_deref(), Some("Volvo"));
        assert_eq!(n.rooms, None);
        assert_eq!(n.features, vec!["roof rack".to_string()]);
        assert_eq!(n.quantity, 2);
        assert!(n.expiry > App::now());
        app.put_draft(l.clone()).unwrap();
        let mut again = app.listing(&l.id).unwrap();
        again.title = "Renamed".into();
        app.put_draft(again).unwrap();
        assert_eq!(app.listings().len(), 1);
        assert_eq!(app.listing(&l.id).unwrap().title, "Renamed");
        assert!(app.listings_needing_refresh().is_empty());
        let f: Found = n.into();
        assert_eq!(f.specs.get("make").and_then(Value::as_str), Some("Volvo"));
        app.remember_cell("dqche", None, &[f.clone()]);
        assert_eq!(app.browse_cached("dqche", None).len(), 1);
    }
}
