//! Publications (§16.20): a serial with a master key, one derived key per
//! period, issues shelved on the DHT (small) or shipped on the swarm
//! (big), sold or given to subscribers by key. The phone's
//! `Publications.kt` — both sides: the press and the library.
//!
//! Periods are immutable: a period's key opens exactly the bytes shelved
//! under it, and a mirror never chases a head. The market board (listing a
//! publication where strangers browse) lives with the boards.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use ducat_mobile::contacts::{publication_master_create, publication_open_chunk, publication_period_key, publication_seal_chunk};
use ducat_mobile::node::{node_dht_create, node_dht_get, node_dht_open, node_dht_set};
use ducat_mobile::swarm;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use crate::contacts::{b64, bump, hex, unb64, BillItem, Contact, StoredMessage};
use crate::mailbox::Outgoing;
use crate::tabs::ORIGIN_PUB;
use crate::{log, App, Error};

const TAG: &str = "Publications";
const STORE: &str = "ducat_publications";
const MAX_PERIOD_ID_BYTES: usize = 64;
/// One DHT value minus the seal's overhead.
pub const SHELF_CHUNK_PLAIN: usize = 32_768 - 40;
const SHELF_MAX_CHUNKS: usize = 32;
pub const SHELF_CAP_BYTES: u64 = SHELF_CHUNK_PLAIN as u64 * SHELF_MAX_CHUNKS as u64;
pub const SHELF_MAX_RECORDS: usize = 8;
pub const SHELF_MULTI_CAP_BYTES: u64 = SHELF_CAP_BYTES * SHELF_MAX_RECORDS as u64;
const PRESS_CODE_TTL_SECS: u64 = 7 * 24 * 60 * 60;
pub const NOTE_NEW_ISSUE: &str = "A new issue";

pub fn is_safe_period_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= MAX_PERIOD_ID_BYTES && id != "." && id != ".." && !id.chars().any(|c| c == '/' || c == '\\' || c == '\0')
}

fn random_bytes(n: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(n);
    while out.len() < n {
        out.extend(ducat_mobile::create_persona_secret());
    }
    out.truncate(n);
    out
}

// ----- the reader's side: subscriptions ----------------------------------------

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Ship {
    #[serde(default)]
    pub key: String,
    #[serde(default)]
    pub digest: String,
    /// The edition actually fetched, if any — a period is immutable, so
    /// this is what "held" means.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gotkey: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub got: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Subscription {
    /// period id → period key (b64).
    #[serde(default)]
    pub periods: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub record: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head: Option<String>,
    #[serde(default)]
    pub ships: BTreeMap<String, Ship>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub mirror: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub muted: bool,
}

impl Subscription {
    pub fn period_key(&self, period: &str) -> Option<Vec<u8>> {
        self.periods.get(period).and_then(|s| unb64(s))
    }
    pub fn head_key(&self) -> Option<Vec<u8>> {
        self.head.as_deref().and_then(unb64)
    }
}

// ----- the press's side: publications ----------------------------------------

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Issue {
    #[serde(default)]
    pub file: String,
    #[serde(default)]
    pub key: String,
    #[serde(default)]
    pub digest: String,
    #[serde(default)]
    pub sent: Vec<String>,
    #[serde(default)]
    pub rec: String,
    #[serde(default)]
    pub rec_chunks: u32,
    #[serde(default)]
    pub rec_bytes: u64,
    #[serde(default)]
    pub rec_pub: String,
    #[serde(default)]
    pub rec_sec: String,
    #[serde(default)]
    pub recs: Vec<String>,
    #[serde(default)]
    pub recs_pub: Vec<String>,
    #[serde(default)]
    pub recs_sec: Vec<String>,
    /// subscriber → tab id.
    #[serde(default)]
    pub billed: BTreeMap<String, String>,
}

impl Issue {
    pub fn on_swarm(&self) -> bool {
        !self.key.is_empty() && !self.digest.is_empty()
    }
    pub fn on_shelf(&self) -> bool {
        !self.rec.is_empty() || !self.recs.is_empty()
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Publication {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub master: String,
    #[serde(default)]
    pub created: u64,
    #[serde(default)]
    pub price: u64,
    #[serde(default)]
    pub subs: Vec<String>,
    #[serde(default)]
    pub issues: BTreeMap<String, Issue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root_rec: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root_pub: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root_sec: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub press_code: Option<String>,
    #[serde(default)]
    pub press_code_exp: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mkt_cat: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mkt_board: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mkt_subkey: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mkt_lang: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mkt_blurb: Option<String>,
    #[serde(default)]
    pub mkt_at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mkt_cell: Option<String>,
}

impl Publication {
    /// Newest first, by period id — periods are named so they sort.
    pub fn issues_sorted(&self) -> Vec<(String, Issue)> {
        let mut v: Vec<(String, Issue)> = self.issues.iter().map(|(k, i)| (k.clone(), i.clone())).collect();
        v.sort_by(|a, b| b.0.cmp(&a.0));
        v
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct Due {
    pub pub_id: String,
    pub period_id: String,
    pub persona_hex: String,
}

fn take<T: serde::de::DeserializeOwned>(m: &Map<String, Value>, key: &str) -> Option<T> {
    m.get(key).cloned().and_then(crate::store::value_as)
}

fn set<T: Serialize>(m: &mut Map<String, Value>, key: &str, v: &T) {
    if let Ok(v) = serde_json::to_value(v) {
        m.insert(key.to_string(), v);
    }
}

impl App {
    fn p(&self) -> crate::store::Store {
        self.store(STORE)
    }

    // ----- subscriptions (reader) -------------------------------------------------

    pub fn subscriptions(&self) -> BTreeMap<String, Subscription> {
        self.p().get("subs").unwrap_or_default()
    }

    pub fn subscription(&self, publisher_hex: &str) -> Option<Subscription> {
        self.subscriptions().remove(publisher_hex)
    }

    fn edit_subscription<F: FnOnce(&mut Subscription)>(&self, publisher_hex: &str, f: F) -> Result<(), Error> {
        self.p().update(|m| {
            let mut all: BTreeMap<String, Subscription> = take(m, "subs").unwrap_or_default();
            let sub = all.entry(publisher_hex.to_string()).or_default();
            f(sub);
            set(m, "subs", &all);
        })?;
        bump();
        Ok(())
    }

    /// A period key arrived (kind 13): file it, with the shelf and the
    /// shipment if they came along. Refused from a muted publisher, or
    /// for a period id that is not a name.
    pub fn absorb_key(&self, publisher_hex: &str, m: &StoredMessage) -> Result<(), Error> {
        let (Some(period), Some(key)) = (m.pub_period_id.as_deref(), m.pub_period_key.as_deref()) else { return Ok(()) };
        if !is_safe_period_id(period) {
            log::warn(TAG, format!("refused a period id from {}… that is not a name: {}", &publisher_hex[..8.min(publisher_hex.len())], period.chars().take(72).collect::<String>()));
            return Ok(());
        }
        if self.subscription(publisher_hex).map_or(false, |s| s.muted) {
            log::info(TAG, format!("muted {}… — key not filed", &publisher_hex[..8.min(publisher_hex.len())]));
            return Ok(());
        }
        let record = m.pub_record.clone();
        let head = m.pub_head_key.as_deref().map(b64);
        let ship = match (m.pub_swarm_key.clone(), m.pub_swarm_digest.clone()) {
            (Some(k), Some(d)) => Some((k, d)),
            _ => None,
        };
        let period_s = period.to_string();
        let key_b64 = b64(key);
        self.edit_subscription(publisher_hex, move |s| {
            s.periods.insert(period_s.clone(), key_b64);
            if record.is_some() {
                s.record = record;
            }
            if head.is_some() {
                s.head = head;
            }
            if let Some((k, d)) = ship {
                let e = s.ships.entry(period_s).or_default();
                e.key = k;
                e.digest = d;
            }
        })?;
        log::info(TAG, format!("filed period '{period}' from {}…", &publisher_hex[..8.min(publisher_hex.len())]));
        Ok(())
    }

    pub fn subscribed_publishers(&self) -> Vec<String> {
        self.subscriptions().into_keys().collect()
    }

    pub fn set_muted(&self, publisher_hex: &str, muted: bool) -> Result<(), Error> {
        self.edit_subscription(publisher_hex, |s| s.muted = muted)
    }

    /// Mirroring keeps every fetched issue seeding; off, the shares stop.
    pub fn set_mirroring(&self, publisher_hex: &str, on: bool) -> Result<(), Error> {
        self.edit_subscription(publisher_hex, |s| s.mirror = on)?;
        if !on {
            if let Some(sub) = self.subscription(publisher_hex) {
                for (_, ship) in sub.ships {
                    if !ship.key.is_empty() {
                        swarm::swarm_stop_share(ship.key);
                    }
                }
            }
            log::info(TAG, format!("stopped mirroring {}…", &publisher_hex[..8.min(publisher_hex.len())]));
        }
        Ok(())
    }

    pub fn held_edition(&self, publisher_hex: &str, period: &str) -> Option<(String, String)> {
        let ship = self.subscription(publisher_hex)?.ships.remove(period)?;
        Some((ship.gotkey?, ship.got?))
    }

    fn mark_held(&self, publisher_hex: &str, period: &str, share_key: &str, digest_hex: &str) -> Result<(), Error> {
        if share_key.is_empty() || digest_hex.is_empty() {
            return Ok(());
        }
        let (p, k, d) = (period.to_string(), share_key.to_string(), digest_hex.to_string());
        self.edit_subscription(publisher_hex, move |s| {
            let e = s.ships.entry(p).or_default();
            e.gotkey = Some(k);
            e.got = Some(d);
        })
    }

    /// What the publisher's index says is on the shelf: period → bytes.
    pub fn shelved_periods(&self, publisher_hex: &str) -> BTreeMap<String, u64> {
        let v: Value = self.p().get("shelfseen").unwrap_or(Value::Null);
        v.get(publisher_hex)
            .and_then(|o| o.get("periods"))
            .and_then(Value::as_object)
            .map(|o| o.iter().map(|(k, v)| (k.clone(), v.as_u64().unwrap_or(0))).collect())
            .unwrap_or_default()
    }

    pub fn shelf_seen_at(&self, publisher_hex: &str) -> u64 {
        let v: Value = self.p().get("shelfseen").unwrap_or(Value::Null);
        v.get(publisher_hex).and_then(|o| o.get("at")).and_then(Value::as_u64).unwrap_or(0)
    }

    /// Read a publisher's index. -1 when there is no shelf to read.
    pub fn refresh_shelf(&self, publisher_hex: &str) -> i64 {
        let Some(sub) = self.subscription(publisher_hex) else { return -1 };
        let (Some(root), Some(head)) = (sub.record.clone(), sub.head_key()) else { return -1 };
        let index = (|| -> Result<Value, Error> {
            node_dht_open(root.clone(), None, None)?;
            let raw = node_dht_get(root.clone(), 0, true)?.ok_or_else(|| Error::Refused("no index".into()))?;
            let plain = publication_open_chunk(head, root.clone(), 0, raw)?;
            Ok(serde_json::from_slice(&plain)?)
        })();
        let Ok(index) = index else { return -1 };
        let periods: Map<String, Value> = index
            .get("periods")
            .and_then(Value::as_object)
            .map(|o| o.iter().map(|(k, v)| (k.clone(), Value::from(v.get("bytes").and_then(Value::as_u64).unwrap_or(0)))).collect())
            .unwrap_or_default();
        let n = periods.len() as i64;
        let _ = self.p().update(|m| {
            let mut all: Map<String, Value> = take(m, "shelfseen").unwrap_or_default();
            all.insert(publisher_hex.to_string(), json!({ "periods": periods, "at": crate::contacts::now_ms() }));
            set(m, "shelfseen", &all);
        });
        bump();
        n
    }

    pub fn shipment(&self, publisher_hex: &str, period: &str) -> Option<(String, String)> {
        let ship = self.subscription(publisher_hex)?.ships.remove(period)?;
        (!ship.key.is_empty() && !ship.digest.is_empty()).then_some((ship.key, ship.digest))
    }

    /// Where a fetched issue lives.
    pub fn issue_dir(&self, publisher_hex: &str, period: &str) -> PathBuf {
        self.files_dir().join("library").join(publisher_hex).join(period)
    }

    pub fn fetched_bytes(&self, publisher_hex: &str, period: &str) -> Option<u64> {
        let dir = self.issue_dir(publisher_hex, period);
        let mut total = 0;
        let mut any = false;
        for e in std::fs::read_dir(&dir).ok()?.flatten() {
            if let Ok(md) = e.metadata() {
                if md.is_file() {
                    total += md.len();
                    any = true;
                }
            }
        }
        any.then_some(total)
    }

    /// Pull one issue off the shelf, chunk by chunk, into `out_dir`.
    pub fn fetch_shelf(&self, publisher_hex: &str, period: &str, out_dir: &Path) -> Result<PathBuf, Error> {
        let sub = self.subscription(publisher_hex).ok_or_else(|| Error::Refused("no subscription filed".into()))?;
        let root = sub.record.clone().ok_or_else(|| Error::Refused("no shelf on file".into()))?;
        let head = sub.head_key().ok_or_else(|| Error::Refused("no head key on file".into()))?;
        let period_key = sub.period_key(period).ok_or_else(|| Error::Refused(format!("no key for '{period}'")))?;
        node_dht_open(root.clone(), None, None)?;
        let raw = node_dht_get(root.clone(), 0, true)?.ok_or_else(|| Error::Refused("the shelf's index is not on the network".into()))?;
        let index: Value = serde_json::from_slice(&publication_open_chunk(head, root.clone(), 0, raw)?)?;
        let entry = index.get("periods").and_then(|p| p.get(period)).cloned().ok_or_else(|| Error::Refused(format!("'{period}' is not on the shelf yet")))?;
        let chunks = entry.get("chunks").and_then(Value::as_u64).unwrap_or(0) as usize;
        let name = entry
            .get("name")
            .and_then(Value::as_str)
            .map(|n| Path::new(n).file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default())
            .filter(|n| !n.is_empty() && n != "." && n != "..")
            .unwrap_or_else(|| "issue.bin".into());
        let recs: Vec<String> = match entry.get("recs").and_then(Value::as_array) {
            Some(a) => a.iter().filter_map(|v| v.as_str().map(String::from)).collect(),
            None => vec![entry.get("rec").and_then(Value::as_str).map(String::from).ok_or_else(|| Error::Refused("the index names no shelf".into()))?],
        };
        std::fs::create_dir_all(out_dir)?;
        let out = out_dir.join(&name);
        let mut bytes: Vec<u8> = Vec::new();
        let mut left = chunks;
        for rec in recs {
            let here = SHELF_MAX_CHUNKS.min(left);
            node_dht_open(rec.clone(), None, None)?;
            for i in 0..here {
                let value = node_dht_get(rec.clone(), i as u32, true)?.ok_or_else(|| Error::Refused(format!("chunk {i} is missing from the shelf")))?;
                bytes.extend(publication_open_chunk(period_key.clone(), rec.clone(), i as u32, value)?);
            }
            left -= here;
        }
        if left != 0 {
            return Err(Error::Refused("the index promised more chunks than its shelves hold".into()));
        }
        std::fs::write(&out, &bytes)?;
        Ok(out)
    }

    /// Fetch an issue: the swarm shipment if there is one, else the shelf.
    /// Mirroring keeps a swarm fetch seeding.
    pub fn fetch_issue(&self, publisher_hex: &str, period: &str) -> Result<PathBuf, Error> {
        let dir = self.issue_dir(publisher_hex, period);
        if self.fetched_bytes(publisher_hex, period).is_some() {
            return Ok(dir);
        }
        let mirroring = self.subscription(publisher_hex).map_or(false, |s| s.mirror);
        let part = dir.with_extension("part");
        let _ = std::fs::remove_dir_all(&part);
        std::fs::create_dir_all(&part)?;
        if let Some((key, digest)) = self.shipment(publisher_hex, period) {
            log::info(TAG, format!("fetching '{period}' from {}… off the swarm", &publisher_hex[..8.min(publisher_hex.len())]));
            swarm::swarm_fetch(key.clone(), digest.clone(), part.to_string_lossy().into_owned(), mirroring)?;
            self.mark_held(publisher_hex, period, &key, &digest)?;
        } else {
            log::info(TAG, format!("fetching '{period}' from {}… off the shelf", &publisher_hex[..8.min(publisher_hex.len())]));
            self.fetch_shelf(publisher_hex, period, &part)?;
        }
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::rename(&part, &dir)?;
        bump();
        Ok(dir)
    }

    /// Put every mirrored issue back on the network after a restart.
    pub fn reseed_library(&self) {
        for (publisher, sub) in self.subscriptions() {
            if !sub.mirror {
                continue;
            }
            for (period, ship) in sub.ships {
                let (Some(key), Some(digest)) = (ship.gotkey, ship.got) else { continue };
                if self.fetched_bytes(&publisher, &period).is_none() {
                    continue;
                }
                let dir = self.issue_dir(&publisher, &period);
                swarm::swarm_stop_share(key.clone());
                if let Err(e) = swarm::swarm_fetch(key, digest, dir.to_string_lossy().into_owned(), true) {
                    log::warn(TAG, format!("re-park '{period}': {e}"));
                }
            }
        }
    }

    /// §16.20's ask: a kind-16 naming the period wanted.
    pub fn ask_for_period(&self, c: &Contact, period: &str) -> Result<(), Error> {
        if !is_safe_period_id(period) {
            return Err(Error::Refused("that period is not a name".into()));
        }
        self.send(
            c,
            Outgoing {
                body: format!("Could I have the issue '{period}'?"),
                kind: 16,
                publication: Some(ducat_mobile::contacts::PublicationSend {
                    period_id: None,
                    period_key: None,
                    record_key: None,
                    head_key: None,
                    swarm_key: None,
                    swarm_digest: None,
                    wanted_period: Some(period.to_string()),
                }),
                ..Default::default()
            },
        )?;
        Ok(())
    }

    pub fn asked_for(&self, publisher_hex: &str, period: &str) -> bool {
        self.thread(publisher_hex).iter().any(|m| m.outgoing && m.kind == 16 && m.pub_wanted.as_deref() == Some(period))
    }

    // ----- publications (press) --------------------------------------------------

    pub fn publications(&self) -> BTreeMap<String, Publication> {
        self.p().get("pubs").unwrap_or_default()
    }

    pub fn publication(&self, pub_id: &str) -> Option<Publication> {
        self.publications().remove(pub_id)
    }

    fn edit_pub<F: FnOnce(&mut Publication)>(&self, pub_id: &str, f: F) -> Result<bool, Error> {
        let hit = self.p().update(|m| {
            let mut all: BTreeMap<String, Publication> = take(m, "pubs").unwrap_or_default();
            let Some(p) = all.get_mut(pub_id) else { return false };
            f(p);
            set(m, "pubs", &all);
            true
        })?;
        bump();
        Ok(hit)
    }

    /// A new publication: a master key, and an id from its first bytes.
    pub fn create_publication(&self, title: &str) -> Result<String, Error> {
        let master = publication_master_create();
        let id = hex(&master[..8]);
        let title = ducat_mobile::contacts::clean_display_text(title.trim().to_string());
        let created = App::now();
        let idc = id.clone();
        self.p().update(|m| {
            let mut all: BTreeMap<String, Publication> = take(m, "pubs").unwrap_or_default();
            all.insert(idc, Publication { title, master: b64(&master), created, ..Default::default() });
            set(m, "pubs", &all);
        })?;
        bump();
        Ok(id)
    }

    pub fn delete_publication(&self, pub_id: &str) -> Result<(), Error> {
        self.p().update(|m| {
            let mut all: BTreeMap<String, Publication> = take(m, "pubs").unwrap_or_default();
            all.remove(pub_id);
            set(m, "pubs", &all);
            if m.get("press_pub").and_then(Value::as_str) == Some(pub_id) {
                m.remove("press_pub");
            }
        })?;
        bump();
        Ok(())
    }

    pub fn set_price(&self, pub_id: &str, pxmr: u64) -> Result<(), Error> {
        self.edit_pub(pub_id, |p| p.price = pxmr).map(|_| ())
    }

    pub fn set_subscriber(&self, pub_id: &str, persona_hex: &str, on: bool) -> Result<(), Error> {
        self.edit_pub(pub_id, |p| {
            p.subs.retain(|h| h != persona_hex);
            if on {
                p.subs.push(persona_hex.to_string());
            }
        })
        .map(|_| ())
    }

    pub fn period_key(&self, pub_id: &str, period: &str) -> Option<Vec<u8>> {
        let master = unb64(&self.publication(pub_id)?.master)?;
        publication_period_key(master, period.to_string()).ok()
    }

    /// Hand one period's key to one subscriber (kind 13); the shelf's
    /// address and head ride the first key only.
    pub fn send_period(&self, c: &Contact, pub_id: &str, period: &str, note: &str) -> Result<(), Error> {
        let key = self.period_key(pub_id, period).ok_or_else(|| Error::Refused("no such publication".into()))?;
        let p = self.publication(pub_id).ok_or_else(|| Error::Refused("no such publication".into()))?;
        let issue = p.issues.get(period).cloned().unwrap_or_default();
        let first_time = !self.thread(&c.persona_hex).iter().any(|m| m.outgoing && m.kind == 13);
        let shelf = self.shelf_of(pub_id);
        let body = if note.trim().is_empty() { NOTE_NEW_ISSUE.to_string() } else { note.trim().to_string() };
        self.send(
            c,
            Outgoing {
                body,
                kind: 13,
                publication: Some(ducat_mobile::contacts::PublicationSend {
                    period_id: Some(period.to_string()),
                    period_key: Some(key),
                    record_key: if first_time { shelf.as_ref().map(|s| s.0.clone()) } else { None },
                    head_key: if first_time { shelf.as_ref().map(|s| s.1.clone()) } else { None },
                    swarm_key: Some(issue.key.clone()).filter(|k| !k.is_empty()),
                    swarm_digest: Some(issue.digest.as_str()).filter(|d| !d.is_empty()).and_then(crate::contacts::hex_to_bytes),
                    wanted_period: None,
                }),
                ..Default::default()
            },
        )?;
        self.mark_sent(pub_id, period, &c.persona_hex)?;
        Ok(())
    }

    pub fn mark_sent(&self, pub_id: &str, period: &str, persona_hex: &str) -> Result<(), Error> {
        self.edit_pub(pub_id, |p| {
            let i = p.issues.entry(period.to_string()).or_default();
            if !i.sent.iter().any(|h| h == persona_hex) {
                i.sent.push(persona_hex.to_string());
            }
        })
        .map(|_| ())
    }

    /// Bill every subscriber (or `only` one) for a period, one tab each.
    /// Returns how many bills went out.
    pub fn bill_period(&self, pub_id: &str, period: &str, only: Option<&str>) -> usize {
        let Some(p) = self.publication(pub_id) else { return 0 };
        if p.price == 0 {
            return 0;
        }
        let already: Vec<String> = p.issues.get(period).map(|i| i.billed.keys().cloned().collect()).unwrap_or_default();
        let mut sent = 0;
        for hex in p.subs.clone() {
            if only.map_or(false, |o| o != hex) || already.contains(&hex) || self.contact(&hex).is_none() {
                continue;
            }
            let Ok(opened) = self.open_tab(&hex, ORIGIN_PUB) else { continue };
            let title = p.title.clone();
            let price = p.price;
            let Ok(Some(lined)) = self.mutate_tab(&opened.id, move |mut t| {
                t.lines = vec![BillItem { description: format!("{title} — {period}"), amount_pxmr: price }];
                t
            }) else {
                continue;
            };
            match self.settle_tab(&lined) {
                Ok(_) => {
                    let _ = self.edit_pub(pub_id, |p| {
                        p.issues.entry(period.to_string()).or_default().billed.insert(hex.clone(), opened.id.clone());
                    });
                    sent += 1;
                }
                Err(e) => {
                    let _ = self.delete_tab(&opened.id);
                    log::warn(TAG, format!("bill to {}… failed: {e}", &hex[..8.min(hex.len())]));
                }
            }
        }
        sent
    }

    pub fn billed_tab_ids(&self) -> Vec<String> {
        self.publications().values().flat_map(|p| p.issues.values().flat_map(|i| i.billed.values().cloned())).collect()
    }

    /// Issues whose bill was paid and whose key has not gone out.
    pub fn due_settled(&self) -> Vec<Due> {
        let tabs: BTreeMap<String, crate::tabs::RunningTab> = self.tabs().into_iter().map(|t| (t.id.clone(), t)).collect();
        let mut out = Vec::new();
        for (pub_id, p) in self.publications() {
            for (period, issue) in p.issues {
                if !issue.on_swarm() && !issue.on_shelf() {
                    continue;
                }
                for (hex, tab_id) in issue.billed.iter() {
                    if issue.sent.contains(hex) {
                        continue;
                    }
                    if tabs.get(tab_id).map_or(false, |t| t.state.starts_with("paid")) {
                        out.push(Due { pub_id: pub_id.clone(), period_id: period.clone(), persona_hex: hex.clone() });
                    }
                }
            }
        }
        out
    }

    /// Paid → the key goes out.
    pub fn reconcile_settled(&self) {
        for d in self.due_settled() {
            let Some(c) = self.contact(&d.persona_hex) else { continue };
            match self.send_period(&c, &d.pub_id, &d.period_id, "") {
                Ok(()) => log::info(TAG, format!("settled → sent '{}' to {}…", d.period_id, &d.persona_hex[..8.min(d.persona_hex.len())])),
                Err(e) => log::warn(TAG, format!("settled but could not send '{}': {e}", d.period_id)),
            }
        }
    }

    // ----- the shelf ---------------------------------------------------------------

    pub fn shelf_of(&self, pub_id: &str) -> Option<(String, Vec<u8>)> {
        let p = self.publication(pub_id)?;
        Some((p.root_rec?, unb64(&p.head?)?))
    }

    fn ensure_shelf(&self, pub_id: &str) -> Result<(String, Vec<u8>), Error> {
        if let Some(s) = self.shelf_of(pub_id) {
            return Ok(s);
        }
        let head = random_bytes(32);
        let rec = node_dht_create(1)?;
        let (k, hp, op, os) = (rec.key.clone(), b64(&head), b64(&rec.owner_public), b64(&rec.owner_secret));
        self.edit_pub(pub_id, move |p| {
            p.head = Some(hp);
            p.root_rec = Some(k);
            p.root_pub = Some(op);
            p.root_sec = Some(os);
        })?;
        Ok((rec.key, head))
    }

    fn seal_to(key: &[u8], record_key: &str, subkey: u32, plain: &[u8]) -> Result<Vec<u8>, Error> {
        Ok(publication_seal_chunk(key.to_vec(), record_key.to_string(), subkey, random_bytes(24), plain.to_vec())?)
    }

    fn write_index<F: FnOnce(&mut Value)>(&self, pub_id: &str, mutate: F) -> Result<(), Error> {
        let p = self.publication(pub_id).ok_or_else(|| Error::Refused("no such publication".into()))?;
        let root = p.root_rec.clone().ok_or_else(|| Error::Refused("no shelf".into()))?;
        let head = p.head.as_deref().and_then(unb64).ok_or_else(|| Error::Refused("no head".into()))?;
        let own_pub = p.root_pub.as_deref().and_then(unb64).ok_or_else(|| Error::Refused("no shelf owner key".into()))?;
        let own_sec = p.root_sec.as_deref().and_then(unb64).ok_or_else(|| Error::Refused("no shelf owner key".into()))?;
        node_dht_open(root.clone(), Some(own_pub), Some(own_sec))?;
        let existing = node_dht_get(root.clone(), 0, true).ok().flatten();
        let mut index = existing
            .and_then(|raw| publication_open_chunk(head.clone(), root.clone(), 0, raw).ok())
            .and_then(|plain| serde_json::from_slice::<Value>(&plain).ok())
            .unwrap_or_else(|| json!({ "v": 1, "periods": {} }));
        mutate(&mut index);
        let bytes = serde_json::to_vec(&index)?;
        node_dht_set(root.clone(), 0, App::seal_to(&head, &root, 0, &bytes)?)?;
        Ok(())
    }

    /// Put a file on the shelf under a period: sealed chunks across as
    /// many records as it needs, then the index.
    pub fn shelve_issue(&self, pub_id: &str, period: &str, file: &Path) -> Result<(), Error> {
        let bytes = std::fs::read(file)?;
        if bytes.is_empty() || bytes.len() as u64 > SHELF_MULTI_CAP_BYTES {
            return Err(Error::Refused(format!("{} bytes is not shelf-sized (up to {} MB)", bytes.len(), SHELF_MULTI_CAP_BYTES / 1_000_000)));
        }
        let key = self.period_key(pub_id, period).ok_or_else(|| Error::Refused("no such publication".into()))?;
        self.ensure_shelf(pub_id)?;
        let total_chunks = (bytes.len() + SHELF_CHUNK_PLAIN - 1) / SHELF_CHUNK_PLAIN;
        let mut slabs: Vec<std::ops::Range<usize>> = Vec::new();
        let mut at = 0;
        while at < total_chunks {
            let n = SHELF_MAX_CHUNKS.min(total_chunks - at);
            slabs.push(at..at + n);
            at += n;
        }
        let mut recs = Vec::new();
        for slab in &slabs {
            recs.push(node_dht_create(slab.len() as u32)?);
        }
        for (r, slab) in recs.iter().zip(slabs.iter()) {
            for (sub, i) in slab.clone().enumerate() {
                let end = ((i + 1) * SHELF_CHUNK_PLAIN).min(bytes.len());
                node_dht_set(r.key.clone(), sub as u32, App::seal_to(&key, &r.key, sub as u32, &bytes[i * SHELF_CHUNK_PLAIN..end])?)?;
            }
        }
        let name = file.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_else(|| "issue.bin".into());
        let keys: Vec<String> = recs.iter().map(|r| r.key.clone()).collect();
        let n_bytes = bytes.len() as u64;
        let (keys_i, name_i) = (keys.clone(), name.clone());
        self.write_index(pub_id, move |index| {
            let mut entry = json!({ "chunks": total_chunks, "bytes": n_bytes, "name": name_i });
            if keys_i.len() == 1 {
                entry["rec"] = Value::from(keys_i[0].clone());
            } else {
                entry["recs"] = Value::from(keys_i.clone());
            }
            if !index["periods"].is_object() {
                index["periods"] = json!({});
            }
            index["periods"][period] = entry;
        })?;
        let file_s = file.to_string_lossy().into_owned();
        let pubs: Vec<String> = recs.iter().map(|r| b64(&r.owner_public)).collect();
        let secs: Vec<String> = recs.iter().map(|r| b64(&r.owner_secret)).collect();
        self.edit_pub(pub_id, move |p| {
            let i = p.issues.entry(period.to_string()).or_default();
            i.file = file_s;
            i.rec = keys[0].clone();
            i.rec_chunks = total_chunks as u32;
            i.rec_bytes = n_bytes;
            i.rec_pub = pubs[0].clone();
            i.rec_sec = secs[0].clone();
            i.recs = keys;
            i.recs_pub = pubs;
            i.recs_sec = secs;
        })?;
        log::info(TAG, format!("shelved '{period}': {total_chunks} chunk(s) over {} record(s)", slabs.len()));
        Ok(())
    }

    /// Ship a file on the swarm under a period (for anything bigger than
    /// a shelf). The share stays seeding while the desk runs.
    pub fn ship_issue(&self, pub_id: &str, period: &str, file: &Path) -> Result<(), Error> {
        let staging = self.files_dir().join("publish_staging").join(pub_id).join(format!("{}", crate::contacts::now_ms()));
        std::fs::create_dir_all(&staging)?;
        let name = file.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_else(|| "issue.bin".into());
        let dest = staging.join(&name);
        std::fs::copy(file, &dest)?;
        let share = swarm::swarm_seed(dest.to_string_lossy().into_owned())?;
        let (k, d, f) = (share.share_key.clone(), share.index_digest_hex.clone(), dest.to_string_lossy().into_owned());
        self.edit_pub(pub_id, move |p| {
            let i = p.issues.entry(period.to_string()).or_default();
            i.file = f;
            i.key = k;
            i.digest = d;
        })?;
        log::info(TAG, format!("shipped '{period}' on the swarm: {}…", &share.share_key[..16.min(share.share_key.len())]));
        Ok(())
    }

    /// Re-seed every shipped issue after a restart.
    pub fn reseed_issues(&self) {
        for (_, p) in self.publications() {
            for (period, i) in p.issues {
                if !i.on_swarm() || !Path::new(&i.file).exists() {
                    continue;
                }
                let dir = Path::new(&i.file).parent().map(|d| d.to_path_buf()).unwrap_or_default();
                swarm::swarm_stop_share(i.key.clone());
                if let Err(e) = swarm::swarm_fetch(i.key, i.digest, dir.to_string_lossy().into_owned(), true) {
                    log::warn(TAG, format!("re-seed '{period}': {e}"));
                }
            }
        }
    }

    /// Publish an issue to everyone: shelved and/or shipped already; the
    /// key goes to every subscriber a free publication has, and to nobody
    /// until they pay for a priced one.
    pub fn release_issue(&self, pub_id: &str, period: &str, note: &str) -> Result<usize, Error> {
        let p = self.publication(pub_id).ok_or_else(|| Error::Refused("no such publication".into()))?;
        let issue = p.issues.get(period).ok_or_else(|| Error::Refused("nothing shelved or shipped under that period".into()))?;
        if !issue.on_shelf() && !issue.on_swarm() {
            return Err(Error::Refused("nothing shelved or shipped under that period".into()));
        }
        if p.price > 0 {
            let n = self.bill_period(pub_id, period, None);
            self.reconcile_settled();
            return Ok(n);
        }
        let mut sent = 0;
        for hex in p.subs {
            if issue.sent.contains(&hex) {
                continue;
            }
            let Some(c) = self.contact(&hex) else { continue };
            match self.send_period(&c, pub_id, period, note) {
                Ok(()) => sent += 1,
                Err(e) => log::warn(TAG, format!("'{period}' to {}: {e}", c.display_name())),
            }
        }
        Ok(sent)
    }

    /// Once an hour: rewrite every index and re-push each shelf's first
    /// chunk, so the records stay held.
    pub fn tend_shelf(&self) {
        let last: u64 = self.p().get("shelf_tended").unwrap_or(0);
        let now = crate::contacts::now_ms();
        if now.saturating_sub(last) < 60 * 60 * 1000 {
            return;
        }
        let _ = self.p().put("shelf_tended", &now);
        for (pub_id, p) in self.publications() {
            if self.shelf_of(&pub_id).is_none() {
                continue;
            }
            if let Err(e) = self.write_index(&pub_id, |_| {}) {
                log::warn(TAG, format!("tend index: {e}"));
            }
            for (period, i) in p.issues {
                let recs = if i.recs.is_empty() { vec![i.rec.clone()] } else { i.recs.clone() };
                let pubs = if i.recs_pub.is_empty() { vec![i.rec_pub.clone()] } else { i.recs_pub.clone() };
                let secs = if i.recs_sec.is_empty() { vec![i.rec_sec.clone()] } else { i.recs_sec.clone() };
                for j in 0..recs.len() {
                    if recs[j].is_empty() {
                        continue;
                    }
                    let r: Result<(), Error> = (|| {
                        node_dht_open(recs[j].clone(), pubs.get(j).and_then(|s| unb64(s)), secs.get(j).and_then(|s| unb64(s)))?;
                        if let Some(v) = node_dht_get(recs[j].clone(), 0, true)? {
                            node_dht_set(recs[j].clone(), 0, v)?;
                        }
                        Ok(())
                    })();
                    if let Err(e) = r {
                        log::warn(TAG, format!("tend '{period}': {e}"));
                    }
                }
            }
        }
    }

    // ----- cards: scan-to-subscribe --------------------------------------------------

    pub fn bind_card(&self, pub_id: &str, inbox_key: &str) -> Result<(), Error> {
        self.p().update(|m| {
            let mut map: BTreeMap<String, String> = take(m, "subcards").unwrap_or_default();
            map.insert(inbox_key.to_string(), pub_id.to_string());
            set(m, "subcards", &map);
        })?;
        Ok(())
    }

    /// A card bound to a publication was answered: enroll the claimant,
    /// and give them the latest issue or a bill for it.
    pub fn enroll_from_card(&self, inbox_key: &str, subscriber_hex: &str) -> Result<(), Error> {
        let map: BTreeMap<String, String> = self.p().get("subcards").unwrap_or_default();
        let Some(pub_id) = map.get(inbox_key).cloned() else { return Ok(()) };
        self.set_subscriber(&pub_id, subscriber_hex, true)?;
        log::info(TAG, format!("card claim enrolled {}… into '{pub_id}'", &subscriber_hex[..8.min(subscriber_hex.len())]));
        let Some(c) = self.contact(subscriber_hex) else { return Ok(()) };
        let Some(p) = self.publication(&pub_id) else { return Ok(()) };
        let latest = p.issues_sorted().into_iter().next();
        if p.price > 0 {
            let period = latest.map(|(id, _)| id).unwrap_or_else(|| current_month());
            self.bill_period(&pub_id, &period, Some(subscriber_hex));
        } else if let Some((period, _)) = latest {
            self.send_period(&c, &pub_id, &period, "")?;
        }
        Ok(())
    }

    /// The standing "subscribe by scanning" code for a publication, good
    /// for a week; cut fresh when the old one is within a day of expiring.
    pub fn press_code(&self, pub_id: &str) -> Result<String, Error> {
        let p = self.publication(pub_id).ok_or_else(|| Error::Refused("no such publication".into()))?;
        let now = App::now();
        if let Some(uri) = p.press_code.clone() {
            if p.press_code_exp.saturating_sub(now) > PRESS_CODE_TTL_SECS / 4 {
                return Ok(uri);
            }
        }
        let worn = self.worn()?;
        let h = self.issue_card(Some(&p.title), PRESS_CODE_TTL_SECS, "publish", Some(&worn))?;
        self.bind_card(pub_id, &h.inbox_key)?;
        let uri = h.uri.clone();
        self.edit_pub(pub_id, move |p| {
            p.press_code = Some(uri);
            p.press_code_exp = now + PRESS_CODE_TTL_SECS;
        })?;
        Ok(h.uri)
    }

    // ----- asks ------------------------------------------------------------------------

    fn wanted_target(&self, reader_hex: &str, period: &str) -> Option<String> {
        if !is_safe_period_id(period) {
            return None;
        }
        let pubs = self.publications();
        let holding: Vec<&String> = pubs.iter().filter(|(_, p)| p.issues.contains_key(period)).map(|(id, _)| id).collect();
        let mine: Vec<&String> = {
            let m: Vec<&String> = holding.iter().copied().filter(|id| pubs[*id].subs.iter().any(|s| s == reader_hex)).collect();
            if m.is_empty() { holding.clone() } else { m }
        };
        let short = &reader_hex[..8.min(reader_hex.len())];
        let pub_id = match mine.len() {
            1 => mine[0].clone(),
            0 => {
                log::info(TAG, format!("{short}… asked for '{period}' — nothing of ours holds it"));
                return None;
            }
            n => {
                log::warn(TAG, format!("{short}… asked for '{period}' — {n} of our publications hold that period; not guessing which"));
                return None;
            }
        };
        if pubs[&pub_id].issues[period].sent.iter().any(|s| s == reader_hex) {
            log::info(TAG, format!("{short}… re-asked for '{period}' — already sent"));
            return None;
        }
        Some(pub_id)
    }

    /// A reader asked for a period (kind 16): a bill if it is priced, the
    /// key if it is free.
    pub fn on_wanted(&self, reader_hex: &str, period: &str) {
        let Some(pub_id) = self.wanted_target(reader_hex, period) else { return };
        let Some(c) = self.contact(reader_hex) else { return };
        let Some(p) = self.publication(&pub_id) else { return };
        let short = &reader_hex[..8.min(reader_hex.len())];
        if p.price > 0 {
            if !p.subs.iter().any(|s| s == reader_hex) {
                let _ = self.set_subscriber(&pub_id, reader_hex, true);
            }
            let n = self.bill_period(&pub_id, period, Some(reader_hex));
            log::info(TAG, if n > 0 { format!("billed {short}… for '{pub_id}' {period}") } else { format!("{short}… already billed for '{pub_id}' {period}") });
        } else {
            match self.send_period(&c, &pub_id, period, &format!("Here is '{period}', as asked")) {
                Ok(()) => log::info(TAG, format!("sent free period '{period}' of '{pub_id}' to {short}…")),
                Err(e) => log::warn(TAG, format!("could not send '{period}' to {short}…: {e}")),
            }
        }
    }

    /// The publications' turn on the lap: shelves tended, paid bills
    /// answered with keys, every subscription's index re-read.
    pub fn publications_lap(&self) {
        self.tend_shelf();
        self.reconcile_settled();
    }

    /// Hourly: what each publisher's shelf holds now.
    pub fn refresh_shelves(&self) {
        for publisher in self.subscribed_publishers() {
            self.refresh_shelf(&publisher);
        }
    }
}

fn current_month() -> String {
    // YYYY-MM from the clock, without a calendar crate: days since the
    // epoch to a civil date (Howard Hinnant's algorithm).
    let days = (App::now() / 86_400) as i64;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn period_ids_are_names_not_paths() {
        assert!(is_safe_period_id("2026-09"));
        assert!(!is_safe_period_id(""));
        assert!(!is_safe_period_id(".."));
        assert!(!is_safe_period_id("a/b"));
        assert!(!is_safe_period_id(&"x".repeat(65)));
    }

    #[test]
    fn the_month_is_civil() {
        let m = current_month();
        assert_eq!(m.len(), 7);
        assert!(m.starts_with("20"));
    }

    #[test]
    fn a_publication_keeps_its_price_subscribers_and_keys() {
        let dir = std::env::temp_dir().join(format!("ducat-pubs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let app = App::open(&dir).unwrap();
        let id = app.create_publication("The Gazette").unwrap();
        assert_eq!(id.len(), 16);
        app.set_price(&id, 5_000).unwrap();
        app.set_subscriber(&id, "reader1", true).unwrap();
        app.set_subscriber(&id, "reader1", true).unwrap();
        let p = app.publication(&id).unwrap();
        assert_eq!(p.price, 5_000);
        assert_eq!(p.subs, vec!["reader1".to_string()]);
        let k1 = app.period_key(&id, "2026-09").unwrap();
        let k2 = app.period_key(&id, "2026-10").unwrap();
        assert_ne!(k1, k2);
        assert_eq!(app.period_key(&id, "2026-09").unwrap(), k1);
        app.mark_sent(&id, "2026-09", "reader1").unwrap();
        assert!(app.publication(&id).unwrap().issues["2026-09"].sent.contains(&"reader1".to_string()));
        // The reader's side files what arrives and refuses a path.
        let m = StoredMessage { kind: 13, pub_period_id: Some("2026-09".into()), pub_period_key: Some(k1.clone()), pub_record: Some("VLD0:root".into()), ..Default::default() };
        app.absorb_key("pubhex", &m).unwrap();
        let bad = StoredMessage { kind: 13, pub_period_id: Some("../x".into()), pub_period_key: Some(k1.clone()), ..Default::default() };
        app.absorb_key("pubhex", &bad).unwrap();
        let sub = app.subscription("pubhex").unwrap();
        assert_eq!(sub.period_key("2026-09"), Some(k1));
        assert_eq!(sub.periods.len(), 1);
        assert_eq!(sub.record.as_deref(), Some("VLD0:root"));
        app.set_muted("pubhex", true).unwrap();
        let later = StoredMessage { kind: 13, pub_period_id: Some("2026-10".into()), pub_period_key: Some(k2), ..Default::default() };
        app.absorb_key("pubhex", &later).unwrap();
        assert_eq!(app.subscription("pubhex").unwrap().periods.len(), 1);
    }
}
