//! The whole contact flow over records, with nobody online together (§16.12).
//!
//! Three processes, each of which starts a node, does its part, and **exits**:
//!
//!   --card-issue           issuer: inbox + outbox, details in subkey 0, prints a card
//!   --card-claim <uri>     claimant: reads 0, writes 1, sends a message, exits
//!   --card-collect         issuer: reads 1, then reads the claimant's outbox
//!
//! That separation is the test. The `app_call` build could only ever be checked
//! with both halves running at once, which is exactly the condition it turned
//! out to require and the reason its failures looked like flakiness.

use std::time::Instant;

use ducat_core::cbor::decode;
use ducat_core::contact::*;
use ducat_core::hpke::{self, PreKey, PreKeyBundle, PreKeyStore, SealedMessage};
use ducat_core::sig::{ObjectType, PublicKey, SecretKey, SignedBytes, Suite};
use ducat_core::wire::{open as open_env, peek_body, seal as seal_env};
use veilid_core::*;

/// A deterministic CSPRNG stand-in, so a failed harness run reproduces exactly.
/// Never appropriate outside a harness, which is why it lives here and not in
/// `core` — `core` holds no randomness at all and takes it as a parameter.
pub struct HarnessRng(pub u8);

impl ducat_core::hpke::rand_core::TryRng for HarnessRng {
    type Error = core::convert::Infallible;
    fn try_next_u32(&mut self) -> Result<u32, Self::Error> {
        let mut b = [0u8; 4];
        self.try_fill_bytes(&mut b)?;
        Ok(u32::from_le_bytes(b))
    }
    fn try_next_u64(&mut self) -> Result<u64, Self::Error> {
        let mut b = [0u8; 8];
        self.try_fill_bytes(&mut b)?;
        Ok(u64::from_le_bytes(b))
    }
    fn try_fill_bytes(&mut self, dst: &mut [u8]) -> Result<(), Self::Error> {
        for d in dst.iter_mut() {
            self.0 = self.0.wrapping_mul(31).wrapping_add(17);
            *d = self.0;
        }
        Ok(())
    }
}
impl ducat_core::hpke::rand_core::TryCryptoRng for HarnessRng {}

/// A head plus 31 slots, matching the app's ring. Eight was "small on
/// purpose: a wrap that happens is a wrap that is tested" — and it earned its
/// keep three times (the slot-8 landmine, the stale-tenant reads, the
/// patience window all came from wraps). Tuition paid; now the desk stops
/// wrapping during a coffee.
const LOG_SUBKEYS: u32 = 32;
// Sixteen, up from four. The app burns one per inbound message and refreshes
// its cached copy of our bundle only from our log head — which this harness,
// being mostly offline, rarely rewrites. Four keys was six messages of real
// conversation before every send of theirs fell back to the signed prekey and
// wore the open lock. A harness with fixed seeds has no real secrecy to
// protect; what it owes the phone across the table is a supply that lasts.
const ONE_TIME_KEYS: u32 = 16;

/// Where the harness keeps who it has met.
///
/// **Not `/tmp`.** It was, and the consequence was not hypothetical: a cleared
/// `/tmp` took the issuer's record of a real contact with it, and a payment
/// request the other side had already sent became unreadable from this machine
/// — the message was still in the DHT, still sealed to keys this harness can
/// still derive, and there was no longer anything here saying *whose log to
/// look in*. Ephemeral storage for the one file that has to outlive a reboot.
///
/// `$XDG_STATE_HOME`, then `~/.local/state`, then `/tmp` as a last resort so a
/// machine with no home directory still runs.
pub fn state_path_pub(who: &str) -> String { state_path(who) }

/// Name the currently-claimed thread, so the next claim cannot erase it.
///
/// The single-slot state made every new claim an amnesia event: the desk
/// forgot the previous conversation entirely, chain links and spent-key
/// ledgers included. Saving moves the default slot's three files under
/// `contacts/<name>/`; talk to them again with `DUCAT_CONTACT=<name>`.
pub fn contact_save(name: &str) -> Result<(), Box<dyn std::error::Error>> {
    if name.is_empty()
        || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err("a contact name is letters, digits, - or _".into());
    }
    let src = state_path("claimant");
    if !std::path::Path::new(&src).exists() {
        return Err("no claimed contact in the default slot".into());
    }
    let dir = std::path::Path::new(&src)
        .parent()
        .ok_or("no state dir")?
        .join("contacts")
        .join(name);
    std::fs::create_dir_all(&dir)?;
    for who in ["claimant", "claimant-used", "claimant-theirs"] {
        let from = state_path(who);
        if std::path::Path::new(&from).exists() {
            std::fs::rename(&from, dir.join(format!("{who}.json")))?;
        }
    }
    println!("  saved — talk with DUCAT_CONTACT={name}");
    Ok(())
}

/// Every thread this desk knows: the default slot and every saved name.
pub fn contacts_list() -> Result<(), Box<dyn std::error::Error>> {
    println!("\n\x1b[1mDUCAT — contacts on this desk\x1b[0m\n");
    let default = state_path("claimant");
    let dir = std::path::Path::new(&default).parent().ok_or("no state dir")?.to_path_buf();
    let mut any = false;
    let mut show = |label: &str, path: &std::path::Path| {
        if let Ok(st) = std::fs::read_to_string(path) {
            let mut it = st.lines();
            let outbox = it.next().unwrap_or("?");
            let persona = it.next().unwrap_or("?");
            println!("  {label:<16} persona {}…  outbox {}…",
                &persona[..persona.len().min(12)],
                &outbox[..outbox.len().min(24)]);
            any = true;
        }
    };
    show("(default)", std::path::Path::new(&default));
    if let Ok(entries) = std::fs::read_dir(dir.join("contacts")) {
        let mut names: Vec<_> = entries
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_dir())
            .filter_map(|e| e.file_name().into_string().ok())
            .collect();
        names.sort();
        for n in names {
            show(&n, &dir.join("contacts").join(&n).join("claimant.json"));
        }
    }
    if !any {
        println!("  none yet — claim a card first");
    }
    println!();
    Ok(())
}

fn state_path(who: &str) -> String {
    if let Ok(p) = std::env::var("DUCAT_STATE") {
        return p;
    }
    let base = std::env::var("XDG_STATE_HOME")
        .ok()
        .filter(|v| !v.is_empty())
        .or_else(|| std::env::var("HOME").ok().map(|h| format!("{h}/.local/state")));
    // One desk, many people: DUCAT_CONTACT selects whose thread the state
    // belongs to. The unselected path stays where it always was, so every
    // existing thread keeps working with no flag — a single-slot state that
    // silently overwrote the last conversation on every new claim was the
    // harness's biggest lie about being a client.
    let contact = std::env::var("DUCAT_CONTACT").ok().filter(|c| {
        !c.is_empty()
            && c != "default"
            && c.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
    });
    match base {
        Some(b) => {
            let dir = match &contact {
                Some(c) => format!("{b}/ducat/contacts/{c}"),
                None => format!("{b}/ducat"),
            };
            let _ = std::fs::create_dir_all(&dir);
            format!("{dir}/{who}.json")
        }
        None => format!("/tmp/ducat-{who}.json"),
    }
}

/// One-time ids the peer has already consumed, so the published bundle can
/// stop advertising them (§16.11). A missing file is an empty ledger.
fn used_one_time_ids() -> std::collections::HashSet<u32> {
    std::fs::read_to_string(state_path("claimant-used"))
        .unwrap_or_default()
        .split(',')
        .filter_map(|v| v.trim().parse().ok())
        .collect()
}

/// Ids of *their* one-time keys this desk has already sealed to. The peer
/// burns a key when they read the message sealed to it — and since this desk
/// is the only writer in the thread, their burned set is exactly this set.
/// Skipping it makes a stale head harmless: however old the fetched bundle,
/// the ids we personally spent never get picked twice.
fn used_their_ids() -> std::collections::HashSet<u32> {
    std::fs::read_to_string(state_path("claimant-theirs"))
        .unwrap_or_default()
        .split(',')
        .filter_map(|v| v.trim().parse().ok())
        .collect()
}

fn record_used_their_id(id: u32) {
    let mut used = used_their_ids();
    if used.insert(id) {
        let mut v: Vec<String> = used.iter().map(|i| i.to_string()).collect();
        v.sort();
        let _ = std::fs::write(state_path("claimant-theirs"), v.join(","));
    }
}

fn record_used_one_time_id(id: u32) {
    let mut used = used_one_time_ids();
    if used.insert(id) {
        let mut v: Vec<String> = used.iter().map(|i| i.to_string()).collect();
        v.sort();
        let _ = std::fs::write(state_path("claimant-used"), v.join(","));
    }
}

/// Derived rather than stored: the harness needs the same prekeys back in a
/// later process, and a seed is one field instead of a key ring. A real client
/// stores the secrets and deletes each on use (§16.11).
fn prekeys(seed: u8) -> (PreKeyStore, PreKeyBundle) {
    let (signed_sk, signed_pk) = hpke::derive_keypair(&[seed; 32]);
    let mut store = PreKeyStore::new(signed_sk);
    let mut one_time = Vec::new();
    for id in 1..=ONE_TIME_KEYS {
        let (sk, pk) = hpke::derive_keypair(&[seed.wrapping_add(id as u8); 32]);
        store.insert_one_time(id, sk);
        one_time.push(PreKey { id, public: pk });
    }
    let bundle = PreKeyBundle {
        version: 1,
        suite: 1,
        signed_prekey: signed_pk,
        one_time,
        expiry: crate::payee::now() + 86_400,
    };
    (store, bundle)
}

/// Create a log **under a keypair we generated**, so we can still write to it
/// in a later process.
///
/// A record created with `None` is writable only by the process that made it:
/// re-opening it afterwards yields a read-only handle, and the write comes back
/// "value is not writable", which reads as the network refusing and is us
/// having thrown the key away. The app hit this and left a comment about it;
/// the harness had the same bug and no comment.
async fn make_log(
    rc: &RoutingContext,
) -> Result<(RecordKey, KeyPair), Box<dyn std::error::Error>> {
    let api = rc.api();
    let cs = api.crypto()?;
    let crypto = cs.get(CRYPTO_KIND_VLD0).ok_or("no VLD0")?;
    let kp = crypto.generate_keypair();
    let desc = rc
        .create_dht_record(CRYPTO_KIND_VLD0, DHTSchema::dflt(LOG_SUBKEYS as u16)?, Some(kp.clone()))
        .await?;
    let head = LogHead {
        version: 1, suite: 1, next_seq: 0, prekey_bundle: None, read_up_to: None,
        ring: if LOG_SUBKEYS == 8 { None } else { Some(LOG_SUBKEYS) },
    };
    rc.set_dht_value(desc.key().clone(), 0, head.to_value().encode(), None)
        .await?;
    Ok((desc.key().clone(), kp))
}

/// Append one sealed message and move the head.
///
/// Head **after** the slot, deliberately. A reader that sees `next_seq` bumped
/// and then finds an unwritten slot has been told a message exists that does
/// not; the other order merely makes it briefly late.
async fn append(
    rc: &RoutingContext,
    log: &RecordKey,
    seq: u64,
    body: &[u8],
    // The ring of the record actually being written — never a constant. A
    // constant is how the app put slot 8 in an 8-subkey record; the desk
    // does not get to relearn that one.
    ring: u32,
) -> Result<(), Box<dyn std::error::Error>> {
    // Prime the local record store with the slot's current tenant first. A
    // fresh process has no local copy, so its write goes out at value_seq 0 —
    // and the network silently refuses to replace the seq-0 tenant already
    // there. Every message since the ring first wrapped (seq 7 on) was lost
    // to exactly this; the head survived only because it gets read before
    // every write as a matter of course.
    let _ = rc.get_dht_value(log.clone(), subkey_for(seq, ring), true).await;
    if let Some(newer) = rc
        .set_dht_value(log.clone(), subkey_for(seq, ring), body.to_vec(), None)
        .await?
    {
        return Err(format!(
            "slot {} already holds a newer value (seq {}) — write refused",
            subkey_for(seq, ring),
            newer.seq()
        )
        .into());
    }
    let _ = rc.get_dht_value(log.clone(), 0, true).await;
    // §16.12: the head carries our current bundle, so every poll a reader
    // makes is also a prekey refresh. Writing None here was why the app's
    // cached copy of this harness's bundle could only ever shrink — it burned
    // a key per message and nothing ever restocked the shelf.
    //
    // And pruned of consumed ids, per §16.11's table: republishing the full
    // list re-advertised keys the peer had already spent, their client took
    // the refresh as the spec commands, sealed the next message to a key this
    // side had consumed, and the read refused it as a replay. Observed live,
    // one message after the first bill.
    let (_, mut bundle) = prekeys(0x80);
    let used = used_one_time_ids();
    bundle.one_time.retain(|k| !used.contains(&k.id));
    let head = LogHead {
        version: 1,
        suite: 1,
        next_seq: seq + 1,
        prekey_bundle: Some(bundle.to_value().encode()),
        read_up_to: None,
        // Eight is encoded by omission (§18.1); anything else travels.
        ring: if ring == 8 { None } else { Some(ring) },
    };
    rc.set_dht_value(log.clone(), 0, head.to_value().encode(), None)
        .await?;
    Ok(())
}

pub async fn issue() -> Result<(), Box<dyn std::error::Error>> {
    println!("\n\x1b[1mDUCAT — issue a contact card (§16.12)\x1b[0m\n");
    let (api, _c) = crate::veilid::start("issue").await?;
    let rc = api.routing_context()?;

    let persona = SecretKey::ed25519_from_bytes(&[0x71; 32]);
    let all = api.crypto()?;
    let crypto = all.get(CRYPTO_KIND_VLD0).ok_or("no VLD0")?;
    let writer = crypto.generate_keypair();

    let t0 = Instant::now();
    let member = BareMemberId::new(&writer.value().key().bytes());
    let inbox = rc
        .create_dht_record(
            CRYPTO_KIND_VLD0,
            DHTSchema::smpl(1, vec![DHTSchemaSMPLMember { m_key: member, m_cnt: 1 }])?,
            None,
        )
        .await?;
    let (outbox, _outbox_kp) = make_log(&rc).await?;
    println!("  inbox    {}", inbox.key());
    println!("  outbox   {}", outbox);
    println!("  built    in {} ms", t0.elapsed().as_millis());

    let (_, bundle) = prekeys(0x40);
    let details = ContactDetails {
        version: 1,
        suite: 1,
        persona: persona.public().to_bytes().to_vec(),
        outbox_key: outbox.to_string(),
        prekey_bundle: bundle.to_value().encode(),
        display_name: Some("kara".into()),
        // Off unless asked, which is §16.12's rule: publishing is the
        // contact's own choice about their own linkability.
        payto: std::env::var("DUCAT_PAYTO").ok().filter(|v| !v.is_empty()),
        avatar: None, email: None, phone: None, signal: None, pronouns: None,
        car_model: None,
        car_color: None,
        plate: None,
    };
    rc.set_dht_value(inbox.key().clone(), 0, details.to_value().encode(), None)
        .await?;
    println!("  wrote    subkey 0 — who I am and where to leave things");

    let card = ContactCard {
        version: 1,
        suite: 1,
        persona: persona.public().to_bytes().to_vec(),
        inbox_key: inbox.key().to_string(),
        writer_public: writer.value().key().bytes().to_vec(),
        display_name: Some("kara".into()),
        expiry: crate::payee::now() + 86_400,
    };
    let env = seal_env(
        &SignedBytes::from_received(card.to_value().encode()).unwrap(),
        ObjectType::ContactOffer,
        &persona,
    );
    let wsec: [u8; 32] = writer.value().secret().bytes().as_ref().try_into()?;
    let uri = card_to_uri(&env, &wsec);

    std::fs::write(
        state_path("issuer"),
        format!(
            "{}\n{}\n{}\n{}\n",
            inbox.key(),
            outbox,
            hex::encode(writer.value().key().bytes()),
            hex::encode(writer.value().secret().bytes())
        ),
    )?;

    // Closed and gone. Everything the claimant needs is now in the network.
    rc.close_dht_record(inbox.key().clone()).await?;
    rc.close_dht_record(outbox).await?;
    println!("\n  card ({} chars):\n{}\n", uri.len(), uri);
    println!("  claimant runs:  --card-claim '<that card>'");
    println!("  then issuer:    --card-collect\n");
    api.shutdown().await;
    Ok(())
}

pub async fn claim(uri: &str) -> Result<(), Box<dyn std::error::Error>> {
    use std::str::FromStr;
    println!("\n\x1b[1mDUCAT — claim a card (issuer offline)\x1b[0m\n");

    let (env, wsec) = card_from_uri(uri).map_err(|e| format!("{e:?}"))?;
    // Persona out of the payload first, then verify under it: a card that
    // verifies under some *other* key is one claiming an identity it does not
    // hold.
    let peek = ContactCard::from_value(
        decode(&peek_body(&env).map_err(|e| format!("{e:?}"))?).map_err(|e| format!("{e:?}"))?,
    )
    .map_err(|e| format!("{e:?}"))?;
    let their_pk = PublicKey::from_bytes(Suite::Ed25519X25519, &peek.persona)
        .map_err(|e| format!("bad persona: {e:?}"))?;
    let (ty, body) = open_env(&env, &their_pk).map_err(|e| format!("signature: {e:?}"))?;
    if ty != ObjectType::ContactOffer {
        return Err("not a contact card".into());
    }
    let card = ContactCard::from_value(decode(body.bytes()).map_err(|e| format!("{e:?}"))?)
        .map_err(|e| format!("{e:?}"))?;
    println!("  from     {} (unverified — self-asserted)",
             card.display_name.as_deref().unwrap_or("(no name)"));
    if crate::payee::now() > card.expiry {
        return Err("that card has expired".into());
    }

    let (api, _c) = crate::veilid::start("claim").await?;
    let rc = api.routing_context()?;

    let kp = KeyPair::new(
        CRYPTO_KIND_VLD0,
        BareKeyPair::new(
            BarePublicKey::new(&card.writer_public),
            BareSecretKey::new(&wsec),
        ),
    );
    let inbox = RecordKey::from_str(&card.inbox_key).map_err(|e| format!("inbox key: {e}"))?;
    rc.open_dht_record(inbox.clone(), Some(kp)).await?;

    // §16.9's single use, checked by *reading* rather than trusting a flag.
    if rc.get_dht_value(inbox.clone(), 1, true).await?.map(|v| !v.data().is_empty()).unwrap_or(false) {
        return Err("that card has already been claimed".into());
    }

    let t0 = Instant::now();
    let theirs = match rc.get_dht_value(inbox.clone(), 0, true).await? {
        Some(v) => ContactDetails::from_value(decode(v.data()).map_err(|e| format!("{e:?}"))?)
            .map_err(|e| format!("{e:?}"))?,
        None => return Err("subkey 0 empty — the issuer never published their details".into()),
    };
    println!("  read 0   in {} ms — outbox {}", t0.elapsed().as_millis(), theirs.outbox_key);

    let persona = SecretKey::ed25519_from_bytes(&[0x72; 32]);
    let (outbox, outbox_kp) = make_log(&rc).await?;
    let (_, bundle) = prekeys(0x80);
    let mine = ContactDetails {
        version: 1,
        suite: 1,
        persona: persona.public().to_bytes().to_vec(),
        outbox_key: outbox.to_string(),
        prekey_bundle: bundle.to_value().encode(),
        display_name: Some("desktop".into()),
        payto: std::env::var("DUCAT_PAYTO").ok().filter(|v| !v.is_empty()),
        avatar: None, email: None, phone: None, signal: None, pronouns: None,
        car_model: None,
        car_color: None,
        plate: None,
    };
    rc.set_dht_value(inbox.clone(), 1, mine.to_value().encode(), None).await?;
    println!("  wrote    subkey 1 — the handshake is complete");

    // And say something, sealed to prekeys read out of the record rather than
    // fetched from a peer who is not there.
    let their_bundle =
        PreKeyBundle::from_value(decode(&theirs.prekey_bundle).map_err(|e| format!("{e:?}"))?)
            .map_err(|e| format!("{e:?}"))?;
    let aad = thread_aad(
        &hex::encode(persona.public().to_bytes()),
        &hex::encode(&theirs.persona),
    );
    let mut prev = [0u8; 32];
    let script: Vec<String> = std::env::var("DUCAT_SAY")
        .ok()
        .filter(|v| !v.is_empty())
        .map(|v| v.split('|').map(|s| s.to_string()).collect())
        .unwrap_or_else(|| {
            vec![
                "hello from a process that is about to exit".into(),
                "and a second one".into(),
            ]
        });
    for (seq, text) in script
        .iter()
        .enumerate()
    {
        let m = Message {
            version: 1,
            suite: 1,
            seq: seq as u64,
            prev,
            body: text.clone(),
            timestamp: crate::payee::now(),
            kind: MessageKind::Text,
            items: Vec::new(),
            tax_pxmr: None,
            re_seq: None,
            re_own: false,
            eta_secs: None,
            attachment: None,
            amount_pxmr: None,
            txid: None,
            payto: None,
        };
        prev = m.link();
        let (chosen, fs) = {
            let mut b = their_bundle.clone();
            b.one_time.retain(|k| k.id as usize > seq);
            b.select()
        };
        if !fs {
            println!("  \x1b[33m!\x1b[0m one-time keys exhausted — no forward secrecy");
        }
        let mut rng = HarnessRng(0x5A ^ seq as u8);
        let (enc, ct) = hpke::seal(&mut rng, &chosen.public, &hpke::message_info(1), &aad,
                                   &m.to_value().encode())
            .map_err(|e| format!("seal: {e:?}"))?;
        let sealed = SealedMessage { version: 1, suite: 1, prekey_id: chosen.id, enc, ciphertext: ct };
        append(&rc, &outbox, seq as u64, &sealed.to_value().encode(), LOG_SUBKEYS).await?;
        println!("  \x1b[35m→\x1b[0m [{seq}] {text}");
    }

    // Their log *and* their persona. Only the outbox was kept, and the persona
    // is half of the thread AAD — so reading their messages in a later process
    // meant re-deriving it from the card, and a card is a one-shot thing handed
    // over in person. Keeping one line instead of two made every future read
    // depend on still having the URI.
    // **Theirs first.** This wrote our own outbox in that slot, and `watch`
    // reads slot one as the log to poll — so it sat decrypting our own
    // outgoing messages with the receiving keys and reported BadSig on every
    // one of them. Two record keys in scope named almost the same thing, and
    // the wrong one is not a decode error anywhere: it is a valid record full
    // of valid ciphertext that simply is not addressed to us.
    std::fs::write(
        state_path("claimant"),
        format!(
            "{}\n{}\n{}\n{}\n{}\n",
            theirs.outbox_key,          // the log we read
            hex::encode(&theirs.persona),
            outbox,                     // the log we write
            format!(
                "{}:{}",
                hex::encode(outbox_kp.value().key().bytes()),
                hex::encode(outbox_kp.value().secret().bytes()),
            ),
            // The chain link of the last message sent above. Without this
            // line, `say` cannot prove its next message follows the ones the
            // claim already wrote — observed live: the first hail's thread
            // was stranded one command after it opened.
            hex::encode(prev),
        ),
    )?;
    rc.close_dht_record(inbox).await?;
    rc.close_dht_record(outbox).await?;
    println!("\n  \x1b[32mclaimed and sent, with the issuer offline throughout\x1b[0m");
    api.shutdown().await;
    Ok(())
}

/// Bill the thread: one itemised PAYMENT_REQUEST (§16.13, §15.11).
///
/// `items` is "desc=pxmr,desc=pxmr"; the amount is their sum, because the sum
/// rule is checked by every conformant reader and a bill that does not add up
/// is refused. `payto` rides in the request itself (§16.13's self-contained
/// destination).
pub async fn bill(items_arg: &str, payto: &str, body: &str) -> Result<(), Box<dyn std::error::Error>> {
    // An empty destination is no destination. Encoding Some("") put an empty
    // text field on the wire, which §18.1 refuses on decode — the receiver's
    // queue wedged on a message this side should never have sealed.
    let payto = Some(payto.to_string()).filter(|p| !p.is_empty());
    send_money_message(items_arg, payto, MessageKind::PaymentRequest, None,
        if body.is_empty() { "Your bill" } else { body }).await
}

/// Receipt for what actually arrived (§16.13): same shape as a bill, kind 3,
/// tip as a visible line so the sum rule holds over the amount received.
/// `txid_hex` names the transaction it acknowledges — the field that lets the
/// payer's Activity screen staple this paperwork to the chain event.
pub async fn receipt(items_arg: &str, txid_hex: &str) -> Result<(), Box<dyn std::error::Error>> {
    send_money_message(items_arg, None, MessageKind::Receipt,
        if txid_hex.is_empty() { None } else { Some(hex::decode(txid_hex)?) },
        "Receipt — thank you").await
}

/// Everything a non-text message might carry; text is `None` of this.
pub struct Outgoing {
    pub items: Vec<(String, u64)>,
    pub amount: Option<u64>,
    pub payto: Option<String>,
    pub kind: MessageKind,
    pub txid: Option<Vec<u8>>,
    pub re_seq: Option<u64>,
    pub re_own: bool,
    pub eta_secs: Option<u64>,
}

async fn send_money_message(
    items_arg: &str,
    payto: Option<String>,
    kind: MessageKind,
    txid: Option<Vec<u8>>,
    body: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let items: Vec<(String, u64)> = items_arg
        .split(',')
        .filter(|s| !s.is_empty())
        .map(|kv| {
            let (d, a) = kv.split_once('=').ok_or("items are desc=pxmr,desc=pxmr")?;
            Ok::<_, Box<dyn std::error::Error>>((d.to_string(), a.parse::<u64>()?))
        })
        .collect::<Result<_, _>>()?;
    if items.is_empty() {
        return Err("a bill needs at least one line".into());
    }
    let total: u64 = items.iter().map(|(_, a)| a).sum();
    bill_or_say(Some(Outgoing {
        items, amount: Some(total), payto, kind, txid,
        re_seq: None, re_own: false, eta_secs: None,
    }), body).await
}

/// §15.12's ceremony, from the desk: the driver's terms after a claim.
pub async fn ride_offer(fare: &str, eta: &str, body: &str) -> Result<(), Box<dyn std::error::Error>> {
    bill_or_say(Some(Outgoing {
        items: Vec::new(), amount: Some(fare.parse()?), payto: None,
        kind: MessageKind::RideOffer, txid: None,
        re_seq: None, re_own: false, eta_secs: Some(eta.parse()?),
    }), if body.is_empty() { "On my way" } else { body }).await
}

/// The rider's yes: names the offer, echoes its fare.
pub async fn ride_accept(offer_seq: &str, fare: &str) -> Result<(), Box<dyn std::error::Error>> {
    bill_or_say(Some(Outgoing {
        items: Vec::new(), amount: Some(fare.parse()?), payto: None,
        kind: MessageKind::RideAccept, txid: None,
        re_seq: Some(offer_seq.parse()?), re_own: false, eta_secs: None,
    }), "See you there").await
}

/// Withdraw a message: our own earlier one (`own`), or decline theirs.
pub async fn retract(seq: &str, own: &str, body: &str) -> Result<(), Box<dyn std::error::Error>> {
    bill_or_say(Some(Outgoing {
        items: Vec::new(), amount: None, payto: None,
        kind: MessageKind::Retract, txid: None,
        re_seq: Some(seq.parse()?), re_own: own == "1" || own == "own",
        eta_secs: None,
    }), if body.is_empty() { "withdrawn" } else { body }).await
}

/// Say something in a thread that already exists.
///
/// The card is single use by design — it is the handshake, not the channel —
/// so without this the harness could read a contact's messages forever and
/// never answer one. Their current prekeys come from the head of their own log
/// (§16.12's refresh), which is exactly what that field is for: no round trip
/// to a peer who may not be there.
pub async fn say(text: &str) -> Result<(), Box<dyn std::error::Error>> {
    if text.is_empty() {
        return Err("nothing to say — pass the message after --say".into());
    }
    bill_or_say(None, text).await
}

async fn bill_or_say(
    billing: Option<Outgoing>,
    text: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    use std::str::FromStr;
    println!("\n\x1b[1mDUCAT — {}\x1b[0m\n", if billing.is_some() { "bill" } else { "say" });
    let st = std::fs::read_to_string(state_path("claimant"))
        .map_err(|_| "no claimed contact on this machine")?;
    let mut it = st.lines();
    let their_log = it.next().unwrap_or_default().to_string();
    let their_persona = hex::decode(it.next().unwrap_or_default())?;
    let my_log = it.next().unwrap_or_default().to_string();
    if my_log.is_empty() {
        return Err("this contact predates the outbox being kept; re-claim a card".into());
    }

    let (api, _c) = crate::veilid::start("say").await?;
    let rc = api.routing_context()?;
    let theirs = RecordKey::from_str(&their_log)?;
    rc.open_dht_record(theirs.clone(), None).await?;

    // Their published keys, and our own next sequence number, both live in the
    // head of a log rather than in anything we stored.
    let their_head = match rc.get_dht_value(theirs.clone(), 0, true).await? {
        Some(v) => LogHead::from_value(decode(v.data()).map_err(|e| format!("{e:?}"))?)
            .map_err(|e| format!("{e:?}"))?,
        None => return Err("their log has no head".into()),
    };
    // §16.12 refreshes the bundle on every head write, which is what makes a
    // reply possible without asking them for anything.
    let raw = their_head
        .prekey_bundle
        .ok_or("their log head carries no prekeys — they have not written since claiming")?;
    let their_bundle =
        PreKeyBundle::from_value(decode(&raw).map_err(|e| format!("{e:?}"))?)
            .map_err(|e| format!("{e:?}"))?;

    let mine = RecordKey::from_str(&my_log)?;
    let owner = it.next().unwrap_or_default();
    let (pk_hex, sk_hex) = owner
        .split_once(':')
        .ok_or("this contact predates the log key being kept; re-claim a fresh card")?;
    let kp = KeyPair::new(
        CRYPTO_KIND_VLD0,
        BareKeyPair::new(
            BarePublicKey::new(&hex::decode(pk_hex)?),
            BareSecretKey::new(&hex::decode(sk_hex)?),
        ),
    );
    rc.open_dht_record(mine.clone(), Some(kp)).await?;
    let head = match rc.get_dht_value(mine.clone(), 0, true).await? {
        Some(v) => LogHead::from_value(decode(v.data()).map_err(|e| format!("{e:?}"))?)
            .map_err(|e| format!("{e:?}"))?,
        None => return Err("our own log has no head".into()),
    };
    let seq = head.next_seq;
    let my_ring = head.ring.unwrap_or(8);

    let persona = SecretKey::ed25519_from_bytes(&[0x72; 32]);
    let aad = thread_aad(
        &hex::encode(persona.public().to_bytes()),
        &hex::encode(&their_persona),
    );
    // The previous message's link, from state. Zero only opens a thread.
    let stored_link = it.next().unwrap_or_default().to_string();
    let prev: [u8; 32] = if seq == 0 {
        [0u8; 32]
    } else if let Ok(b) = hex::decode(&stored_link) {
        match b.try_into() {
            Ok(l) => l,
            Err(_) => return Err("stored chain link is not 32 bytes; re-claim a fresh card".into()),
        }
    } else {
        return Err(
            "this thread predates link persistence — the harness can read it but \
             cannot speak in it; re-claim a fresh card"
                .into(),
        )
    };

    let m = match &billing {
        None => Message {
            version: 1, suite: 1, seq, prev,
            body: text.to_string(),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_secs(),
            kind: MessageKind::Text,
            amount_pxmr: None, txid: None, payto: None,
            items: Vec::new(), tax_pxmr: None, re_seq: None, re_own: false, eta_secs: None, attachment: None,
        },
        Some(o) => Message {
            version: 1, suite: 1, seq, prev,
            body: text.to_string(),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_secs(),
            kind: o.kind,
            amount_pxmr: o.amount,
            txid: o.txid.clone(),
            payto: o.payto.clone(),
            items: o.items.iter().map(|(d, a)| LineItem {
                description: d.clone(), amount_pxmr: *a,
            }).collect(),
            tax_pxmr: None,
            re_seq: o.re_seq,
            re_own: o.re_own,
            eta_secs: o.eta_secs,
            attachment: None,
        },
    };
    // DUCAT_SIGNED_ONLY: seal to the signed prekey even with one-time keys
    // advertised. The escape hatch for exactly what the field showed: replicas
    // served this node heads up to half an hour stale, so every one-time id it
    // picked was long burned on the phone. The signed key never burns; the
    // recipient renders the open lock, which is the honest trade (§16.11).
    let their_bundle = if std::env::var("DUCAT_SIGNED_ONLY").is_ok() {
        let mut b = their_bundle.clone();
        b.one_time.clear();
        b
    } else {
        // Never an id we already spent, however stale the head we fetched.
        let used = used_their_ids();
        let mut b = their_bundle.clone();
        b.one_time.retain(|k| !used.contains(&k.id));
        b
    };
    let (chosen, fs) = their_bundle.select();
    if fs {
        record_used_their_id(chosen.id);
    }
    if !fs {
        println!("  \x1b[33m!\x1b[0m one-time keys exhausted — no forward secrecy");
    }
    let mut rng = HarnessRng(0xC3 ^ seq as u8);
    let (enc, ct) = hpke::seal(&mut rng, &chosen.public, &hpke::message_info(1), &aad,
                               &m.to_value().encode())
        .map_err(|e| format!("seal: {e:?}"))?;
    let sealed = SealedMessage { version: 1, suite: 1, prekey_id: chosen.id, enc, ciphertext: ct };
    append(&rc, &mine, seq, &sealed.to_value().encode(), my_ring).await?;
    match &billing {
        None => println!("  \x1b[35m→\x1b[0m [{seq}] {text}"),
        Some(o) => {
            for (d, a) in &o.items { println!("  \x1b[35m→\x1b[0m   {d}  {a} pXMR"); }
            println!(
                "  \x1b[35m→\x1b[0m [{seq}] {:?}{}",
                o.kind,
                o.amount.map(|t| format!(" total {t} pXMR")).unwrap_or_default(),
            );
        }
    }

    // Move the stored chain link forward, or the *next* say is the stranded
    // one. Read-modify-write of the whole file: the first four lines are
    // identity and do not change.
    let st = std::fs::read_to_string(state_path("claimant"))?;
    let kept: Vec<&str> = st.lines().take(4).collect();
    std::fs::write(
        state_path("claimant"),
        format!("{}\n{}\n", kept.join("\n"), hex::encode(m.link())),
    )?;

    // "Delivered" is a claim about the network, not about this process's
    // memory — and an inspect scope only measures, it does not push. The one
    // confirmation that cannot lie: read the slot back from the network and
    // compare the bytes to what was sent.
    let sent_bytes = sealed.to_value().encode();
    let slot = subkey_for(seq, my_ring);
    print!("  confirming slot {slot} on the network");
    let deadline = Instant::now() + std::time::Duration::from_secs(240);
    let mut confirmed = false;
    while Instant::now() < deadline {
        if let Ok(Some(v)) = rc.get_dht_value(mine.clone(), slot, true).await {
            if v.data() == &sent_bytes[..] {
                confirmed = true;
                break;
            }
        }
        print!(".");
        use std::io::Write as _;
        let _ = std::io::stdout().flush();
        tokio::time::sleep(std::time::Duration::from_secs(10)).await;
    }
    println!();
    if confirmed {
        println!("  \x1b[32mthe network holds this message\x1b[0m");
    } else {
        println!("  \x1b[31mNOT confirmed after 240s\x1b[0m");
    }

    rc.close_dht_record(theirs).await?;
    rc.close_dht_record(mine).await?;
    api.shutdown().await;
    Ok(())
}

/// Rewrite our log head with the current sequence and a full prekey bundle.
///
/// The head is the only place a peer's client refreshes our keys from
/// (§16.12), and this harness — mostly offline — left it carrying None, so a
/// phone talking to us could only ever run its cached supply down to the
/// signed-prekey fallback and the open lock. This restocks without sending:
/// same `next_seq`, fresh bundle, no chain involvement, safe at any time.
pub async fn refresh_keys() -> Result<(), Box<dyn std::error::Error>> {
    use std::str::FromStr;
    println!("\n\x1b[1mDUCAT — republish prekeys\x1b[0m\n");
    let st = std::fs::read_to_string(state_path("claimant"))
        .map_err(|_| "no claimed contact on this machine")?;
    let mut it = st.lines();
    let _their_log = it.next();
    let _persona = it.next();
    let my_log = it.next().unwrap_or_default().to_string();
    let owner = it.next().unwrap_or_default().to_string();
    let (pk_hex, sk_hex) = owner
        .split_once(':')
        .ok_or("this contact predates the log key being kept; re-claim a fresh card")?;
    let kp = KeyPair::new(
        CRYPTO_KIND_VLD0,
        BareKeyPair::new(
            BarePublicKey::new(&hex::decode(pk_hex)?),
            BareSecretKey::new(&hex::decode(sk_hex)?),
        ),
    );

    let (api, _c) = crate::veilid::start("refresh").await?;
    let rc = api.routing_context()?;
    let mine = RecordKey::from_str(&my_log)?;
    rc.open_dht_record(mine.clone(), Some(kp)).await?;
    let next = match rc.get_dht_value(mine.clone(), 0, true).await? {
        Some(v) => LogHead::from_value(decode(v.data()).map_err(|e| format!("{e:?}"))?)
            .map_err(|e| format!("{e:?}"))?
            .next_seq,
        None => 0,
    };
    let (_, bundle) = prekeys(0x80);
    let head = LogHead {
        version: 1,
        suite: 1,
        next_seq: next,
        prekey_bundle: Some(bundle.to_value().encode()),
        read_up_to: None,
        ring: None,
    };
    rc.set_dht_value(mine.clone(), 0, head.to_value().encode(), None).await?;
    println!(
        "  head rewritten: next_seq {next} unchanged, {ONE_TIME_KEYS} one-time keys published"
    );
    rc.close_dht_record(mine).await?;
    api.shutdown().await;
    Ok(())
}

pub async fn collect(card: &str) -> Result<(), Box<dyn std::error::Error>> {
    use std::str::FromStr;
    println!("\n\x1b[1mDUCAT — collect (claimant now offline)\x1b[0m\n");
    // The state file is a convenience, not the source of truth: the inbox key
    // is inside the card we handed out, and reading a claimant's reply needs
    // nothing else. Depending on the file alone meant a card issued from a
    // machine whose /tmp had since been cleared could never be collected on —
    // even though the record was still there and still readable.
    let inbox = if card.is_empty() {
        let st = std::fs::read_to_string(state_path("issuer"))
            .map_err(|_| "no issuer state here — pass the card URI you handed out")?;
        RecordKey::from_str(st.lines().next().ok_or("no inbox in state")?)?
    } else {
        let (env, _) = card_from_uri(card).map_err(|e| format!("{e:?}"))?;
        let c = ContactCard::from_value(
            decode(&peek_body(&env).map_err(|e| format!("{e:?}"))?)
                .map_err(|e| format!("{e:?}"))?,
        )
        .map_err(|e| format!("{e:?}"))?;
        RecordKey::from_str(&c.inbox_key)?
    };

    let (api, _c) = crate::veilid::start("collect").await?;
    let rc = api.routing_context()?;
    rc.open_dht_record(inbox.clone(), None).await?;

    let theirs = match rc.get_dht_value(inbox.clone(), 1, true).await? {
        Some(v) => ContactDetails::from_value(decode(v.data()).map_err(|e| format!("{e:?}"))?)
            .map_err(|e| format!("{e:?}"))?,
        None => return Err("nobody has claimed this card".into()),
    };
    println!("  claimed by {} — outbox {}",
             theirs.display_name.as_deref().unwrap_or("(no name)"), theirs.outbox_key);

    let log = RecordKey::from_str(&theirs.outbox_key)?;
    rc.open_dht_record(log.clone(), None).await?;
    let head = match rc.get_dht_value(log.clone(), 0, true).await? {
        Some(v) => LogHead::from_value(decode(v.data()).map_err(|e| format!("{e:?}"))?)
            .map_err(|e| format!("{e:?}"))?,
        None => return Err("their outbox has no head".into()),
    };
    println!("  head     next_seq = {}\n", head.next_seq);

    let persona = SecretKey::ed25519_from_bytes(&[0x71; 32]);
    let (mut store, _) = prekeys(0x40);
    let aad = thread_aad(
        &hex::encode(persona.public().to_bytes()),
        &hex::encode(&theirs.persona),
    );

    let mut prev: Option<Message> = None;
    for seq in 0..head.next_seq {
        if !still_in_ring(seq, head.next_seq, head.ring.unwrap_or(LOG_SUBKEYS)) {
            println!("  \x1b[33m[{seq}] gone — the ring wrapped past it\x1b[0m");
            continue;
        }
        let raw = match rc.get_dht_value(log.clone(), subkey_for(seq, head.ring.unwrap_or(LOG_SUBKEYS)), true).await? {
            Some(v) => v.data().to_vec(),
            None => { println!("  \x1b[31m[{seq}] slot empty\x1b[0m"); continue }
        };
        let sealed = SealedMessage::from_value(decode(&raw).map_err(|e| format!("{e:?}"))?)
            .map_err(|e| format!("{e:?}"))?;
        let (plain, one_time) = store
            .open_and_consume(&sealed, &hpke::message_info(1), &aad)
            .map_err(|e| format!("open {seq}: {e:?}"))?;
        let m = Message::from_value(decode(&plain).map_err(|e| format!("{e:?}"))?)
            .map_err(|e| format!("{e:?}"))?;
        check_message(&m, seq, prev.as_ref()).map_err(|e| format!("chain at {seq}: {e:?}"))?;
        let mark = if one_time { "" } else { " \x1b[33m(no forward secrecy)\x1b[0m" };
        println!("  \x1b[36m←\x1b[0m [{}] {}{}", m.seq, m.body, mark);
        prev = Some(m);
    }

    rc.close_dht_record(inbox).await?;
    rc.close_dht_record(log).await?;
    println!("\n  \x1b[32mfull round trip, and the two were never online together\x1b[0m");
    api.shutdown().await;
    Ok(())
}


/// Poll a contact's outbox, as the claimant, until interrupted.
///
/// The claimant half of `--card-collect`: same reads, but the outbox and
/// persona come from the card's inbox rather than a state file, so this works
/// against a card issued by anything — including a phone.
/// Forensics: what does the network actually hold in *my* log record?
/// Prints the head, the advertised ring, and every subkey's occupant
/// (sealed prekey id) alongside local/network value sequence numbers.
pub async fn peek_own() -> Result<(), Box<dyn std::error::Error>> {
    use std::str::FromStr;
    println!("\n\x1b[1mDUCAT — my own log, as the network sees it\x1b[0m\n");
    let st = std::fs::read_to_string(state_path("claimant"))?;
    let mut it = st.lines();
    let _their_log = it.next().unwrap_or_default();
    let _their_persona = it.next().unwrap_or_default();
    let my_log = it.next().unwrap_or_default().to_string();
    println!("  record {my_log}");

    let (api, _c) = crate::veilid::start("peek").await?;
    let rc = api.routing_context()?;
    let mine = RecordKey::from_str(&my_log)?;
    let desc = rc.open_dht_record(mine.clone(), None).await?;
    println!("  schema reports {} subkeys", desc.schema().max_subkey());

    match rc.get_dht_value(mine.clone(), 0, true).await? {
        Some(v) => {
            let h = LogHead::from_value(decode(v.data()).map_err(|e| format!("{e:?}"))?)
                .map_err(|e| format!("{e:?}"))?;
            println!("  head: next_seq={} ring={:?} (value_seq {})", h.next_seq, h.ring, v.seq());
        }
        None => println!("  head: none"),
    }

    let report = rc
        .inspect_dht_record(mine.clone(), None, veilid_core::DHTReportScope::SyncGet)
        .await?;
    println!("  inspect(NetworkGet): {} local seqs, {} network seqs",
        report.local_seqs().len(), report.network_seqs().len());
    for (i, (l, n)) in report.local_seqs().iter().zip(report.network_seqs().iter()).enumerate() {
        println!("    subkey {i}: local {l} network {n}");
    }

    for sub in 1u32..=40 {
        match rc.get_dht_value(mine.clone(), sub, true).await {
            Ok(Some(v)) => {
                let pid = SealedMessage::from_value(decode(v.data()).map_err(|e| format!("{e:?}"))?)
                    .map(|sm| sm.prekey_id.to_string())
                    .unwrap_or_else(|_| "?".into());
                println!("  slot {sub}: {} bytes, prekey {}, value_seq {}", v.data().len(), pid, v.seq());
            }
            Ok(None) => println!("  slot {sub}: empty"),
            Err(e) => { println!("  slot {sub}: {e}"); break; }
        }
    }
    api.shutdown().await;
    Ok(())
}

pub async fn watch(uri: &str) -> Result<(), Box<dyn std::error::Error>> {
    use std::str::FromStr;
    println!("\n\x1b[1mDUCAT — watching their outbox\x1b[0m\n");

    // No card given: pick up where `--card-claim` left off. A card is handed
    // over once, in person; needing it again to read a thread that already
    // exists is the wrong dependency.
    if uri.is_empty() {
        let st = std::fs::read_to_string(state_path("claimant"))
            .map_err(|_| "no claimed contact on this machine — pass a card URI")?;
        let mut it = st.lines();
        let outbox = it.next().unwrap_or_default().to_string();
        let persona_hex = it.next().unwrap_or_default().to_string();
        if outbox.is_empty() || persona_hex.is_empty() {
            return Err("this contact was claimed before the persona was kept; pass a card URI".into());
        }
        let persona = hex::decode(&persona_hex).map_err(|e| format!("persona: {e}"))?;
        return watch_log(&outbox, &persona).await;
    }

    let (env, _) = card_from_uri(uri).map_err(|e| format!("{e:?}"))?;
    let peek = ContactCard::from_value(
        decode(&peek_body(&env).map_err(|e| format!("{e:?}"))?).map_err(|e| format!("{e:?}"))?,
    )
    .map_err(|e| format!("{e:?}"))?;
    let their_pk = PublicKey::from_bytes(Suite::Ed25519X25519, &peek.persona)
        .map_err(|e| format!("bad persona: {e:?}"))?;
    let (_, body) = open_env(&env, &their_pk).map_err(|e| format!("signature: {e:?}"))?;
    let card = ContactCard::from_value(decode(body.bytes()).map_err(|e| format!("{e:?}"))?)
        .map_err(|e| format!("{e:?}"))?;

    let (api, _c) = crate::veilid::start("watch").await?;
    let rc = api.routing_context()?;
    let inbox = RecordKey::from_str(&card.inbox_key)?;
    rc.open_dht_record(inbox.clone(), None).await?;
    let theirs = match rc.get_dht_value(inbox.clone(), 0, true).await? {
        Some(v) => ContactDetails::from_value(decode(v.data()).map_err(|e| format!("{e:?}"))?)
            .map_err(|e| format!("{e:?}"))?,
        None => return Err("their details are not published".into()),
    };
    let outbox_key = theirs.outbox_key.clone();
    let their_persona = theirs.persona.clone();
    rc.close_dht_record(inbox).await?;
    api.shutdown().await;
    watch_log(&outbox_key, &their_persona).await
}

/// Poll one contact's log, given only their outbox and persona.
async fn watch_log(outbox_key: &str, their_persona: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
    use std::str::FromStr;
    let (api, _c) = crate::veilid::start("watch").await?;
    let rc = api.routing_context()?;
    let log = RecordKey::from_str(outbox_key)?;
    rc.open_dht_record(log.clone(), None).await?;
    println!("  watching {}\n", outbox_key);

    // The claimant's keys, matching what `--card-claim` published.
    let persona = SecretKey::ed25519_from_bytes(&[0x72; 32]);
    let (mut store, _) = prekeys(0x80);
    let aad = thread_aad(
        &hex::encode(persona.public().to_bytes()),
        &hex::encode(their_persona),
    );

    let mut seq = 0u64;
    let mut prev: Option<Message> = None;
    let deadline = Instant::now() + std::time::Duration::from_secs(240);
    while Instant::now() < deadline {
        let (next, ring) = match rc.get_dht_value(log.clone(), 0, true).await? {
            Some(v) => {
                let h = LogHead::from_value(decode(v.data()).map_err(|e| format!("{e:?}"))?)
                    .map_err(|e| format!("{e:?}"))?;
                // §16.12: the ring comes from the head, never from a constant.
                (h.next_seq, h.ring.unwrap_or(LOG_SUBKEYS))
            }
            None => (0, LOG_SUBKEYS),
        };
        while seq < next {
            if !still_in_ring(seq, next, ring) {
                println!("  \x1b[33m[{seq}] gone — the ring wrapped past it\x1b[0m");
                seq += 1;
                prev = None;
                continue;
            }
            let raw = match rc
                .get_dht_value(log.clone(), subkey_for(seq, ring), true)
                .await?
            {
                Some(v) => v.data().to_vec(),
                None => break,
            };
            let sealed = SealedMessage::from_value(decode(&raw).map_err(|e| format!("{e:?}"))?)
                .map_err(|e| format!("{e:?}"))?;
            match store.open_and_consume(&sealed, &hpke::message_info(1), &aad) {
                Ok((plain, one_time)) => {
                    if one_time {
                        record_used_one_time_id(sealed.prekey_id);
                    }
                    let m = Message::from_value(decode(&plain).map_err(|e| format!("{e:?}"))?)
                        .map_err(|e| format!("{e:?}"))?;
                    if let Err(e) = check_message(&m, seq, prev.as_ref()) {
                        println!("  \x1b[31m[{seq}] chain: {:?}\x1b[0m", e.code);
                        break;
                    }
                    let mark = if one_time { "" } else { " \x1b[33m(no forward secrecy)\x1b[0m" };
                    println!("  \x1b[36m←\x1b[0m [{}] {}{}", m.seq, m.body, mark);
                    // A payment message is mostly *not* its text. Printing only
                    // the body threw away the amount, the destination and the
                    // transaction — which is to say it threw away every field
                    // §16.13 added, and made a request unactionable from here.
                    if m.kind != MessageKind::Text {
                        println!(
                            "        \x1b[35m{}\x1b[0m{}",
                            match m.kind {
                                MessageKind::PaymentRequest => "asks for payment",
                                MessageKind::PaymentSent => "says they sent",
                                MessageKind::Receipt => "receipt for",
                                MessageKind::Reaction => "reacted",
                                MessageKind::Retract => "withdraws an earlier message",
                                MessageKind::RideOffer => "offers to drive, fare",
                                MessageKind::RideAccept => "accepts the ride at",
                                MessageKind::Text => unreachable!(),
                            },
                            m.amount_pxmr
                                .map(|a| format!(" {a} pXMR ({:.6} XMR)", a as f64 / 1e12))
                                .unwrap_or_default(),
                        );
                        if let Some(p) = &m.payto {
                            println!("        pay to: {p}");
                        }
                        if let Some(e) = m.eta_secs {
                            println!("        about {} min away", e.div_ceil(60));
                        }
                        if let Some(rs) = m.re_seq {
                            println!("        re: {} message {rs}",
                                if m.re_own { "their own" } else { "our" });
                        }
                        if let Some(t) = &m.txid {
                            println!("        txid:   {}", hex::encode(t));
                        }
                        for i in &m.items {
                            println!(
                                "        {:<28} {:>14.6}",
                                i.description,
                                i.amount_pxmr as f64 / 1e12
                            );
                        }
                        if let Some(t) = m.tax_pxmr {
                            println!("        {:<28} {:>14.6}", "tax", t as f64 / 1e12);
                        }
                    }
                    prev = Some(m);
                    seq += 1;
                }
                Err(e) => {
                    // The phone's policy since the placeholder fix: a message
                    // this reader can no longer open is lost, not a roadblock.
                    // Breaking here re-served the same corpse every pass and
                    // starved every message behind it.
                    println!("  \x1b[31m[{seq}] {:?} — skipping\x1b[0m", e.code);
                    seq += 1;
                    prev = None;
                }
            }
        }
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    }

    rc.close_dht_record(log).await?;
    api.shutdown().await;
    Ok(())
}
