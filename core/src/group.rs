//! Group boards (§16.24): a member's page of messages on the shared record.
//!
//! A board is a DHT record with the SMPL schema — one owner, a list of
//! members, each writing only their own subkeys. Each member's subkeys are
//! a ring of pages; a page is the wire object here, sealed under the
//! generation's group key with the record key and subkey as associated
//! data, exactly as a publication chunk is.

use std::collections::BTreeMap;

use crate::cbor::{self, Value};
use crate::contact::MAX_MESSAGE_CHARS;
use crate::reject::{Reject, RejectCode};
use crate::wire::{f, Reader};

pub const PAGE_VERSION: u64 = 1;
/// Pages a member writes, a ring.
pub const PAGES: u32 = 4;
/// The DHT's 1024 subkeys over PAGES each.
pub const MAX_MEMBERS: usize = 256;
/// A reaction is a glyph or a word, not a paragraph.
pub const MAX_REACTION_CHARS: usize = 64;
/// More entries than a 32 KiB page could hold at the smallest entry.
pub const MAX_ENTRIES: usize = 2048;
pub const NONCE_LEN: usize = 24;
pub const TAG_LEN: usize = 16;

pub const KIND_TEXT: u64 = 0;
pub const KIND_REACTION: u64 = 4;
pub const KIND_RETRACT: u64 = 5;

/// One message on a page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GroupEntry {
    /// The sender's own counter in the group.
    pub seq: u64,
    /// Seconds since the epoch.
    pub ts: u64,
    pub kind: u64,
    pub body: Option<String>,
    /// The target of a reply, reaction or retraction: sender key and counter.
    pub re: Option<([u8; 32], u64)>,
}

/// A member's page: their entries under one generation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GroupPage {
    pub generation: u64,
    pub entries: Vec<GroupEntry>,
}

/// The most a sealed page may be on a record with `total_subkeys` subkeys:
/// the DHT bounds a subkey at 32 KiB and the record at 1 MiB.
pub fn page_cap(total_subkeys: u32) -> usize {
    let per = 1_048_576 / (total_subkeys.max(1) as usize);
    per.min(32_768)
}

impl GroupEntry {
    pub fn to_value(&self) -> Value {
        let mut m = BTreeMap::new();
        m.insert(f::GB_SEQ, Value::Uint(self.seq));
        m.insert(f::GB_TS, Value::Uint(self.ts));
        m.insert(f::GB_KIND, Value::Uint(self.kind));
        if let Some(b) = &self.body {
            m.insert(f::GB_BODY, Value::Text(b.clone()));
        }
        if let Some((who, seq)) = &self.re {
            m.insert(f::GB_RE_SENDER, Value::Bytes(who.to_vec()));
            m.insert(f::GB_RE_SEQ, Value::Uint(*seq));
        }
        Value::Map(m)
    }

    pub fn from_value(v: Value) -> Result<Self, Reject> {
        let mut r = Reader::new(v)?;
        let seq = r.uint(f::GB_SEQ)?;
        let ts = r.uint(f::GB_TS)?;
        let kind = r.uint(f::GB_KIND)?;
        let body = r.opt_text(f::GB_BODY, MAX_MESSAGE_CHARS)?;
        let re_sender = r.opt_bytes(f::GB_RE_SENDER, Some(32))?;
        let re_seq = r.opt_uint(f::GB_RE_SEQ)?;
        r.finish()?;
        if seq == 0 {
            return Err(Reject::with_detail(RejectCode::Malformed, "a group counter starts at one"));
        }
        if ts == 0 {
            return Err(Reject::with_detail(RejectCode::Malformed, "an entry carries when it was said"));
        }
        let re = match (re_sender, re_seq) {
            (None, None) => None,
            (Some(who), Some(seq)) => {
                if seq == 0 {
                    return Err(Reject::with_detail(RejectCode::Malformed, "a group counter starts at one"));
                }
                Some((who.try_into().expect("checked 32"), seq))
            }
            _ => {
                return Err(Reject::with_detail(
                    RejectCode::Malformed,
                    "a group reference is a sender and a counter, together or not at all",
                ))
            }
        };
        match kind {
            KIND_TEXT => {
                if body.as_deref().map_or(true, |b| b.trim().is_empty()) {
                    return Err(Reject::with_detail(RejectCode::Malformed, "a text says something"));
                }
            }
            KIND_REACTION => {
                match &body {
                    Some(b) if !b.trim().is_empty() && b.chars().count() <= MAX_REACTION_CHARS => {}
                    _ => return Err(Reject::with_detail(RejectCode::Malformed, "a reaction is a short body")),
                }
                if re.is_none() {
                    return Err(Reject::with_detail(RejectCode::Malformed, "a reaction names its target"));
                }
            }
            KIND_RETRACT => {
                if body.is_some() {
                    return Err(Reject::with_detail(RejectCode::Malformed, "a retraction carries no body"));
                }
                if re.is_none() {
                    return Err(Reject::with_detail(RejectCode::Malformed, "a retraction names its target"));
                }
            }
            _ => return Err(Reject::with_detail(RejectCode::Malformed, "not a kind a board carries")),
        }
        Ok(Self { seq, ts, kind, body, re })
    }
}

impl GroupPage {
    pub fn to_value(&self) -> Value {
        let mut m = BTreeMap::new();
        m.insert(f::GB_VERSION, Value::Uint(PAGE_VERSION));
        m.insert(f::GB_GEN, Value::Uint(self.generation));
        m.insert(f::GB_ENTRIES, Value::Array(self.entries.iter().map(GroupEntry::to_value).collect()));
        Value::Map(m)
    }

    pub fn encode(&self) -> Vec<u8> {
        self.to_value().encode()
    }

    pub fn from_value(v: Value) -> Result<Self, Reject> {
        let mut r = Reader::new(v)?;
        let version = r.uint(f::GB_VERSION)?;
        if version != PAGE_VERSION {
            return Err(Reject::with_detail(RejectCode::Malformed, "unknown group page version"));
        }
        let generation = r.uint(f::GB_GEN)?;
        if generation == 0 {
            return Err(Reject::with_detail(RejectCode::Malformed, "a board generation starts at one"));
        }
        let entries = r
            .opt_array(f::GB_ENTRIES)?
            .ok_or_else(|| Reject::with_detail(RejectCode::Malformed, "a page carries its entries"))?;
        r.finish()?;
        if entries.is_empty() {
            return Err(Reject::with_detail(RejectCode::Malformed, "a page says at least one thing"));
        }
        if entries.len() > MAX_ENTRIES {
            return Err(Reject::with_detail(RejectCode::Malformed, "more entries than a page can hold"));
        }
        let entries: Vec<GroupEntry> = entries.into_iter().map(GroupEntry::from_value).collect::<Result<_, _>>()?;
        for pair in entries.windows(2) {
            if pair[1].seq <= pair[0].seq {
                return Err(Reject::with_detail(RejectCode::Malformed, "entries ascend by counter, without repeats"));
            }
        }
        Ok(Self { generation, entries })
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, Reject> {
        let v = cbor::decode(bytes).map_err(|_| Reject::with_detail(RejectCode::Malformed, "not CBOR"))?;
        Self::from_value(v)
    }
}

fn page_aad(record_key: &str, subkey: u32) -> Vec<u8> {
    format!("ducat group page\n{record_key}\n{subkey}").into_bytes()
}

/// Seal a page for one subkey of one record. The caller draws the nonce
/// fresh per write. Output is `nonce ‖ ciphertext`.
pub fn seal_page(key: &[u8; 32], record_key: &str, subkey: u32, nonce: &[u8; NONCE_LEN], plaintext: &[u8]) -> Vec<u8> {
    use chacha20poly1305::aead::{Aead, KeyInit, Payload};
    use chacha20poly1305::{XChaCha20Poly1305, XNonce};
    let cipher = XChaCha20Poly1305::new(key.into());
    let ct = cipher
        .encrypt(XNonce::from_slice(nonce), Payload { msg: plaintext, aad: &page_aad(record_key, subkey) })
        .expect("XChaCha encrypt is infallible for in-memory buffers");
    let mut out = Vec::with_capacity(NONCE_LEN + ct.len());
    out.extend_from_slice(nonce);
    out.extend_from_slice(&ct);
    out
}

/// Open a page read from `record_key`'s `subkey`: the landing site is the
/// AAD, so a page moved between subkeys or boards fails to open.
pub fn open_page(key: &[u8; 32], record_key: &str, subkey: u32, value: &[u8]) -> Result<Vec<u8>, Reject> {
    use chacha20poly1305::aead::{Aead, KeyInit, Payload};
    use chacha20poly1305::{XChaCha20Poly1305, XNonce};
    if value.len() < NONCE_LEN + TAG_LEN {
        return Err(Reject::with_detail(RejectCode::Malformed, "shorter than a nonce and a tag; nothing sealed is this small"));
    }
    let (nonce, ct) = value.split_at(NONCE_LEN);
    let cipher = XChaCha20Poly1305::new(key.into());
    cipher
        .decrypt(XNonce::from_slice(nonce), Payload { msg: ct, aad: &page_aad(record_key, subkey) })
        .map_err(|_| Reject::with_detail(RejectCode::Malformed, "the page does not open under this key at this subkey"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page() -> GroupPage {
        GroupPage {
            generation: 3,
            entries: vec![
                GroupEntry { seq: 1, ts: 1_700_000_000, kind: KIND_TEXT, body: Some("hello".into()), re: None },
                GroupEntry { seq: 2, ts: 1_700_000_005, kind: KIND_REACTION, body: Some("👍".into()), re: Some(([7u8; 32], 9)) },
                GroupEntry { seq: 5, ts: 1_700_000_009, kind: KIND_RETRACT, body: None, re: Some(([7u8; 32], 1)) },
            ],
        }
    }

    #[test]
    fn page_round_trips() {
        let p = page();
        let bytes = p.encode();
        assert_eq!(GroupPage::decode(&bytes).unwrap(), p);
        assert_eq!(GroupPage::decode(&bytes).unwrap().encode(), bytes);
    }

    #[test]
    fn a_field_nobody_assigned_is_refused() {
        let mut v = page().to_value();
        if let Value::Map(m) = &mut v {
            m.insert(299, Value::Uint(1));
        }
        assert!(GroupPage::from_value(v).is_err());
    }

    #[test]
    fn entries_ascend_and_references_pair() {
        let mut p = page();
        p.entries.swap(0, 1);
        assert!(GroupPage::decode(&p.encode()).is_err());
        let mut v = page().entries[0].to_value();
        if let Value::Map(m) = &mut v {
            m.insert(f::GB_RE_SEQ, Value::Uint(4));
        }
        assert!(GroupEntry::from_value(v).is_err());
    }

    #[test]
    fn a_reaction_and_a_retraction_name_a_target() {
        let e = GroupEntry { seq: 1, ts: 1, kind: KIND_REACTION, body: Some("x".into()), re: None };
        assert!(GroupEntry::from_value(e.to_value()).is_err());
        let e = GroupEntry { seq: 1, ts: 1, kind: KIND_RETRACT, body: Some("x".into()), re: Some(([1u8; 32], 1)) };
        assert!(GroupEntry::from_value(e.to_value()).is_err());
        let e = GroupEntry { seq: 1, ts: 1, kind: 12, body: Some("roster".into()), re: None };
        assert!(GroupEntry::from_value(e.to_value()).is_err());
    }

    #[test]
    fn a_page_opens_only_where_it_was_sealed() {
        let key = [9u8; 32];
        let nonce = [1u8; NONCE_LEN];
        let sealed = seal_page(&key, "VLD0:abc", 5, &nonce, &page().encode());
        assert_eq!(GroupPage::decode(&open_page(&key, "VLD0:abc", 5, &sealed).unwrap()).unwrap(), page());
        assert!(open_page(&key, "VLD0:abc", 6, &sealed).is_err());
        assert!(open_page(&key, "VLD0:abd", 5, &sealed).is_err());
        assert!(open_page(&[8u8; 32], "VLD0:abc", 5, &sealed).is_err());
    }

    #[test]
    fn the_cap_follows_the_record() {
        assert_eq!(page_cap(8), 32_768);
        assert_eq!(page_cap(128), 8_192);
        assert_eq!(page_cap(1024), 1_024);
    }
}
