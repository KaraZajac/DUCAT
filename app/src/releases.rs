//! A file put on the network once, at an address that cannot change.
//!
//! The third shape beside §16.20's paid periods and §16.22's sites: a
//! record of a CD, a film, a dataset — something shared rather than sold,
//! and finished the moment it leaves. A site is one mutable head at a
//! stable key; a release is a single fixed thing and nothing more.
//!
//! **Immutability is arithmetic here, not policy.** The address carries the
//! swarm share key *and* the content digest, so changing a byte changes the
//! address: there is no version of this object that can be updated, not
//! even by whoever made it, and no head for a mirror to chase. A mirror
//! announces the pair it holds, for as long as it holds it, and can never
//! be announcing the wrong edition of anything.
//!
//! The honest cost, which a screen must say rather than imply away: nobody
//! is *obliged* to serve a release. It lives exactly as long as somebody
//! keeps it alive, and when the last mirror drops it the address still
//! parses and no longer resolves.

use std::path::{Path, PathBuf};

use ducat_mobile::swarm;
use serde::{Deserialize, Serialize};

use crate::{dir_bytes, has_any_file, log, App, Error};

const PREFIX: &str = "ducat:file/";
const STORE: &str = "ducat_releases";
const TAG: &str = "Releases";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Release {
    #[serde(rename = "key")]
    pub share_key: String,
    #[serde(rename = "digest")]
    pub digest_hex: String,
    /// The publisher's own name for it; display only, never a path.
    #[serde(default)]
    pub title: String,
    #[serde(rename = "added", default)]
    pub added_at: u64,
    #[serde(default)]
    pub bytes: u64,
    /// Kept alive for other readers — the only thing keeping this address
    /// alive once whoever shared it has gone.
    #[serde(rename = "keep", default)]
    pub keep_alive: bool,
    /// True when this device is where it came from. Not a claim of
    /// authorship: a release has no owner and no write authority, only a
    /// first seeder.
    #[serde(default)]
    pub mine: bool,
}

/// `ducat:file/<share-key>:<digest-hex>` — the whole of the address.
pub fn uri_of(share_key: &str, digest_hex: &str) -> String {
    format!("{PREFIX}{share_key}:{digest_hex}")
}

/// The pair back out of an address, or None.
///
/// Share keys carry colons of their own (`VLD0:<key>:<secret>`), so the
/// digest is taken from the LAST colon and the key is everything before it
/// — splitting on the first would hand back a truncated key that looks
/// plausible and resolves to nothing.
pub fn parse(uri: &str) -> Option<(String, String)> {
    let body = uri.trim().strip_prefix(PREFIX)?;
    let cut = body.rfind(':')?;
    if cut == 0 || cut == body.len() - 1 {
        return None;
    }
    let key = &body[..cut];
    let digest = &body[cut + 1..];
    if key.trim().is_empty() || digest.len() != 64 || !digest.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    Some((key.to_string(), digest.to_ascii_lowercase()))
}

impl App {
    fn releases_store(&self) -> crate::store::Store {
        self.store(STORE)
    }

    pub fn releases(&self) -> Vec<Release> {
        self.releases_store().get::<Vec<Release>>("releases").unwrap_or_default()
    }

    fn save_releases(&self, items: &[Release]) -> Result<(), Error> {
        self.releases_store().put("releases", &items)?;
        Ok(())
    }

    /// Replaced where it stands, appended only when new — a row that moves
    /// under the hand about to tap it is the fault Listings and Sites both
    /// had. Keyed by digest, since that is the identity.
    fn put_release(&self, r: Release) -> Result<(), Error> {
        self.releases_store().edit::<Vec<Release>, _>("releases", |cur| {
            if cur.iter().any(|x| x.digest_hex == r.digest_hex) {
                cur.into_iter().map(|x| if x.digest_hex == r.digest_hex { r.clone() } else { x }).collect()
            } else {
                let mut v = cur;
                v.push(r.clone());
                v
            }
        })?;
        Ok(())
    }

    /// Where a release's bytes live. Named by the digest, because that is
    /// what the address promises and what a second copy of the same file
    /// would hash to anyway.
    pub fn release_dir(&self, digest_hex: &str) -> PathBuf {
        self.files_dir().join("releases").join(safe_name(digest_hex))
    }

    pub fn release_is_here(&self, digest_hex: &str) -> bool {
        has_any_file(&self.release_dir(digest_hex))
    }

    /// Put a file on the network and keep serving it.
    ///
    /// Seeds from a directory holding one file, so the swarm index carries
    /// a name the far end can write to disk. Returns the release, whose
    /// address is the only thing that needs handing over.
    pub fn share_file(&self, source: &Path, title: &str) -> Result<Release, Error> {
        if !source.is_file() {
            return Err(Error::Refused("that is not a file".into()));
        }
        let staging = self.files_dir().join("release_staging");
        let _ = std::fs::remove_dir_all(&staging);
        std::fs::create_dir_all(&staging)?;
        let name = source
            .file_name()
            .and_then(|n| n.to_str())
            .filter(|n| !n.trim().is_empty())
            .unwrap_or("file")
            .to_string();
        std::fs::copy(source, staging.join(&name))?;
        let share = swarm::swarm_seed(staging.to_string_lossy().into_owned())?;
        let dir = self.release_dir(&share.index_digest_hex);
        let _ = std::fs::remove_dir_all(&dir);
        if let Some(p) = dir.parent() {
            std::fs::create_dir_all(p)?;
        }
        std::fs::rename(&staging, &dir)?;
        // Seeded from the staging path, which has just moved: park it again
        // from where it will stay, or the seeder serves a directory that is
        // no longer there.
        swarm::swarm_stop_share(share.share_key.clone());
        let r = Release {
            share_key: share.share_key,
            digest_hex: share.index_digest_hex,
            title: if title.trim().is_empty() { name } else { title.to_string() },
            added_at: App::now(),
            bytes: dir_bytes(&dir),
            keep_alive: true,
            mine: true,
        };
        self.put_release(r.clone())?;
        self.reseed_release(&r.digest_hex);
        log::info(TAG, format!("shared '{}' at {}", r.title, uri_of(&r.share_key, &r.digest_hex)));
        Ok(r)
    }

    /// File an address somebody handed over. Nothing is fetched yet.
    pub fn add_release(&self, uri: &str, title: &str) -> Result<Release, Error> {
        let (key, digest) = parse(uri).ok_or_else(|| Error::Refused("that is not a ducat:file/ address".into()))?;
        let prior = self.releases().into_iter().find(|r| r.digest_hex == digest);
        let r = Release {
            share_key: key,
            digest_hex: digest,
            title: prior
                .as_ref()
                .map(|p| p.title.clone())
                .filter(|t| !t.trim().is_empty())
                .unwrap_or_else(|| title.to_string()),
            added_at: prior.as_ref().map(|p| p.added_at).unwrap_or_else(App::now),
            bytes: prior.as_ref().map(|p| p.bytes).unwrap_or(0),
            keep_alive: prior.as_ref().map(|p| p.keep_alive).unwrap_or(false),
            mine: prior.as_ref().map(|p| p.mine).unwrap_or(false),
        };
        self.put_release(r.clone())?;
        Ok(r)
    }

    pub fn set_release_keep_alive(&self, digest_hex: &str, keep: bool) -> Result<(), Error> {
        self.releases_store().edit::<Vec<Release>, _>("releases", |cur| {
            cur.into_iter()
                .map(|mut r| {
                    if r.digest_hex == digest_hex {
                        r.keep_alive = keep;
                    }
                    r
                })
                .collect()
        })?;
        if keep {
            self.reseed_release(digest_hex);
        } else if let Some(r) = self.releases().into_iter().find(|r| r.digest_hex == digest_hex) {
            swarm::swarm_stop_share(r.share_key);
        }
        Ok(())
    }

    pub fn remove_release(&self, digest_hex: &str) -> Result<(), Error> {
        if let Some(r) = self.releases().into_iter().find(|r| r.digest_hex == digest_hex) {
            swarm::swarm_stop_share(r.share_key);
        }
        self.releases_store().edit::<Vec<Release>, _>("releases", |cur| {
            cur.into_iter().filter(|r| r.digest_hex != digest_hex).collect()
        })?;
        let _ = std::fs::remove_dir_all(self.release_dir(digest_hex));
        Ok(())
    }

    /// Fetch it, verifying every piece against the digest in the address.
    /// Blocks until the bytes are here. Stays seeding when the reader chose
    /// to keep it alive.
    pub fn fetch_release(&self, digest_hex: &str) -> Result<PathBuf, Error> {
        let r = self
            .releases()
            .into_iter()
            .find(|r| r.digest_hex == digest_hex)
            .ok_or_else(|| Error::Refused("no such release".into()))?;
        let dir = self.release_dir(&r.digest_hex);
        if self.release_is_here(&r.digest_hex) {
            return Ok(dir);
        }
        let part = dir.with_extension("part");
        let _ = std::fs::remove_dir_all(&part);
        std::fs::create_dir_all(&part)?;
        log::info(TAG, format!("fetching '{}' ({}…)", r.title, &r.digest_hex[..12]));
        let t0 = std::time::Instant::now();
        if let Err(e) = swarm::swarm_fetch(
            r.share_key.clone(),
            r.digest_hex.clone(),
            part.to_string_lossy().into_owned(),
            false,
        ) {
            log::warn(TAG, format!("'{}' did not arrive after {:.0}s: {e}", r.title, t0.elapsed().as_secs_f64()));
            return Err(e.into());
        }
        log::info(TAG, format!("'{}' arrived in {:.0}s", r.title, t0.elapsed().as_secs_f64()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::rename(&part, &dir)?;
        let mut filled = r.clone();
        filled.bytes = dir_bytes(&dir);
        // A release added by address arrives nameless; the file it turns
        // out to be is the best name there is, and it is the one the
        // phone shows.
        if filled.title.trim().is_empty() {
            if let Some(name) = single_file_name(&dir) {
                filled.title = name;
            }
        }
        self.put_release(filled)?;
        if self.releases().iter().any(|x| x.digest_hex == digest_hex && x.keep_alive) {
            self.reseed_release(digest_hex);
        }
        Ok(dir)
    }

    /// Serve what we hold, on a thread of its own.
    ///
    /// No head to re-read, unlike a site: the address names one fixed
    /// thing, so the pair on disk is the only pair there has ever been for
    /// this release and a mirror can announce it without asking anybody.
    pub fn reseed_release(&self, digest_hex: &str) {
        let Some(r) = self.releases().into_iter().find(|r| r.digest_hex == digest_hex) else { return };
        if !r.keep_alive {
            return;
        }
        let dir = self.release_dir(digest_hex);
        if !self.release_is_here(digest_hex) {
            log::info(TAG, format!("'{}' is not on this device yet — nothing to serve", r.title));
            return;
        }
        let app = self.clone();
        let digest = digest_hex.to_string();
        std::thread::Builder::new()
            .name("release-reseed".into())
            .spawn(move || {
                let still = |a: &App| a.releases().iter().any(|x| x.digest_hex == digest && x.keep_alive);
                if !still(&app) {
                    return;
                }
                swarm::swarm_stop_share(r.share_key.clone());
                let res = swarm::swarm_fetch(
                    r.share_key.clone(),
                    r.digest_hex.clone(),
                    dir.to_string_lossy().into_owned(),
                    true,
                );
                match res {
                    Ok(_) => {
                        if !still(&app) {
                            swarm::swarm_stop_share(r.share_key.clone());
                            log::info(TAG, "reseed finished for a release no longer kept — stopped");
                        } else {
                            log::info(TAG, format!("'{}' serving", r.title));
                        }
                    }
                    Err(e) => log::warn(TAG, format!("reseed '{}': {e:?}", r.title)),
                }
            })
            .ok();
    }

    /// Re-park every kept release. Once per process, after the node is
    /// up: a node restart drops the seed registry.
    pub fn reseed_all_releases(&self) {
        for r in self.releases() {
            if r.keep_alive {
                self.reseed_release(&r.digest_hex);
            }
        }
    }

    /// Drop directories no saved release claims.
    pub fn sweep_release_orphans(&self) -> usize {
        let root = self.files_dir().join("releases");
        let Ok(rd) = std::fs::read_dir(&root) else { return 0 };
        let keep: std::collections::HashSet<String> =
            self.releases().into_iter().map(|r| safe_name(&r.digest_hex)).collect();
        let mut gone = 0;
        for e in rd.flatten() {
            let p = e.path();
            if !p.is_dir() {
                continue;
            }
            let name = e.file_name().to_string_lossy().into_owned();
            let base = name.strip_suffix(".part").unwrap_or(&name).to_string();
            if !keep.contains(&base) {
                let _ = std::fs::remove_dir_all(&p);
                gone += 1;
            }
        }
        if gone > 0 {
            log::info(TAG, format!("swept {gone} orphaned release(s)"));
        }
        gone
    }
}

/// A digest is hex, but the address is somebody else's text: only hex
/// reaches the filesystem.
fn safe_name(digest_hex: &str) -> String {
    digest_hex.chars().filter(|c| c.is_ascii_hexdigit()).take(64).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &str = "VLD0:JRLsL7DGWZF56faYNxCMnHifsKxpG_YQQwPWhtnBoKw:XHRpyzz_a0YglByPJW07WIE";
    const DIG: &str = "734414d4729b2eeab097f8cda30299107d74de58c8107688482e798c3669e61f";

    #[test]
    fn an_address_round_trips_and_keeps_its_colons() {
        let uri = uri_of(KEY, DIG);
        assert_eq!(parse(&uri), Some((KEY.to_string(), DIG.to_string())));
        assert_eq!(parse(&format!("  {uri}\n")), Some((KEY.to_string(), DIG.to_string())));
        assert_eq!(parse(&uri_of(KEY, &DIG.to_uppercase())).unwrap().1, DIG);
    }

    #[test]
    fn malformed_addresses_are_refused() {
        for bad in [
            "",
            "ducat:site/VLD0:abc",
            "ducat:file/",
            &format!("ducat:file/{KEY}"),
            &format!("ducat:file/:{DIG}"),
            &format!("ducat:file/{KEY}:"),
            &format!("ducat:file/{KEY}:{}", &DIG[..63]),
            &format!("ducat:file/{KEY}:{}zz", &DIG[..62]),
            &format!("{KEY}:{DIG}"),
        ] {
            assert!(parse(bad).is_none(), "accepted {bad:?}");
        }
    }

    #[test]
    fn the_store_keeps_rows_where_they_stand() {
        let dir = std::env::temp_dir().join(format!("ducat-rel-{}", std::process::id()));
        let app = App::open(&dir).unwrap();
        let a = app.add_release(&uri_of(KEY, DIG), "first").unwrap();
        let b = app.add_release(&uri_of(KEY, &DIG.replace('7', "8")), "second").unwrap();
        assert_eq!(app.releases().len(), 2);
        // Re-adding the first keeps its title and its position.
        app.add_release(&uri_of(KEY, DIG), "").unwrap();
        let rows = app.releases();
        assert_eq!(rows[0].digest_hex, a.digest_hex);
        assert_eq!(rows[0].title, "first");
        assert_eq!(rows[1].digest_hex, b.digest_hex);
        app.remove_release(&a.digest_hex).unwrap();
        assert_eq!(app.releases().len(), 1);
        std::fs::remove_dir_all(dir).ok();
    }
}

/// The one file in a directory, by name — or nothing when there are none
/// or several, because then no single name is *the* name.
fn single_file_name(dir: &std::path::Path) -> Option<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    if names.len() == 1 {
        names.pop()
    } else {
        None
    }
}
