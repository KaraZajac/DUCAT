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

/// Enough subkeys for a head and seven messages. Small on purpose: a ring that
/// wraps during a test is a ring whose wrap is tested.
const LOG_SUBKEYS: u32 = 8;
const ONE_TIME_KEYS: u32 = 4;

fn state_path(who: &str) -> String {
    std::env::var("DUCAT_STATE").unwrap_or_else(|_| format!("/tmp/ducat-{who}.json"))
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

async fn make_log(rc: &RoutingContext) -> Result<RecordKey, Box<dyn std::error::Error>> {
    let desc = rc
        .create_dht_record(CRYPTO_KIND_VLD0, DHTSchema::dflt(LOG_SUBKEYS as u16)?, None)
        .await?;
    let head = LogHead { version: 1, suite: 1, next_seq: 0, prekey_bundle: None };
    rc.set_dht_value(desc.key().clone(), 0, head.to_value().encode(), None)
        .await?;
    Ok(desc.key().clone())
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
) -> Result<(), Box<dyn std::error::Error>> {
    rc.set_dht_value(log.clone(), subkey_for(seq, LOG_SUBKEYS), body.to_vec(), None)
        .await?;
    let head = LogHead { version: 1, suite: 1, next_seq: seq + 1, prekey_bundle: None };
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
    let outbox = make_log(&rc).await?;
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
    let outbox = make_log(&rc).await?;
    let (_, bundle) = prekeys(0x80);
    let mine = ContactDetails {
        version: 1,
        suite: 1,
        persona: persona.public().to_bytes().to_vec(),
        outbox_key: outbox.to_string(),
        prekey_bundle: bundle.to_value().encode(),
        display_name: Some("desktop".into()),
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
    for (seq, text) in ["hello from a process that is about to exit", "and a second one"]
        .iter()
        .enumerate()
    {
        let m = Message {
            version: 1,
            suite: 1,
            seq: seq as u64,
            prev,
            body: (*text).into(),
            timestamp: crate::payee::now(),
            kind: MessageKind::Text,
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
        append(&rc, &outbox, seq as u64, &sealed.to_value().encode()).await?;
        println!("  \x1b[35m→\x1b[0m [{seq}] {text}");
    }

    std::fs::write(state_path("claimant"), format!("{}\n", outbox))?;
    rc.close_dht_record(inbox).await?;
    rc.close_dht_record(outbox).await?;
    println!("\n  \x1b[32mclaimed and sent, with the issuer offline throughout\x1b[0m");
    api.shutdown().await;
    Ok(())
}

pub async fn collect() -> Result<(), Box<dyn std::error::Error>> {
    use std::str::FromStr;
    println!("\n\x1b[1mDUCAT — collect (claimant now offline)\x1b[0m\n");
    let st = std::fs::read_to_string(state_path("issuer"))?;
    let mut lines = st.lines();
    let inbox = RecordKey::from_str(lines.next().ok_or("no inbox in state")?)?;

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
        if !still_in_ring(seq, head.next_seq, LOG_SUBKEYS) {
            println!("  \x1b[33m[{seq}] gone — the ring wrapped past it\x1b[0m");
            continue;
        }
        let raw = match rc.get_dht_value(log.clone(), subkey_for(seq, LOG_SUBKEYS), true).await? {
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
pub async fn watch(uri: &str) -> Result<(), Box<dyn std::error::Error>> {
    use std::str::FromStr;
    println!("\n\x1b[1mDUCAT — watching their outbox\x1b[0m\n");

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
    let log = RecordKey::from_str(&theirs.outbox_key)?;
    rc.open_dht_record(log.clone(), None).await?;
    println!("  watching {}\n", theirs.outbox_key);

    // The claimant's keys, matching what `--card-claim` published.
    let persona = SecretKey::ed25519_from_bytes(&[0x72; 32]);
    let (mut store, _) = prekeys(0x80);
    let aad = thread_aad(
        &hex::encode(persona.public().to_bytes()),
        &hex::encode(&theirs.persona),
    );

    let mut seq = 0u64;
    let mut prev: Option<Message> = None;
    let deadline = Instant::now() + std::time::Duration::from_secs(240);
    while Instant::now() < deadline {
        let next = match rc.get_dht_value(log.clone(), 0, true).await? {
            Some(v) => LogHead::from_value(decode(v.data()).map_err(|e| format!("{e:?}"))?)
                .map_err(|e| format!("{e:?}"))?
                .next_seq,
            None => 0,
        };
        while seq < next {
            if !still_in_ring(seq, next, LOG_SUBKEYS) {
                println!("  \x1b[33m[{seq}] gone — the ring wrapped past it\x1b[0m");
                seq += 1;
                prev = None;
                continue;
            }
            let raw = match rc
                .get_dht_value(log.clone(), subkey_for(seq, LOG_SUBKEYS), true)
                .await?
            {
                Some(v) => v.data().to_vec(),
                None => break,
            };
            let sealed = SealedMessage::from_value(decode(&raw).map_err(|e| format!("{e:?}"))?)
                .map_err(|e| format!("{e:?}"))?;
            match store.open_and_consume(&sealed, &hpke::message_info(1), &aad) {
                Ok((plain, one_time)) => {
                    let m = Message::from_value(decode(&plain).map_err(|e| format!("{e:?}"))?)
                        .map_err(|e| format!("{e:?}"))?;
                    if let Err(e) = check_message(&m, seq, prev.as_ref()) {
                        println!("  \x1b[31m[{seq}] chain: {:?}\x1b[0m", e.code);
                        break;
                    }
                    let mark = if one_time { "" } else { " \x1b[33m(no forward secrecy)\x1b[0m" };
                    println!("  \x1b[36m←\x1b[0m [{}] {}{}", m.seq, m.body, mark);
                    prev = Some(m);
                    seq += 1;
                }
                Err(e) => {
                    println!("  \x1b[31m[{seq}] {:?}\x1b[0m", e.code);
                    break;
                }
            }
        }
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    }

    rc.close_dht_record(inbox).await?;
    rc.close_dht_record(log).await?;
    api.shutdown().await;
    Ok(())
}
