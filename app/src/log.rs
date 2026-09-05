//! `ducat.log`, in the phone's own format.
//!
//! `<millis>|<I|W|E>|<tag>|<message>`, one line per event, written through
//! to disk on every call — the same shape and the same file name the phone
//! keeps, so everything written for reading one (the tombstone decoder,
//! the crash breadcrumbs, the exit reporter) reads the other unchanged.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

static FILE: OnceLock<Mutex<Option<PathBuf>>> = OnceLock::new();

const CAP_BYTES: u64 = 512 * 1024;

pub(crate) fn init(root: &Path) {
    let slot = FILE.get_or_init(|| Mutex::new(None));
    if let Ok(mut g) = slot.lock() {
        *g = Some(root.join("ducat.log"));
    }
}

fn write(level: char, tag: &str, msg: &str) {
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let line = format!("{millis}|{level}|{tag}|{}\n", msg.replace('\n', "\\u000A"));
    eprint!("{line}");
    let Some(slot) = FILE.get() else { return };
    let Ok(guard) = slot.lock() else { return };
    let Some(path) = guard.as_ref() else { return };
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = f.write_all(line.as_bytes());
        // Kept small enough to share. Half is dropped from the front
        // whenever the cap is passed; the newest lines always survive.
        if let Ok(m) = f.metadata() {
            if m.len() > CAP_BYTES {
                drop(f);
                if let Ok(all) = std::fs::read(path) {
                    let keep = &all[all.len().saturating_sub((CAP_BYTES / 2) as usize)..];
                    let start = keep.iter().position(|&b| b == b'\n').map(|i| i + 1).unwrap_or(0);
                    let _ = std::fs::write(path, &keep[start..]);
                }
            }
        }
    }
}

pub fn info(tag: &str, msg: impl AsRef<str>) {
    write('I', tag, msg.as_ref());
}

pub fn warn(tag: &str, msg: impl AsRef<str>) {
    write('W', tag, msg.as_ref());
}

pub fn error(tag: &str, msg: impl AsRef<str>) {
    write('E', tag, msg.as_ref());
}
