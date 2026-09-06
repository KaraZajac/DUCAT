//! Groups (§16.19): a name, a roster, and no server — every message is
//! written into each member's pairwise thread with the group's id and
//! the sender's own counter there, and a member's view is the merge of
//! those threads. The phone's `Groups.kt`.
//!
//! The mesh must be complete to speak: a member this desk has no thread
//! with cannot be written to, and a group message that reaches some
//! members and not others is a conversation that did not happen.

use std::time::Instant;
use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

use ducat_mobile::node::{node_dht_board_create, node_dht_board_key, node_dht_get_versioned, node_dht_inspect, node_dht_open, node_dht_set, node_dht_watch, GroupBoardCreate, GroupBoardSpec};
use ducat_mobile::contacts::{group_board_nameplate, group_board_pages, group_page_cap, group_page_decode, group_page_encode, group_page_open, group_page_seal, group_roster_decode, group_roster_encode, GroupBoardOut, GroupEntryOut, GroupPageSeal, GroupSend};
use serde::{Deserialize, Serialize};

use crate::contacts::{bump, hex, hex_to_bytes, StoredMessage};
use crate::mailbox::{plan_due, plan_set_watched, plan_settle, plan_touch, plan_watched, Outgoing};
use crate::{log, App, Error};

const TAG: &str = "Groups";
const STORE: &str = "ducat_groups";
const RETRIES_PER_PASS: usize = 8;
const MAX_QUEUED: usize = 200;

static GROUPS: Mutex<()> = Mutex::new(());
/// One lock per board: the lap's read and my own write both look at what
/// is held before they append, and interleaved they would keep an entry
/// twice.
static BOARD_LOCKS: Mutex<Option<HashMap<String, std::sync::Arc<Mutex<()>>>>> = Mutex::new(None);
/// The last subkey sequences narrated per board.
static LAST_SEQS: Mutex<Option<HashMap<String, Vec<i64>>>> = Mutex::new(None);

fn board_lock(id_hex: &str) -> std::sync::Arc<Mutex<()>> {
    let mut g = BOARD_LOCKS.lock().unwrap_or_else(|e| e.into_inner());
    g.get_or_insert_with(HashMap::new)
        .entry(id_hex.to_string())
        .or_insert_with(|| std::sync::Arc::new(Mutex::new(())))
        .clone()
}
static RETRY_CURSOR: Mutex<usize> = Mutex::new(0);

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Group {
    #[serde(rename = "id")]
    pub id_hex: String,
    pub name: String,
    #[serde(default)]
    pub members: Vec<String>,
    #[serde(rename = "my_seq", default)]
    pub my_group_seq: u64,
    #[serde(default)]
    pub disclosed: bool,
    /// The board this group's words ride on (§16.24); a group without
    /// one fans out over pairwise threads as §16.19 says.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub board: Option<Board>,
}

/// One generation of a group's board (§16.24): the record, its key, and
/// where this member's own pen is.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Board {
    #[serde(rename = "gen")]
    pub generation: u64,
    /// The persona that formed this generation — the record's owner.
    pub owner: String,
    /// The record key; computed from the roster, cached here.
    #[serde(default)]
    pub key: String,
    #[serde(rename = "gkey")]
    pub group_key_hex: String,
    pub pages: u32,
    /// The page of my ring I am writing.
    #[serde(default)]
    pub page: u32,
    /// My current page has words the record does not hold yet.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub dirty: bool,
    /// The sequence last read, per subkey.
    #[serde(default)]
    pub seen: HashMap<String, u32>,
}

/// An entry of my current page, as kept on disk so a page can be sealed
/// again after a restart or a failed write.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct PageEntry {
    s: u64,
    t: u64,
    k: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    b: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    rs: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    rq: Option<u64>,
}

impl PageEntry {
    fn out(&self) -> GroupEntryOut {
        GroupEntryOut {
            seq: self.s,
            ts: self.t,
            kind: self.k,
            body: self.b.clone(),
            re_sender: self.rs.as_deref().and_then(hex_to_bytes),
            re_seq: self.rq,
        }
    }
}

/// The shape a board's record takes from its roster: owner first, the
/// nameplate, then every other member in ascending key order — the same
/// on every phone, so the same record key.
fn board_spec(id_hex: &str, members: &[String], board: &Board) -> Result<GroupBoardSpec, Error> {
    let owner_public = hex_to_bytes(&board.owner).ok_or_else(|| Error::Refused("a board owner is a persona key".into()))?;
    let group_id = hex_to_bytes(id_hex).ok_or_else(|| Error::Refused("a group id is hex".into()))?;
    let mut others: Vec<Vec<u8>> = members
        .iter()
        .filter(|m| **m != board.owner)
        .filter_map(|m| hex_to_bytes(m))
        .collect();
    others.sort();
    others.dedup();
    if others.len() + 1 > 255 {
        return Err(Error::Refused("a board holds at most 255 members".into()));
    }
    Ok(GroupBoardSpec {
        owner_public,
        nameplate: group_board_nameplate(group_id, board.generation),
        members: others,
        pages: board.pages,
    })
}

/// Subkeys on the record: the owner's pages, the nameplate's one, and a
/// ring per other member.
fn board_subkeys(spec: &GroupBoardSpec) -> u32 {
    spec.pages * (1 + spec.members.len() as u32) + 1
}

/// The first subkey of a member's ring, or None for a key not on the board.
fn board_base(spec: &GroupBoardSpec, member: &[u8]) -> Option<u32> {
    if member == spec.owner_public.as_slice() {
        return Some(0);
    }
    spec.members
        .iter()
        .position(|m| m.as_slice() == member)
        .map(|i| spec.pages * (1 + i as u32) + 1)
}

/// Whose ring a subkey is in; None for the nameplate's.
fn board_member_of(spec: &GroupBoardSpec, subkey: u32) -> Option<Vec<u8>> {
    let pages = spec.pages.max(1);
    if subkey < pages {
        return Some(spec.owner_public.clone());
    }
    if subkey == pages {
        return None;
    }
    spec.members.get(((subkey - pages - 1) / pages) as usize).cloned()
}

/// Sealing adds a nonce and a tag; a page must fit under the cap with them.
const SEAL_OVERHEAD: usize = 40;

/// One undelivered copy, kept until it lands.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Retry {
    g: String,
    m: String,
    #[serde(default)]
    b: String,
    #[serde(default)]
    k: u32,
    s: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    rs: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    rq: Option<u64>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    roster: bool,
}

impl Retry {
    fn same(&self, o: &Retry) -> bool {
        self.g == o.g && self.m == o.m && self.s == o.s && self.roster == o.roster
    }
}

/// One line of a group's merged view: who said it, and their row.
#[derive(Clone, Debug, Serialize)]
pub struct GroupRow {
    pub sender_hex: String,
    pub message: StoredMessage,
}

/// How far a reader has looked: each member's highest counter, and how
/// many rows of theirs there were.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Look {
    #[serde(default)]
    pub high: HashMap<String, u64>,
    #[serde(default)]
    pub rows: HashMap<String, u64>,
}

impl App {
    pub fn groups(&self) -> Vec<Group> {
        self.store(STORE).get("groups").unwrap_or_default()
    }

    pub fn group(&self, id_hex: &str) -> Option<Group> {
        self.groups().into_iter().find(|g| g.id_hex == id_hex)
    }

    fn save_groups(&self, groups: &[Group]) -> Result<(), Error> {
        self.store(STORE).put("groups", &groups)?;
        bump();
        Ok(())
    }

    fn upsert_group(&self, g: Group) -> Result<(), Error> {
        let _l = GROUPS.lock().unwrap_or_else(|e| e.into_inner());
        let mut cur = self.groups();
        match cur.iter_mut().find(|x| x.id_hex == g.id_hex) {
            Some(slot) => *slot = g,
            None => cur.push(g),
        }
        self.save_groups(&cur)
    }

    /// Which of our personas is in this roster — the one the group was
    /// joined under, else the primary.
    pub fn mine_in(&self, members: &[String]) -> String {
        let ours = self.persona_hexes();
        members.iter().find(|m| ours.contains(*m)).cloned().unwrap_or_else(|| self.primary_hex().unwrap_or_default())
    }

    pub fn create_group(&self, name: &str, member_hexes: &[String]) -> Result<Group, Error> {
        let mine = self.worn()?;
        let id = hex(&ducat_mobile::create_persona_secret()[..16]);
        let mut members: Vec<String> = member_hexes.to_vec();
        if !members.contains(&mine) {
            members.push(mine);
        }
        members.dedup();
        let g = Group { id_hex: id, name: ducat_mobile::contacts::clean_display_text(name.trim().to_string()), members: members.clone(), my_group_seq: 0, disclosed: false, board: None };
        // The board first: a group made here rides on one from its first
        // word. Forming it needs the node; a group cannot be made offline.
        let board = self.form_board(&g.id_hex, &g.members, 1)?;
        let g = Group { board: Some(board), ..g };
        self.upsert_group(g.clone())?;
        self.send_roster(&g);
        log::info(TAG, format!("created {} with {} member(s), on a board", g.name, members.len()));
        Ok(g)
    }

    pub fn add_to_group(&self, id_hex: &str, persona_hex: &str) -> Result<(), Error> {
        let Some(g) = self.group(id_hex) else { return Ok(()) };
        if g.members.iter().any(|m| m == persona_hex) {
            return Ok(());
        }
        let mut grown = g.clone();
        grown.members.push(persona_hex.to_string());
        // A different member list is a different record: the adder forms
        // the next generation and tells everyone, the newcomer included.
        let next = g.board.as_ref().map_or(1, |b| b.generation + 1);
        grown.board = Some(self.form_board(&grown.id_hex, &grown.members, next)?);
        self.upsert_group(grown.clone())?;
        self.send_roster(&grown);
        log::info(TAG, format!("{}: added {}…", g.name, &persona_hex[..8.min(persona_hex.len())]));
        Ok(())
    }

    /// A roster arrived (kind 12): join a group we are named in, or grow
    /// one we know — from a member, never from anyone else.
    pub fn absorb_roster(&self, sender_hex: &str, group_id: Option<&[u8]>, payload: Option<&[u8]>) {
        let (Some(gid), Some(payload)) = (group_id, payload) else { return };
        let id_hex = hex(gid);
        let Ok(roster) = group_roster_decode(payload.to_vec()) else { return };
        let members: Vec<String> = roster.members.iter().map(|m| hex(m)).collect();
        let short = &sender_hex[..8.min(sender_hex.len())];
        if !members.iter().any(|m| m == sender_hex) {
            log::warn(TAG, format!("roster from {short}… does not include them — ignored"));
            return;
        }
        let incoming = roster.board.as_ref().map(|b| Board {
            generation: b.generation,
            owner: hex(&b.owner),
            key: String::new(),
            group_key_hex: hex(&b.group_key),
            pages: b.pages,
            page: 0,
            dirty: false,
            seen: HashMap::new(),
        });
        match self.group(&id_hex) {
            None => {
                let ours = self.persona_hexes();
                if !members.iter().any(|m| ours.contains(m)) {
                    log::warn(TAG, "roster for a group we are not in — ignored");
                    return;
                }
                let _ = self.upsert_group(Group { id_hex: id_hex.clone(), name: roster.name.clone(), members: members.clone(), my_group_seq: 0, disclosed: false, board: incoming });
                plan_touch(&format!("g:{id_hex}"));
                let adder = self.contact(sender_hex).map(|c| c.display_name()).unwrap_or_else(|| format!("{short}…"));
                log::info(TAG, format!("joined {} ({} member(s)) — added by {adder}", roster.name, members.len()));
            }
            Some(known) => {
                if !known.members.iter().any(|m| m == sender_hex) {
                    log::warn(TAG, format!("roster for {} from a non-member — ignored", known.name));
                    return;
                }
                let mut merged = known.members.clone();
                for m in members {
                    if !merged.contains(&m) {
                        merged.push(m);
                    }
                }
                // The board: a higher generation moves us; an equal one
                // from a different owner is the tie §16.24 breaks by the
                // lower owner key, and the loser re-forms with the union.
                let mut board = known.board.clone();
                let mut lost = false;
                if let Some(b) = incoming {
                    match &known.board {
                        None => board = Some(b),
                        Some(k) if b.generation > k.generation => board = Some(b),
                        Some(k) if b.generation == k.generation && b.owner != k.owner => {
                            if b.owner < k.owner {
                                lost = self.persona_hexes().contains(&k.owner);
                                board = Some(b);
                            } else {
                                lost = self.persona_hexes().contains(&b.owner);
                            }
                        }
                        Some(_) => {}
                    }
                }
                let moved = board.as_ref().map(|b| b.generation) != known.board.as_ref().map(|b| b.generation)
                    || board.as_ref().map(|b| &b.owner) != known.board.as_ref().map(|b| &b.owner);
                if merged.len() != known.members.len() || moved {
                    let n = merged.len();
                    let gen = board.as_ref().map(|b| b.generation);
                    let _ = self.upsert_group(Group { members: merged.clone(), board, ..known.clone() });
                    if moved {
                        // A new generation is a new record: everything on it is unread.
                        plan_touch(&format!("g:{id_hex}"));
                        log::info(TAG, format!("{}: on generation {} now, {n} member(s)", known.name, gen.unwrap_or(0)));
                    } else {
                        log::info(TAG, format!("{}: roster grew to {n}", known.name));
                    }
                }
                if lost {
                    // Two of us formed the same generation; mine lost the
                    // tie. The next one carries everyone.
                    let next = self.group(&id_hex).and_then(|g| g.board).map_or(1, |b| b.generation + 1);
                    match self.form_board(&id_hex, &merged, next) {
                        Ok(b) => {
                            if let Some(g) = self.group(&id_hex) {
                                let g = Group { board: Some(b), ..g };
                                let _ = self.upsert_group(g.clone());
                                self.send_roster(&g);
                            }
                        }
                        Err(e) => log::warn(TAG, format!("{}: could not re-form after a tie: {e}", known.name)),
                    }
                }
            }
        }
    }

    fn roster_payload(g: &Group) -> Result<Vec<u8>, Error> {
        let members: Option<Vec<Vec<u8>>> = g.members.iter().map(|m| hex_to_bytes(m)).collect();
        let board = match &g.board {
            Some(b) => Some(GroupBoardOut {
                generation: b.generation,
                owner: hex_to_bytes(&b.owner).ok_or_else(|| Error::Refused("a board owner is a persona key".into()))?,
                group_key: hex_to_bytes(&b.group_key_hex).ok_or_else(|| Error::Refused("a group key is hex".into()))?,
                pages: b.pages,
            }),
            None => None,
        };
        Ok(group_roster_encode(g.name.clone(), members.ok_or_else(|| Error::Refused("a member key is not hex".into()))?, board)?)
    }

    fn send_roster(&self, g: &Group) {
        let mine = self.mine_in(&g.members);
        let Ok(payload) = App::roster_payload(g) else { return };
        let fresh = self.group(&g.id_hex).unwrap_or_else(|| g.clone());
        let seq = fresh.my_group_seq + 1;
        let _ = self.upsert_group(Group { my_group_seq: seq, ..fresh });
        for m in g.members.iter().filter(|m| **m != mine) {
            let Some(c) = self.contact(m) else {
                log::warn(TAG, format!("roster: {}… is not a contact — not sent", &m[..8.min(m.len())]));
                continue;
            };
            let out = Outgoing {
                body: format!("group: {}", g.name),
                kind: 12,
                payload: Some(payload.clone()),
                group: Some(GroupSend { id: hex_to_bytes(&g.id_hex), seq: Some(seq), re_sender: None, re_seq: None }),
                ..Default::default()
            };
            if let Err(e) = self.send(&c, out) {
                self.queue_retry(Retry { g: g.id_hex.clone(), m: m.clone(), b: String::new(), k: 12, s: seq, rs: None, rq: None, roster: true });
                log::warn(TAG, format!("{}: roster to {} queued ({e})", g.name, c.display_name()));
            }
        }
    }

    /// Members this desk cannot write to yet.
    pub fn group_missing(&self, id_hex: &str) -> Vec<String> {
        let Some(g) = self.group(id_hex) else { return Vec::new() };
        // A board needs no mesh: one write reaches everyone.
        if g.board.is_some() {
            return Vec::new();
        }
        let mine = self.mine_in(&g.members);
        let contacts: HashSet<String> = self.contacts().into_iter().map(|c| c.persona_hex).collect();
        g.members.into_iter().filter(|m| *m != mine && !contacts.contains(m)).collect()
    }

    /// Say something to the group: one copy into each member's thread,
    /// under one group counter. Copies that do not go out are queued.
    pub fn send_group(&self, id_hex: &str, body: &str, kind: u32, re_sender: Option<&str>, re_seq: Option<u64>) -> Result<bool, Error> {
        let g = self.group(id_hex).ok_or_else(|| Error::Refused("no such group".into()))?;
        if !self.group_missing(id_hex).is_empty() {
            return Err(Error::Refused("the group's mesh is incomplete — somebody in it is not a contact yet".into()));
        }
        let mine = self.mine_in(&g.members);
        let seq = {
            let _l = GROUPS.lock().unwrap_or_else(|e| e.into_inner());
            let mut all = self.groups();
            let n = all.iter().find(|x| x.id_hex == id_hex).map_or(g.my_group_seq, |x| x.my_group_seq) + 1;
            if let Some(x) = all.iter_mut().find(|x| x.id_hex == id_hex) {
                x.my_group_seq = n;
            }
            self.save_groups(&all)?;
            n
        };
        if let Some(board) = g.board.clone() {
            return self.send_on_board(&g, board, &mine, seq, body, kind, re_sender, re_seq);
        }
        let mut failed = 0;
        for m in g.members.iter().filter(|m| **m != mine) {
            let Some(c) = self.contact(m) else {
                log::warn(TAG, format!("send: {}… is not a contact — their copy not written", &m[..8.min(m.len())]));
                continue;
            };
            let out = Outgoing {
                body: body.to_string(),
                kind,
                group: Some(GroupSend { id: hex_to_bytes(id_hex), seq: Some(seq), re_sender: re_sender.and_then(hex_to_bytes), re_seq }),
                ..Default::default()
            };
            if let Err(e) = self.send(&c, out) {
                failed += 1;
                self.queue_retry(Retry { g: id_hex.to_string(), m: m.clone(), b: body.to_string(), k: kind, s: seq, rs: re_sender.map(String::from), rq: re_seq, roster: false });
                log::warn(TAG, format!("{}: {} not reached — queued ({e})", g.name, c.display_name()));
            }
        }
        Ok(failed == 0)
    }

    // ----- the board (§16.24) ----------------------------------------------

    /// Form a generation: mint its key, create its record as the member
    /// whose persona is in this roster.
    fn form_board(&self, id_hex: &str, members: &[String], generation: u64) -> Result<Board, Error> {
        let mine = self.mine_in(members);
        let secret = self
            .persona_secret(&mine)?
            .ok_or_else(|| Error::Refused("no secret for my persona in this group".into()))?;
        let draft = Board {
            generation,
            owner: mine,
            key: String::new(),
            group_key_hex: hex(&ducat_mobile::random_bytes(32)),
            pages: group_board_pages(),
            page: 0,
            dirty: false,
            seen: HashMap::new(),
        };
        let spec = board_spec(id_hex, members, &draft)?;
        let key = node_dht_board_create(GroupBoardCreate { spec, owner_secret: secret })?;
        Ok(Board { key, ..draft })
    }

    /// The board's record key: computed from the roster every time (a
    /// local derivation, no network), and the cache refreshed when it
    /// differs — a client that once computed it under an older layout
    /// heals itself on its next read instead of watching an empty record
    /// for ever.
    fn board_key(&self, g: &Group, board: &Board) -> Result<String, Error> {
        let key = node_dht_board_key(board_spec(&g.id_hex, &g.members, board)?)?;
        if key != board.key {
            if let Some(fresh) = self.group(&g.id_hex) {
                if let Some(mut b) = fresh.board.clone() {
                    if b.generation == board.generation {
                        b.key = key.clone();
                        b.seen.clear();
                        let _ = self.upsert_group(Group { board: Some(b), ..fresh });
                    }
                }
            }
        }
        Ok(key)
    }

    fn my_page(&self, id_hex: &str) -> Vec<PageEntry> {
        self.store(STORE).get(&format!("page_{id_hex}")).unwrap_or_default()
    }

    fn save_my_page(&self, id_hex: &str, entries: &[PageEntry]) -> Result<(), Error> {
        self.store(STORE).put(&format!("page_{id_hex}"), &entries)?;
        Ok(())
    }

    fn open_board_as_me(&self, g: &Group, board: &Board) -> Result<(String, String), Error> {
        let mine = self.mine_in(&g.members);
        let secret = self
            .persona_secret(&mine)?
            .ok_or_else(|| Error::Refused("no secret for my persona in this group".into()))?;
        let key = self.board_key(g, board)?;
        let public = hex_to_bytes(&mine).ok_or_else(|| Error::Refused("my persona key is not hex".into()))?;
        node_dht_open(key.clone(), Some(public), Some(secret))?;
        Ok((key, mine))
    }

    /// Say something on the board: append to my page, seal it, write it.
    /// The row and the page go to disk first, so a write the network
    /// refuses is retried from disk by the lap, not lost.
    #[allow(clippy::too_many_arguments)]
    fn send_on_board(
        &self,
        g: &Group,
        board: Board,
        mine: &str,
        seq: u64,
        body: &str,
        kind: u32,
        re_sender: Option<&str>,
        re_seq: Option<u64>,
    ) -> Result<bool, Error> {
        let lock = board_lock(&g.id_hex);
        let _pen = lock.lock().unwrap_or_else(|e| e.into_inner());
        let entry = PageEntry {
            s: seq,
            t: App::now(),
            k: kind,
            b: if kind == 5 { None } else { Some(body.to_string()) },
            rs: re_sender.map(String::from),
            rq: re_seq,
        };
        let spec = board_spec(&g.id_hex, &g.members, &board)?;
        let cap = group_page_cap(board_subkeys(&spec)) as usize;
        let mut page = board.page;
        let mut entries = self.my_page(&g.id_hex);
        entries.push(entry.clone());
        let mut bytes = group_page_encode(board.generation, entries.iter().map(PageEntry::out).collect())?;
        if bytes.len() + SEAL_OVERHEAD > cap && entries.len() > 1 {
            // The page is full: this word opens the next one in the ring.
            page = (page + 1) % board.pages.max(1);
            entries = vec![entry.clone()];
            bytes = group_page_encode(board.generation, entries.iter().map(PageEntry::out).collect())?;
        }
        if bytes.len() + SEAL_OVERHEAD > cap {
            return Err(Error::Refused("too long for this board's pages".into()));
        }
        self.append_group_row(
            mine,
            StoredMessage {
                outgoing: true,
                seq,
                body: body.to_string(),
                timestamp: entry.t,
                kind,
                group_id: Some(g.id_hex.clone()),
                group_seq: seq,
                group_re_sender: re_sender.map(String::from),
                group_re_seq: re_seq,
                ..Default::default()
            },
        )?;
        self.save_my_page(&g.id_hex, &entries)?;
        let set_board = |dirty: bool, page: u32| -> Result<(), Error> {
            if let Some(fresh) = self.group(&g.id_hex) {
                if let Some(mut b) = fresh.board.clone() {
                    if b.generation == board.generation {
                        b.page = page;
                        b.dirty = dirty;
                        self.upsert_group(Group { board: Some(b), ..fresh })?;
                    }
                }
            }
            Ok(())
        };
        set_board(true, page)?;
        plan_touch(&format!("g:{}", g.id_hex));
        match self.write_my_page(g, &board, mine, page, &bytes) {
            Ok(()) => {
                set_board(false, page)?;
                Ok(true)
            }
            Err(e) => {
                log::warn(TAG, format!("{}: the board did not take the page — kept for the lap ({e})", g.name));
                Ok(false)
            }
        }
    }

    fn write_my_page(&self, g: &Group, board: &Board, mine: &str, page: u32, bytes: &[u8]) -> Result<(), Error> {
        let spec = board_spec(&g.id_hex, &g.members, board)?;
        let public = hex_to_bytes(mine).ok_or_else(|| Error::Refused("my persona key is not hex".into()))?;
        let base = board_base(&spec, &public).ok_or_else(|| Error::Refused("my persona is not on this board".into()))?;
        let (key, _) = self.open_board_as_me(g, board)?;
        let group_key = hex_to_bytes(&board.group_key_hex).ok_or_else(|| Error::Refused("a group key is hex".into()))?;
        let sealed = group_page_seal(GroupPageSeal { group_key, record_key: key.clone(), subkey: base + page, bytes: bytes.to_vec() })?;
        node_dht_set(key, base + page, sealed)?;
        Ok(())
    }

    /// Pages the network did not take yet, written again.
    fn flush_boards(&self) {
        for g in self.groups() {
            let Some(board) = g.board.clone() else { continue };
            if !board.dirty {
                continue;
            }
            let entries = self.my_page(&g.id_hex);
            if entries.is_empty() {
                continue;
            }
            let mine = self.mine_in(&g.members);
            let bytes = match group_page_encode(board.generation, entries.iter().map(PageEntry::out).collect()) {
                Ok(b) => b,
                Err(e) => {
                    log::warn(TAG, format!("{}: my page does not encode: {e}", g.name));
                    continue;
                }
            };
            match self.write_my_page(&g, &board, &mine, board.page, &bytes) {
                Ok(()) => {
                    if let Some(fresh) = self.group(&g.id_hex) {
                        if let Some(mut b) = fresh.board.clone() {
                            b.dirty = false;
                            let _ = self.upsert_group(Group { board: Some(b), ..fresh });
                        }
                    }
                    log::info(TAG, format!("{}: page landed", g.name));
                }
                Err(e) => log::warn(TAG, format!("{}: page still not taken ({e})", g.name)),
            }
        }
    }

    /// Read what moved on one board: one inspection, then only the pages
    /// whose sequence changed. Returns how many entries were new.
    fn read_board(&self, g: &Group, board: &Board) -> Result<usize, Error> {
        let lock = board_lock(&g.id_hex);
        let _held = lock.lock().unwrap_or_else(|e| e.into_inner());
        let (key, mine) = self.open_board_as_me(g, board)?;
        let spec = board_spec(&g.id_hex, &g.members, board)?;
        let group_key = hex_to_bytes(&board.group_key_hex).ok_or_else(|| Error::Refused("a group key is hex".into()))?;
        let seqs = node_dht_inspect(key.clone())?;
        {
            // The record as this node sees it, whenever that view changes:
            // a page another member wrote that never shows here is a
            // question for veilid, and this line is the evidence.
            let shown: Vec<i64> = seqs.iter().map(|s| if *s == u32::MAX { -1 } else { *s as i64 }).collect();
            let mut last = LAST_SEQS.lock().unwrap_or_else(|e| e.into_inner());
            let map = last.get_or_insert_with(HashMap::new);
            if map.get(&g.id_hex) != Some(&shown) {
                log::info(TAG, format!("{}: board inspected — subkey seqs {shown:?}", g.name));
                map.insert(g.id_hex.clone(), shown);
            }
        }
        let mut got = 0;
        let mut pending = false;
        let mut seen = board.seen.clone();
        for (subkey, seq) in seqs.iter().enumerate() {
            let subkey = subkey as u32;
            if *seq == u32::MAX || seen.get(&subkey.to_string()) == Some(seq) {
                continue;
            }
            // The network first; then the node's own copy, which a watch
            // on the record keeps current and which answers when a get
            // stops three nodes short of the one that holds the page. A
            // page the inspection names but neither can produce keeps the
            // board hot for the next lap instead of backing it off.
            let t = Instant::now();
            let network = node_dht_get_versioned(key.clone(), subkey, true);
            let net_ms = t.elapsed().as_millis();
            let read = match network {
                Ok(Some(r)) => Some(r),
                Ok(None) => node_dht_get_versioned(key.clone(), subkey, false).ok().flatten(),
                Err(e) => {
                    log::warn(TAG, format!("{}: page {subkey} (seq {seq}) network get failed after {net_ms} ms: {e}", g.name));
                    node_dht_get_versioned(key.clone(), subkey, false).ok().flatten()
                }
            };
            let Some(read) = read else {
                log::info(TAG, format!("{}: page {subkey} is at seq {seq} on the network but neither the network ({net_ms} ms) nor this node's copy produced it — again next lap", g.name));
                pending = true;
                continue;
            };
            let Some(sender) = board_member_of(&spec, subkey) else { continue };
            let sender_hex = hex(&sender);
            let plain = match group_page_open(GroupPageSeal { group_key: group_key.clone(), record_key: key.clone(), subkey, bytes: read.data }) {
                Ok(p) => p,
                Err(e) => {
                    log::warn(TAG, format!("{}: page {subkey} does not open: {e}", g.name));
                    seen.insert(subkey.to_string(), *seq);
                    continue;
                }
            };
            let page = match group_page_decode(plain) {
                Ok(p) => p,
                Err(e) => {
                    log::warn(TAG, format!("{}: page {subkey} refused: {e}", g.name));
                    seen.insert(subkey.to_string(), *seq);
                    continue;
                }
            };
            if page.generation != board.generation {
                log::warn(TAG, format!("{}: page {subkey} is from generation {}, not {}", g.name, page.generation, board.generation));
                seen.insert(subkey.to_string(), *seq);
                continue;
            }
            // What is held of THIS sender's counter: their rows in their
            // thread, or mine in mine. A member's thread also holds my
            // outgoing rows to them — the roster among them, under my
            // counter — and those must not mask their entries.
            let theirs = sender_hex != mine;
            let held: HashSet<u64> = self
                .thread(&sender_hex)
                .into_iter()
                .filter(|r| r.group_id.as_deref() == Some(g.id_hex.as_str()) && r.outgoing != theirs)
                .map(|r| r.group_seq)
                .collect();
            for e in page.entries {
                if held.contains(&e.seq) {
                    continue;
                }
                self.append_group_row(
                    &sender_hex,
                    StoredMessage {
                        outgoing: sender_hex == mine,
                        seq: e.seq,
                        body: e.body.unwrap_or_default(),
                        timestamp: e.ts,
                        kind: e.kind,
                        group_id: Some(g.id_hex.clone()),
                        group_seq: e.seq,
                        group_re_sender: e.re_sender.as_deref().map(hex),
                        group_re_seq: e.re_seq,
                        ..Default::default()
                    },
                )?;
                got += 1;
            }
            seen.insert(subkey.to_string(), read.seq.unwrap_or(*seq));
        }
        if pending {
            plan_touch(&format!("g:{}", g.id_hex));
        }
        if seen != board.seen {
            if board.seen.is_empty() {
                log::info(TAG, format!("{}: first pages read — {} subkey(s) hold something", g.name, seen.len()));
            }
            if let Some(fresh) = self.group(&g.id_hex) {
                if let Some(mut b) = fresh.board.clone() {
                    if b.generation == board.generation {
                        b.seen = seen;
                        let _ = self.upsert_group(Group { board: Some(b), ..fresh });
                    }
                }
            }
        }
        Ok(got)
    }

    /// The boards that are due, on the same plan as the logs: a board
    /// that spoke is read every lap, a quiet one backs off, the watch on
    /// its record rings the lap for it alone. Returns how many entries
    /// arrived.
    pub fn groups_lap(&self) -> usize {
        self.flush_boards();
        let now = Instant::now();
        let mut got = 0;
        for g in self.groups() {
            let Some(board) = g.board.clone() else { continue };
            let slot = format!("g:{}", g.id_hex);
            if plan_due(&slot).map_or(false, |at| at > now) {
                continue;
            }
            if board.seen.is_empty() {
                log::info(TAG, format!("{}: reading its board (generation {})", g.name, board.generation));
            }
            match self.read_board(&g, &board) {
                Ok(n) => {
                    got += n;
                    plan_settle(&slot, n > 0);
                }
                Err(e) => {
                    log::warn(TAG, format!("{}: board not read ({e})", g.name));
                    plan_settle(&slot, false);
                }
            }
            let stale = plan_watched(&slot).map_or(true, |t| now.duration_since(t).as_secs() >= 8 * 60);
            if stale && !board.key.is_empty() && node_dht_watch(board.key.clone()).unwrap_or(false) {
                plan_set_watched(&slot, Some(now));
            }
        }
        got
    }

    /// Looking at a group: its board is read every lap while the page is open.
    pub fn touch_group(&self, id_hex: &str) {
        plan_touch(&format!("g:{id_hex}"));
    }

    /// Records the network rang for that are boards: due now.
    pub(crate) fn mark_boards_changed(&self, keys: &[String]) -> usize {
        let mut n = 0;
        for g in self.groups() {
            if let Some(b) = &g.board {
                if !b.key.is_empty() && keys.iter().any(|k| k == &b.key) {
                    self.touch_group(&g.id_hex);
                    n += 1;
                }
            }
        }
        n
    }

    fn retries(&self) -> Vec<Retry> {
        self.store(STORE).get("retry").unwrap_or_default()
    }

    fn queue_retry(&self, r: Retry) {
        let _l = GROUPS.lock().unwrap_or_else(|e| e.into_inner());
        let mut arr = self.retries();
        arr.push(r);
        if arr.len() > MAX_QUEUED {
            let dropped = arr.len() - MAX_QUEUED;
            arr.drain(0..dropped);
            log::warn(TAG, format!("retry queue full — {dropped} undelivered copy(s) dropped"));
        }
        let _ = self.store(STORE).put("retry", &arr);
    }

    /// A few queued copies per turn, round robin.
    pub fn retry_group_outbox(&self) {
        let arr = self.retries();
        let n = arr.len();
        if n == 0 {
            return;
        }
        let mut landed: Vec<Retry> = Vec::new();
        let start = {
            let mut c = RETRY_CURSOR.lock().unwrap_or_else(|e| e.into_inner());
            if *c >= n {
                *c = 0;
            }
            *c
        };
        let mut at = start;
        for _ in 0..RETRIES_PER_PASS.min(n) {
            let o = arr[at % n].clone();
            at += 1;
            let Some(c) = self.contact(&o.m) else { continue };
            let ok = if o.roster {
                let Some(g) = self.group(&o.g) else { continue };
                App::roster_payload(&g)
                    .and_then(|payload| {
                        self.send(
                            &c,
                            Outgoing {
                                body: format!("group: {}", g.name),
                                kind: 12,
                                payload: Some(payload),
                                group: Some(GroupSend { id: hex_to_bytes(&g.id_hex), seq: Some(o.s), re_sender: None, re_seq: None }),
                                ..Default::default()
                            },
                        )
                    })
                    .is_ok()
            } else {
                self.send(
                    &c,
                    Outgoing {
                        body: o.b.clone(),
                        kind: o.k,
                        group: Some(GroupSend { id: hex_to_bytes(&o.g), seq: Some(o.s), re_sender: o.rs.as_deref().and_then(hex_to_bytes), re_seq: o.rq }),
                        ..Default::default()
                    },
                )
                .is_ok()
            };
            if ok {
                log::info(TAG, format!("group retry landed for {}…", &o.m[..8.min(o.m.len())]));
                landed.push(o);
            }
        }
        *RETRY_CURSOR.lock().unwrap_or_else(|e| e.into_inner()) = at % n;
        if landed.is_empty() {
            return;
        }
        let _l = GROUPS.lock().unwrap_or_else(|e| e.into_inner());
        let keep: Vec<Retry> = self.retries().into_iter().filter(|o| !landed.iter().any(|l| l.same(o))).collect();
        let _ = self.store(STORE).put("retry", &keep);
    }

    /// The group as one conversation: every member's copies merged, each
    /// (sender, counter) once, in time order.
    /// An emoji on a member's message: kind 4 naming (sender, counter).
    pub fn react_in_group(&self, id_hex: &str, target_sender_hex: &str, target_seq: u64, emoji: &str) -> Result<bool, Error> {
        let e: String = emoji.trim().chars().take(8).collect();
        self.send_group(id_hex, &e, 4, Some(target_sender_hex), Some(target_seq))
    }

    /// Taking back words of ours in a group: a kind 5 naming our own
    /// (sender, counter). Only the author's withdrawal is honoured anywhere,
    /// so this is the one place the button exists.
    pub fn unsend_in_group(&self, id_hex: &str, seq: u64) -> Result<bool, Error> {
        let g = self.group(id_hex).ok_or_else(|| Error::Refused("no such group".into()))?;
        let mine = self.mine_in(&g.members);
        self.send_group(id_hex, "This message was withdrawn.", 5, Some(&mine), Some(seq))
    }

    pub fn group_thread(&self, id_hex: &str) -> Vec<GroupRow> {
        let Some(g) = self.group(id_hex) else { return Vec::new() };
        let mine = self.mine_in(&g.members);
        let mut seen: HashSet<(String, u64)> = HashSet::new();
        let mut out = Vec::new();
        // Pairwise copies of my own words live in each member's thread;
        // on a board they live once, under my own key, so that thread is
        // read too (the dedupe makes reading both harmless).
        for m in g.members.iter() {
            for msg in self.thread(m) {
                if msg.group_id.as_deref() != Some(&g.id_hex) || msg.kind == 12 {
                    continue;
                }
                let sender = if msg.outgoing { mine.clone() } else { m.clone() };
                if !seen.insert((sender.clone(), msg.group_seq)) {
                    continue;
                }
                out.push(GroupRow { sender_hex: sender, message: msg });
            }
        }
        out.sort_by(|a, b| a.message.timestamp.cmp(&b.message.timestamp).then_with(|| a.message.group_seq.cmp(&b.message.group_seq)));
        out
    }

    pub fn mark_group_disclosed(&self, id_hex: &str) -> Result<(), Error> {
        if let Some(g) = self.group(id_hex).filter(|g| !g.disclosed) {
            self.upsert_group(Group { disclosed: true, ..g })?;
        }
        Ok(())
    }

    pub fn look_at(&self, rows: &[GroupRow]) -> Look {
        let ours = self.persona_hexes();
        let mut look = Look::default();
        for r in rows.iter().filter(|r| !ours.contains(&r.sender_hex)) {
            let h = look.high.entry(r.sender_hex.clone()).or_insert(0);
            *h = (*h).max(r.message.group_seq);
            *look.rows.entry(r.sender_hex.clone()).or_insert(0) += 1;
        }
        look
    }

    pub fn group_seen(&self, id_hex: &str) -> Look {
        Look {
            high: self.store(STORE).get(&format!("seen_{id_hex}")).unwrap_or_default(),
            rows: self.store(STORE).get(&format!("rows_{id_hex}")).unwrap_or_default(),
        }
    }

    pub fn group_unread(seen: &Look, now: &Look) -> bool {
        now.high.iter().any(|(m, s)| *s > seen.high.get(m).copied().unwrap_or(0)) || now.rows.iter().any(|(m, n)| seen.rows.get(m).map_or(false, |k| *n > *k))
    }

    pub fn mark_group_seen(&self, id_hex: &str, now: &Look) -> Result<(), Error> {
        let seen = self.group_seen(id_hex);
        let mut merged = seen.high.clone();
        for (m, s) in &now.high {
            let e = merged.entry(m.clone()).or_insert(0);
            *e = (*e).max(*s);
        }
        if merged == seen.high && now.rows == seen.rows {
            return Ok(());
        }
        self.store(STORE).update(|t| {
            t.insert(format!("seen_{id_hex}"), serde_json::to_value(&merged).unwrap_or_default());
            t.insert(format!("rows_{id_hex}"), serde_json::to_value(&now.rows).unwrap_or_default());
        })?;
        bump();
        Ok(())
    }

    pub fn unread_groups(&self) -> usize {
        self.groups().into_iter().filter(|g| {
            let rows = self.group_thread(&g.id_hex);
            App::group_unread(&self.group_seen(&g.id_hex), &self.look_at(&rows))
        }).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_group_merges_its_members_threads_once_each() {
        let dir = std::env::temp_dir().join(format!("ducat-groups-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let app = App::open(&dir).unwrap();
        let me = app.primary_hex().unwrap();
        let g = Group { id_hex: "aa".repeat(16), name: "Crew".into(), members: vec!["p1".into(), "p2".into(), me.clone()], my_group_seq: 0, disclosed: false, board: None };
        app.upsert_group(g.clone()).unwrap();
        let mk = |out: bool, seq: u64, gseq: u64, ts: u64| StoredMessage { outgoing: out, seq, body: format!("m{gseq}"), timestamp: ts, group_id: Some(g.id_hex.clone()), group_seq: gseq, ..Default::default() };
        // My copy to p1 and to p2 carry the same group counter: one row.
        app.store(crate::contacts::CONTACTS).put("thread_p1", &vec![mk(true, 0, 1, 10), mk(false, 0, 1, 11)]).unwrap();
        app.store(crate::contacts::CONTACTS).put("thread_p2", &vec![mk(true, 0, 1, 10), mk(false, 0, 1, 12)]).unwrap();
        let rows = app.group_thread(&g.id_hex);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows.iter().filter(|r| r.sender_hex == me).count(), 1);
        let look = app.look_at(&rows);
        assert_eq!(look.high.get("p1"), Some(&1));
        assert!(App::group_unread(&app.group_seen(&g.id_hex), &look));
        app.mark_group_seen(&g.id_hex, &look).unwrap();
        assert!(!App::group_unread(&app.group_seen(&g.id_hex), &look));
        assert_eq!(app.unread_groups(), 0);
        // A roster from a non-member is ignored; from a member it grows.
        let payload = group_roster_encode("Crew".into(), vec![vec![1; 32], vec![2; 32]], None).unwrap();
        app.absorb_roster("zz", Some(&hex_to_bytes(&g.id_hex).unwrap()), Some(&payload));
        assert_eq!(app.group(&g.id_hex).unwrap().members.len(), 3);
    }
}

/// What decorates a group's words: the latest emoji each member gave a
/// message, and the withdrawals honoured — only from the message's own
/// author, since anyone can send a kind 5 naming anything, and honouring a
/// stranger's would let any member blank any other's words.
#[derive(Default, Debug)]
pub struct GroupMarks {
    /// (author, counter) → (reactor, emoji), one per reactor.
    pub reactions: HashMap<(String, u64), Vec<(String, String)>>,
    pub unsent: HashSet<(String, u64)>,
}

pub fn group_marks(rows: &[GroupRow]) -> GroupMarks {
    let mut out = GroupMarks::default();
    let mut latest: HashMap<((String, u64), String), (u64, String)> = HashMap::new();
    for r in rows {
        let m = &r.message;
        let (Some(rs), Some(rq)) = (m.group_re_sender.clone(), m.group_re_seq) else { continue };
        match m.kind {
            4 => {
                let e = latest.entry(((rs, rq), r.sender_hex.clone())).or_insert((0, String::new()));
                if m.timestamp >= e.0 {
                    *e = (m.timestamp, m.body.clone());
                }
            }
            5 if rs == r.sender_hex => {
                out.unsent.insert((rs, rq));
            }
            _ => {}
        }
    }
    let mut keys: Vec<_> = latest.into_iter().collect();
    keys.sort_by_key(|(_, (t, _))| *t);
    for ((target, reactor), (_, emoji)) in keys {
        if !emoji.is_empty() {
            out.reactions.entry(target).or_default().push((reactor, emoji));
        }
    }
    out
}

/// The quote a group reply shows — the reader's own copy of the target,
/// found by (author, counter), never bytes from the wire.
pub fn group_reply_line(rows: &[GroupRow], m: &StoredMessage, marks: &GroupMarks) -> Option<String> {
    let rs = m.group_re_sender.as_ref()?;
    let rq = m.group_re_seq?;
    let t = rows.iter().find(|r| &r.sender_hex == rs && r.message.group_seq == rq);
    Some(match t {
        None => "a message that is no longer here".to_string(),
        Some(_) if marks.unsent.contains(&(rs.clone(), rq)) => "This message was withdrawn.".to_string(),
        Some(t) if !t.message.body.trim().is_empty() => t.message.body.clone(),
        Some(_) => "a message".to_string(),
    })
}
