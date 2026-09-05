//! §16.22: sites — pages that travel like publications.
//!
//! A site is one mutable head at a stable record key (`ducat:site/<key>`)
//! naming the current bundle, which rides the swarm whole and renders in a
//! sealed room. This store keeps saved sites and the publisher state for
//! sites this device owns; bundles cache under `files/sites/<id>/current`.

use std::path::{Path, PathBuf};

use base64::Engine;
use ducat_mobile::contacts::{site_head_decode, site_head_encode, SiteHeadIo};
use ducat_mobile::{node, swarm};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{copy_tree, has_any_file, log, App, Error};

const STORE: &str = "ducat_sites";
const TAG: &str = "Sites";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Site {
    #[serde(rename = "rec")]
    pub record_key: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub share: String,
    #[serde(rename = "digest", default)]
    pub digest_hex: String,
    #[serde(default)]
    pub updated: u64,
    #[serde(rename = "added", default)]
    pub added_at: u64,
    #[serde(rename = "keep", default)]
    pub keep_alive: bool,
    /// The digest of the bundle actually on disk, if any.
    #[serde(rename = "fetched", default, skip_serializing_if = "Option::is_none")]
    pub fetched_digest_hex: Option<String>,
    /// The share the cached bundle actually came from. The head's share
    /// can rotate ahead of the disk; a mirror serves what it has, under
    /// the key it was fetched from.
    #[serde(rename = "fetched_share", default, skip_serializing_if = "Option::is_none")]
    pub fetched_share: Option<String>,
    /// The record's owner keypair — this site's write authority — or None
    /// for the ordinary case of a site somebody else made. A reader opens
    /// the record with no writer at all, so a write is not refused by a
    /// rule anybody could relax: there is nothing to sign it with. Only
    /// the holder of this can change what `ducat:site/<key>` points at.
    #[serde(rename = "own_pub", default, with = "b64opt", skip_serializing_if = "Option::is_none")]
    pub owner_public: Option<Vec<u8>>,
    #[serde(rename = "own_sec", default, with = "b64opt", skip_serializing_if = "Option::is_none")]
    pub owner_secret: Option<Vec<u8>>,
    /// The answers a page was written from, when it came from a form
    /// rather than a picked archive.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page: Option<String>,
}

impl Site {
    /// Whether this device can rewrite this site's head.
    pub fn mine(&self) -> bool {
        self.owner_public.is_some() && self.owner_secret.is_some()
    }
}

/// Owner keys as base64 in the table, matching the phone's spelling.
mod b64opt {
    use base64::Engine;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    pub fn serialize<S: Serializer>(v: &Option<Vec<u8>>, s: S) -> Result<S::Ok, S::Error> {
        match v {
            Some(b) => base64::engine::general_purpose::STANDARD.encode(b).serialize(s),
            None => s.serialize_none(),
        }
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Vec<u8>>, D::Error> {
        let s: Option<String> = Option::deserialize(d)?;
        Ok(s.filter(|x| !x.is_empty())
            .and_then(|x| base64::engine::general_purpose::STANDARD.decode(x).ok()))
    }
}

/// `ducat:site/<record-key>` — the address for the life of the site.
pub fn uri_of(record_key: &str) -> String {
    format!("ducat:site/{record_key}")
}

pub fn parse_uri(uri: &str) -> Option<String> {
    let rest = uri.trim().strip_prefix("ducat:site/")?;
    if rest.trim().is_empty() {
        return None;
    }
    Some(rest.to_string())
}

/// A page that reaches for the clearnet, and where.
///
/// §16.22 says a publisher tool SHOULD refuse to seed a bundle that
/// references external resources: one external fetch hands the reader's
/// address and timing to a third party, an unfetched resource is unsigned
/// content inside a digest-verified page, and a bundle with clearnet
/// dependencies neither works offline nor survives its publisher. The
/// viewer refuses these at render; this is the wall being hit at the
/// keyboard, by the one person who can fix it.
///
/// Returns None when the bundle is sealed, or `"<file>: <the offending
/// text>"` for the first thing that is not.
pub fn clearnet_in(dir: &Path) -> Option<String> {
    fn walk(root: &Path, p: &Path, out: &mut Option<String>) {
        let Ok(rd) = std::fs::read_dir(p) else { return };
        for e in rd.flatten() {
            if out.is_some() {
                return;
            }
            let path = e.path();
            if path.is_dir() {
                walk(root, &path, out);
                continue;
            }
            let ext = path.extension().and_then(|x| x.to_str()).unwrap_or("").to_ascii_lowercase();
            if !matches!(ext.as_str(), "html" | "htm" | "css" | "svg") {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else { continue };
            if let Some(hit) = first_external(&text) {
                let rel = path.strip_prefix(root).unwrap_or(&path).to_string_lossy().into_owned();
                *out = Some(format!("{rel}: {hit}"));
                return;
            }
        }
    }
    let mut out = None;
    walk(dir, dir, &mut out);
    out
}

/// The first `src=`/`href=`/`url(`/`@import` pointing at `//` or
/// `http(s)://`, as the offending text. A hand-rolled scan rather than a
/// regex dependency: the grammar is three prefixes and a scheme.
fn first_external(text: &str) -> Option<String> {
    let lower = text.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    let starts: [&str; 4] = ["src", "href", "url(", "@import"];
    let mut i = 0;
    while i < bytes.len() {
        let mut matched = None;
        for s in starts {
            if lower[i..].starts_with(s) {
                matched = Some(s);
                break;
            }
        }
        let Some(s) = matched else { i += 1; continue };
        // After the keyword: optional whitespace, `=` for attributes,
        // optional whitespace, optional quote — then the value.
        let mut j = i + s.len();
        while j < bytes.len() && bytes[j].is_ascii_whitespace() { j += 1 }
        if s == "src" || s == "href" {
            if j >= bytes.len() || bytes[j] != b'=' { i += 1; continue }
            j += 1;
            while j < bytes.len() && bytes[j].is_ascii_whitespace() { j += 1 }
        }
        if j < bytes.len() && (bytes[j] == b'"' || bytes[j] == b'\'') { j += 1 }
        let val = &lower[j..];
        if val.starts_with("//") || val.starts_with("http://") || val.starts_with("https://") {
            let end = text[i..].find(|c: char| c == '>' || c == ')' || c == '\n').map(|e| i + e).unwrap_or(text.len());
            return Some(text[i..end.min(i + 120)].trim().to_string());
        }
        i += 1;
    }
    None
}

impl App {
    fn sites_store(&self) -> crate::store::Store {
        self.store(STORE)
    }

    pub fn sites(&self) -> Vec<Site> {
        self.sites_store().get::<Vec<Site>>("sites").unwrap_or_default()
    }

    /// Replace in place, append only when new — a row that moves under
    /// the hand about to tap it is the fault this store used to have.
    fn upsert_site(&self, s: Site) -> Result<(), Error> {
        self.sites_store().edit::<Vec<Site>, _>("sites", |cur| {
            if cur.iter().any(|x| x.record_key == s.record_key) {
                cur.into_iter().map(|x| if x.record_key == s.record_key { s.clone() } else { x }).collect()
            } else {
                let mut v = cur;
                v.push(s.clone());
                v
            }
        })?;
        Ok(())
    }

    fn map_site(&self, record_key: &str, f: impl Fn(Site) -> Site) -> Result<(), Error> {
        self.sites_store().edit::<Vec<Site>, _>("sites", |cur| {
            cur.into_iter().map(|x| if x.record_key == record_key { f(x) } else { x }).collect()
        })?;
        Ok(())
    }

    /// Where a site's bundle lives, named by a digest of its address —
    /// a full identifier, like every other cache, so no two publishers can
    /// be made to share a directory.
    pub fn site_bundle_dir(&self, record_key: &str) -> PathBuf {
        self.files_dir().join("sites").join(dir_name_of(record_key)).join("current")
    }

    /// Is the bundle actually on this device? The table's word is a claim
    /// that outlives the files; the disk is asked as well.
    pub fn site_is_cached(&self, record_key: &str) -> bool {
        has_any_file(&self.site_bundle_dir(record_key))
    }

    /// Read the head at a record key — the resolve a saved or pasted
    /// address runs. The record must be openable read-only.
    pub fn read_site_head(&self, record_key: &str) -> Result<SiteHeadIo, Error> {
        node::node_dht_open(record_key.to_string(), None, None)?;
        let bytes = node::node_dht_get(record_key.to_string(), 0, true)?
            .ok_or_else(|| Error::Node("the site's head answered nothing".into()))?;
        Ok(site_head_decode(bytes)?)
    }

    /// Add (or refresh) a site by address; returns the stored entry.
    pub fn add_site(&self, record_key: &str) -> Result<Site, Error> {
        let head = self.read_site_head(record_key)?;
        let now = App::now();
        let prior = self.sites().into_iter().find(|s| s.record_key == record_key);
        let entry = Site {
            record_key: record_key.to_string(),
            title: head.title,
            share: head.share,
            digest_hex: head.digest_hex,
            updated: head.updated,
            added_at: prior.as_ref().map(|p| p.added_at).unwrap_or(now),
            keep_alive: prior.as_ref().map(|p| p.keep_alive).unwrap_or(false),
            fetched_digest_hex: prior.as_ref().and_then(|p| p.fetched_digest_hex.clone()),
            fetched_share: prior.as_ref().and_then(|p| p.fetched_share.clone()),
            // Carried: this rebuilds the row from the head, and the head is
            // public. A publisher who pastes their own address would
            // otherwise come back with the write authority gone.
            owner_public: prior.as_ref().and_then(|p| p.owner_public.clone()),
            owner_secret: prior.as_ref().and_then(|p| p.owner_secret.clone()),
            page: prior.as_ref().and_then(|p| p.page.clone()),
        };
        self.upsert_site(entry.clone())?;
        Ok(entry)
    }

    /// Fetch the current bundle if the cache is stale; returns the dir.
    /// Blocks until the bytes are here.
    pub fn fetch_site_bundle(&self, record_key: &str) -> Result<PathBuf, Error> {
        let site = self
            .sites()
            .into_iter()
            .find(|s| s.record_key == record_key)
            .ok_or_else(|| Error::Refused("no such site".into()))?;
        let dir = self.site_bundle_dir(record_key);
        if site.fetched_digest_hex.as_deref() == Some(site.digest_hex.as_str()) && has_any_file(&dir) {
            return Ok(dir);
        }
        let fresh = dir.parent().map(|p| p.join("next")).unwrap_or_else(|| dir.with_extension("next"));
        let _ = std::fs::remove_dir_all(&fresh);
        std::fs::create_dir_all(&fresh)?;
        // Never staySeeding here: a seed parked now would be rooted at
        // `next/`, and the rename below pulls the floor out from under it.
        swarm::swarm_fetch(site.share.clone(), site.digest_hex.clone(), fresh.to_string_lossy().into_owned(), false)?;
        swarm::swarm_stop_share(site.fetched_share.clone().unwrap_or_else(|| site.share.clone()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::rename(&fresh, &dir)?;
        let (digest, share) = (site.digest_hex.clone(), site.share.clone());
        self.map_site(record_key, move |mut s| {
            s.fetched_digest_hex = Some(digest.clone());
            s.fetched_share = Some(share.clone());
            s
        })?;
        // The choice as it stands now, not as it stood when the fetch began.
        self.reseed_site(record_key);
        Ok(dir)
    }

    /// Publish `source` as this device's site, or update one it already
    /// owns (`record_key` given): lint, mint or reopen the record, seed
    /// the bundle from its final path, write the head last.
    pub fn publish_site(
        &self,
        source: &Path,
        title: &str,
        record_key: Option<&str>,
        page: Option<&str>,
    ) -> Result<Site, Error> {
        if !source.join("index.html").is_file() {
            return Err(Error::Refused("a site needs an index.html at its root".into()));
        }
        if let Some(hit) = clearnet_in(source) {
            return Err(Error::Refused(format!("that page reaches the network — {hit}")));
        }

        // 1. The address first, because everything below is named after it.
        let prior = record_key.and_then(|k| self.sites().into_iter().find(|s| s.record_key == k));
        let (key, pubk, sec) = match prior.as_ref().filter(|p| p.mine()) {
            Some(p) => {
                let (pubk, sec) = (p.owner_public.clone().unwrap(), p.owner_secret.clone().unwrap());
                node::node_dht_open(p.record_key.clone(), Some(pubk.clone()), Some(sec.clone()))?;
                (p.record_key.clone(), pubk, sec)
            }
            None => {
                // One subkey: the head is subkey 0 and the record holds
                // nothing else. Minted once per site — the key IS the
                // address, so a second mint is a second site.
                let rec = node::node_dht_create(1)?;
                (rec.key, rec.owner_public, rec.owner_secret)
            }
        };

        // 2. Committed before anything else can fail. A death after this
        //    costs a record with no head yet, which the next publish
        //    rewrites; a death before it, having minted, costs a record
        //    this device owns and can no longer prove it owns.
        let now = App::now();
        let base = Site {
            record_key: key.clone(),
            title: title.to_string(),
            share: prior.as_ref().map(|p| p.share.clone()).unwrap_or_default(),
            digest_hex: prior.as_ref().map(|p| p.digest_hex.clone()).unwrap_or_default(),
            updated: now,
            added_at: prior.as_ref().map(|p| p.added_at).unwrap_or(now),
            keep_alive: true,
            fetched_digest_hex: prior.as_ref().and_then(|p| p.fetched_digest_hex.clone()),
            fetched_share: prior.as_ref().and_then(|p| p.fetched_share.clone()),
            owner_public: Some(pubk),
            owner_secret: Some(sec),
            page: page.map(|s| s.to_string()).or_else(|| prior.as_ref().and_then(|p| p.page.clone())),
        };
        self.upsert_site(base.clone())?;

        // 3. Into the same place a fetched bundle lives, seeded from its
        //    final path, never from a staging dir.
        let dir = self.site_bundle_dir(&key);
        let fresh = dir.parent().map(|p| p.join("next")).unwrap_or_else(|| dir.with_extension("next"));
        let _ = std::fs::remove_dir_all(&fresh);
        copy_tree(source, &fresh)?;
        if let Some(old) = self.sites().into_iter().find(|s| s.record_key == key).and_then(|s| s.fetched_share) {
            swarm::swarm_stop_share(old);
        }
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::rename(&fresh, &dir)?;
        let share = swarm::swarm_seed(dir.to_string_lossy().into_owned())?;

        // 4. The head last: until it is written the address points at
        //    nothing, and after it the whole network can read the page.
        let mut entry = base;
        entry.share = share.share_key.clone();
        entry.digest_hex = share.index_digest_hex.clone();
        entry.fetched_digest_hex = Some(share.index_digest_hex.clone());
        entry.fetched_share = Some(share.share_key.clone());
        self.upsert_site(entry.clone())?;
        node::node_dht_set(
            key.clone(),
            0,
            site_head_encode(SiteHeadIo {
                title: title.to_string(),
                share: share.share_key,
                digest_hex: share.index_digest_hex,
                updated: now,
            })?,
        )?;
        log::info(
            TAG,
            format!(
                "published '{title}' at {} ({})",
                uri_of(&key),
                if prior.as_ref().map(|p| p.mine()).unwrap_or(false) { "update" } else { "new" }
            ),
        );
        Ok(entry)
    }

    /// Put a kept site's cached bundle back into serving — a verify-only
    /// stay-fetch over complete files that downloads nothing. What makes
    /// the keep-alive checkbox a promise rather than a mood: it runs after
    /// every fetch when the box is ticked, and once per process start.
    pub fn reseed_site(&self, record_key: &str) {
        let Some(site) = self.sites().into_iter().find(|s| s.record_key == record_key) else { return };
        if !site.keep_alive {
            return;
        }
        let Some(digest) = site.fetched_digest_hex.clone() else { return };
        let share = site.fetched_share.clone().unwrap_or_else(|| site.share.clone());
        let dir = self.site_bundle_dir(record_key);
        if !has_any_file(&dir) {
            log::info(TAG, format!("keep-alive for {}… has no bundle to serve yet — open it once", &record_key[..record_key.len().min(8)]));
            return;
        }
        let app = self.clone();
        let key = record_key.to_string();
        std::thread::Builder::new()
            .name("site-reseed".into())
            .spawn(move || {
                let kept = |a: &App| a.sites().iter().any(|s| s.record_key == key && s.keep_alive);
                if !kept(&app) {
                    return;
                }
                // The head first, because seeding the wrong edition is
                // worse than seeding nothing: peers compare the index
                // digest, and a mirror announcing last month's edition is
                // rejected by everyone holding the current one and dropped
                // from the swarm for good.
                match app.add_site(&key) {
                    Ok(head) if head.digest_hex != digest => {
                        log::info(TAG, format!("keep-alive for {}… is a stale edition — refetching", &key[..key.len().min(8)]));
                        if let Err(e) = app.fetch_site_bundle(&key) {
                            log::warn(TAG, format!("refetch for keep-alive: {e}"));
                        }
                        return;
                    }
                    Ok(_) => {}
                    Err(_) => log::info(TAG, format!("keep-alive for {}… could not re-read the head — serving the copy on disk", &key[..key.len().min(8)])),
                }
                // Stop-then-stay: parking the same share twice would
                // strand the first task; stopping one nobody serves is a
                // no-op.
                swarm::swarm_stop_share(share.clone());
                match swarm::swarm_fetch(share.clone(), digest.clone(), dir.to_string_lossy().into_owned(), true) {
                    Ok(_) => {
                        // Whatever the store says now wins: a remove or an
                        // unticked box that landed while the fetch was out.
                        if !kept(&app) {
                            swarm::swarm_stop_share(share.clone());
                            log::info(TAG, "reseed finished for a site no longer kept — stopped");
                        } else {
                            log::info(TAG, format!("'{}' serving", site.title));
                        }
                    }
                    Err(e) => log::warn(TAG, format!("reseed '{}': {e:?}", site.title)),
                }
            })
            .ok();
    }

    /// Re-park every kept site. Once per process, after the node is up.
    pub fn reseed_all_sites(&self) {
        for s in self.sites() {
            if s.keep_alive {
                self.reseed_site(&s.record_key);
            }
        }
    }

    pub fn set_site_keep_alive(&self, record_key: &str, keep: bool) -> Result<(), Error> {
        self.map_site(record_key, move |mut s| {
            s.keep_alive = keep;
            s
        })?;
        if keep {
            self.reseed_site(record_key);
        } else if let Some(s) = self.sites().into_iter().find(|s| s.record_key == record_key) {
            swarm::swarm_stop_share(s.fetched_share.unwrap_or(s.share));
        }
        Ok(())
    }

    pub fn remove_site(&self, record_key: &str) -> Result<(), Error> {
        if let Some(s) = self.sites().into_iter().find(|s| s.record_key == record_key) {
            swarm::swarm_stop_share(s.fetched_share.unwrap_or(s.share));
        }
        self.sites_store().edit::<Vec<Site>, _>("sites", |cur| {
            cur.into_iter().filter(|s| s.record_key != record_key).collect()
        })?;
        let _ = std::fs::remove_dir_all(self.files_dir().join("sites").join(dir_name_of(record_key)));
        Ok(())
    }

    /// Drop bundle directories no saved site claims.
    pub fn sweep_site_orphans(&self) -> usize {
        let root = self.files_dir().join("sites");
        let Ok(rd) = std::fs::read_dir(&root) else { return 0 };
        let keep: std::collections::HashSet<String> =
            self.sites().into_iter().map(|s| dir_name_of(&s.record_key)).collect();
        let mut gone = 0;
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() && !keep.contains(&e.file_name().to_string_lossy().into_owned()) {
                let _ = std::fs::remove_dir_all(&p);
                gone += 1;
            }
        }
        if gone > 0 {
            log::info(TAG, format!("swept {gone} orphaned bundle(s)"));
        }
        gone
    }
}

fn dir_name_of(record_key: &str) -> String {
    hex::encode(Sha256::digest(record_key.as_bytes()))
}

#[allow(dead_code)]
fn b64(b: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses_parse_both_ways() {
        assert_eq!(parse_uri("ducat:site/VLD0:abc"), Some("VLD0:abc".into()));
        assert_eq!(parse_uri(" ducat:site/VLD0:abc\n"), Some("VLD0:abc".into()));
        assert!(parse_uri("ducat:site/").is_none());
        assert!(parse_uri("ducat:file/VLD0:abc:00").is_none());
        assert_eq!(uri_of("VLD0:abc"), "ducat:site/VLD0:abc");
    }

    #[test]
    fn the_clearnet_lint_finds_every_door_out() {
        let dir = std::env::temp_dir().join(format!("ducat-lint-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("css")).unwrap();
        std::fs::write(dir.join("index.html"), "<html><img src=\"pic.png\"><a href=\"about.html\">x</a></html>").unwrap();
        std::fs::write(dir.join("css/a.css"), "body { background: url('bg.png') }").unwrap();
        assert!(clearnet_in(&dir).is_none(), "a sealed bundle passed");

        for (name, body) in [
            ("index.html", "<img src=\"https://cdn.example/x.png\">"),
            ("index.html", "<a href='//example.org/'>"),
            ("index.html", "<link href = \"http://x/y.css\">"),
            ("css/a.css", "@import \"https://fonts.example/f.css\";"),
            ("css/a.css", "div{background:url(//x/y.png)}"),
        ] {
            std::fs::write(dir.join(name), body).unwrap();
            let hit = clearnet_in(&dir);
            assert!(hit.is_some(), "missed {body:?}");
            assert!(hit.unwrap().starts_with(name), "wrong file for {body:?}");
            // put it back
            std::fs::write(dir.join(name), if name.ends_with("css") { "body{}" } else { "<p>ok</p>" }).unwrap();
        }
        // Not fooled by the scheme inside prose or a non-page file.
        std::fs::write(dir.join("notes.txt"), "see https://example.org").unwrap();
        std::fs::write(dir.join("index.html"), "<p>visit https://example.org yourself</p>").unwrap();
        assert!(clearnet_in(&dir).is_none());
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn owner_keys_survive_the_table() {
        let dir = std::env::temp_dir().join(format!("ducat-sites-{}", std::process::id()));
        let app = App::open(&dir).unwrap();
        let s = Site {
            record_key: "VLD0:key".into(),
            title: "Mine".into(),
            share: "VLD0:s".into(),
            digest_hex: "ab".repeat(32),
            updated: 5,
            added_at: 4,
            keep_alive: true,
            fetched_digest_hex: None,
            fetched_share: None,
            owner_public: Some(vec![1, 2, 3]),
            owner_secret: Some(vec![9, 8, 7, 6]),
            page: None,
        };
        app.upsert_site(s.clone()).unwrap();
        let back = app.sites();
        assert_eq!(back, vec![s.clone()]);
        assert!(back[0].mine());
        // Refreshing from a head keeps the keys (simulated via upsert of a
        // row that carried them forward, as add_site does).
        let mut refreshed = s.clone();
        refreshed.title = "Renamed".into();
        app.upsert_site(refreshed).unwrap();
        assert_eq!(app.sites().len(), 1);
        assert!(app.sites()[0].mine());
        assert_eq!(app.sites()[0].title, "Renamed");
        std::fs::remove_dir_all(dir).ok();
    }
}
