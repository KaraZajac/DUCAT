//! Groups (§16.19): a name, a roster, and no server — every message is
//! written into each member's pairwise thread with the group's id and
//! the sender's own counter there, and a member's view is the merge of
//! those threads. The phone's `Groups.kt`.
//!
//! The mesh must be complete to speak: a member this desk has no thread
//! with cannot be written to, and a group message that reaches some
//! members and not others is a conversation that did not happen.

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

use ducat_mobile::contacts::{group_roster_decode, group_roster_encode, GroupSend};
use serde::{Deserialize, Serialize};

use crate::contacts::{bump, hex, hex_to_bytes, StoredMessage};
use crate::mailbox::Outgoing;
use crate::{log, App, Error};

const TAG: &str = "Groups";
const STORE: &str = "ducat_groups";
const RETRIES_PER_PASS: usize = 8;
const MAX_QUEUED: usize = 200;

static GROUPS: Mutex<()> = Mutex::new(());
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
}

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
        let g = Group { id_hex: id, name: ducat_mobile::contacts::clean_display_text(name.trim().to_string()), members: members.clone(), my_group_seq: 0, disclosed: false };
        self.upsert_group(g.clone())?;
        self.send_roster(&g);
        log::info(TAG, format!("created {} with {} member(s)", g.name, members.len()));
        Ok(g)
    }

    pub fn add_to_group(&self, id_hex: &str, persona_hex: &str) -> Result<(), Error> {
        let Some(g) = self.group(id_hex) else { return Ok(()) };
        if g.members.iter().any(|m| m == persona_hex) {
            return Ok(());
        }
        let mut grown = g.clone();
        grown.members.push(persona_hex.to_string());
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
        match self.group(&id_hex) {
            None => {
                let ours = self.persona_hexes();
                if !members.iter().any(|m| ours.contains(m)) {
                    log::warn(TAG, "roster for a group we are not in — ignored");
                    return;
                }
                let _ = self.upsert_group(Group { id_hex, name: roster.name.clone(), members: members.clone(), my_group_seq: 0, disclosed: false });
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
                if merged.len() != known.members.len() {
                    let n = merged.len();
                    let _ = self.upsert_group(Group { members: merged, ..known.clone() });
                    log::info(TAG, format!("{}: roster grew to {n}", known.name));
                }
            }
        }
    }

    fn roster_payload(g: &Group) -> Result<Vec<u8>, Error> {
        let members: Option<Vec<Vec<u8>>> = g.members.iter().map(|m| hex_to_bytes(m)).collect();
        Ok(group_roster_encode(g.name.clone(), members.ok_or_else(|| Error::Refused("a member key is not hex".into()))?)?)
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
    pub fn group_thread(&self, id_hex: &str) -> Vec<GroupRow> {
        let Some(g) = self.group(id_hex) else { return Vec::new() };
        let mine = self.mine_in(&g.members);
        let mut seen: HashSet<(String, u64)> = HashSet::new();
        let mut out = Vec::new();
        for m in g.members.iter().filter(|m| **m != mine) {
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
        let g = Group { id_hex: "aa".repeat(16), name: "Crew".into(), members: vec!["p1".into(), "p2".into(), me.clone()], my_group_seq: 0, disclosed: false };
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
        let payload = group_roster_encode("Crew".into(), vec![vec![1; 32], vec![2; 32]]).unwrap();
        app.absorb_roster("zz", Some(&hex_to_bytes(&g.id_hex).unwrap()), Some(&payload));
        assert_eq!(app.group(&g.id_hex).unwrap().members.len(), 3);
    }
}
