//! Attachments (§16.15): a picture or a file in a thread, sealed under its
//! own key and parked in its own record (small) or on the swarm (big);
//! the message carries the reference. The phone's send helpers and
//! `Mailbox.fetch*Attachment`.

use std::path::{Path, PathBuf};

use ducat_mobile::contacts::{attachment_open, attachment_seal, AttachmentRef};
use ducat_mobile::node::{node_dht_create, node_dht_delete, node_dht_get, node_dht_open, node_dht_set};
use ducat_mobile::swarm;
use sha2::{Digest, Sha256};

use crate::contacts::{bump, hex, now_ms, StoredMessage, CONTACTS};
use crate::mailbox::Outgoing;
use crate::{log, App, Error};

const TAG: &str = "Attachments";
const CHUNK: usize = 32_768;
/// The record road: up to 32 chunks minus the AEAD tag.
pub const MAX_RECORD_BYTES: usize = 32 * CHUNK - 16;
/// The swarm road: sixty-four megabytes.
pub const MAX_SWARM_BYTES: usize = 64 * 1024 * 1024;
/// A picture is shrunk to under this before it goes.
const PICTURE_BUDGET: usize = 900_000;
const ATT_BUDGET_BYTES: u64 = 512 * 1024 * 1024;
const ATT_FREE_FLOOR_BYTES: u64 = 500 * 1024 * 1024;
const ATT_SWEEP_GRACE_MS: u64 = 60 * 60 * 1000;
const TRIES_BEFORE_SAYING_SO: i64 = 8;
pub const NO_ROOM: i64 = -1;

fn random_bytes(n: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(n);
    while out.len() < n {
        out.extend(ducat_mobile::create_persona_secret());
    }
    out.truncate(n);
    out
}

fn sha256_hex(b: &[u8]) -> String {
    hex(&Sha256::digest(b))
}

pub fn room_for(used: u64, free: u64, incoming: u64) -> bool {
    used + incoming <= ATT_BUDGET_BYTES && free.saturating_sub(incoming) >= ATT_FREE_FLOOR_BYTES
}

fn mime_of(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).as_deref() {
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("png") => "image/png",
        Some("webp") => "image/webp",
        Some("gif") => "image/gif",
        Some("pdf") => "application/pdf",
        Some("txt" | "md") => "text/plain",
        Some("mp3") => "audio/mpeg",
        Some("m4a" | "mp4") => "audio/mp4",
        Some("wav") => "audio/wav",
        Some("ogg" | "opus") => "audio/ogg",
        Some("zip") => "application/zip",
        _ => "application/octet-stream",
    }
}

/// A picture as it goes: JPEG, under the budget, the long edge at most
/// 1600 — the same ladder the phone walks.
fn shrink_picture(bytes: &[u8]) -> Option<Vec<u8>> {
    use image::imageops::FilterType;
    let src = image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format().ok()?.decode().ok()?;
    let (w, h) = (src.width(), src.height());
    let long = w.max(h);
    let scaled = if long > 1600 {
        let f = 1600.0 / long as f32;
        src.resize_exact(((w as f32 * f) as u32).max(1), ((h as f32 * f) as u32).max(1), FilterType::Triangle)
    } else {
        src
    };
    let rgb = scaled.to_rgb8();
    for q in [85u8, 70, 55, 40] {
        let mut out = Vec::new();
        let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, q);
        if enc.encode(rgb.as_raw(), rgb.width(), rgb.height(), image::ExtendedColorType::Rgb8).is_ok() && out.len() <= PICTURE_BUDGET {
            return Some(out);
        }
    }
    None
}

impl App {
    fn attachment_dir(&self) -> PathBuf {
        let d = self.files_dir().join("att");
        let _ = std::fs::create_dir_all(&d);
        d
    }

    /// Where an attachment's plaintext lives once here, by ciphertext hash.
    pub fn attachment_file(&self, ct_hash_hex: &str) -> PathBuf {
        self.attachment_dir().join(ct_hash_hex)
    }

    fn att_trouble(&self, hash: &str) -> i64 {
        self.store(CONTACTS).get(&format!("att_trouble_{hash}")).unwrap_or(0)
    }

    fn set_att_trouble(&self, hash: &str, delta: i64) {
        let n = if delta == NO_ROOM { NO_ROOM } else { self.att_trouble(hash).max(0) + delta };
        let _ = self.store(CONTACTS).put(&format!("att_trouble_{hash}"), &n);
    }

    fn clear_att_trouble(&self, hash: &str) {
        let _ = self.store(CONTACTS).remove(&format!("att_trouble_{hash}"));
    }

    /// Send a file to a contact. Pictures are shrunk; anything up to the
    /// record road's size rides a record, up to the swarm's a share.
    pub fn send_attachment(&self, persona_hex: &str, path: &Path, caption: Option<&str>) -> Result<(), Error> {
        let c = self.contact(persona_hex).ok_or_else(|| Error::Refused("no such contact".into()))?;
        let raw = std::fs::read(path)?;
        let mut mime = mime_of(path).to_string();
        let name = path.file_name().map(|f| f.to_string_lossy().into_owned());
        let (bytes, name, body) = if mime.starts_with("image/") {
            let small = shrink_picture(&raw).ok_or_else(|| Error::Refused("that picture could not be shrunk to send".into()))?;
            mime = "image/jpeg".into();
            (small, None, caption.map(String::from).unwrap_or_else(|| "📷".into()))
        } else {
            (raw, name.clone(), caption.map(String::from).unwrap_or_else(|| format!("📎 {}", name.clone().unwrap_or_default())))
        };
        if bytes.is_empty() {
            return Err(Error::Refused("that file is empty".into()));
        }
        if bytes.len() > MAX_SWARM_BYTES {
            return Err(Error::Refused(format!("{} MB is more than an attachment can be (64 MB)", bytes.len() / 1024 / 1024)));
        }
        let key = random_bytes(32);
        let nonce = random_bytes(24);
        let ct = attachment_seal(key.clone(), nonce.clone(), bytes.clone())?;
        let hash = Sha256::digest(&ct).to_vec();
        let hash_hex = hex(&hash);
        let reference = if bytes.len() <= MAX_RECORD_BYTES {
            let chunks = (ct.len() + CHUNK - 1) / CHUNK;
            let rec = node_dht_create(chunks as u32)?;
            for i in 0..chunks {
                let end = ((i + 1) * CHUNK).min(ct.len());
                node_dht_set(rec.key.clone(), i as u32, ct[i * CHUNK..end].to_vec())?;
            }
            AttachmentRef { record_key: Some(rec.key), swarm_key: None, swarm_digest: None, key, nonce, len: bytes.len() as u64, ct_hash: hash, mime: mime.clone(), name: name.clone() }
        } else {
            let out_dir = self.files_dir().join("swarm_out").join(&hash_hex);
            std::fs::create_dir_all(&out_dir)?;
            let blob = out_dir.join("payload.bin");
            std::fs::write(&blob, &ct)?;
            let share = swarm::swarm_seed(blob.to_string_lossy().into_owned())?;
            std::fs::write(out_dir.join("share.json"), serde_json::json!({ "share": share.share_key, "digest": share.index_digest_hex, "sent": App::now() }).to_string())?;
            AttachmentRef {
                record_key: None,
                swarm_key: Some(share.share_key),
                swarm_digest: crate::contacts::hex_to_bytes(&share.index_digest_hex),
                key,
                nonce,
                len: bytes.len() as u64,
                ct_hash: hash,
                mime: mime.clone(),
                name: name.clone(),
            }
        };
        std::fs::write(self.attachment_file(&hash_hex), &bytes)?;
        self.send(&c, Outgoing { body, attachment: Some(reference), ..Default::default() })?;
        log::info(TAG, format!("sent {} ({} bytes) to {}", mime, bytes.len(), c.display_name()));
        Ok(())
    }

    /// Re-seed every big-road attachment this desk sent, after a restart.
    pub fn reseed_attachments(&self) {
        let root = self.files_dir().join("swarm_out");
        let Ok(rd) = std::fs::read_dir(&root) else { return };
        for e in rd.flatten() {
            let dir = e.path();
            let Ok(meta) = std::fs::read_to_string(dir.join("share.json")) else { continue };
            let Ok(v) = serde_json::from_str::<serde_json::Value>(&meta) else { continue };
            let (Some(share), Some(digest)) = (v.get("share").and_then(|s| s.as_str()), v.get("digest").and_then(|s| s.as_str())) else { continue };
            swarm::swarm_stop_share(share.to_string());
            if let Err(e) = swarm::swarm_fetch(share.to_string(), digest.to_string(), dir.to_string_lossy().into_owned(), true) {
                log::warn(TAG, format!("re-park {}: {e}", dir.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default()));
            }
        }
    }

    fn dir_used(&self) -> (u64, u64) {
        let dir = self.attachment_dir();
        let used = std::fs::read_dir(&dir).map(|rd| rd.flatten().filter_map(|e| e.metadata().ok()).map(|m| m.len()).sum()).unwrap_or(0);
        // Free space is not asked of the filesystem here; the budget alone
        // bounds the store on a desk, which has room a phone does not.
        (used, u64::MAX / 2)
    }

    /// One record-road attachment the threads hold and this desk does not,
    /// least-troubled first. True when one landed.
    pub fn fetch_one_attachment(&self) -> bool {
        let mut wanted: Vec<(StoredMessage, i64)> = Vec::new();
        for c in self.contacts() {
            for m in self.thread(&c.persona_hex) {
                let Some(hash) = m.att_hash.clone() else { continue };
                if m.att_record.is_none() || m.att_key.is_none() || m.att_nonce.is_none() {
                    continue;
                }
                if self.attachment_file(&hash).exists() {
                    continue;
                }
                let t = self.att_trouble(&hash);
                wanted.push((m, t));
            }
        }
        wanted.sort_by_key(|(_, n)| (*n).max(0));
        for (m, n) in wanted {
            let hash = m.att_hash.clone().unwrap_or_default();
            if n >= TRIES_BEFORE_SAYING_SO && (now_ms() / 15_000) % (1 << (n - TRIES_BEFORE_SAYING_SO + 1).min(6)) != 0 {
                continue;
            }
            let (used, free) = self.dir_used();
            if !room_for(used, free, m.att_len) {
                if n != NO_ROOM {
                    log::warn(TAG, format!("attachment {}… not fetched: {} MiB held", &hash[..12], used / 1024 / 1024));
                    self.set_att_trouble(&hash, NO_ROOM);
                }
                continue;
            }
            let r: Result<usize, Error> = (|| {
                let rec = m.att_record.clone().unwrap_or_default();
                node_dht_open(rec.clone(), None, None)?;
                let ct_len = m.att_len as usize + 16;
                let chunks = (ct_len + CHUNK - 1) / CHUNK;
                let mut ct = Vec::with_capacity(ct_len);
                for i in 0..chunks {
                    let part = node_dht_get(rec.clone(), i as u32, true)?.ok_or_else(|| Error::Refused(format!("chunk {i} missing")))?;
                    ct.extend(part);
                }
                if sha256_hex(&ct) != hash {
                    return Err(Error::Refused("ciphertext hash mismatch".into()));
                }
                let plain = attachment_open(m.att_key.clone().unwrap_or_default(), m.att_nonce.clone().unwrap_or_default(), ct)?;
                std::fs::write(self.attachment_file(&hash), &plain)?;
                if !m.outgoing {
                    let _ = node_dht_delete(rec);
                }
                Ok(plain.len())
            })();
            match r {
                Ok(n) => {
                    log::info(TAG, format!("fetched attachment {}… ({n} bytes)", &hash[..12]));
                    self.clear_att_trouble(&hash);
                    bump();
                    return true;
                }
                Err(e) => {
                    log::warn(TAG, format!("attachment {}…: {e}", &hash[..12]));
                    self.set_att_trouble(&hash, 1);
                    return false;
                }
            }
        }
        false
    }

    /// A swarm-road attachment, on request — bigger than a poll should
    /// pull unasked.
    pub fn fetch_swarm_attachment(&self, persona_hex: &str, seq: u64, outgoing: bool) -> Result<PathBuf, Error> {
        let m = self.thread(persona_hex).into_iter().find(|m| m.seq == seq && m.outgoing == outgoing).ok_or_else(|| Error::Refused("no such message".into()))?;
        let hash = m.att_hash.clone().ok_or_else(|| Error::Refused("no attachment on that message".into()))?;
        let out = self.attachment_file(&hash);
        if out.exists() {
            return Ok(out);
        }
        let (Some(share), Some(digest), Some(key), Some(nonce)) = (m.att_swarm.clone(), m.att_swarm_digest.clone(), m.att_key.clone(), m.att_nonce.clone()) else {
            return Err(Error::Refused("that attachment is not on the swarm".into()));
        };
        let (used, free) = self.dir_used();
        if !room_for(used, free, m.att_len) {
            self.set_att_trouble(&hash, NO_ROOM);
            return Err(Error::Refused("no room kept for attachments".into()));
        }
        let tmp = self.files_dir().join("att_tmp").join(&hash);
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp)?;
        let r: Result<PathBuf, Error> = (|| {
            swarm::swarm_fetch(share.clone(), digest.clone(), tmp.to_string_lossy().into_owned(), false)?;
            let blob = walk_largest(&tmp).ok_or_else(|| Error::Refused("the share held no file".into()))?;
            let ct = std::fs::read(&blob)?;
            if sha256_hex(&ct) != hash {
                return Err(Error::Refused("ciphertext hash mismatch".into()));
            }
            let plain = attachment_open(key, nonce, ct)?;
            std::fs::write(&out, &plain)?;
            self.clear_att_trouble(&hash);
            log::info(TAG, format!("fetched swarm attachment {}… ({} bytes)", &hash[..12], plain.len()));
            Ok(out.clone())
        })();
        let _ = std::fs::remove_dir_all(&tmp);
        if r.is_err() {
            self.set_att_trouble(&hash, 1);
        }
        bump();
        r
    }

    /// Plaintext no thread refers to any more, older than an hour, goes.
    pub fn sweep_attachments(&self) -> u64 {
        let mut live = std::collections::HashSet::new();
        for c in self.contacts() {
            for m in self.thread(&c.persona_hex) {
                if let Some(h) = m.att_hash {
                    live.insert(h);
                }
            }
        }
        let settled = std::time::SystemTime::now() - std::time::Duration::from_millis(ATT_SWEEP_GRACE_MS);
        let mut freed = 0;
        if let Ok(rd) = std::fs::read_dir(self.attachment_dir()) {
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().into_owned();
                let Ok(meta) = e.metadata() else { continue };
                if !live.contains(&name) && meta.modified().map_or(false, |t| t < settled) {
                    if std::fs::remove_file(e.path()).is_ok() {
                        freed += meta.len();
                    }
                }
            }
        }
        if freed > 0 {
            log::info(TAG, format!("attachments: reclaimed {} KiB", freed / 1024));
        }
        freed
    }
}

fn walk_largest(dir: &Path) -> Option<PathBuf> {
    let mut best: Option<(u64, PathBuf)> = None;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).ok()?.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if let Ok(m) = e.metadata() {
                if best.as_ref().map_or(true, |(n, _)| m.len() > *n) {
                    best = Some((m.len(), p));
                }
            }
        }
    }
    best.map(|(_, p)| p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_budget_bounds_the_store() {
        assert!(room_for(0, u64::MAX / 2, 1_000));
        assert!(!room_for(ATT_BUDGET_BYTES, u64::MAX / 2, 1));
        assert!(!room_for(0, ATT_FREE_FLOOR_BYTES, 1));
    }

    #[test]
    fn a_picture_is_shrunk_and_a_file_is_not() {
        assert_eq!(mime_of(Path::new("a/b.PNG")), "image/png");
        assert_eq!(mime_of(Path::new("notes.md")), "text/plain");
        let mut img = image::RgbImage::new(2400, 1800);
        for (x, y, p) in img.enumerate_pixels_mut() {
            *p = image::Rgb([(x % 251) as u8, (y % 241) as u8, ((x * y) % 239) as u8]);
        }
        let mut png = Vec::new();
        image::DynamicImage::ImageRgb8(img).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
        let small = shrink_picture(&png).expect("shrunk");
        assert!(small.len() <= PICTURE_BUDGET);
        let back = image::load_from_memory(&small).unwrap();
        assert_eq!(back.width(), 1600);
    }
}
