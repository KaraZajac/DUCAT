//! The mailbox: cards out and in, messages out and in — the phone's
//! `Mailbox.kt`, line for line where the line was earned.
//!
//! Every rule here was paid for on a phone, and the comments in the Kotlin
//! say by what; this file keeps the rules and points at the reason in a
//! sentence. Two things to hold on to:
//!
//! - **Everything local lands before anything remote.** A published slot
//!   with the counter lost to a crash reuses the seq with different bytes
//!   — a fork every reader keeps for ever. A persisted counter with the
//!   slot unwritten is only a late slot, replayed on the next send.
//! - **A poll must not block** (§16.11). A slot that cannot be read is a
//!   dead letter in the thread, a patience window, or a wait for the
//!   network — never an exception that walls off everything behind it.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use ducat_mobile::contacts::{
    build_contact_details, build_log_head, create_contact_card, generate_prekeys, generate_writer_keys,
    log_still_readable, log_subkey, open_message, parse_contact_details, parse_log_head, prune_prekey,
    read_contact_card, seal_message, sealed_prekey_id, thread_aad, AttachmentRef, BillLine, CallSend, GroupSend,
    OpenedMessage, PositionSend, PublicationSend, SealIn,
};
use ducat_mobile::node::{
    node_dht_create, node_dht_create_shared, node_dht_get, node_dht_get_versioned, node_dht_open, node_dht_set,
    DhtRecord,
};

use crate::contacts::{bump, fold_card_address, hex, hex_to_bytes, now_ms, BillItem, Contact, IssuedCard, StoredMessage};
use crate::{log, App, CardProblem, Error};

const TAG: &str = "Mailbox";

/// A legacy log's size; the ring a head claims when it says nothing.
pub const LOG_SUBKEYS: u32 = 8;
/// The ring every new log is minted with — and its head says so.
pub const NEW_RING: u32 = 32;
const ONE_TIME_KEYS: u32 = 32;
const ONE_TIME_VALID_SECS: u64 = 60 * 60 * 24 * 30;
/// How long an unreadable slot is waited on before it is declared lost.
const STUCK_PATIENCE_MS: u64 = 10 * 60 * 1000;
const SLOT_FIX_GIVE_UP: u32 = 3;
const LATE_SLOT_GIVE_UP: u32 = 5;
/// How many outstanding cards one sweep looks at, in turns.
const CLAIMS_PER_PASS: usize = 8;
/// Consecutive sweeps a card's record must be missing before it is
/// retired — one miss is a network that could not find a holder this
/// minute.
const MISSES_BEFORE_RETIRING: u32 = 5;

/// A card just cut: its URI, and the inbox that is its identity — a flow
/// showing this code waits for *this card's* claimant.
#[derive(Clone, Debug, serde::Serialize)]
pub struct IssuedHandle {
    pub uri: String,
    pub inbox_key: String,
}

/// What claiming a card produced: a new thread, or the one this desk
/// already had with that person (the card had been claimed here before).
#[derive(Clone, Debug)]
pub enum Claim {
    New(Contact),
    Known(Contact),
}

impl Claim {
    pub fn contact(self) -> Contact {
        match self {
            Claim::New(c) | Claim::Known(c) => c,
        }
    }
}

/// Everything a message can carry on the way out. `Default` is a plain
/// text message; the rest are §16's kinds and their riders, refused by
/// core when a half of a pair is missing.
#[derive(Clone, Default)]
pub struct Outgoing {
    pub body: String,
    pub kind: u32,
    pub amount_pxmr: Option<u64>,
    pub payto: Option<String>,
    pub txid_hex: Option<String>,
    pub items: Vec<BillItem>,
    pub tax_pxmr: Option<u64>,
    pub re_seq: Option<u64>,
    pub re_own: bool,
    pub attachment: Option<AttachmentRef>,
    pub oob: bool,
    pub eta_secs: Option<u64>,
    pub payload: Option<Vec<u8>>,
    pub round: Option<u64>,
    pub ceremony_id: Option<Vec<u8>>,
    pub position: Option<PositionSend>,
    pub group: Option<GroupSend>,
    pub publication: Option<PublicationSend>,
    pub call: Option<CallSend>,
}

impl Outgoing {
    pub fn text(body: &str) -> Outgoing {
        Outgoing { body: body.to_string(), ..Default::default() }
    }
}

// ----- process-wide state ------------------------------------------------------

static SEND_LOCKS: Mutex<Option<HashMap<String, Arc<Mutex<()>>>>> = Mutex::new(None);
static POLL_LOCKS: Mutex<Option<HashMap<String, Arc<Mutex<()>>>>> = Mutex::new(None);
static CLAIMING: AtomicBool = AtomicBool::new(false);
static LAST_POLL_OFFLINE: AtomicBool = AtomicBool::new(false);
static CLAIM_CURSOR: Mutex<usize> = Mutex::new(0);
static MISSING_CARD: Mutex<Option<HashMap<String, u32>>> = Mutex::new(None);

/// One lock per contact per direction: two sends in flight read the same
/// `out_seq` and write the same slot twice; two polls re-read what the
/// first just advanced past.
fn lock_for(table: &Mutex<Option<HashMap<String, Arc<Mutex<()>>>>>, key: &str) -> Arc<Mutex<()>> {
    let mut g = table.lock().unwrap_or_else(|e| e.into_inner());
    g.get_or_insert_with(HashMap::new).entry(key.to_string()).or_insert_with(|| Arc::new(Mutex::new(()))).clone()
}

/// Veilid's TryAgain surfacing through the bridge as message text: the
/// node is not up yet, not a broken card and not a refused write.
pub fn is_offline(e: &Error) -> bool {
    e.to_string().to_lowercase().contains("tryagain")
}

pub fn is_missing(e: &Error) -> bool {
    e.to_string().to_lowercase().contains("key not found")
}

/// A card's one reply slot after exactly one honest write: an honest
/// claimant reads first and refuses a card that already holds a reply, so
/// anything past seq 0 is somebody's second answer.
fn claimed_once(seq: Option<u32>) -> bool {
    matches!(seq, None | Some(0))
}

fn kind_name(kind: u32) -> &'static str {
    match kind {
        0 => "message",
        1 => "bill",
        2 => "payment note",
        3 => "receipt",
        4 => "reaction",
        5 => "retraction",
        6 => "ride offer",
        7 => "ride accept",
        8 | 9 => "ceremony round",
        10 => "ceremony abort",
        11 => "position",
        12 => "group roster",
        13 => "publication key",
        16 => "publication ask",
        _ => "message of another kind",
    }
}

/// Equality only — the phone's `contentHashCode`, for "the same bytes are
/// still in this slot".
fn content_hash(b: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for x in b {
        h ^= *x as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

fn short(key: &str) -> String {
    key.chars().take(24).collect::<String>() + "…"
}

fn zip_secrets(ids: &[u32], secrets: &[Vec<u8>]) -> Vec<(u32, Vec<u8>)> {
    ids.iter().copied().zip(secrets.iter().cloned()).collect()
}

impl App {
    // ----- bookkeeping in the contacts table --------------------------------------

    /// When this seq first sat there unreadable; started now if it has not.
    fn stuck_since(&self, key: &str) -> u64 {
        let store = self.store(crate::contacts::CONTACTS);
        let k = format!("stuck_{key}");
        if let Some(t) = store.get::<u64>(&k).filter(|t| *t > 0) {
            return t;
        }
        let now = now_ms();
        let _ = store.put(&k, &now);
        now
    }

    fn clear_stuck(&self, key: &str) {
        let _ = self.store(crate::contacts::CONTACTS).remove(&format!("stuck_{key}"));
    }

    fn slot_seen(&self, key: &str) -> Option<u64> {
        self.store(crate::contacts::CONTACTS).get(&format!("slotseen_{key}"))
    }

    fn slot_seen_seq(&self, key: &str) -> Option<u64> {
        self.store(crate::contacts::CONTACTS).get(&format!("slotseenq_{key}"))
    }

    fn record_slot_seen(&self, key: &str, hash: u64, seq: u64) {
        let _ = self.store(crate::contacts::CONTACTS).update(|m| {
            m.insert(format!("slotseen_{key}"), serde_json::Value::from(hash));
            m.insert(format!("slotseenq_{key}"), serde_json::Value::from(seq));
        });
    }

    fn dead_letter_time(&self, persona_hex: &str) -> u64 {
        self.thread(persona_hex).last().map(|m| m.timestamp).unwrap_or_else(App::now)
    }

    // ----- cards -----------------------------------------------------------------------

    /// Cut a card: a shared inbox the claimant writes its reply into, a
    /// fresh outbox of ours, a one-time batch, and our details in subkey 0
    /// — written now, so a claimant later needs nothing from us but the
    /// record. `purpose` is "profile" for the standing code, "sale" for a
    /// till's handshake, "publish" for scan-to-subscribe.
    pub fn issue_card(
        &self,
        display_name: Option<&str>,
        valid_secs: u64,
        purpose: &str,
        as_persona: Option<&str>,
    ) -> Result<IssuedHandle, Error> {
        let owner_hex = match as_persona {
            Some(h) => h.to_string(),
            None => self.worn()?,
        };
        let persona = match self.persona_secret(&owner_hex)? {
            Some(s) => s,
            None => self.primary_secret()?,
        };
        let display_name = display_name.map(str::trim).filter(|n| !n.is_empty()).map(String::from);
        let writer = generate_writer_keys();
        let inbox = node_dht_create_shared(writer.public.clone())?;
        let outbox = self.create_log()?;
        // Fresh one-time ids from the device-wide counter, and the signed
        // prekey *reused*: rotating it as a side effect of making a card
        // is how messages sealed to the old one arrived unreadable.
        let prekeys = generate_prekeys(
            ONE_TIME_KEYS,
            ONE_TIME_VALID_SECS,
            self.next_prekey_start(ONE_TIME_KEYS)?,
            self.signed_prekey_secret(),
        );
        self.save_prekeys(
            &prekeys.bundle,
            &prekeys.signed_secret,
            &zip_secrets(&prekeys.one_time_ids, &prekeys.one_time_secrets),
            false,
        )?;
        node_dht_set(
            inbox.key.clone(),
            0,
            build_contact_details(
                persona.clone(),
                outbox.key.clone(),
                prekeys.bundle.clone(),
                display_name.clone(),
                // §16.12 makes publishing an address a choice; the desk has
                // no wallet yet, so the choice is not offered.
                None,
                // §16.9: the profile rides the record, scoped to the purpose.
                self.profile_wire(&owner_hex, Some(purpose), false),
                Some(purpose.to_string()),
            )?,
        )?;
        let card = create_contact_card(
            persona,
            inbox.key.clone(),
            writer.public.clone(),
            display_name,
            writer.secret.clone(),
            valid_secs,
        )?;
        self.save_issued_card(IssuedCard {
            inbox_key: inbox.key.clone(),
            writer_public: writer.public,
            writer_secret: writer.secret,
            outbox_key: outbox.key,
            outbox_owner_public: outbox.owner_public,
            outbox_owner_secret: outbox.owner_secret,
            uri: card.uri.clone(),
            purpose: purpose.to_string(),
            owner: owner_hex,
            made: now_ms(),
            ttl: valid_secs,
            answered_by: None,
        })?;
        log::info(TAG, format!("issued card ({purpose}): inbox={} outbox={}", short(&inbox.key), short(&self_outbox_of(&card.uri))));
        Ok(IssuedHandle { uri: card.uri, inbox_key: inbox.key })
    }

    /// A fresh append-only log, as big as the ring its head advertises,
    /// with the bundle riding the head from birth — a head without keys
    /// strands a counterparty who claimed the card and wants to speak
    /// first.
    fn create_log(&self) -> Result<DhtRecord, Error> {
        let rec = node_dht_create(NEW_RING)?;
        let bundle = self.topup_if_low(&rec.key)?;
        node_dht_set(rec.key.clone(), 0, build_log_head(0, bundle, None, Some(NEW_RING)))?;
        Ok(rec)
    }

    /// Accept someone's card: read their details, publish ours in the
    /// reply subkey, and keep both. `as_driver` only when accepting a
    /// hail — the one claim where the car belongs in what we publish.
    pub fn claim_card(
        &self,
        uri: &str,
        petname: Option<&str>,
        as_driver: bool,
        as_persona: Option<&str>,
    ) -> Result<Claim, Error> {
        let scanned = read_contact_card(uri.trim().to_string())?;
        if scanned.expired {
            return Err(Error::Card(CardProblem::Expired));
        }
        let inbox = scanned.inbox_key.clone();
        node_dht_open(inbox.clone(), Some(scanned.writer_public.clone()), Some(scanned.writer_secret.clone()))?;
        // Single use, checked by reading rather than trusting a local flag:
        // the inbox has exactly one reply subkey.
        if let Some(already) = node_dht_get(inbox.clone(), 1, true)?.filter(|a| !a.is_empty()) {
            // Whose reply? If it is this desk's, the card was claimed here
            // and the thread it opened still exists — the right answer is
            // that thread, not "somebody got there first".
            let mine = parse_contact_details(already).ok().filter(|d| self.persona_hexes().contains(&hex(&d.persona)));
            if let Some(mine) = mine {
                let known = self.contacts().into_iter().find(|c| c.my_outbox == mine.outbox_key).or_else(|| {
                    node_dht_get(inbox.clone(), 0, true)
                        .ok()
                        .flatten()
                        .and_then(|raw| parse_contact_details(raw).ok())
                        .and_then(|theirs| self.contact(&hex(&theirs.persona)))
                });
                if let Some(k) = known {
                    return Ok(Claim::Known(k));
                }
            }
            return Err(Error::Card(CardProblem::AlreadyUsed));
        }
        let raw = node_dht_get(inbox.clone(), 0, true)?
            .filter(|r| !r.is_empty())
            .ok_or(Error::Card(CardProblem::NotPublished))?;
        let theirs = parse_contact_details(raw)?;
        let theirs_hex = hex(&theirs.persona);
        // **Not yourself.** Your own listings are on the board you read,
        // and claiming your own card burns its one reply slot.
        if self.persona_hexes().contains(&theirs_hex) {
            return Err(Error::Card(CardProblem::Own));
        }
        // The answering persona, settled BEFORE the reply seals: a prior
        // relationship's owner wins over the worn hat.
        let prior = self.contact(&theirs_hex);
        let owner_hex = match (&prior, as_persona) {
            (Some(p), _) => self.owner_hex_of(p),
            (None, Some(h)) => h.to_string(),
            (None, None) => self.worn()?,
        };
        let persona = match self.persona_secret(&owner_hex)? {
            Some(s) => s,
            None => self.primary_secret()?,
        };
        let outbox = self.create_log()?;
        let prekeys = generate_prekeys(
            ONE_TIME_KEYS,
            ONE_TIME_VALID_SECS,
            self.next_prekey_start(ONE_TIME_KEYS)?,
            self.signed_prekey_secret(),
        );
        self.save_prekeys(
            &prekeys.bundle,
            &prekeys.signed_secret,
            &zip_secrets(&prekeys.one_time_ids, &prekeys.one_time_secrets),
            false,
        )?;
        node_dht_set(
            inbox.clone(),
            1,
            build_contact_details(
                persona,
                outbox.key.clone(),
                prekeys.bundle.clone(),
                // **Our** name — what the reply asserts about its sender —
                // never the petname we just chose for them.
                self.my_name(Some(&owner_hex))?,
                None,
                // Scoped to what the issuer said the handshake is for
                // (§16.9); a null purpose — an older card — is not a
                // contact exchange, the private default.
                self.profile_wire(&owner_hex, theirs.purpose.as_deref(), as_driver),
                theirs.purpose.clone(),
            )?,
        )?;
        // What this thread already had, if we have met before: their log is
        // only new if the card names a different one. Ours is new by
        // construction.
        let same_log = prior.as_ref().map_or(false, |p| p.their_outbox == theirs.outbox_key);
        let (payto, held) = fold_card_address(prior.as_ref(), theirs.payto.as_deref());
        let petname = petname
            .map(|p| ducat_mobile::contacts::clean_display_text(p.to_string()))
            .filter(|p| !p.trim().is_empty())
            .or_else(|| prior.as_ref().and_then(|p| p.petname.clone()));
        let built = Contact {
            persona_hex: theirs_hex.clone(),
            petname,
            asserted_name: theirs.asserted_name.clone(),
            in_seq: if same_log { prior.as_ref().map_or(0, |p| p.in_seq) } else { 0 },
            in_prev_link: if same_log { prior.as_ref().and_then(|p| p.in_prev_link.clone()) } else { None },
            my_outbox: outbox.key,
            my_outbox_owner_public: outbox.owner_public,
            my_outbox_owner_secret: outbox.owner_secret,
            their_outbox: theirs.outbox_key.clone(),
            their_bundle: Some(theirs.prekey_bundle.clone()),
            their_address: payto,
            pending_address: held.clone(),
            avatar: theirs.profile.avatar.clone(),
            email: theirs.profile.email.clone(),
            phone: theirs.profile.phone.clone(),
            signal: theirs.profile.signal.clone(),
            pronouns: theirs.profile.pronouns,
            my_ring: NEW_RING,
            car_model: theirs.profile.car_model.clone(),
            car_color: theirs.profile.car_color.clone(),
            plate: theirs.profile.plate.clone(),
            their_read_up_to: None,
            // What this card said it was for — a thread born from a
            // `donate` card is the thread whose unprompted payments are
            // donations. The other direction's memory rides through.
            card_purpose: theirs.purpose.clone().or_else(|| prior.as_ref().and_then(|p| p.card_purpose.clone())),
            my_card_purpose: prior.as_ref().and_then(|p| p.my_card_purpose.clone()),
            my_card_purpose_at: prior.as_ref().map_or(0, |p| p.my_card_purpose_at),
            out_seq: 0,
            out_prev_link: None,
            chat_visible: true,
            owner: owner_hex,
        };
        let c = self.merge_rebuilt(built)?;
        if held.is_some() && held != prior.as_ref().and_then(|p| p.pending_address.clone()) {
            self.warn_address_held(&c);
        }
        log::info(TAG, format!("claimed: their outbox={}", short(&theirs.outbox_key)));
        Ok(Claim::New(c))
    }

    /// A rebuilt contact into the store: counters kept where the logs are
    /// the same, and the outbound bookkeeping dropped where ours is not —
    /// the slot-insurance watermark is a seq in the *old* log's numbering,
    /// a pending slot is the old chain's bytes, and their read watermark
    /// is frozen onto the rows it already covered before the new record,
    /// which carries none, replaces it.
    fn merge_rebuilt(&self, built: Contact) -> Result<Contact, Error> {
        let mut ours_retired: Option<Contact> = None;
        let mut theirs_retired = false;
        let hex = built.persona_hex.clone();
        let c = self.merge_contact(&hex, |cur| {
            ours_retired = cur.filter(|k| k.my_outbox != built.my_outbox).cloned();
            theirs_retired = cur.map_or(false, |k| k.their_outbox != built.their_outbox);
            keep_counters(built, cur)
        })?;
        if let Some(old) = ours_retired {
            self.retire_outbox(&hex, old.their_read_up_to)?;
            self.set_last_slot_verified(&hex, -1)?;
            self.set_slot_fix_tries(&hex, 0)?;
            self.clear_pending_slot(&hex)?;
        }
        if theirs_retired {
            self.clear_stuck_clocks(&hex)?;
        }
        Ok(c)
    }

    /// Somebody handed us a card that wants to be paid somewhere new. The
    /// single field where being wrong costs money, so it is said out loud.
    fn warn_address_held(&self, c: &Contact) {
        log::warn(TAG, format!("{}… card wants a different payment address — holding it for you", &c.persona_hex[..12.min(c.persona_hex.len())]));
        bump();
    }

    /// Sweep the outstanding cards for answers. Returns how many were
    /// collected; at most one sweep runs at a time.
    pub fn collect_claims(&self, only: Option<&str>) -> usize {
        if CLAIMING.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire).is_err() {
            return 0;
        }
        let n = self.collect_claims_locked(only);
        CLAIMING.store(false, Ordering::Release);
        n
    }

    fn collect_claims_locked(&self, only: Option<&str>) -> usize {
        let mut collected = 0;
        // Every outstanding card, not "the" card: the registry is what lets
        // a till's handshake and the profile code be outstanding at once.
        let outstanding: Vec<IssuedCard> = self.issued_cards().into_iter().filter(|c| c.answered_by.is_none()).collect();
        let looking: Vec<IssuedCard> = match only {
            Some(k) => outstanding.into_iter().filter(|c| c.inbox_key == k).collect(),
            None if outstanding.len() <= CLAIMS_PER_PASS => outstanding,
            None => {
                // In turns, from where the last sweep stopped.
                let mut cursor = CLAIM_CURSOR.lock().unwrap_or_else(|e| e.into_inner());
                if *cursor >= outstanding.len() {
                    *cursor = 0;
                }
                let take: Vec<IssuedCard> = (0..CLAIMS_PER_PASS).map(|i| outstanding[(*cursor + i) % outstanding.len()].clone()).collect();
                *cursor = (*cursor + CLAIMS_PER_PASS) % outstanding.len();
                take
            }
        };
        for issued in looking {
            match self.collect_one(&issued) {
                Ok(true) => collected += 1,
                Ok(false) => {}
                Err(e) if is_offline(&e) => {
                    // One line, not one per card: offline fails them all alike.
                    log::info(TAG, "offline — claims wait for the network");
                    break;
                }
                Err(e) if is_missing(&e) => {
                    // **A card whose record the network has lost is dead.**
                    // There is no local expiry stamp to read; "Key not
                    // found" is the only news we get, and it takes several
                    // in a row — one miss is a network that could not find
                    // a holder this minute.
                    let n = {
                        let mut g = MISSING_CARD.lock().unwrap_or_else(|e| e.into_inner());
                        let map = g.get_or_insert_with(HashMap::new);
                        let n = map.get(&issued.inbox_key).copied().unwrap_or(0) + 1;
                        if n >= MISSES_BEFORE_RETIRING {
                            map.remove(&issued.inbox_key);
                        } else {
                            map.insert(issued.inbox_key.clone(), n);
                        }
                        n
                    };
                    if n >= MISSES_BEFORE_RETIRING {
                        let _ = self.forget_issued_card(&issued.inbox_key);
                        log::info(TAG, "retired an expired code — the network no longer holds it");
                        if issued.purpose == "profile" {
                            self.reissue_profile_code(&issued);
                        }
                    }
                }
                Err(e) => log::warn(TAG, format!("collect_claims({}…): {e}", &issued.inbox_key[..16.min(issued.inbox_key.len())])),
            }
        }
        collected
    }

    /// One card's reply slot. Ok(true) when a claimant was folded in.
    fn collect_one(&self, issued: &IssuedCard) -> Result<bool, Error> {
        node_dht_open(issued.inbox_key.clone(), None, None)?;
        {
            let mut g = MISSING_CARD.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(map) = g.as_mut() {
                map.remove(&issued.inbox_key);
            }
        }
        let Some(read) = node_dht_get_versioned(issued.inbox_key.clone(), 1, true)? else { return Ok(false) };
        if read.data.is_empty() {
            return Ok(false);
        }
        // **Claim-once, enforced by reading the sequence.** For a hail or
        // a listing the card URI — writer secret and all — is public board
        // text, so anyone can overwrite whoever answered and be adopted as
        // the counterparty, payment address included. Discarded rather than
        // resolved: there is no way to tell which writer was the person in
        // front of you, and a card nobody can trust is worse than none.
        if !claimed_once(read.seq) {
            log::warn(TAG, format!("card ({}) was answered more than once (seq {:?}) — discarding it unclaimed", issued.purpose, read.seq));
            self.forget_issued_card(&issued.inbox_key)?;
            return Ok(false);
        }
        let theirs = parse_contact_details(read.data)?;
        let persona_hex = hex(&theirs.persona);
        let prior = self.contact(&persona_hex);
        // Prior relationship keeps its persona; a new one belongs to
        // whichever persona cut the card that was answered.
        let owner_hex = prior
            .as_ref()
            .map(|p| p.owner.clone())
            .filter(|o| !o.is_empty())
            .or_else(|| Some(issued.owner.clone()).filter(|o| !o.is_empty()))
            .unwrap_or(self.primary_hex()?);
        let (payto, held) = fold_card_address(prior.as_ref(), theirs.payto.as_deref());
        let purpose_changed = !issued.purpose.is_empty() && prior.as_ref().and_then(|p| p.my_card_purpose.as_deref()) != Some(issued.purpose.as_str());
        let built = Contact {
            persona_hex: persona_hex.clone(),
            petname: prior.as_ref().and_then(|p| p.petname.clone()),
            asserted_name: theirs.asserted_name.clone(),
            my_outbox: issued.outbox_key.clone(),
            my_outbox_owner_public: issued.outbox_owner_public.clone(),
            my_outbox_owner_secret: issued.outbox_owner_secret.clone(),
            their_outbox: theirs.outbox_key.clone(),
            their_bundle: Some(theirs.prekey_bundle.clone()),
            their_address: payto,
            pending_address: held.clone(),
            avatar: theirs.profile.avatar.clone(),
            email: theirs.profile.email.clone(),
            phone: theirs.profile.phone.clone(),
            signal: theirs.profile.signal.clone(),
            pronouns: theirs.profile.pronouns,
            my_ring: NEW_RING,
            car_model: theirs.profile.car_model.clone(),
            car_color: theirs.profile.car_color.clone(),
            plate: theirs.profile.plate.clone(),
            their_read_up_to: None,
            // Two directions, two fields: what THEIR card said survives
            // from the prior record; what OUR card said goes in its own
            // field, with the moment it was established.
            card_purpose: prior.as_ref().and_then(|p| p.card_purpose.clone()),
            my_card_purpose: Some(issued.purpose.clone()).filter(|p| !p.is_empty()).or_else(|| prior.as_ref().and_then(|p| p.my_card_purpose.clone())),
            my_card_purpose_at: if purpose_changed { App::now() } else { prior.as_ref().map_or(0, |p| p.my_card_purpose_at) },
            out_seq: 0,
            out_prev_link: None,
            in_seq: 0,
            in_prev_link: None,
            chat_visible: true,
            owner: owner_hex,
        };
        self.merge_rebuilt(built)?;
        if issued.purpose == "publish" {
            // §16.20's scan-to-subscribe: enroll, and let the publication
            // decide whether the newcomer gets the latest issue or a bill.
            if let Err(e) = self.enroll_from_card(&issued.inbox_key, &persona_hex) {
                log::warn(TAG, format!("enroll: {e}"));
            }
        }
        if held.is_some() && held != prior.as_ref().and_then(|p| p.pending_address.clone()) {
            if let Some(c) = self.contact(&persona_hex) {
                self.warn_address_held(&c);
            }
        }
        self.mark_card_answered(&issued.inbox_key, &persona_hex)?;
        if issued.purpose == "sale" {
            self.on_sale_claimed(&issued.inbox_key, &persona_hex);
        }
        log::info(TAG, format!("card ({}) answered by {}", issued.purpose, theirs.asserted_name.clone().unwrap_or_else(|| "an unnamed contact".into())));
        if issued.purpose == "profile" {
            crate::notify::post("New contact", format!("{} answered your code", theirs.asserted_name.clone().unwrap_or_else(|| "Somebody".into())), Some(persona_hex.clone()));
        }
        // Only the standing profile code replaces itself — a sale's
        // handshake was for that sale.
        if issued.purpose == "profile" {
            self.reissue_profile_code(issued);
        }
        Ok(true)
    }

    /// The replacement code belongs to the persona whose code was taken,
    /// not to whoever is worn right now.
    fn reissue_profile_code(&self, issued: &IssuedCard) {
        let owner = Some(issued.owner.as_str()).filter(|o| !o.is_empty());
        let name = self.my_name(owner).ok().flatten();
        match self.issue_card(name.as_deref(), 60 * 60 * 24, "profile", owner) {
            Ok(_) => log::info(TAG, "a fresh profile code is ready"),
            Err(e) => log::warn(TAG, format!("could not pre-issue: {e}")),
        }
    }

    /// The standing profile code for the worn persona, cut if there is
    /// none outstanding.
    pub fn profile_code(&self, as_persona: Option<&str>) -> Result<IssuedHandle, Error> {
        let owner = match as_persona {
            Some(h) => h.to_string(),
            None => self.worn()?,
        };
        if let Some(c) = self.current_card(&owner, "profile") {
            return Ok(IssuedHandle { uri: c.uri, inbox_key: c.inbox_key });
        }
        let name = self.my_name(Some(&owner))?;
        self.issue_card(name.as_deref(), 60 * 60 * 24, "profile", Some(&owner))
    }

    // ----- sending -----------------------------------------------------------------

    /// Append one message to our outbox for this contact. Returns the
    /// contact as it is afterwards — the counters moved, and a caller that
    /// sends twice from one snapshot writes two messages into one slot.
    pub fn send(&self, c: &Contact, out: Outgoing) -> Result<Contact, Error> {
        let lock = lock_for(&SEND_LOCKS, &c.persona_hex);
        let _g = lock.lock().unwrap_or_else(|e| e.into_inner());
        self.send_locked(c, out)
    }

    pub fn send_text(&self, persona_hex: &str, body: &str) -> Result<Contact, Error> {
        let c = self.contact(persona_hex).ok_or_else(|| Error::Refused("no such contact".into()))?;
        self.send(&c, Outgoing::text(body))
    }

    /// React to a row (§16.14): an emoji, or nothing to take one back.
    pub fn react(&self, persona_hex: &str, seq: u64, re_own: bool, emoji: &str) -> Result<(), Error> {
        let c = self.contact(persona_hex).ok_or_else(|| Error::Refused("no such contact".into()))?;
        self.send(&c, Outgoing { body: emoji.trim().chars().take(8).collect(), kind: 4, re_seq: Some(seq), re_own, ..Default::default() })?;
        Ok(())
    }

    /// Take back a plain message of ours (kind 5 naming our own row). The
    /// row stays here marked unsent; their client hides theirs.
    pub fn retract(&self, persona_hex: &str, seq: u64) -> Result<(), Error> {
        let c = self.contact(persona_hex).ok_or_else(|| Error::Refused("no such contact".into()))?;
        let thread = self.thread(persona_hex);
        let target = thread.iter().find(|m| m.outgoing && m.seq == seq).ok_or_else(|| Error::Refused("no such message of ours".into()))?;
        if target.kind != 0 || !target.delivered {
            return Err(Error::Refused("only a delivered plain message can be taken back".into()));
        }
        self.send(&c, Outgoing { body: "Took a message back".into(), kind: 5, re_seq: Some(seq), re_own: true, ..Default::default() })?;
        Ok(())
    }

    fn send_locked(&self, c0: &Contact, out: Outgoing) -> Result<Contact, Error> {
        // Who speaks is the contact's to say, not the caller's.
        let mine_hex = self.owner_hex_of(c0);
        // The caller's copy is a snapshot, and counters move.
        let c = self.contact(&c0.persona_hex).unwrap_or_else(|| c0.clone());
        // The recipient's core refuses a body carrying a bidirectional
        // override — after the slot is spent. Cleaned once, at the door.
        let body = ducat_mobile::contacts::clean_display_text(out.body.clone());
        let bundle = c.their_bundle.clone().ok_or_else(|| Error::Refused("no prekey bundle for this contact — the handshake has not completed".into()))?;
        if c.my_outbox_owner_secret.is_empty() {
            return Err(Error::Refused("this conversation predates the current outbox format; ask for a new card".into()));
        }
        // Re-opened **as the owner**: a plain re-open is read-only.
        node_dht_open(c.my_outbox.clone(), Some(c.my_outbox_owner_public.clone()), Some(c.my_outbox_owner_secret.clone()))?;
        let mut ring = c.my_ring;
        // A previous send persisted its message and counters but died
        // before the DHT took the slot: the same seq and the same bytes go
        // out first — never a re-seal.
        if let Some((pseq, pbytes)) = self.pending_slot(&c.persona_hex) {
            if pseq < c.out_seq {
                log::info(TAG, format!("delivering seq {pseq} to {} left over from an interrupted send", c.display_name()));
                ring = self.write_slot_clamped(&c, pseq, &pbytes, ring)?;
                self.mark_delivered(&c.persona_hex, pseq)?;
            }
            self.clear_pending_slot(&c.persona_hex)?;
        }
        log::info(TAG, format!("sending {} seq {} to {}", kind_name(out.kind), c.out_seq, c.display_name()));
        let sealed = seal_message(SealIn {
            bundle_bytes: bundle.clone(),
            seq: c.out_seq,
            prev_link: c.out_prev_link.clone().unwrap_or_else(|| vec![0; 32]),
            body: body.clone(),
            thread_aad: thread_aad(mine_hex.clone(), c.persona_hex.clone()),
            kind: out.kind as u8,
            amount_pxmr: out.amount_pxmr,
            txid: out.txid_hex.as_deref().and_then(hex_to_bytes),
            payto: out.payto.clone(),
            items: out.items.iter().map(|i| BillLine { description: i.description.clone(), amount_pxmr: i.amount_pxmr }).collect(),
            tax_pxmr: out.tax_pxmr,
            re_seq: out.re_seq,
            re_own: out.re_own,
            attachment: out.attachment.clone(),
            eta_secs: out.eta_secs,
            payload: out.payload.clone(),
            round: out.round,
            ceremony_id: out.ceremony_id.clone(),
            position: out.position.clone(),
            group: out.group.clone(),
            publication: out.publication.clone(),
            call: out.call.clone(),
        })?;
        log::info(TAG, format!("sealed seq {} ({} B)", c.out_seq, sealed.bytes.len()));
        let row = StoredMessage {
            outgoing: true,
            seq: c.out_seq,
            body,
            timestamp: App::now(),
            forward_secret: sealed.forward_secret,
            kind: out.kind,
            amount_pxmr: out.amount_pxmr.unwrap_or(0),
            payto: out.payto.clone(),
            txid_hex: out.txid_hex.clone(),
            items: out.items.clone(),
            tax_pxmr: out.tax_pxmr,
            re_seq: out.re_seq,
            re_own: out.re_own,
            att_record: out.attachment.as_ref().and_then(|a| a.record_key.clone()),
            att_swarm: out.attachment.as_ref().and_then(|a| a.swarm_key.clone()),
            att_swarm_digest: out.attachment.as_ref().and_then(|a| a.swarm_digest.as_deref().map(hex)),
            att_key: out.attachment.as_ref().map(|a| a.key.clone()),
            att_nonce: out.attachment.as_ref().map(|a| a.nonce.clone()),
            att_len: out.attachment.as_ref().map_or(0, |a| a.len),
            att_hash: out.attachment.as_ref().map(|a| hex(&a.ct_hash)),
            att_mime: out.attachment.as_ref().map(|a| a.mime.clone()),
            att_name: out.attachment.as_ref().and_then(|a| a.name.clone()),
            oob: out.oob,
            eta_secs: out.eta_secs,
            group_id: out.group.as_ref().and_then(|g| g.id.as_deref().map(hex)),
            group_seq: out.group.as_ref().and_then(|g| g.seq).unwrap_or(0),
            group_re_sender: out.group.as_ref().and_then(|g| g.re_sender.as_deref().map(hex)),
            group_re_seq: out.group.as_ref().and_then(|g| g.re_seq),
            pub_period_id: out.publication.as_ref().and_then(|p| p.period_id.clone()),
            pub_period_key: out.publication.as_ref().and_then(|p| p.period_key.clone()),
            pub_record: out.publication.as_ref().and_then(|p| p.record_key.clone()),
            pub_head_key: out.publication.as_ref().and_then(|p| p.head_key.clone()),
            pub_swarm_key: out.publication.as_ref().and_then(|p| p.swarm_key.clone()),
            pub_swarm_digest: out.publication.as_ref().and_then(|p| p.swarm_digest.as_deref().map(hex)),
            pub_wanted: out.publication.as_ref().and_then(|p| p.wanted_period.clone()),
            call_route: out.call.as_ref().and_then(|k| k.route.as_deref().map(hex)),
            call_id: out.call.as_ref().and_then(|k| k.id.as_deref().map(hex)),
            // Not yet: until the write lands this row is a message that
            // has not left the desk.
            delivered: false,
            dead_letter: false,
            read_by_them: None,
        };
        // Everything local lands before anything remote.
        self.append_and_advance_outbound(&c.persona_hex, row, c.out_seq + 1, sealed.next_link.clone(), &sealed.bytes)?;
        // Withdraw the key just used from our cached copy of their bundle
        // — select() takes the first one-time entry, so without this every
        // message seals to the same key.
        if sealed.prekey_id != 0 {
            if sealed.forward_secret {
                self.record_used_their_id(&c.persona_hex, sealed.prekey_id)?;
            }
            if let Ok(pruned) = prune_prekey(bundle, sealed.prekey_id) {
                self.set_their_bundle(&c.persona_hex, pruned)?;
            }
        }
        log::info(TAG, format!("filed seq {}, writing the slot", c.out_seq));
        ring = self.write_slot_clamped(&c, c.out_seq, &sealed.bytes, ring)?;
        log::info(TAG, "slot written, publishing the head");
        // Republish our keys with every head write — the only route back
        // from an exhausted supply — and, if opted in, §16.16's watermark.
        node_dht_set(
            c.my_outbox.clone(),
            0,
            build_log_head(
                c.out_seq + 1,
                self.topup_if_low(&c.my_outbox)?,
                if self.read_receipts() { Some(c.in_seq) } else { None },
                Some(ring).filter(|r| *r != 8),
            ),
        )?;
        // After the head, not after the slot: a slot the head does not
        // advertise is a message the reader cannot see.
        self.mark_delivered(&c.persona_hex, c.out_seq)?;
        self.clear_pending_slot(&c.persona_hex)?;
        log::info(
            TAG,
            format!(
                "delivered seq {} to {}{}",
                c.out_seq,
                c.display_name(),
                if sealed.forward_secret { "" } else { " (no forward secrecy — their one-time keys ran out)" }
            ),
        );
        self.contact(&c.persona_hex).ok_or_else(|| Error::Refused("the contact vanished mid-send".into()))
    }

    /// One slot write, healing legacy logs on the way: a record minted
    /// with 8 subkeys under a head that claims 32 is clamped back — slots
    /// 0–6 map identically — and the head republishes the honest ring.
    fn write_slot_clamped(&self, c: &Contact, seq: u64, bytes: &[u8], ring0: u32) -> Result<u32, Error> {
        let mut ring = ring0;
        match node_dht_set(c.my_outbox.clone(), log_subkey(seq, ring), bytes.to_vec()) {
            Ok(()) => {}
            Err(e) if format!("{e:?}").contains("out of range") && ring > LOG_SUBKEYS => {
                log::warn(TAG, format!("legacy log smaller than its ring — clamping {} to {LOG_SUBKEYS}", c.display_name()));
                ring = LOG_SUBKEYS;
                self.set_my_ring(&c.persona_hex, LOG_SUBKEYS)?;
                node_dht_set(c.my_outbox.clone(), log_subkey(seq, ring), bytes.to_vec())?;
            }
            Err(e) => return Err(e.into()),
        }
        Ok(ring)
    }

    /// Deliver a slot a dead send left behind, and a head past it.
    fn flush_pending(&self, persona_hex: &str) -> Result<bool, Error> {
        let lock = lock_for(&SEND_LOCKS, persona_hex);
        let _g = lock.lock().unwrap_or_else(|e| e.into_inner());
        let Some((pseq, pbytes)) = self.pending_slot(persona_hex) else { return Ok(false) };
        let Some(c) = self.contact(persona_hex) else { return Ok(false) };
        if pseq >= c.out_seq {
            self.clear_pending_slot(persona_hex)?;
            return Ok(false);
        }
        if c.my_outbox_owner_secret.is_empty() {
            return Ok(false);
        }
        node_dht_open(c.my_outbox.clone(), Some(c.my_outbox_owner_public.clone()), Some(c.my_outbox_owner_secret.clone()))?;
        let ring = self.write_slot_clamped(&c, pseq, &pbytes, c.my_ring)?;
        node_dht_set(
            c.my_outbox.clone(),
            0,
            build_log_head(
                c.out_seq,
                self.topup_if_low(&c.my_outbox)?,
                if self.read_receipts() { Some(c.in_seq) } else { None },
                Some(ring).filter(|r| *r != 8),
            ),
        )?;
        self.mark_delivered(persona_hex, pseq)?;
        self.clear_pending_slot(persona_hex)?;
        log::info(TAG, format!("delivered seq {pseq} to {} left over from an interrupted send", c.display_name()));
        Ok(true)
    }

    /// The poll's turn at a late slot: a few tries, then it is left for
    /// the next send, which replays it anyway.
    fn late_slot(&self, c: &Contact) -> Result<(), Error> {
        let Some((pseq, _)) = self.pending_slot(&c.persona_hex) else { return Ok(()) };
        let store = self.store(crate::contacts::CONTACTS);
        let key = &c.persona_hex;
        let tries = if store.get::<u64>(&format!("lateq_{key}")) == Some(pseq) {
            store.get::<u32>(&format!("late_{key}")).unwrap_or(0)
        } else {
            0
        };
        if tries >= LATE_SLOT_GIVE_UP {
            return Ok(());
        }
        match self.flush_pending(key) {
            Ok(_) => {
                let _ = store.remove(&format!("late_{key}"));
                let _ = store.remove(&format!("lateq_{key}"));
                Ok(())
            }
            Err(e) if is_offline(&e) => Err(e),
            Err(e) => {
                let _ = store.put(&format!("late_{key}"), &(tries + 1));
                let _ = store.put(&format!("lateq_{key}"), &pseq);
                log::warn(
                    TAG,
                    format!(
                        "late slot seq {pseq} to {}: {e}{}",
                        c.display_name(),
                        if tries + 1 >= LATE_SLOT_GIVE_UP { " — leaving it for the next send" } else { "" }
                    ),
                );
                Ok(())
            }
        }
    }

    // ----- bundles -----------------------------------------------------------------

    /// The one-time offer for one of our outboxes, cut fresh when it runs
    /// low; the signed prekey rotates on schedule, and the old one opens
    /// for thirty days more.
    fn topup_if_low(&self, outbox: &str) -> Result<Option<Vec<u8>>, Error> {
        if self.thread_one_time_remaining(outbox) > 6 {
            return Ok(self.thread_bundle(outbox));
        }
        let rotate = self.signed_prekey_due();
        let m = generate_prekeys(
            ONE_TIME_KEYS,
            ONE_TIME_VALID_SECS,
            self.next_prekey_start(ONE_TIME_KEYS)?,
            if rotate { None } else { self.signed_prekey_secret() },
        );
        self.save_prekeys(&[], &m.signed_secret, &zip_secrets(&m.one_time_ids, &m.one_time_secrets), rotate)?;
        if rotate {
            log::info(TAG, "rotated the signed prekey; the old one opens for 30 days");
        }
        self.set_thread_bundle(outbox, &m.bundle)?;
        log::info(TAG, "cut a fresh one-time batch for this thread");
        Ok(Some(m.bundle))
    }

    /// After a restore: the desk is advertising keys it does not hold, and
    /// every message written against them is lost on arrival. Each thread
    /// gets a fresh offer, and a head the network shows to be ahead of the
    /// bundle's counter is resumed from the network's number.
    fn republish_bundles(&self) {
        let mut all_landed = true;
        for c0 in self.contacts() {
            if c0.my_outbox_owner_secret.is_empty() {
                continue;
            }
            let mut c = c0;
            let r: Result<(), Error> = (|| {
                node_dht_open(c.my_outbox.clone(), Some(c.my_outbox_owner_public.clone()), Some(c.my_outbox_owner_secret.clone()))?;
                match node_dht_get(c.my_outbox.clone(), 0, true) {
                    Ok(Some(raw)) => {
                        if let Ok(mine) = parse_log_head(raw) {
                            if mine.next_seq > c.out_seq {
                                log::info(TAG, format!("our log for {} reached {}, the bundle said {} — resuming from theirs", c.display_name(), mine.next_seq, c.out_seq));
                                self.advance_outbound(&c.persona_hex, mine.next_seq, c.out_prev_link.clone().unwrap_or_else(|| vec![0; 32]))?;
                                if let Some(fresh) = self.contact(&c.persona_hex) {
                                    c = fresh;
                                }
                            }
                        }
                    }
                    Ok(None) => {}
                    Err(e) => log::warn(TAG, format!("own head for {}: {e:?}", c.display_name())),
                }
                let bundle = self.recut_thread_bundle(&c.my_outbox)?;
                node_dht_set(
                    c.my_outbox.clone(),
                    0,
                    build_log_head(
                        c.out_seq,
                        Some(bundle),
                        if self.read_receipts() { Some(c.in_seq) } else { None },
                        Some(c.my_ring).filter(|r| *r != 8),
                    ),
                )?;
                Ok(())
            })();
            if let Err(e) = r {
                let gone = is_missing(&e);
                if !gone {
                    all_landed = false;
                }
                log::warn(TAG, format!("republish {}: {e}{}", c.display_name(), if gone { " — that record is gone, nothing to republish" } else { "" }));
            }
        }
        if all_landed {
            let _ = self.set_bundles_need_republish(false);
            log::info(TAG, "republished every thread's one-time offer after a restore");
        }
    }

    fn recut_thread_bundle(&self, outbox: &str) -> Result<Vec<u8>, Error> {
        let m = generate_prekeys(
            ONE_TIME_KEYS,
            ONE_TIME_VALID_SECS,
            self.next_prekey_start(ONE_TIME_KEYS)?,
            self.signed_prekey_secret(),
        );
        self.save_prekeys(&[], &m.signed_secret, &zip_secrets(&m.one_time_ids, &m.one_time_secrets), false)?;
        self.set_thread_bundle(outbox, &m.bundle)?;
        Ok(m.bundle)
    }

    /// Slot insurance: re-push the last few slots of one thread whose
    /// writes have not been confirmed since, in case a flood died with a
    /// slot not yet propagated. One contact per call, on the poller's lap.
    pub fn verify_last_writes(&self) {
        if LAST_POLL_OFFLINE.load(Ordering::Relaxed) {
            return;
        }
        let Some(c) = self.contacts().into_iter().find(|k| {
            k.out_seq > 0 && !k.my_outbox.is_empty() && !k.my_outbox_owner_secret.is_empty() && self.last_slot_verified(&k.persona_hex) < k.out_seq as i64 - 1
        }) else {
            return;
        };
        let ring = c.my_ring;
        let from = c.out_seq.saturating_sub(4);
        let r: Result<(), Error> = (|| {
            node_dht_open(c.my_outbox.clone(), Some(c.my_outbox_owner_public.clone()), Some(c.my_outbox_owner_secret.clone()))?;
            let mut pushed = 0;
            for seq in from..c.out_seq {
                let sub = log_subkey(seq, ring);
                let Some(local) = node_dht_get(c.my_outbox.clone(), sub, false)? else { continue };
                node_dht_set(c.my_outbox.clone(), sub, local)?;
                pushed += 1;
            }
            if let Some(head) = node_dht_get(c.my_outbox.clone(), 0, false)? {
                node_dht_set(c.my_outbox.clone(), 0, head)?;
            }
            if pushed > 0 {
                log::info(TAG, format!("re-pushed {pushed} trailing slot(s) to {} (seq {from}..{})", c.display_name(), c.out_seq - 1));
            }
            self.set_last_slot_verified(&c.persona_hex, c.out_seq as i64 - 1)?;
            self.set_slot_fix_tries(&c.persona_hex, 0)?;
            Ok(())
        })();
        if let Err(e) = r {
            let tries = self.slot_fix_tries(&c.persona_hex) + 1;
            if tries >= SLOT_FIX_GIVE_UP {
                log::warn(TAG, format!("slot re-push: {e} — giving up on seq {from}..{}", c.out_seq - 1));
                let _ = self.set_last_slot_verified(&c.persona_hex, c.out_seq as i64 - 1);
                let _ = self.set_slot_fix_tries(&c.persona_hex, 0);
            } else {
                log::warn(TAG, format!("slot re-push: {e} (try {tries})"));
                let _ = self.set_slot_fix_tries(&c.persona_hex, tries);
            }
        }
    }

    // ----- receiving --------------------------------------------------------------

    /// Read every contact's log forward. Returns how many messages
    /// arrived; offline stops the pass with one line.
    pub fn poll(&self) -> usize {
        // Before reading anyone: a restored desk is advertising keys it
        // does not hold.
        if self.bundles_need_republish() {
            self.republish_bundles();
        }
        // Each poll is also the clock for the forward-secrecy delete.
        let _ = self.sweep_burned_prekeys();
        let mut got = 0;
        let mut offline = false;
        for c in self.contacts() {
            let r: Result<usize, Error> = (|| {
                // Ours first: a slot an interrupted send left behind is
                // older than anything this pass will read.
                self.late_slot(&c)?;
                self.poll_one(&c)
            })();
            match r {
                Ok(n) => got += n,
                Err(e) if is_offline(&e) => {
                    log::info(TAG, "offline — messages wait for the network");
                    offline = true;
                    break;
                }
                Err(e) => log::warn(TAG, format!("poll {}: {e}", c.display_name())),
            }
        }
        LAST_POLL_OFFLINE.store(offline, Ordering::Relaxed);
        got
    }

    pub fn poll_contact(&self, persona_hex: &str) -> usize {
        let Some(c) = self.contact(persona_hex) else { return 0 };
        self.poll_one(&c).unwrap_or(0)
    }

    fn poll_one(&self, c: &Contact) -> Result<usize, Error> {
        let lock = lock_for(&POLL_LOCKS, &c.persona_hex);
        let _g = lock.lock().unwrap_or_else(|e| e.into_inner());
        // Each reader starts from the freshest counters.
        let Some(fresh) = self.contact(&c.persona_hex) else { return Ok(0) };
        let mine_hex = self.owner_hex_of(&fresh);
        self.poll_one_locked(&fresh, &mine_hex)
    }

    fn poll_one_locked(&self, c: &Contact, mine_hex: &str) -> Result<usize, Error> {
        node_dht_open(c.their_outbox.clone(), None, None)?;
        let Some(head_raw) = node_dht_get(c.their_outbox.clone(), 0, true)? else { return Ok(0) };
        let head = parse_log_head(head_raw)?;
        let next = head.next_seq;
        // Their refreshed keys, if they published any: a stale cached
        // bundle seals to keys they consumed long ago.
        if let Some(b) = head.prekey_bundle.clone() {
            self.set_their_bundle(&c.persona_hex, b)?;
        }
        // §16.12: their ring is whatever their head says it is.
        let ring = head.ring.unwrap_or(LOG_SUBKEYS);
        // §16.16: their claim about how far they have read our log.
        if let Some(r) = head.read_up_to {
            self.set_their_read_up_to(&c.persona_hex, r)?;
        }
        let mut seq = c.in_seq;
        let mut prev = c.in_prev_link.clone();
        let mut count = 0;
        let who = c.display_name();
        // **One marker for the whole gap, not one per sequence number.**
        // `next` is the peer's word, taken off the wire with no bound; a
        // head advertising 2^40 must not write four billion rows.
        if next > seq && next - seq > ring as u64 {
            let lost = next - seq - ring as u64;
            log::warn(TAG, format!("{lost} message(s) from {who} passed the ring while this desk was away — recorded as one gap"));
            let row = StoredMessage::dead(seq, &format!("[{lost} messages were lost — this device was away too long]"), self.dead_letter_time(&c.persona_hex));
            self.append_and_advance(&c.persona_hex, row, next - ring as u64, None)?;
            self.clear_stuck(&format!("{}:{seq}", c.persona_hex));
            seq = next - ring as u64;
            prev = None;
        }
        while seq < next {
            if !log_still_readable(seq, next, ring) {
                // The ring passed us. Placeholder and cursor in one commit.
                log::warn(TAG, format!("lost message {seq} from {who} — ring wrapped"));
                let row = StoredMessage::dead(seq, "[a message was lost — this device was away too long]", self.dead_letter_time(&c.persona_hex));
                self.append_and_advance(&c.persona_hex, row, seq + 1, None)?;
                self.clear_stuck(&format!("{}:{seq}", c.persona_hex));
                seq += 1;
                prev = None;
                continue;
            }
            let subkey = log_subkey(seq, ring);
            let Some(raw) = node_dht_get(c.their_outbox.clone(), subkey, true)? else { break };
            let slot_key = format!("{}:{subkey}", c.persona_hex);
            let raw_hash = content_hash(&raw);
            // A pure decode: these bytes will fail the same way for ever, so
            // a failure is final — recorded and skipped, never rethrown.
            let id = match sealed_prekey_id(raw.clone()) {
                Ok(id) => id,
                Err(e) => {
                    log::warn(TAG, format!("message {seq} from {who} is not a readable message at all ({e}) — recorded and skipped"));
                    let row = StoredMessage::dead(seq, "[a message could not be read — it was not in a form this app understands]", self.dead_letter_time(&c.persona_hex));
                    self.append_and_advance(&c.persona_hex, row, seq + 1, None)?;
                    self.record_slot_seen(&slot_key, raw_hash, seq);
                    seq += 1;
                    prev = None;
                    continue;
                }
            };
            let is_one_time = id != 0;
            // Plural for the signed key: it rotates, and a peer sealing
            // from a cached bundle addressed the one just retired.
            let secrets: Vec<Vec<u8>> = if is_one_time {
                self.one_time_secret(id).into_iter().collect()
            } else {
                self.signed_prekey_secrets()
            };
            if secrets.is_empty() {
                if self.slot_seen(&slot_key) == Some(raw_hash) {
                    if self.slot_seen_seq(&slot_key) == Some(seq) {
                        // These bytes were processed once as exactly this
                        // sequence and the process died before the row was
                        // kept; the one-time key was burned then, so the
                        // words are gone. Keep the thread honest: a marker,
                        // and everything behind the hole delivered.
                        log::warn(TAG, format!("message {seq} from {who} was processed once and lost to a crash — keeping a marker and moving on"));
                        let row = StoredMessage::dead(seq, "[a message arrived here and was lost to an interruption before it could be kept]", self.dead_letter_time(&c.persona_hex));
                        self.append_and_advance(&c.persona_hex, row, seq + 1, None)?;
                        self.record_slot_seen(&slot_key, raw_hash, seq);
                        seq += 1;
                        prev = None;
                        continue;
                    }
                    // The slot's previous tenant — bytes already processed
                    // as an earlier sequence. The real write is still
                    // propagating; wait as long as it takes.
                    log::info(TAG, format!("slot for message {seq} from {who} still holds its previous tenant — waiting (subkey {subkey})"));
                    break;
                }
                // Bytes this reader has no memory of: this seq's real write
                // sealed to a key we no longer hold, or an older tenant that
                // never got processed here. Both get the patience window.
                let key = format!("{}:{seq}", c.persona_hex);
                let since = self.stuck_since(&key);
                if now_ms().saturating_sub(since) < STUCK_PATIENCE_MS {
                    // Start the clocks of everything behind it now, or the
                    // windows run end to end.
                    let mut behind = seq + 1;
                    while behind < next {
                        self.stuck_since(&format!("{}:{behind}", c.persona_hex));
                        behind += 1;
                    }
                    log::info(TAG, format!("message {seq} from {who} not readable yet (prekey {id}) — waiting for the slot to settle"));
                    break;
                }
                self.clear_stuck(&key);
                log::warn(TAG, format!("prekey {id} is gone; message {seq} from {who} is lost"));
                let row = StoredMessage::dead(seq, "[a message could not be opened — it was sealed to a key this device no longer holds]", self.dead_letter_time(&c.persona_hex));
                self.append_and_advance(&c.persona_hex, row, seq + 1, None)?;
                // After the append, never before.
                self.record_slot_seen(&slot_key, raw_hash, seq);
                seq += 1;
                prev = None;
                continue;
            }
            let aad = thread_aad(mine_hex.to_string(), c.persona_hex.clone());
            let opened = match open_with_any(&secrets, |sk| open_message(raw.clone(), sk.to_vec(), is_one_time, seq, prev.clone(), aad.clone())) {
                Ok(o) => o,
                Err(e) => {
                    let msg = e.to_string();
                    if msg.contains("Malformed") {
                        // Decrypted but refused: final for these bytes.
                        log::warn(TAG, format!("message {seq} from {who} is malformed — recorded and skipped"));
                        let row = StoredMessage::dead(seq, "[a message could not be understood — the sender's client encoded it wrongly]", self.dead_letter_time(&c.persona_hex));
                        self.append_and_advance(&c.persona_hex, row, seq + 1, None)?;
                        self.clear_stuck(&format!("{}:{seq}", c.persona_hex));
                        self.record_slot_seen(&slot_key, raw_hash, seq);
                        seq += 1;
                        prev = None;
                        continue;
                    }
                    // Two refusals a restore produces: a message that does
                    // not authenticate, and one that does not follow the one
                    // before it. A patience window, then a dead letter —
                    // never a wall.
                    let unopenable = msg.contains("BadSig") || msg.contains("did not authenticate");
                    let out_of_chain = msg.contains("does not follow");
                    if unopenable || out_of_chain {
                        let bad_key = format!("{}:{seq}", c.persona_hex);
                        if now_ms().saturating_sub(self.stuck_since(&bad_key)) < STUCK_PATIENCE_MS {
                            let mut behind = seq + 1;
                            while behind < next {
                                self.stuck_since(&format!("{}:{behind}", c.persona_hex));
                                behind += 1;
                            }
                            log::info(TAG, format!("message {seq} from {who} {} — waiting for the slot to settle", if out_of_chain { "does not follow the one before it" } else { "does not open with the key it names" }));
                            break;
                        }
                        self.clear_stuck(&bad_key);
                        log::warn(TAG, format!("message {seq} from {who} {} — recorded and skipped", if out_of_chain { "broke the chain" } else { "never authenticated" }));
                        let body = if out_of_chain {
                            "[a message is missing here — the sender lost it before this device could read it]"
                        } else {
                            "[a message could not be opened — it was sealed to a key this device no longer holds]"
                        };
                        let row = StoredMessage::dead(seq, body, self.dead_letter_time(&c.persona_hex));
                        self.append_and_advance(&c.persona_hex, row, seq + 1, None)?;
                        self.record_slot_seen(&slot_key, raw_hash, seq);
                        seq += 1;
                        prev = None;
                        continue;
                    }
                    log::warn(TAG, format!("message {seq} from {who} could not be opened ({msg}) — recorded and skipped"));
                    let row = StoredMessage::dead(seq, "[a message could not be read — it was not in a form this app understands]", self.dead_letter_time(&c.persona_hex));
                    self.append_and_advance(&c.persona_hex, row, seq + 1, None)?;
                    self.record_slot_seen(&slot_key, raw_hash, seq);
                    seq += 1;
                    prev = None;
                    continue;
                }
            };
            self.clear_stuck(&format!("{}:{seq}", c.persona_hex));
            log::info(TAG, format!("received {} seq {} from {who}", kind_name(opened.kind as u32), opened.seq));
            let arrived = row_of(&opened, is_one_time);
            let link = opened.link.clone();
            self.append_and_advance(&c.persona_hex, arrived.clone(), seq + 1, Some(link.clone()))?;
            self.record_slot_seen(&slot_key, raw_hash, seq);
            if arrived.surfaces() && arrived.kind != 14 && arrived.kind != 15 {
                let line = match arrived.kind {
                    1 => format!("Bill · {} XMR", crate::wallet::format_xmr(arrived.amount_pxmr)),
                    2 => format!("Payment · {} XMR", crate::wallet::format_xmr(arrived.amount_pxmr)),
                    3 => "Receipt".to_string(),
                    _ => arrived.body.chars().take(120).collect(),
                };
                crate::notify::post(who.clone(), line, Some(c.persona_hex.clone()));
            }
            self.on_arrival(c, &opened, &arrived);
            if let Some(p) = opened.payto.as_deref() {
                self.set_their_address(&c.persona_hex, p)?;
            }
            if opened.consumed_one_time {
                self.burn_one_time(opened.prekey_id)?;
            }
            prev = Some(link);
            seq += 1;
            count += 1;
        }
        Ok(count)
    }

    /// What a kind does beyond being kept. The phone dispatches ceremony
    /// rounds, rosters, publication keys and asks from here; the desk says
    /// so in the log until those rails are ported.
    fn on_arrival(&self, c: &Contact, opened: &OpenedMessage, arrived: &StoredMessage) {
        match arrived.kind {
            7 => {
                let thread = self.thread(&c.persona_hex);
                if let Some(mine) = crate::contacts::referent(&thread, arrived) {
                    if mine.outgoing && mine.kind == 6 {
                        log::info(TAG, format!("{} accepted the ride", c.display_name()));
                    }
                }
            }
            8 | 9 | 10 => log::warn(TAG, format!("a ceremony round from {} — ceremonies are not on the desk yet", c.display_name())),
            11 => log::info(TAG, format!("{} offered a live position — not shown on the desk yet", c.display_name())),
            12 => self.absorb_roster(&c.persona_hex, opened.group_id.as_deref(), opened.payload.as_deref()),
            13 => {
                if let Err(e) = self.absorb_key(&c.persona_hex, arrived) {
                    log::warn(TAG, format!("publication key: {e}"));
                }
            }
            16 => {
                if let Some(want) = opened.wanted_period.as_deref() {
                    self.on_wanted(&c.persona_hex, want);
                }
            }
            _ => {}
        }
    }
}

/// Counters kept where the logs are the same as the record on disk.
fn keep_counters(built: Contact, fresh: Option<&Contact>) -> Contact {
    let Some(fresh) = fresh else { return built };
    let ours = fresh.my_outbox == built.my_outbox;
    let theirs = fresh.their_outbox == built.their_outbox;
    Contact {
        out_seq: if ours { fresh.out_seq } else { built.out_seq },
        out_prev_link: if ours { fresh.out_prev_link.clone() } else { built.out_prev_link },
        in_seq: if theirs { fresh.in_seq } else { built.in_seq },
        in_prev_link: if theirs { fresh.in_prev_link.clone() } else { built.in_prev_link },
        ..built
    }
}

/// Try each key, newest first; a "Malformed" refusal wins over the others
/// because it is the one that says the bytes decrypted.
fn open_with_any<T, F: FnMut(&[u8]) -> Result<T, ducat_mobile::contacts::ContactError>>(keys: &[Vec<u8>], mut open: F) -> Result<T, Error> {
    let mut best: Option<Error> = None;
    for k in keys {
        match open(k) {
            Ok(v) => return Ok(v),
            Err(e) => {
                let e: Error = e.into();
                if best.is_none() || e.to_string().contains("Malformed") {
                    best = Some(e);
                }
            }
        }
    }
    Err(best.unwrap_or_else(|| Error::Refused("no key to open with".into())))
}

fn row_of(o: &OpenedMessage, forward_secret: bool) -> StoredMessage {
    StoredMessage {
        outgoing: false,
        seq: o.seq,
        body: o.body.clone(),
        timestamp: o.timestamp,
        forward_secret,
        kind: o.kind as u32,
        amount_pxmr: o.amount_pxmr.unwrap_or(0),
        payto: o.payto.clone(),
        txid_hex: o.txid.as_deref().map(hex),
        items: o.items.iter().map(|i| BillItem { description: i.description.clone(), amount_pxmr: i.amount_pxmr }).collect(),
        tax_pxmr: o.tax_pxmr,
        re_seq: o.re_seq,
        re_own: o.re_own,
        eta_secs: o.eta_secs,
        att_record: o.attachment.as_ref().and_then(|a| a.record_key.clone()),
        att_swarm: o.attachment.as_ref().and_then(|a| a.swarm_key.clone()),
        att_swarm_digest: o.attachment.as_ref().and_then(|a| a.swarm_digest.as_deref().map(hex)),
        att_key: o.attachment.as_ref().map(|a| a.key.clone()),
        att_nonce: o.attachment.as_ref().map(|a| a.nonce.clone()),
        att_len: o.attachment.as_ref().map_or(0, |a| a.len),
        att_hash: o.attachment.as_ref().map(|a| hex(&a.ct_hash)),
        att_mime: o.attachment.as_ref().map(|a| a.mime.clone()),
        att_name: o.attachment.as_ref().and_then(|a| a.name.clone()),
        group_id: o.group_id.as_deref().map(hex),
        group_seq: o.group_seq.unwrap_or(0),
        group_re_sender: o.group_re_sender.as_deref().map(hex),
        group_re_seq: o.group_re_seq,
        pub_wanted: o.wanted_period.clone(),
        pub_period_id: o.publication.as_ref().map(|p| p.period_id.clone()),
        pub_period_key: o.publication.as_ref().map(|p| p.period_key.clone()),
        pub_record: o.publication.as_ref().and_then(|p| p.record_key.clone()),
        pub_head_key: o.publication.as_ref().and_then(|p| p.head_key.clone()),
        pub_swarm_key: o.publication.as_ref().and_then(|p| p.swarm_key.clone()),
        pub_swarm_digest: o.publication.as_ref().and_then(|p| p.swarm_digest.as_deref().map(hex)),
        call_route: o.call_route.as_deref().map(hex),
        call_id: o.call_id.as_deref().map(hex),
        delivered: true,
        oob: false,
        dead_letter: false,
        read_by_them: None,
    }
}

/// The outbox a card URI does not carry — only for the log line.
fn self_outbox_of(_uri: &str) -> String {
    String::from("(new)")
}
