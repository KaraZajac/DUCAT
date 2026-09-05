//! Contacts, threads, cards, and prekeys — the desk's copy of the phone's
//! `ContactStore`, in the same JSON shapes under the same keys, so a log
//! line, a backup, and a bug report read the same on both.
//!
//! Everything lives in one table, `ducat_contacts`, because the phone's
//! rules about what must land together are rules about one file: a
//! message row and the counter it advanced, a burned key and the bundle
//! that no longer offers it, a card and the contact it produced. The
//! store's `update` writes the whole table once; nothing here writes
//! half of a change.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};

use base64::Engine;
use ducat_mobile::contacts::{bundle_one_time_count, bundle_one_time_ids, prune_prekey};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::{log, App, Error};

const TAG: &str = "Contacts";

/// The table every identity-bearing thing lives in.
pub const CONTACTS: &str = "ducat_contacts";

/// Message kinds that never surface as a chat row on their own (ceremony
/// rounds, live position, group rosters) — a thread that holds only these
/// stays out of the conversation list.
pub const HIDDEN_KINDS: [u32; 5] = [8, 9, 10, 11, 12];

/// Thirty days: how long a rotated signed prekey still opens, and how
/// long a one-time offer is advertised for.
const SIGNED_PREKEY_LIFETIME_MS: u64 = 30 * 24 * 60 * 60 * 1000;
const SIGNED_PREKEY_GRACE_MS: u64 = 30 * 24 * 60 * 60 * 1000;
/// A burned one-time secret stays openable this long, for the slot that
/// was processed once and lost to a crash before its row was kept.
pub const BURN_GRACE_MS: u64 = 30 * 60 * 1000;

/// Bumped on every write. Screens poll it instead of being told: a
/// counter cannot be missed, and it costs nothing to compare.
static GENERATION: AtomicU64 = AtomicU64::new(1);

pub fn generation() -> u64 {
    GENERATION.load(Ordering::Relaxed)
}

pub(crate) fn bump() {
    GENERATION.fetch_add(1, Ordering::Relaxed);
}

pub(crate) fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub(crate) fn b64(b: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(b)
}

pub(crate) fn unb64(s: &str) -> Option<Vec<u8>> {
    base64::engine::general_purpose::STANDARD.decode(s).ok()
}

pub(crate) fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// Null rather than a partial array: a half-parsed transaction id is
/// worse than an absent one, because it points at nothing and looks like
/// it points.
pub fn hex_to_bytes(s: &str) -> Option<Vec<u8>> {
    let t = s.trim();
    if t.is_empty() || t.len() % 2 != 0 {
        return None;
    }
    (0..t.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&t[i..i + 2], 16).ok())
        .collect()
}

/// Bytes as the phone writes them: base64, no line breaks, "" for none.
pub(crate) mod bytes_b64 {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S: Serializer>(v: &Vec<u8>, s: S) -> Result<S::Ok, S::Error> {
        super::b64(v).serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        let s: Option<String> = Option::deserialize(d)?;
        Ok(s.as_deref().and_then(super::unb64).unwrap_or_default())
    }
}

pub(crate) mod opt_bytes_b64 {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S: Serializer>(v: &Option<Vec<u8>>, s: S) -> Result<S::Ok, S::Error> {
        v.as_deref().map(super::b64).serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Vec<u8>>, D::Error> {
        let s: Option<String> = Option::deserialize(d)?;
        Ok(s.as_deref().filter(|s| !s.is_empty()).and_then(super::unb64))
    }
}

fn default_true() -> bool {
    true
}

fn default_ring() -> u32 {
    8
}

/// One counterparty: their persona, the two logs between us, and the
/// counters that say where each log has got to. `owner` is which of our
/// personas this relationship was born under — a thread is bound to a
/// persona at its doorway and never re-homed.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Contact {
    #[serde(rename = "persona")]
    pub persona_hex: String,
    #[serde(default)]
    pub petname: Option<String>,
    #[serde(rename = "asserted", default)]
    pub asserted_name: Option<String>,
    #[serde(rename = "my_outbox", default)]
    pub my_outbox: String,
    #[serde(rename = "my_outbox_pub", with = "bytes_b64", default)]
    pub my_outbox_owner_public: Vec<u8>,
    #[serde(rename = "my_outbox_sec", with = "bytes_b64", default)]
    pub my_outbox_owner_secret: Vec<u8>,
    #[serde(rename = "their_outbox", default)]
    pub their_outbox: String,
    #[serde(rename = "their_bundle", with = "opt_bytes_b64", default)]
    pub their_bundle: Option<Vec<u8>>,
    #[serde(rename = "their_address", default)]
    pub their_address: Option<String>,
    #[serde(rename = "pending_address", default)]
    pub pending_address: Option<String>,
    #[serde(with = "opt_bytes_b64", default)]
    pub avatar: Option<Vec<u8>>,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub phone: Option<String>,
    #[serde(default)]
    pub signal: Option<String>,
    #[serde(default)]
    pub pronouns: Option<u32>,
    #[serde(rename = "my_ring", default = "default_ring")]
    pub my_ring: u32,
    #[serde(rename = "car_model", default)]
    pub car_model: Option<String>,
    #[serde(rename = "car_color", default)]
    pub car_color: Option<String>,
    #[serde(default)]
    pub plate: Option<String>,
    #[serde(rename = "their_read", default)]
    pub their_read_up_to: Option<u64>,
    #[serde(rename = "card_purpose", default)]
    pub card_purpose: Option<String>,
    #[serde(rename = "my_card_purpose", default)]
    pub my_card_purpose: Option<String>,
    #[serde(rename = "my_card_purpose_at", default)]
    pub my_card_purpose_at: u64,
    #[serde(rename = "out_seq", default)]
    pub out_seq: u64,
    #[serde(rename = "out_prev", with = "opt_bytes_b64", default)]
    pub out_prev_link: Option<Vec<u8>>,
    #[serde(rename = "in_seq", default)]
    pub in_seq: u64,
    #[serde(rename = "in_prev", with = "opt_bytes_b64", default)]
    pub in_prev_link: Option<Vec<u8>>,
    #[serde(rename = "chat_visible", default = "default_true")]
    pub chat_visible: bool,
    #[serde(default)]
    pub owner: String,
}

impl Contact {
    pub fn display_name(&self) -> String {
        self.petname
            .clone()
            .or_else(|| self.asserted_name.clone())
            .unwrap_or_else(|| "Unnamed contact".to_string())
    }

    /// Whether anyone has actually named them.
    pub fn named(&self) -> bool {
        self.petname.is_some() || self.asserted_name.is_some()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct BillItem {
    #[serde(rename = "d")]
    pub description: String,
    #[serde(rename = "a")]
    pub amount_pxmr: u64,
}

/// One row of a thread, as kept. Field names follow the phone's JSON.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StoredMessage {
    #[serde(rename = "out")]
    pub outgoing: bool,
    pub seq: u64,
    pub body: String,
    #[serde(rename = "ts")]
    pub timestamp: u64,
    /// 0 text, 1 request, 2 notice, 3 receipt (§16.13), and the rest of
    /// §16's kinds.
    #[serde(default)]
    pub kind: u32,
    #[serde(rename = "amt", default)]
    pub amount_pxmr: u64,
    #[serde(default)]
    pub payto: Option<String>,
    #[serde(rename = "re_seq", default, skip_serializing_if = "Option::is_none")]
    pub re_seq: Option<u64>,
    #[serde(rename = "re_own", default, skip_serializing_if = "std::ops::Not::not")]
    pub re_own: bool,
    #[serde(rename = "att_rec", default, skip_serializing_if = "Option::is_none")]
    pub att_record: Option<String>,
    #[serde(rename = "att_swarm", default, skip_serializing_if = "Option::is_none")]
    pub att_swarm: Option<String>,
    #[serde(rename = "att_swarm_dig", default, skip_serializing_if = "Option::is_none")]
    pub att_swarm_digest: Option<String>,
    #[serde(rename = "att_key", with = "opt_bytes_b64", default, skip_serializing_if = "Option::is_none")]
    pub att_key: Option<Vec<u8>>,
    #[serde(rename = "att_nonce", with = "opt_bytes_b64", default, skip_serializing_if = "Option::is_none")]
    pub att_nonce: Option<Vec<u8>>,
    #[serde(rename = "att_len", default)]
    pub att_len: u64,
    #[serde(rename = "att_hash", default, skip_serializing_if = "Option::is_none")]
    pub att_hash: Option<String>,
    #[serde(rename = "att_mime", default, skip_serializing_if = "Option::is_none")]
    pub att_mime: Option<String>,
    #[serde(rename = "att_name", default, skip_serializing_if = "Option::is_none")]
    pub att_name: Option<String>,
    #[serde(rename = "txid", default)]
    pub txid_hex: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<BillItem>,
    #[serde(rename = "tax", default, skip_serializing_if = "Option::is_none")]
    pub tax_pxmr: Option<u64>,
    /// False means it went out under the signed prekey — no forward
    /// secrecy until that key rotates (§16.11). Shown, not hidden.
    #[serde(rename = "fs", default = "default_true")]
    pub forward_secret: bool,
    #[serde(default = "default_true")]
    pub delivered: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub oob: bool,
    #[serde(rename = "eta", default, skip_serializing_if = "Option::is_none")]
    pub eta_secs: Option<u64>,
    #[serde(rename = "grp", default, skip_serializing_if = "Option::is_none")]
    pub group_id: Option<String>,
    #[serde(rename = "gseq", default)]
    pub group_seq: u64,
    #[serde(rename = "gre_s", default, skip_serializing_if = "Option::is_none")]
    pub group_re_sender: Option<String>,
    #[serde(rename = "gre_q", default, skip_serializing_if = "Option::is_none")]
    pub group_re_seq: Option<u64>,
    #[serde(rename = "pub_period", default, skip_serializing_if = "Option::is_none")]
    pub pub_period_id: Option<String>,
    #[serde(rename = "pub_key", with = "opt_bytes_b64", default, skip_serializing_if = "Option::is_none")]
    pub pub_period_key: Option<Vec<u8>>,
    #[serde(rename = "pub_rec", default, skip_serializing_if = "Option::is_none")]
    pub pub_record: Option<String>,
    #[serde(rename = "pub_head", with = "opt_bytes_b64", default, skip_serializing_if = "Option::is_none")]
    pub pub_head_key: Option<Vec<u8>>,
    #[serde(rename = "pub_swarm", default, skip_serializing_if = "Option::is_none")]
    pub pub_swarm_key: Option<String>,
    #[serde(rename = "pub_swarm_dig", default, skip_serializing_if = "Option::is_none")]
    pub pub_swarm_digest: Option<String>,
    #[serde(rename = "pub_want", default, skip_serializing_if = "Option::is_none")]
    pub pub_wanted: Option<String>,
    #[serde(rename = "call_route", default, skip_serializing_if = "Option::is_none")]
    pub call_route: Option<String>,
    #[serde(rename = "call_id", default, skip_serializing_if = "Option::is_none")]
    pub call_id: Option<String>,
    /// A placeholder for a message that could not be read — the thread
    /// says so where it happened rather than closing the hole (§16.10).
    #[serde(rename = "dead", default, skip_serializing_if = "std::ops::Not::not")]
    pub dead_letter: bool,
    /// §16.16: what their head said about this row, once known.
    #[serde(rename = "read", default, skip_serializing_if = "Option::is_none")]
    pub read_by_them: Option<bool>,
}

impl Default for StoredMessage {
    fn default() -> Self {
        StoredMessage {
            outgoing: false,
            seq: 0,
            body: String::new(),
            timestamp: 0,
            kind: 0,
            amount_pxmr: 0,
            payto: None,
            re_seq: None,
            re_own: false,
            att_record: None,
            att_swarm: None,
            att_swarm_digest: None,
            att_key: None,
            att_nonce: None,
            att_len: 0,
            att_hash: None,
            att_mime: None,
            att_name: None,
            txid_hex: None,
            items: Vec::new(),
            tax_pxmr: None,
            forward_secret: true,
            delivered: true,
            oob: false,
            eta_secs: None,
            group_id: None,
            group_seq: 0,
            group_re_sender: None,
            group_re_seq: None,
            pub_period_id: None,
            pub_period_key: None,
            pub_record: None,
            pub_head_key: None,
            pub_swarm_key: None,
            pub_swarm_digest: None,
            pub_wanted: None,
            call_route: None,
            call_id: None,
            dead_letter: false,
            read_by_them: None,
        }
    }
}

impl StoredMessage {
    /// A dead letter, timestamped where the thread currently ends so it
    /// sorts into place.
    pub fn dead(seq: u64, body: &str, timestamp: u64) -> StoredMessage {
        StoredMessage {
            outgoing: false,
            seq,
            body: body.to_string(),
            timestamp,
            dead_letter: true,
            ..Default::default()
        }
    }

    /// Surfaces in the conversation list: a plain message, not a
    /// ceremony round or a roster, and not a group's copy.
    pub fn surfaces(&self) -> bool {
        !HIDDEN_KINDS.contains(&self.kind) && self.group_id.is_none()
    }
}

/// A card this desk cut and is still watching for an answer.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IssuedCard {
    #[serde(rename = "inbox")]
    pub inbox_key: String,
    #[serde(rename = "wpub", with = "bytes_b64", default)]
    pub writer_public: Vec<u8>,
    #[serde(rename = "wsec", with = "bytes_b64", default)]
    pub writer_secret: Vec<u8>,
    #[serde(rename = "outbox", default)]
    pub outbox_key: String,
    #[serde(rename = "opub", with = "bytes_b64", default)]
    pub outbox_owner_public: Vec<u8>,
    #[serde(rename = "osec", with = "bytes_b64", default)]
    pub outbox_owner_secret: Vec<u8>,
    /// The inbox record's own keypair, kept so its first subkey can be
    /// rewritten later (a profile edit); empty on cards cut before this
    /// was kept, which then rely on the node still holding the record.
    #[serde(rename = "ipub", with = "bytes_b64", default)]
    pub inbox_owner_public: Vec<u8>,
    #[serde(rename = "isec", with = "bytes_b64", default)]
    pub inbox_owner_secret: Vec<u8>,
    #[serde(default)]
    pub uri: String,
    #[serde(default = "default_purpose")]
    pub purpose: String,
    #[serde(default)]
    pub owner: String,
    /// Millis when it was cut, and how long it was valid for.
    #[serde(default)]
    pub made: u64,
    #[serde(default)]
    pub ttl: u64,
    #[serde(rename = "answered_by", default)]
    pub answered_by: Option<String>,
}

fn default_purpose() -> String {
    "profile".into()
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct Burned {
    sk: String,
    at: u64,
}

/// The device-wide prekey material, in the phone's shape.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(crate) struct Prekeys {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    bundle: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    signed: Option<String>,
    #[serde(default)]
    signed_at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    signed_prev: Option<String>,
    #[serde(default)]
    signed_prev_at: u64,
    #[serde(default)]
    one_time: BTreeMap<String, String>,
    #[serde(default)]
    one_time_burned: BTreeMap<String, Burned>,
}

// ----- table helpers: read and write inside one `update` ------------------

fn take<T: serde::de::DeserializeOwned>(m: &Map<String, Value>, key: &str) -> Option<T> {
    m.get(key).cloned().and_then(crate::store::value_as)
}

fn set<T: Serialize>(m: &mut Map<String, Value>, key: &str, v: &T) {
    if let Ok(v) = serde_json::to_value(v) {
        m.insert(key.to_string(), v);
    }
}

fn read_contacts(m: &Map<String, Value>) -> Vec<Contact> {
    take::<Vec<Contact>>(m, "contacts").unwrap_or_default()
}

fn write_contacts(m: &mut Map<String, Value>, list: &[Contact]) {
    set(m, "contacts", &list);
}

fn read_thread(m: &Map<String, Value>, persona_hex: &str) -> Vec<StoredMessage> {
    let mut t: Vec<StoredMessage> = take(m, &format!("thread_{persona_hex}")).unwrap_or_default();
    for r in &mut t {
        r.dead_letter = r.dead_letter || r.body.starts_with("[a message ");
    }
    t
}

fn write_thread(m: &mut Map<String, Value>, persona_hex: &str, thread: &[StoredMessage]) {
    set(m, &format!("thread_{persona_hex}"), &thread);
}

/// A receipt (§16.13's kind 3) as filed for the ledger, beside the thread
/// row it came from. Field names follow the phone's JSON.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReceiptRecord {
    #[serde(default)]
    pub txid: Option<String>,
    #[serde(rename = "amt")]
    pub amount_pxmr: u64,
    #[serde(default)]
    pub items: Vec<BillItem>,
    #[serde(default)]
    pub tax: Option<u64>,
    #[serde(rename = "hex")]
    pub contact_hex: String,
    #[serde(rename = "who", default)]
    pub counterparty: String,
    /// True when this desk issued it (we were the payee).
    #[serde(default)]
    pub mine: bool,
    #[serde(rename = "ts", default)]
    pub timestamp: u64,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub oob: bool,
    #[serde(default)]
    pub seq: u64,
}

/// File a kind-3 row as a receipt, once: the same transaction or the same
/// message replaces rather than duplicates.
fn save_receipt(m: &mut Map<String, Value>, persona_hex: &str, row: &StoredMessage) {
    if row.kind != 3 {
        return;
    }
    let name = read_contacts(m).iter().find(|c| c.persona_hex == persona_hex).map(|c| c.display_name()).unwrap_or_else(|| format!("{}…", &persona_hex[..8.min(persona_hex.len())]));
    let mut list: Vec<ReceiptRecord> = take(m, "receipts_v1").unwrap_or_default();
    list.retain(|r| {
        let same_tx = row.txid_hex.as_deref().map_or(false, |t| r.txid.as_deref().map_or(false, |x| x.eq_ignore_ascii_case(t)));
        let same_msg = r.contact_hex == persona_hex && r.seq == row.seq && r.mine == row.outgoing && (r.timestamp == 0 || r.timestamp == row.timestamp);
        !(same_tx || same_msg)
    });
    list.push(ReceiptRecord {
        txid: row.txid_hex.clone(),
        amount_pxmr: row.amount_pxmr,
        items: row.items.clone(),
        tax: row.tax_pxmr,
        contact_hex: persona_hex.to_string(),
        counterparty: name,
        mine: row.outgoing,
        timestamp: row.timestamp,
        oob: row.oob,
        seq: row.seq,
    });
    set(m, "receipts_v1", &list);
}

fn replace_contact(list: &mut Vec<Contact>, c: Contact) {
    list.retain(|k| k.persona_hex != c.persona_hex);
    list.push(c);
}

impl App {
    fn contacts_store(&self) -> crate::store::Store {
        self.store(CONTACTS)
    }

    // ----- contacts ----------------------------------------------------------

    /// Every contact with a log on both sides. Half-made records — a claim
    /// that died before its reply landed — are invisible here, as on the
    /// phone.
    pub fn contacts(&self) -> Vec<Contact> {
        self.contacts_store()
            .view(read_contacts)
            .into_iter()
            .filter(|c| !c.my_outbox.is_empty() && !c.their_outbox.is_empty())
            .collect()
    }

    pub fn contact(&self, persona_hex: &str) -> Option<Contact> {
        self.contacts().into_iter().find(|c| c.persona_hex == persona_hex)
    }

    /// Replace the whole record for this persona.
    pub fn put_contact(&self, c: Contact) -> Result<(), Error> {
        self.contacts_store().update(|m| {
            let mut list = read_contacts(m);
            replace_contact(&mut list, c);
            write_contacts(m, &list);
        })?;
        bump();
        Ok(())
    }

    /// Rebuild a contact against the record as it is *now*, under the
    /// table lock — the doorway for claims, where a snapshot taken before
    /// a network round trip must not rewind counters a poll advanced.
    pub fn merge_contact<F>(&self, persona_hex: &str, f: F) -> Result<Contact, Error>
    where
        F: FnOnce(Option<&Contact>) -> Contact,
    {
        let c = self.contacts_store().update(|m| {
            let mut list = read_contacts(m);
            let cur = list.iter().find(|c| c.persona_hex == persona_hex).cloned();
            let next = f(cur.as_ref());
            replace_contact(&mut list, next.clone());
            write_contacts(m, &list);
            next
        })?;
        bump();
        Ok(c)
    }

    pub fn remove_contact(&self, persona_hex: &str) -> Result<(), Error> {
        self.contacts_store().update(|m| {
            let mut list = read_contacts(m);
            list.retain(|c| c.persona_hex != persona_hex);
            write_contacts(m, &list);
            m.remove(&format!("thread_{persona_hex}"));
            m.remove(&format!("pendingslot_{persona_hex}"));
            m.remove(&format!("usedtheirs_{persona_hex}"));
            m.remove(&format!("seen_{persona_hex}"));
            m.remove(&format!("seenlog_{persona_hex}"));
        })?;
        bump();
        Ok(())
    }

    pub fn set_petname(&self, persona_hex: &str, petname: Option<&str>) -> Result<(), Error> {
        let petname = petname
            .map(|s| ducat_mobile::contacts::clean_display_text(s.to_string()))
            .filter(|s| !s.trim().is_empty());
        self.edit_contact(persona_hex, |c| c.petname = petname)
    }

    fn edit_contact<F: FnOnce(&mut Contact)>(&self, persona_hex: &str, f: F) -> Result<(), Error> {
        let touched = self.contacts_store().update(|m| {
            let mut list = read_contacts(m);
            let Some(c) = list.iter_mut().find(|c| c.persona_hex == persona_hex) else { return false };
            f(c);
            write_contacts(m, &list);
            true
        })?;
        if touched {
            bump();
        }
        Ok(())
    }

    pub fn set_my_ring(&self, persona_hex: &str, ring: u32) -> Result<(), Error> {
        self.edit_contact(persona_hex, |c| c.my_ring = ring)
    }

    pub fn advance_outbound(&self, persona_hex: &str, seq: u64, prev_link: Vec<u8>) -> Result<(), Error> {
        self.edit_contact(persona_hex, |c| {
            c.out_seq = seq;
            c.out_prev_link = Some(prev_link);
        })
    }

    pub fn set_their_read_up_to(&self, persona_hex: &str, v: u64) -> Result<(), Error> {
        self.edit_contact(persona_hex, |c| c.their_read_up_to = Some(v))
    }

    /// Their payment address, settled by a message they signed — which is
    /// the only thing allowed to move one that is already working.
    pub fn set_their_address(&self, persona_hex: &str, address: &str) -> Result<(), Error> {
        if address.trim().is_empty() {
            return Ok(());
        }
        self.edit_contact(persona_hex, |c| {
            if c.their_address.as_deref() == Some(address) && c.pending_address.is_none() {
                return;
            }
            if c.pending_address.is_some() {
                log::info(TAG, format!("{}… address settled by a message they signed", &persona_hex[..12.min(persona_hex.len())]));
            }
            c.their_address = Some(address.to_string());
            c.pending_address = None;
        })
    }

    /// Their refreshed keys, minus every one-time id this desk already
    /// spent against them — a cached copy must never re-offer a key we
    /// sealed to, or the next message seals to it again.
    pub fn set_their_bundle(&self, persona_hex: &str, bundle: Vec<u8>) -> Result<(), Error> {
        let used = self.used_their_ids(persona_hex);
        let mut b = bundle;
        for id in used {
            if let Ok(p) = prune_prekey(b.clone(), id) {
                b = p;
            }
        }
        self.edit_contact(persona_hex, |c| c.their_bundle = Some(b))
    }

    pub fn used_their_ids(&self, persona_hex: &str) -> Vec<u32> {
        self.contacts_store()
            .get_string(&format!("usedtheirs_{persona_hex}"))
            .map(|s| s.split(',').filter_map(|x| x.trim().parse().ok()).collect())
            .unwrap_or_default()
    }

    pub fn record_used_their_id(&self, persona_hex: &str, id: u32) -> Result<(), Error> {
        let mut ids = self.used_their_ids(persona_hex);
        if !ids.contains(&id) {
            ids.push(id);
        }
        let joined = ids.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(",");
        self.contacts_store().put(&format!("usedtheirs_{persona_hex}"), &joined)?;
        Ok(())
    }

    /// Every receipt filed, either direction.
    pub fn receipts(&self) -> Vec<ReceiptRecord> {
        self.contacts_store().get("receipts_v1").unwrap_or_default()
    }

    // ----- flags ---------------------------------------------------------------

    fn flag(&self, key: &str) -> bool {
        self.contacts_store().get::<bool>(key).unwrap_or(false)
    }

    fn set_flag(&self, key: &str, v: bool) -> Result<(), Error> {
        self.contacts_store().put(key, &v)?;
        Ok(())
    }

    /// §16.16: whether our head carries a read watermark.
    pub fn read_receipts(&self) -> bool {
        self.flag("read_receipts")
    }

    pub fn set_read_receipts(&self, v: bool) -> Result<(), Error> {
        self.set_flag("read_receipts", v)
    }

    /// §16.12: whether a payment address rides our cards and replies.
    /// Off until the desk has a wallet to allocate one from.
    pub fn publish_address(&self) -> bool {
        self.flag("publish_address")
    }

    pub fn bundles_need_republish(&self) -> bool {
        self.flag("republish_bundles")
    }

    pub fn set_bundles_need_republish(&self, v: bool) -> Result<(), Error> {
        self.set_flag("republish_bundles", v)
    }

    // ----- threads -------------------------------------------------------------

    pub fn thread(&self, persona_hex: &str) -> Vec<StoredMessage> {
        self.contacts_store().view(|m| read_thread(m, persona_hex))
    }

    /// One inbound row and the inbound counters, in one write.
    pub fn append_and_advance(
        &self,
        persona_hex: &str,
        row: StoredMessage,
        new_in_seq: u64,
        new_prev_link: Option<Vec<u8>>,
    ) -> Result<(), Error> {
        self.contacts_store().update(|m| {
            let mut thread = read_thread(m, persona_hex);
            let surfaces = row.surfaces();
            save_receipt(m, persona_hex, &row);
            thread.push(row);
            write_thread(m, persona_hex, &thread);
            let mut list = read_contacts(m);
            if let Some(c) = list.iter_mut().find(|c| c.persona_hex == persona_hex) {
                // A row that does not surface must not make the thread
                // look unread: keep the seen mark level with it.
                if !surfaces && chat_seen_of(m, c) >= c.in_seq {
                    m.insert(format!("seen_{persona_hex}"), Value::from(new_in_seq));
                    m.insert(format!("seenlog_{persona_hex}"), Value::from(c.their_outbox.clone()));
                }
                c.in_seq = new_in_seq;
                c.in_prev_link = new_prev_link;
                c.chat_visible = c.chat_visible || surfaces;
                write_contacts(m, &list);
            }
        })?;
        bump();
        Ok(())
    }

    /// The local echo, the outbound counters, and the sealed bytes owed to
    /// the network — persisted together before the DHT sees anything.
    pub fn append_and_advance_outbound(
        &self,
        persona_hex: &str,
        row: StoredMessage,
        new_out_seq: u64,
        new_prev_link: Vec<u8>,
        sealed_slot: &[u8],
    ) -> Result<(), Error> {
        self.contacts_store().update(|m| {
            let mut thread = read_thread(m, persona_hex);
            let surfaces = row.surfaces();
            let seq = row.seq;
            save_receipt(m, persona_hex, &row);
            thread.push(row);
            write_thread(m, persona_hex, &thread);
            m.insert(
                format!("pendingslot_{persona_hex}"),
                serde_json::json!({ "seq": seq, "b": b64(sealed_slot) }),
            );
            let mut list = read_contacts(m);
            if let Some(c) = list.iter_mut().find(|c| c.persona_hex == persona_hex) {
                c.out_seq = new_out_seq;
                c.out_prev_link = Some(new_prev_link);
                c.chat_visible = c.chat_visible || surfaces;
                write_contacts(m, &list);
            }
        })?;
        bump();
        Ok(())
    }

    /// Bytes a send persisted but never delivered, with their seq.
    pub fn pending_slot(&self, persona_hex: &str) -> Option<(u64, Vec<u8>)> {
        let v: Value = self.contacts_store().get(&format!("pendingslot_{persona_hex}"))?;
        let seq = v.get("seq")?.as_u64()?;
        let b = unb64(v.get("b")?.as_str()?)?;
        Some((seq, b))
    }

    pub fn clear_pending_slot(&self, persona_hex: &str) -> Result<(), Error> {
        self.contacts_store().remove(&format!("pendingslot_{persona_hex}"))?;
        Ok(())
    }

    pub fn mark_delivered(&self, persona_hex: &str, seq: u64) -> Result<(), Error> {
        let touched = self.contacts_store().update(|m| {
            let mut thread = read_thread(m, persona_hex);
            let Some(at) = thread.iter().rposition(|r| r.outgoing && r.seq == seq) else { return false };
            if thread[at].delivered {
                return false;
            }
            thread[at].delivered = true;
            write_thread(m, persona_hex, &thread);
            true
        })?;
        if touched {
            bump();
        }
        Ok(())
    }

    /// Freeze what their old head said about every row we sent into a log
    /// that is being replaced: read or not, decided now, because the new
    /// log's watermark will count from zero.
    pub fn retire_outbox(&self, persona_hex: &str, read_up_to: Option<u64>) -> Result<(), Error> {
        let touched = self.contacts_store().update(|m| {
            let mut thread = read_thread(m, persona_hex);
            if !thread.iter().any(|r| r.outgoing && r.read_by_them.is_none()) {
                return false;
            }
            for r in thread.iter_mut().filter(|r| r.outgoing && r.read_by_them.is_none()) {
                r.read_by_them = Some(read_up_to.map_or(false, |w| r.seq < w));
            }
            write_thread(m, persona_hex, &thread);
            true
        })?;
        if touched {
            bump();
        }
        Ok(())
    }

    /// Per-seq patience clocks for one contact, dropped when their log is
    /// replaced — they were seqs in the old log's numbering.
    pub fn clear_stuck_clocks(&self, persona_hex: &str) -> Result<(), Error> {
        let prefix = format!("stuck_{persona_hex}:");
        self.contacts_store().update(|m| {
            let gone: Vec<String> = m.keys().filter(|k| k.starts_with(&prefix)).cloned().collect();
            for k in gone {
                m.remove(&k);
            }
        })?;
        Ok(())
    }

    pub fn set_last_slot_verified(&self, persona_hex: &str, seq: i64) -> Result<(), Error> {
        self.contacts_store().put(&format!("slotok_{persona_hex}"), &seq)?;
        Ok(())
    }

    pub fn last_slot_verified(&self, persona_hex: &str) -> i64 {
        self.contacts_store().get(&format!("slotok_{persona_hex}")).unwrap_or(-1)
    }

    pub fn set_slot_fix_tries(&self, persona_hex: &str, n: u32) -> Result<(), Error> {
        self.contacts_store().put(&format!("slotfix_{persona_hex}"), &n)?;
        Ok(())
    }

    pub fn slot_fix_tries(&self, persona_hex: &str) -> u32 {
        self.contacts_store().get(&format!("slotfix_{persona_hex}")).unwrap_or(0)
    }

    // ----- housekeeping a person asks for ---------------------------------------------

    /// Take one row out of a thread, here only — the network keeps its copy.
    pub fn delete_message(&self, persona_hex: &str, seq: u64, outgoing: bool, timestamp: Option<u64>) -> Result<(), Error> {
        self.contacts_store().update(|m| {
            let mut thread = read_thread(m, persona_hex);
            thread.retain(|r| !(r.seq == seq && r.outgoing == outgoing && timestamp.map_or(true, |t| r.timestamp == t)));
            write_thread(m, persona_hex, &thread);
        })?;
        bump();
        Ok(())
    }

    /// Clear a conversation, keeping the group copies that live in it.
    pub fn delete_thread(&self, persona_hex: &str) -> Result<(), Error> {
        self.contacts_store().update(|m| {
            let kept: Vec<StoredMessage> = read_thread(m, persona_hex).into_iter().filter(|r| r.group_id.is_some()).collect();
            if kept.is_empty() {
                m.remove(&format!("thread_{persona_hex}"));
            } else {
                write_thread(m, persona_hex, &kept);
            }
        })?;
        bump();
        Ok(())
    }

    /// Seconds after which this thread's rows are dropped; zero keeps them.
    pub fn disappear_after(&self, persona_hex: &str) -> u64 {
        self.contacts_store().get(&format!("disappear_{persona_hex}")).unwrap_or(0)
    }

    pub fn set_disappear_after(&self, persona_hex: &str, secs: u64) -> Result<(), Error> {
        self.contacts_store().put(&format!("disappear_{persona_hex}"), &secs)?;
        bump();
        Ok(())
    }

    /// Drop rows older than `after_secs`; how many went.
    pub fn expire_old(&self, persona_hex: &str, after_secs: u64) -> usize {
        if after_secs == 0 {
            return 0;
        }
        let cutoff = App::now().saturating_sub(after_secs);
        let gone = self
            .contacts_store()
            .update(|m| {
                let all = read_thread(m, persona_hex);
                let kept: Vec<StoredMessage> = all.iter().filter(|r| r.timestamp >= cutoff).cloned().collect();
                let gone = all.len() - kept.len();
                if gone > 0 {
                    write_thread(m, persona_hex, &kept);
                }
                gone
            })
            .unwrap_or(0);
        if gone > 0 {
            bump();
        }
        gone
    }

    pub fn expire_all(&self) -> usize {
        self.contacts().into_iter().map(|c| self.expire_old(&c.persona_hex, self.disappear_after(&c.persona_hex))).sum()
    }

    /// The half-typed message, per thread.
    pub fn draft_of(&self, persona_hex: &str) -> String {
        self.contacts_store().get_string(&format!("draft_{persona_hex}")).unwrap_or_default()
    }

    pub fn save_draft(&self, persona_hex: &str, text: &str) -> Result<(), Error> {
        let key = format!("draft_{persona_hex}");
        if text.trim().is_empty() {
            self.contacts_store().remove(&key)?;
        } else {
            self.contacts_store().put(&key, &text.chars().take(4000).collect::<String>())?;
        }
        Ok(())
    }

    /// Show or hide a conversation without touching the contact.
    pub fn set_chat_visible(&self, persona_hex: &str, visible: bool) -> Result<(), Error> {
        self.edit_contact(persona_hex, |c| c.chat_visible = visible)
    }

    // ----- seen marks ------------------------------------------------------------

    /// How far into their log the reader has looked, in the numbering of
    /// the log the mark was made against; a replaced log starts at zero.
    pub fn chat_seen(&self, c: &Contact) -> u64 {
        self.contacts_store().view(|m| chat_seen_of(m, c))
    }

    pub fn set_chat_seen(&self, c: &Contact) -> Result<(), Error> {
        let touched = self.contacts_store().update(|m| {
            let log_ok = m.get(&format!("seenlog_{}", c.persona_hex)).and_then(Value::as_str) == Some(c.their_outbox.as_str());
            let mark = m.get(&format!("seen_{}", c.persona_hex)).and_then(Value::as_u64).unwrap_or(0);
            if log_ok && c.in_seq <= mark {
                return false;
            }
            m.insert(format!("seen_{}", c.persona_hex), Value::from(c.in_seq));
            m.insert(format!("seenlog_{}", c.persona_hex), Value::from(c.their_outbox.clone()));
            true
        })?;
        if touched {
            bump();
        }
        Ok(())
    }

    pub fn unread_threads(&self) -> usize {
        let list = self.contacts();
        self.contacts_store().view(|m| list.iter().filter(|c| c.chat_visible && c.in_seq > chat_seen_of(m, c)).count())
    }

    // ----- issued cards ------------------------------------------------------------

    pub fn issued_cards(&self) -> Vec<IssuedCard> {
        self.contacts_store().get("issued_cards").unwrap_or_default()
    }

    pub fn save_issued_card(&self, card: IssuedCard) -> Result<(), Error> {
        self.contacts_store().update(|m| {
            let mut cards: Vec<IssuedCard> = take(m, "issued_cards").unwrap_or_default();
            cards.push(card);
            set(m, "issued_cards", &cards);
        })?;
        bump();
        Ok(())
    }

    pub fn mark_card_answered(&self, inbox_key: &str, persona_hex: &str) -> Result<(), Error> {
        self.contacts_store().update(|m| {
            let mut cards: Vec<IssuedCard> = take(m, "issued_cards").unwrap_or_default();
            for c in cards.iter_mut().filter(|c| c.inbox_key == inbox_key) {
                c.answered_by = Some(persona_hex.to_string());
            }
            set(m, "issued_cards", &cards);
        })?;
        bump();
        Ok(())
    }

    pub fn forget_issued_card(&self, inbox_key: &str) -> Result<(), Error> {
        self.contacts_store().update(|m| {
            let mut cards: Vec<IssuedCard> = take(m, "issued_cards").unwrap_or_default();
            cards.retain(|c| c.inbox_key != inbox_key);
            set(m, "issued_cards", &cards);
        })?;
        bump();
        Ok(())
    }

    /// The standing profile code for one persona, if one is outstanding —
    /// the newest, since a claim pre-issues a replacement.
    pub fn current_card(&self, owner_hex: &str, purpose: &str) -> Option<IssuedCard> {
        self.issued_cards()
            .into_iter()
            .filter(|c| c.answered_by.is_none() && c.purpose == purpose && c.owner == owner_hex)
            .max_by_key(|c| c.made)
    }

    // ----- prekeys ------------------------------------------------------------------

    fn prekeys(&self) -> Prekeys {
        self.contacts_store().get("prekeys").unwrap_or_default()
    }

    /// Keep new material. An empty bundle leaves the advertised bundle
    /// alone; the signed secret is kept only when there is none, or when
    /// `rotate` says the old one moves to the grace slot.
    pub fn save_prekeys(
        &self,
        bundle: &[u8],
        signed_secret: &[u8],
        one_time: &[(u32, Vec<u8>)],
        rotate: bool,
    ) -> Result<(), Error> {
        self.contacts_store().update(|m| {
            let mut p: Prekeys = take(m, "prekeys").unwrap_or_default();
            if !bundle.is_empty() {
                p.bundle = Some(b64(bundle));
            }
            if signed_secret.len() == 32 {
                let now = now_ms();
                match p.signed.clone() {
                    None => {
                        p.signed = Some(b64(signed_secret));
                        p.signed_at = now;
                    }
                    Some(cur) if rotate && unb64(&cur).as_deref() != Some(signed_secret) => {
                        p.signed_prev = Some(cur);
                        p.signed_prev_at = now;
                        p.signed = Some(b64(signed_secret));
                        p.signed_at = now;
                    }
                    _ => {}
                }
            }
            for (id, sk) in one_time {
                p.one_time.insert(id.to_string(), b64(sk));
            }
            set(m, "prekeys", &p);
        })?;
        Ok(())
    }

    /// Reserve `count` one-time ids from the device-wide counter.
    pub fn next_prekey_start(&self, count: u32) -> Result<u32, Error> {
        let next = self.contacts_store().update(|m| {
            let next = m.get("prekey_next_id").and_then(Value::as_u64).unwrap_or(1) as u32;
            m.insert("prekey_next_id".into(), Value::from(next + count));
            next
        })?;
        Ok(next)
    }

    pub fn prekey_bundle(&self) -> Option<Vec<u8>> {
        self.prekeys().bundle.as_deref().and_then(unb64)
    }

    /// The one-time offer riding one of our outboxes' heads.
    pub fn thread_bundle(&self, outbox: &str) -> Option<Vec<u8>> {
        self.contacts_store().get_string(&format!("prekeys_ob_{outbox}")).as_deref().and_then(unb64)
    }

    pub fn set_thread_bundle(&self, outbox: &str, blob: &[u8]) -> Result<(), Error> {
        self.contacts_store().put(&format!("prekeys_ob_{outbox}"), &b64(blob))?;
        Ok(())
    }

    /// How many of an outbox's advertised one-time keys this desk still
    /// holds the secret for.
    pub fn thread_one_time_remaining(&self, outbox: &str) -> usize {
        let Some(blob) = self.thread_bundle(outbox) else { return 0 };
        let p = self.prekeys();
        bundle_one_time_ids(blob)
            .map(|ids| ids.iter().filter(|id| p.one_time.contains_key(&id.to_string())).count())
            .unwrap_or(0)
    }

    pub fn signed_prekey_secret(&self) -> Option<Vec<u8>> {
        self.prekeys().signed.as_deref().and_then(unb64)
    }

    /// Newest first: the current signed secret, then the retired one while
    /// its grace lasts — a peer sealing from a bundle cached before the
    /// rotation addressed the one just retired.
    pub fn signed_prekey_secrets(&self) -> Vec<Vec<u8>> {
        let p = self.prekeys();
        let mut out = Vec::new();
        if let Some(s) = p.signed.as_deref().and_then(unb64) {
            out.push(s);
        }
        if now_ms().saturating_sub(p.signed_prev_at) < SIGNED_PREKEY_GRACE_MS {
            if let Some(s) = p.signed_prev.as_deref().and_then(unb64) {
                out.push(s);
            }
        }
        out
    }

    pub fn signed_prekey_due(&self) -> bool {
        let p = self.prekeys();
        if p.signed.is_none() {
            return false;
        }
        p.signed_at == 0 || now_ms().saturating_sub(p.signed_at) >= SIGNED_PREKEY_LIFETIME_MS
    }

    /// A one-time secret by id — live, or burned within the grace window.
    pub fn one_time_secret(&self, id: u32) -> Option<Vec<u8>> {
        let p = self.prekeys();
        let k = id.to_string();
        if let Some(s) = p.one_time.get(&k) {
            return unb64(s);
        }
        p.one_time_burned.get(&k).and_then(|b| unb64(&b.sk))
    }

    /// Burned secrets past their grace leave for good.
    pub fn sweep_burned_prekeys(&self) -> Result<(), Error> {
        self.contacts_store().update(|m| {
            let mut p: Prekeys = take(m, "prekeys").unwrap_or_default();
            let cutoff = now_ms().saturating_sub(BURN_GRACE_MS);
            let before = p.one_time_burned.len();
            p.one_time_burned.retain(|_, b| b.at >= cutoff);
            if p.one_time_burned.len() != before {
                set(m, "prekeys", &p);
            }
        })?;
        Ok(())
    }

    /// A one-time key was used against us: it leaves the live set for the
    /// grace pen, and every bundle that still offers it is pruned.
    pub fn burn_one_time(&self, id: u32) -> Result<(), Error> {
        self.contacts_store().update(|m| {
            let mut p: Prekeys = take(m, "prekeys").unwrap_or_default();
            let k = id.to_string();
            if let Some(secret) = p.one_time.remove(&k) {
                p.one_time_burned.insert(k, Burned { sk: secret, at: now_ms() });
            }
            if let Some(b) = p.bundle.as_deref().and_then(unb64) {
                if let Ok(pruned) = prune_prekey(b, id) {
                    p.bundle = Some(b64(&pruned));
                }
            }
            set(m, "prekeys", &p);
            let mut offers: Vec<String> = read_contacts(m).iter().map(|c| c.my_outbox.clone()).collect();
            offers.extend(take::<Vec<IssuedCard>>(m, "issued_cards").unwrap_or_default().into_iter().map(|c| c.outbox_key));
            offers.retain(|o| !o.is_empty());
            offers.sort();
            offers.dedup();
            for outbox in offers {
                let key = format!("prekeys_ob_{outbox}");
                let Some(blob) = m.get(&key).and_then(Value::as_str).and_then(unb64) else { continue };
                if let Ok(pruned) = prune_prekey(blob.clone(), id) {
                    if pruned != blob {
                        m.insert(key, Value::from(b64(&pruned)));
                    }
                }
            }
        })?;
        Ok(())
    }

    /// Advertised and still held, whichever is smaller.
    pub fn one_time_remaining(&self) -> usize {
        let p = self.prekeys();
        let advertised = p
            .bundle
            .as_deref()
            .and_then(unb64)
            .and_then(|b| bundle_one_time_count(b).ok())
            .unwrap_or(0) as usize;
        advertised.min(p.one_time.len())
    }
}

fn chat_seen_of(m: &Map<String, Value>, c: &Contact) -> u64 {
    let mark = m.get(&format!("seen_{}", c.persona_hex)).and_then(Value::as_u64).unwrap_or(0);
    match m.get(&format!("seenlog_{}", c.persona_hex)).and_then(Value::as_str) {
        None => {
            if mark > c.in_seq {
                0
            } else {
                mark
            }
        }
        Some(log) if log != c.their_outbox => 0,
        Some(_) => mark,
    }
}

/// What an incoming card is allowed to do to a contact's payment address:
/// the address to keep using, and the one to hold for the user. A card
/// carries a persona and nothing signed over it, so it may not move an
/// address that is already working — §16.12's rotation arrives on an
/// opened message, and only that settles a held one.
pub fn fold_card_address(prior: Option<&Contact>, incoming: Option<&str>) -> (Option<String>, Option<String>) {
    let incoming = incoming.filter(|s| !s.trim().is_empty()).map(str::to_string);
    match prior {
        None => (incoming, None),
        Some(p) if p.their_address.as_deref().map_or(true, |a| a.trim().is_empty()) => (incoming, None),
        Some(p) => match incoming {
            None => (p.their_address.clone(), p.pending_address.clone()),
            Some(a) if Some(a.as_str()) == p.their_address.as_deref() => (p.their_address.clone(), None),
            Some(a) => (p.their_address.clone(), Some(a)),
        },
    }
}

/// Reactions (§16.14) on each row, keyed by (seq, timestamp): the latest
/// from each side — ours, theirs.
pub fn reactions_on(thread: &[StoredMessage]) -> std::collections::HashMap<(u64, u64), (Option<String>, Option<String>)> {
    let mut sorted: Vec<&StoredMessage> = thread.iter().filter(|m| m.kind == 4).collect();
    sorted.sort_by_key(|m| m.timestamp);
    let mut out: std::collections::HashMap<(u64, u64), (Option<String>, Option<String>)> = std::collections::HashMap::new();
    for r in sorted {
        let Some(t) = referent(thread, r) else { continue };
        let e = out.entry((t.seq, t.timestamp)).or_insert((None, None));
        let body = Some(r.body.clone()).filter(|b| !b.trim().is_empty());
        if r.outgoing {
            e.0 = body;
        } else {
            e.1 = body;
        }
    }
    out
}

/// What kind-5 rows did to the thread: bills withdrawn by their sender,
/// bills refused by their reader, plain messages unsent by their sender —
/// and the retraction rows that should stay quiet because their target
/// carries the mark.
#[derive(Debug, Default, Clone)]
pub struct Retractions {
    pub withdrawn: std::collections::HashSet<(u64, u64)>,
    pub refused: std::collections::HashSet<(u64, u64)>,
    pub unsent: std::collections::HashSet<(u64, u64)>,
    pub quiet: std::collections::HashSet<(u64, u64)>,
}

pub fn retractions(thread: &[StoredMessage]) -> Retractions {
    let mut out = Retractions::default();
    for r in thread.iter().filter(|m| m.kind == 5) {
        let Some(t) = referent(thread, r) else { continue };
        match t.kind {
            1 => {
                if r.re_own {
                    out.withdrawn.insert((t.seq, t.timestamp));
                } else {
                    out.refused.insert((t.seq, t.timestamp));
                }
            }
            0 if r.re_own => {
                out.unsent.insert((t.seq, t.timestamp));
                out.quiet.insert((r.seq, r.timestamp));
            }
            _ => {}
        }
    }
    out
}

/// The message a reaction (§16.14) points at, if the thread holds it.
/// What a reply is answering, in the reader's own words — the phone's
/// `replyLine`: the quoted text is never on the wire, only (seq, re_own),
/// so each side describes the referent from its own copy. None when the
/// referent is gone, which the screen says in its own sentence.
pub fn reply_line(thread: &[StoredMessage], m: &StoredMessage, marks: &Retractions) -> Option<String> {
    let t = referent(thread, m)?;
    Some(if marks.unsent.contains(&(t.seq, t.timestamp)) {
        "This message was withdrawn.".to_string()
    } else if t.kind == 1 {
        "a request for money".to_string()
    } else if t.kind == 2 {
        "a payment".to_string()
    } else if t.kind == 3 {
        "a receipt".to_string()
    } else if t.att_hash.is_some() && t.body.trim().is_empty() {
        "an attachment".to_string()
    } else if !t.body.trim().is_empty() {
        t.body.clone()
    } else {
        "a message".to_string()
    })
}

/// Whether a bill of ours has been answered: their payment notice or our
/// receipt pointing at it, and newer than it — seq restarts with every
/// fresh card, so the older bill numbered the same must not count.
pub fn bill_answered(thread: &[StoredMessage], bill: &StoredMessage) -> bool {
    thread.iter().any(|m| {
        m.timestamp >= bill.timestamp
            && m.re_seq == Some(bill.seq)
            && ((!m.outgoing && m.kind == 2) || (m.outgoing && m.kind == 3))
    })
}

/// The phone's "someone's profile" share: emoji labels because the
/// reader's language is unknown, and the persona hex — never a ticket.
pub fn contact_card_text(c: &Contact) -> String {
    let mut s = format!("👤 {}\n", c.display_name());
    if let Some(e) = c.email.as_deref().filter(|e| !e.trim().is_empty()) {
        s.push_str(&format!("✉ {}\n", e.trim()));
    }
    if let Some(p) = c.phone.as_deref().filter(|p| !p.trim().is_empty()) {
        s.push_str(&format!("☎ {}\n", p.trim()));
    }
    if let Some(x) = c.signal.as_deref().filter(|x| !x.trim().is_empty()) {
        s.push_str(&format!("Signal: {}\n", x.trim()));
    }
    s.push_str(&format!("🔑 {}", c.persona_hex));
    s
}

pub fn referent<'a>(thread: &'a [StoredMessage], r: &StoredMessage) -> Option<&'a StoredMessage> {
    const CLOCK_SKEW_SECS: u64 = 900;
    let seq = r.re_seq?;
    let side = if r.re_own { r.outgoing } else { !r.outgoing };
    let on_side: Vec<&StoredMessage> = thread.iter().filter(|m| m.outgoing == side && m.seq == seq).collect();
    on_side
        .iter()
        .filter(|m| m.timestamp <= r.timestamp)
        .max_by_key(|m| m.timestamp)
        .or_else(|| {
            on_side
                .iter()
                .filter(|m| m.timestamp <= r.timestamp + CLOCK_SKEW_SECS)
                .min_by_key(|m| m.timestamp)
        })
        .or_else(|| on_side.iter().min_by_key(|m| m.timestamp))
        .copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_app(tag: &str) -> App {
        // Named per test: tests share a process, and two that started in
        // the same millisecond shared a directory — and each other's rows.
        let dir = std::env::temp_dir().join(format!("ducat-contacts-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        App::open(&dir).unwrap()
    }

    fn contact(hex: &str) -> Contact {
        Contact {
            persona_hex: hex.into(),
            petname: None,
            asserted_name: Some("Pat".into()),
            my_outbox: "VLD0:mine".into(),
            my_outbox_owner_public: vec![1; 32],
            my_outbox_owner_secret: vec![2; 32],
            their_outbox: "VLD0:theirs".into(),
            their_bundle: None,
            their_address: None,
            pending_address: None,
            avatar: None,
            email: None,
            phone: None,
            signal: None,
            pronouns: None,
            my_ring: 32,
            car_model: None,
            car_color: None,
            plate: None,
            their_read_up_to: None,
            card_purpose: None,
            my_card_purpose: None,
            my_card_purpose_at: 0,
            out_seq: 0,
            out_prev_link: None,
            in_seq: 0,
            in_prev_link: None,
            chat_visible: true,
            owner: String::new(),
        }
    }

    #[test]
    fn a_contact_round_trips_in_the_phones_shape() {
        let app = temp_app("a_contact_round_trips_in_the_phones_shape");
        app.put_contact(contact("ab")).unwrap();
        let raw = std::fs::read_to_string(app.root().join("prefs/ducat_contacts.json")).unwrap();
        assert!(raw.contains("\"my_outbox_pub\""), "phone key names: {raw}");
        assert!(raw.contains("\"their_outbox\""));
        let got = app.contact("ab").unwrap();
        assert_eq!(got.my_outbox_owner_secret, vec![2; 32]);
        assert_eq!(got.display_name(), "Pat");
        assert_eq!(got.my_ring, 32);
    }

    #[test]
    fn an_outbound_row_lands_with_its_counters_and_its_slot() {
        let app = temp_app("an_outbound_row_lands_with_its_counters_and_its_slot");
        app.put_contact(contact("cd")).unwrap();
        let row = StoredMessage { outgoing: true, seq: 0, body: "hi".into(), timestamp: 1, delivered: false, ..Default::default() };
        app.append_and_advance_outbound("cd", row, 1, vec![9; 32], b"sealed").unwrap();
        let c = app.contact("cd").unwrap();
        assert_eq!(c.out_seq, 1);
        assert_eq!(c.out_prev_link, Some(vec![9; 32]));
        assert_eq!(app.pending_slot("cd"), Some((0, b"sealed".to_vec())));
        assert!(!app.thread("cd")[0].delivered);
        app.mark_delivered("cd", 0).unwrap();
        assert!(app.thread("cd")[0].delivered);
        app.clear_pending_slot("cd").unwrap();
        assert!(app.pending_slot("cd").is_none());
    }

    #[test]
    fn a_hidden_kind_does_not_make_a_thread_unread() {
        let app = temp_app("a_hidden_kind_does_not_make_a_thread_unread");
        app.put_contact(contact("ef")).unwrap();
        let c = app.contact("ef").unwrap();
        app.set_chat_seen(&c).unwrap();
        let round = StoredMessage { seq: 0, kind: 8, body: String::new(), timestamp: 1, ..Default::default() };
        app.append_and_advance("ef", round, 1, None).unwrap();
        assert_eq!(app.unread_threads(), 0);
        let text = StoredMessage { seq: 1, body: "hello".into(), timestamp: 2, ..Default::default() };
        app.append_and_advance("ef", text, 2, None).unwrap();
        assert_eq!(app.unread_threads(), 1);
    }

    #[test]
    fn a_burned_one_time_key_opens_for_its_grace_and_leaves_every_offer() {
        let app = temp_app("a_burned_one_time_key_opens_for_its_grace_and_leaves_every_offer");
        app.save_prekeys(b"", &[7; 32], &[(5, vec![5; 32]), (6, vec![6; 32])], false).unwrap();
        assert_eq!(app.one_time_secret(5), Some(vec![5; 32]));
        app.burn_one_time(5).unwrap();
        // Still opens: the slot may be re-read after a crash.
        assert_eq!(app.one_time_secret(5), Some(vec![5; 32]));
        assert_eq!(app.one_time_secret(6), Some(vec![6; 32]));
        assert_eq!(app.signed_prekey_secrets(), vec![vec![7; 32]]);
        // Rotation keeps the old signed secret in the grace slot.
        app.save_prekeys(b"", &[8; 32], &[], true).unwrap();
        assert_eq!(app.signed_prekey_secrets(), vec![vec![8; 32], vec![7; 32]]);
    }

    #[test]
    fn reactions_and_retractions_find_their_targets() {
        let text = |out: bool, seq: u64, ts: u64, body: &str| StoredMessage { outgoing: out, seq, body: body.into(), timestamp: ts, ..Default::default() };
        let thread = vec![
            text(true, 0, 100, "hi"),
            text(false, 0, 101, "hello"),
            // Their reaction to our "hi" (re_own=false from their side means our row).
            StoredMessage { outgoing: false, seq: 1, kind: 4, body: "👍".into(), timestamp: 102, re_seq: Some(0), re_own: false, ..Default::default() },
            // Our reaction to their "hello".
            StoredMessage { outgoing: true, seq: 1, kind: 4, body: "❤️".into(), timestamp: 103, re_seq: Some(0), re_own: false, ..Default::default() },
            // We take "hi" back.
            StoredMessage { outgoing: true, seq: 2, kind: 5, body: "took it back".into(), timestamp: 104, re_seq: Some(0), re_own: true, ..Default::default() },
            // A bill of ours, refused by them.
            StoredMessage { outgoing: true, seq: 3, kind: 1, amount_pxmr: 5, body: "bill".into(), timestamp: 105, ..Default::default() },
            StoredMessage { outgoing: false, seq: 2, kind: 5, body: "no".into(), timestamp: 106, re_seq: Some(3), re_own: false, ..Default::default() },
        ];
        let r = reactions_on(&thread);
        assert_eq!(r.get(&(0, 100)).cloned(), Some((None, Some("👍".into()))));
        assert_eq!(r.get(&(0, 101)).cloned(), Some((Some("❤️".into()), None)));
        let m = retractions(&thread);
        assert!(m.unsent.contains(&(0, 100)));
        assert!(m.quiet.contains(&(2, 104)));
        assert!(m.refused.contains(&(3, 105)));
        assert!(m.withdrawn.is_empty());
    }

    #[test]
    fn a_card_may_not_move_a_working_address() {
        let mut prior = contact("aa");
        assert_eq!(fold_card_address(None, Some("4new")), (Some("4new".into()), None));
        assert_eq!(fold_card_address(Some(&prior), Some("4new")), (Some("4new".into()), None));
        prior.their_address = Some("4old".into());
        assert_eq!(fold_card_address(Some(&prior), Some("4new")), (Some("4old".into()), Some("4new".into())));
        assert_eq!(fold_card_address(Some(&prior), Some("4old")), (Some("4old".into()), None));
        assert_eq!(fold_card_address(Some(&prior), None), (Some("4old".into()), None));
    }
}
