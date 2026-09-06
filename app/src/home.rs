//! §16.23: a persona's home — its site at the address its own key names —
//! and the feed inside it; hearts, and the timeline of the people kept.
//!
//! A home is a site (§16.22) whose record the persona keypair owns, so its
//! key is computed from the persona's public key and never carried by a
//! card. The bundle is the persona's pages plus `feed.json`, its thumbnails
//! and a page per post, all written here from one document. Full-size
//! pictures and other files travel as their own immutable shares, so a
//! heart mirrors the small thing and a new post ships only itself.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use ducat_mobile::feed::{self, FeedDoc, FeedEntry, FeedFile, FeedMedia, FeedPost};
use ducat_mobile::node;
use serde::Serialize;

use crate::contacts::hex_to_bytes;
use crate::sites::Site;
use crate::{copy_tree, log, releases, App, Error};

const TAG: &str = "Home";
pub const HOME_SUBKEYS: u32 = 1;
/// A thumbnail's ceiling in the bundle; the picture itself travels as a share.
const THUMB_BUDGET: usize = 160 * 1024;
/// How often the lap looks at hearted homes: the head reads are cheap but
/// not free, and a feed is not a chat.
const FEEDS_EVERY_SECS: u64 = 10 * 60;
/// Heads read side by side on a feeds lap.
const FEED_WIDTH: usize = 8;
/// Posts kept in `feed.json` before the oldest move to an older page.
const PAGE_KEEP: usize = 120;

static LAST_FEEDS: AtomicU64 = AtomicU64::new(0);

/// What a client shows about a persona's home.
#[derive(Clone, Debug, Serialize)]
pub struct HomeView {
    pub persona_hex: String,
    pub record_key: String,
    pub title: String,
    /// A head has been read (or written): the home exists on the network.
    pub has_head: bool,
    pub digest_hex: String,
    pub updated: u64,
    pub hearted: bool,
    pub mine: bool,
    pub posts: u32,
}

fn mime_of(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).as_deref() {
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("png") => "image/png",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("avif") => "image/avif",
        Some("heic") => "image/heic",
        Some("mp4") | Some("m4v") => "video/mp4",
        Some("webm") => "video/webm",
        Some("mov") => "video/quicktime",
        Some("mkv") => "video/x-matroska",
        Some("mp3") => "audio/mpeg",
        Some("ogg") | Some("oga") => "audio/ogg",
        Some("flac") => "audio/flac",
        Some("wav") => "audio/wav",
        Some("pdf") => "application/pdf",
        Some("txt") | Some("md") => "text/plain",
        Some("zip") => "application/zip",
        Some("json") => "application/json",
        _ => "application/octet-stream",
    }
}

fn file_name_of(p: &Path) -> String {
    p.file_name().and_then(|n| n.to_str()).filter(|n| !n.trim().is_empty()).unwrap_or("file").to_string()
}

impl App {
    // ----- where a home is ----------------------------------------------------------

    /// The record key a persona's home has, from the key alone.
    pub fn home_key_of(&self, persona_hex: &str) -> Result<String, Error> {
        let pk = hex_to_bytes(persona_hex).filter(|b| b.len() == 32).ok_or_else(|| Error::Refused("not a persona key".into()))?;
        Ok(node::node_dht_record_key_for(pk, HOME_SUBKEYS)?)
    }

    pub fn my_home_key(&self) -> Result<String, Error> {
        let worn = self.worn()?;
        self.home_key_of(&worn)
    }

    fn home_dir(&self, persona_hex: &str) -> PathBuf {
        self.root().join("home").join(persona_hex)
    }

    /// The persona's own pages, if it keeps any beside the feed.
    pub fn home_pages_dir(&self) -> Result<PathBuf, Error> {
        Ok(self.home_dir(&self.worn()?).join("site"))
    }

    /// Make sure the worn persona's home record exists here and is a site
    /// of mine — owner keys the persona's own — so publishing can find it.
    pub fn ensure_home(&self) -> Result<Site, Error> {
        let worn = self.worn()?;
        let key = self.home_key_of(&worn)?;
        if let Some(s) = self.sites().into_iter().find(|s| s.record_key == key && s.mine()) {
            return Ok(s);
        }
        let secret = self.persona_secret(&worn)?.ok_or_else(|| Error::Refused("no secret for the worn persona".into()))?;
        let pk = hex_to_bytes(&worn).ok_or_else(|| Error::Refused("not a persona key".into()))?;
        let rec = node::node_dht_create_owned(HOME_SUBKEYS, pk, secret)?;
        if rec.key != key {
            return Err(Error::Refused(format!("the home record came back at {} not {}", rec.key, key)));
        }
        let prior = self.sites().into_iter().find(|s| s.record_key == key);
        let now = App::now();
        let s = Site {
            record_key: key,
            title: self.my_name(Some(&worn))?.unwrap_or_default(),
            share: prior.as_ref().map(|p| p.share.clone()).unwrap_or_default(),
            digest_hex: prior.as_ref().map(|p| p.digest_hex.clone()).unwrap_or_default(),
            updated: prior.as_ref().map_or(0, |p| p.updated),
            added_at: prior.as_ref().map_or(now, |p| p.added_at),
            keep_alive: true,
            fetched_digest_hex: prior.as_ref().and_then(|p| p.fetched_digest_hex.clone()),
            fetched_share: prior.as_ref().and_then(|p| p.fetched_share.clone()),
            owner_public: Some(rec.owner_public),
            owner_secret: Some(rec.owner_secret),
            page: None,
        };
        self.upsert_site(s.clone())?;
        log::info(TAG, format!("home record ready at {}…", &s.record_key[..24.min(s.record_key.len())]));
        Ok(s)
    }

    // ----- my feed -------------------------------------------------------------------

    fn my_feed_path(&self) -> Result<PathBuf, Error> {
        Ok(self.home_dir(&self.worn()?).join(feed::FEED_FILE))
    }

    /// The worn persona's own feed as kept here; empty until the first post.
    pub fn my_feed(&self) -> Result<FeedDoc, Error> {
        let worn = self.worn()?;
        let path = self.my_feed_path()?;
        if let Ok(text) = std::fs::read_to_string(&path) {
            return feed::feed_parse(text).map_err(|e| Error::Refused(e.to_string()));
        }
        Ok(FeedDoc { v: feed::FEED_VERSION, persona: worn.clone(), name: self.my_name(Some(&worn))?.unwrap_or_default(), updated: 0, posts: Vec::new(), older: None })
    }

    fn save_my_feed(&self, doc: &FeedDoc) -> Result<(), Error> {
        let path = self.my_feed_path()?;
        if let Some(p) = path.parent() {
            std::fs::create_dir_all(p)?;
        }
        let text = feed::feed_encode(doc.clone()).map_err(|e| Error::Refused(e.to_string()))?;
        std::fs::write(path, text)?;
        Ok(())
    }

    /// Write a post: thumbnails into the bundle, the pictures and files
    /// themselves as shares, the post at the top of the feed — then the
    /// home republished so the network has it.
    pub fn post_to_feed(&self, text: &str, media: &[PathBuf], files: &[PathBuf]) -> Result<FeedPost, Error> {
        let text = text.trim();
        if text.is_empty() && media.is_empty() && files.is_empty() {
            return Err(Error::Refused("a post needs some words, a picture, or a file".into()));
        }
        if text.chars().count() > feed::MAX_TEXT {
            return Err(Error::Refused(format!("a post is at most {} characters", feed::MAX_TEXT)));
        }
        if media.len() > feed::MAX_MEDIA || files.len() > feed::MAX_FILES {
            return Err(Error::Refused("too many pictures or files for one post".into()));
        }
        let worn = self.worn()?;
        let mut doc = self.my_feed()?;
        doc.name = self.my_name(Some(&worn))?.unwrap_or_default();
        let id = feed::feed_new_id();
        let thumb_dir = self.home_dir(&worn).join("feed").join(&id);
        let mut post = FeedPost { id: id.clone(), at: App::now(), edited: None, text: text.to_string(), media: Vec::new(), files: Vec::new(), re: None };
        for (n, src) in media.iter().enumerate() {
            let bytes = std::fs::read(src)?;
            match crate::thumbs::thumbnail(&bytes, THUMB_BUDGET) {
                Some(thumb) => {
                    let (w, h) = image::load_from_memory(&thumb).map(|i| (i.width(), i.height())).unwrap_or((0, 0));
                    std::fs::create_dir_all(&thumb_dir)?;
                    let rel = format!("feed/{}/{}.jpg", id, n + 1);
                    std::fs::write(thumb_dir.join(format!("{}.jpg", n + 1)), &thumb)?;
                    // The picture itself, as a share, unless the thumbnail
                    // already is the picture.
                    let full = if bytes.len() > thumb.len() + 8 * 1024 {
                        let r = self.share_file(src, &file_name_of(src))?;
                        Some(releases::uri_of(&r.share_key, &r.digest_hex))
                    } else {
                        None
                    };
                    post.media.push(FeedMedia { path: rel, full, mime: "image/jpeg".into(), bytes: thumb.len() as u64, w, h, alt: String::new() });
                }
                None => {
                    // Not a picture we can shrink: it travels as a file.
                    let r = self.share_file(src, &file_name_of(src))?;
                    post.files.push(FeedFile { name: file_name_of(src), addr: releases::uri_of(&r.share_key, &r.digest_hex), mime: mime_of(src).into(), bytes: bytes.len() as u64 });
                }
            }
        }
        for src in files {
            let bytes = std::fs::metadata(src)?.len();
            let r = self.share_file(src, &file_name_of(src))?;
            post.files.push(FeedFile { name: file_name_of(src), addr: releases::uri_of(&r.share_key, &r.digest_hex), mime: mime_of(src).into(), bytes });
        }
        doc.posts.insert(0, post.clone());
        doc.updated = App::now();
        self.roll_older_pages(&mut doc)?;
        self.save_my_feed(&doc)?;
        self.publish_home()?;
        log::info(TAG, format!("posted {} ({} picture(s), {} file(s))", id, post.media.len(), post.files.len()));
        Ok(post)
    }

    /// Take back a post of mine: gone from the next edition.
    pub fn delete_post(&self, id: &str) -> Result<(), Error> {
        let mut doc = self.my_feed()?;
        let before = doc.posts.len();
        doc.posts.retain(|p| p.id != id);
        if doc.posts.len() == before {
            return Err(Error::Refused("no such post".into()));
        }
        doc.updated = App::now();
        self.save_my_feed(&doc)?;
        let _ = std::fs::remove_dir_all(self.home_dir(&self.worn()?).join("feed").join(id));
        self.publish_home()?;
        Ok(())
    }

    /// Keep `feed.json` to a page: the oldest posts move to `feed-<n>.json`,
    /// chained by `older`, so a long-running feed does not grow one file forever.
    fn roll_older_pages(&self, doc: &mut FeedDoc) -> Result<(), Error> {
        if doc.posts.len() <= feed::MAX_POSTS {
            return Ok(());
        }
        let dir = self.home_dir(&self.worn()?);
        let mut n = 1;
        while dir.join(format!("feed-{n}.json")).exists() {
            n += 1;
        }
        let older: Vec<FeedPost> = doc.posts.split_off(PAGE_KEEP);
        let page = FeedDoc { v: feed::FEED_VERSION, persona: doc.persona.clone(), name: doc.name.clone(), updated: doc.updated, posts: older, older: doc.older.clone() };
        let text = feed::feed_encode(page).map_err(|e| Error::Refused(e.to_string()))?;
        std::fs::write(dir.join(format!("feed-{n}.json")), text)?;
        doc.older = Some(format!("feed-{n}.json"));
        Ok(())
    }

    /// Put the home on the network: the persona's pages if it keeps any,
    /// `feed.json`, its thumbnails and older pages, and a page per post.
    pub fn publish_home(&self) -> Result<Site, Error> {
        let worn = self.worn()?;
        let home = self.ensure_home()?;
        let doc = self.my_feed()?;
        let dir = self.home_dir(&worn);
        let staging = dir.join("staging");
        let _ = std::fs::remove_dir_all(&staging);
        std::fs::create_dir_all(&staging)?;
        let pages = dir.join("site");
        if pages.join("index.html").is_file() {
            copy_tree(&pages, &staging)?;
        }
        let name = if doc.name.trim().is_empty() { self.my_name(Some(&worn))?.unwrap_or_default() } else { doc.name.clone() };
        // The feed and everything it names.
        std::fs::write(staging.join(feed::FEED_FILE), feed::feed_encode(doc.clone()).map_err(|e| Error::Refused(e.to_string()))?)?;
        if dir.join("feed").is_dir() {
            copy_tree(&dir.join("feed"), &staging.join("feed"))?;
        }
        std::fs::write(staging.join("feed.html"), feed::feed_index_html(doc.clone()))?;
        std::fs::create_dir_all(staging.join("posts"))?;
        let mut all_posts: Vec<FeedPost> = doc.posts.clone();
        let mut older = doc.older.clone();
        while let Some(o) = older.take() {
            let path = dir.join(&o);
            let Ok(text) = std::fs::read_to_string(&path) else { break };
            std::fs::copy(&path, staging.join(&o))?;
            match feed::feed_parse(text) {
                Ok(page) => {
                    all_posts.extend(page.posts);
                    older = page.older;
                }
                Err(_) => break,
            }
        }
        for p in &all_posts {
            std::fs::write(staging.join("posts").join(format!("{}.html", p.id)), feed::feed_post_html(name.clone(), p.clone()))?;
        }
        if !staging.join("index.html").is_file() {
            // No pages of its own: the feed is the front page.
            std::fs::write(staging.join("index.html"), feed::feed_index_html(doc.clone()))?;
        }
        let title = if name.trim().is_empty() { "Home".to_string() } else { name };
        let site = self.publish_site(&staging, &title, Some(&home.record_key), None)?;
        let _ = std::fs::remove_dir_all(&staging);
        Ok(site)
    }

    // ----- hearts and the timeline --------------------------------------------------

    pub fn set_heart(&self, persona_hex: &str, on: bool) -> Result<(), Error> {
        let mut c = self.contact(persona_hex).ok_or_else(|| Error::Refused("no such contact".into()))?;
        c.hearted = on;
        self.put_contact(c)?;
        let key = self.home_key_of(persona_hex)?;
        if on {
            // The home may not exist yet; the lap keeps looking.
            match self.add_site(&key) {
                Ok(_) => {
                    let _ = self.set_site_keep_alive(&key, true);
                    let _ = self.fetch_site_bundle(&key);
                }
                Err(e) => log::info(TAG, format!("hearted a persona whose home is not there yet: {e}")),
            }
        } else if self.sites().iter().any(|s| s.record_key == key && !s.mine()) {
            let _ = self.set_site_keep_alive(&key, false);
        }
        Ok(())
    }

    pub fn hearted(&self) -> Vec<crate::contacts::Contact> {
        self.contacts().into_iter().filter(|c| c.hearted).collect()
    }

    /// Read every hearted home's head; fetch what moved. Returns how many
    /// homes have a new edition. The heads are read FEED_WIDTH side by
    /// side: one read is a second or ten depending on the network, and
    /// a timeline of a few hundred homes must not take the whole of its
    /// ten-minute turn on reads alone.
    pub fn refresh_feeds(&self) -> usize {
        let hearted = self.hearted();
        let mut fresh = 0;
        for chunk in hearted.chunks(FEED_WIDTH) {
            let moved: Vec<bool> = std::thread::scope(|s| {
                let handles: Vec<_> = chunk
                    .iter()
                    .map(|c| {
                        let app = self.clone();
                        let c = c.clone();
                        s.spawn(move || app.refresh_one_feed(&c))
                    })
                    .collect();
                handles.into_iter().map(|h| h.join().unwrap_or(false)).collect()
            });
            fresh += moved.into_iter().filter(|m| *m).count();
        }
        fresh
    }

    /// One hearted home: its head, and its bundle when the head moved.
    /// True when there is a new edition on disk.
    fn refresh_one_feed(&self, c: &crate::contacts::Contact) -> bool {
        let Ok(key) = self.home_key_of(&c.persona_hex) else { return false };
        let before = self.sites().into_iter().find(|s| s.record_key == key).and_then(|s| s.fetched_digest_hex);
        let Ok(site) = self.add_site(&key) else { return false };
        if before.as_deref() == Some(site.digest_hex.as_str()) {
            return false;
        }
        match self.fetch_site_bundle(&key) {
            Ok(_) => {
                log::info(TAG, format!("{} has a new edition", c.display_name()));
                true
            }
            Err(e) => {
                log::warn(TAG, format!("{}'s home: {e}", c.display_name()));
                false
            }
        }
    }
    /// Not every lap: the heads move rarely and the reads add up.
    pub fn feeds_lap(&self) {
        let now = App::now();
        let last = LAST_FEEDS.load(Ordering::Relaxed);
        if now.saturating_sub(last) < FEEDS_EVERY_SECS {
            return;
        }
        LAST_FEEDS.store(now, Ordering::Relaxed);
        if self.hearted().is_empty() {
            return;
        }
        let n = self.refresh_feeds();
        if n > 0 {
            crate::notify::post("Feed", format!("{n} new edition(s) in your timeline"), None);
        }
    }

    /// A persona's feed as fetched here (mine from my own document).
    pub fn feed_of(&self, persona_hex: &str) -> Option<FeedDoc> {
        if self.worn().ok().as_deref() == Some(persona_hex) {
            return self.my_feed().ok();
        }
        let key = self.home_key_of(persona_hex).ok()?;
        let text = std::fs::read_to_string(self.site_bundle_dir(&key).join(feed::FEED_FILE)).ok()?;
        match feed::feed_parse(text) {
            Ok(d) => Some(d),
            Err(e) => {
                log::warn(TAG, format!("{}'s feed is unreadable: {e}", &persona_hex[..8.min(persona_hex.len())]));
                None
            }
        }
    }

    /// Everyone kept, and me, newest first.
    pub fn timeline(&self, limit: u32) -> Vec<FeedEntry> {
        let mut docs = Vec::new();
        if let Ok(mine) = self.my_feed() {
            if !mine.posts.is_empty() {
                docs.push(mine);
            }
        }
        for c in self.hearted() {
            if let Some(d) = self.feed_of(&c.persona_hex) {
                docs.push(d);
            }
        }
        feed::feed_merge(docs, limit)
    }

    /// A file from a persona's home bundle, for the timeline's thumbnails.
    pub fn home_file(&self, persona_hex: &str, rel: &str) -> Option<PathBuf> {
        if !feed::bundle_path_ok(rel) {
            return None;
        }
        let key = self.home_key_of(persona_hex).ok()?;
        let root = self.site_bundle_dir(&key).canonicalize().ok()?;
        let p = root.join(rel.trim_start_matches('/')).canonicalize().ok()?;
        (p.starts_with(&root) && p.is_file()).then_some(p)
    }

    pub fn home_view(&self, persona_hex: &str) -> Result<HomeView, Error> {
        let key = self.home_key_of(persona_hex)?;
        let mine = self.worn().ok().as_deref() == Some(persona_hex);
        let site = self.sites().into_iter().find(|s| s.record_key == key);
        let posts = self.feed_of(persona_hex).map_or(0, |d| d.posts.len() as u32);
        Ok(HomeView {
            persona_hex: persona_hex.to_string(),
            record_key: key,
            title: site.as_ref().map(|s| s.title.clone()).unwrap_or_default(),
            has_head: site.as_ref().map_or(false, |s| !s.digest_hex.is_empty()),
            digest_hex: site.as_ref().map(|s| s.digest_hex.clone()).unwrap_or_default(),
            updated: site.as_ref().map_or(0, |s| s.updated),
            hearted: self.contact(persona_hex).map_or(false, |c| c.hearted),
            mine,
            posts,
        })
    }
}
