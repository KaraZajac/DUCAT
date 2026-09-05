//! A named table on disk: `prefs/<name>.json`, a JSON object of keys.
//!
//! The phone keeps each store as a handful of JSON strings under keys in a
//! `SharedPreferences`; the Compose desk kept the same shape in a JSON file
//! per store. This is that shape, with the two things the callers rely on
//! made explicit: every write is the whole table, atomically (written to a
//! sibling and renamed over), and callers take [`Store::edit`] for a
//! read-modify-write rather than reading and writing in two steps — the
//! writers are on different threads, and a fetch finishing while a
//! checkbox flips is the ordinary case, not the exotic one.
//!
//! Not encrypted at rest yet. The phone's stores are (SecurePrefs, keyed
//! from the Keystore); the Compose desk's were not. Parity is a follow-up
//! with its own design — an OS keyring on three platforms — and is called
//! out here so nobody mistakes the current state for a decision.

use std::path::PathBuf;
use std::sync::Mutex;

use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::{Map, Value};

pub struct Store {
    path: PathBuf,
    lock: Mutex<()>,
}

impl Store {
    pub(crate) fn new(path: PathBuf) -> Store {
        Store { path, lock: Mutex::new(()) }
    }

    fn read_all(&self) -> Map<String, Value> {
        std::fs::read(&self.path)
            .ok()
            .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
            .and_then(|v| v.as_object().cloned())
            .unwrap_or_default()
    }

    fn write_all(&self, m: &Map<String, Value>) -> std::io::Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(m)?)?;
        std::fs::rename(&tmp, &self.path)
    }

    /// One key, decoded, or None when absent or unreadable.
    pub fn get<T: DeserializeOwned>(&self, key: &str) -> Option<T> {
        let _g = self.lock.lock().ok()?;
        self.read_all().remove(key).and_then(value_as)
    }

    pub fn get_string(&self, key: &str) -> Option<String> {
        self.get::<String>(key)
    }

    /// Set one key, leaving the rest of the table as it was.
    pub fn put<T: Serialize>(&self, key: &str, value: &T) -> std::io::Result<()> {
        let _g = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        let mut m = self.read_all();
        m.insert(key.to_string(), serde_json::to_value(value)?);
        self.write_all(&m)
    }

    pub fn remove(&self, key: &str) -> std::io::Result<()> {
        let _g = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        let mut m = self.read_all();
        m.remove(key);
        self.write_all(&m)
    }

    /// Read-modify-write one key under the lock. `f` sees the current
    /// value (or the default) and returns the new one.
    /// Read-modify-write the whole table under one lock and one write, for
    /// the changes that must land together — a thread row and the counter
    /// it advanced, a card and the contact it produced. Returns what the
    /// closure returned.
    pub fn update<R, F>(&self, f: F) -> std::io::Result<R>
    where
        F: FnOnce(&mut Map<String, Value>) -> R,
    {
        let _g = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        let mut m = self.read_all();
        let r = f(&mut m);
        self.write_all(&m)?;
        Ok(r)
    }

    /// Read several keys under one lock, so a pair read together is a
    /// pair written together.
    pub fn view<R, F>(&self, f: F) -> R
    where
        F: FnOnce(&Map<String, Value>) -> R,
    {
        let _g = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        f(&self.read_all())
    }

    pub fn edit<T, F>(&self, key: &str, f: F) -> std::io::Result<T>
    where
        T: Serialize + DeserializeOwned + Default + Clone,
        F: FnOnce(T) -> T,
    {
        let _g = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        let mut m = self.read_all();
        let cur: T = m.get(key).cloned().and_then(value_as).unwrap_or_default();
        let next = f(cur);
        m.insert(key.to_string(), serde_json::to_value(&next)?);
        self.write_all(&m)?;
        Ok(next)
    }
}

/// A value as `T` — and when the table holds a JSON *string* where a
/// structure is wanted, the string parsed as JSON. That is how the
/// previous desk (SharedPreferences over JSON) kept every list and
/// object, so a state directory it left behind reads here as it was;
/// the next write keeps the structure natively.
pub(crate) fn value_as<T: DeserializeOwned>(v: Value) -> Option<T> {
    match serde_json::from_value::<T>(v.clone()) {
        Ok(t) => Some(t),
        Err(_) => match v {
            Value::String(s) => serde_json::from_str::<T>(&s).ok(),
            _ => None,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_table_round_trips_and_edits_in_place() {
        let dir = std::env::temp_dir().join(format!("ducat-store-{}", std::process::id()));
        let s = Store::new(dir.join("t.json"));
        assert!(s.get::<Vec<u32>>("xs").is_none());
        s.put("xs", &vec![1u32, 2]).unwrap();
        assert_eq!(s.get::<Vec<u32>>("xs"), Some(vec![1, 2]));
        let out = s.edit::<Vec<u32>, _>("xs", |mut v| { v.push(3); v }).unwrap();
        assert_eq!(out, vec![1, 2, 3]);
        // Another key does not disturb the first.
        s.put("name", &"desk").unwrap();
        assert_eq!(s.get::<Vec<u32>>("xs"), Some(vec![1, 2, 3]));
        assert_eq!(s.get_string("name").as_deref(), Some("desk"));
        s.remove("xs").unwrap();
        assert!(s.get::<Vec<u32>>("xs").is_none());
        // What the previous desk wrote: a list kept as a JSON string.
        s.put("legacy", &"[4,5]").unwrap();
        assert_eq!(s.get::<Vec<u32>>("legacy"), Some(vec![4, 5]));
        assert_eq!(s.get_string("legacy").as_deref(), Some("[4,5]"));
        std::fs::remove_dir_all(dir).ok();
    }
}
