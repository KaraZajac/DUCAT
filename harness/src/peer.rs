//! A desktop peer that talks to the Android app.
//!
//! `contact.rs` proves the protocol between two copies of itself. That is a
//! weaker claim than it looks: two halves written together agree by
//! construction. **This one speaks the app's wire protocol** — the same frame
//! bytes, the same reply strings, the same AAD — which is the only way to find
//! out whether the phone and something that is not the phone can actually talk.
//!
//! It found the first thing immediately. Send and receive in the app both used
//! "the other party's persona" as AAD, which reads correctly on each side and
//! is a different value on each side, so no message could ever have decrypted.
//! Two phones would have failed the same way; nothing in the app's own tests
//! could see it, because both ends shared the mistake.
//!
//!   ducat-harness --peer '<ducat:card/… from the phone>'
//!
//! Claims the card, then stays up: answering the phone's prekey fetches and
//! its messages, and sending anything typed on stdin.

use std::io::BufRead;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ducat_core::cbor::decode;
use ducat_core::contact::*;
use ducat_core::hpke::{self, PreKey, PreKeyBundle, PreKeyStore, SealedMessage};
use ducat_core::sig::{ObjectType, PublicKey, SecretKey, Suite};
use ducat_core::wire::{open as open_env, peek_body};
use veilid_core::*;

use crate::flow::{frame, unframe};
use crate::payee::now;

const MSG_CLAIM: u8 = 0x40;
const MSG_TEXT: u8 = 0x41;
const MSG_PREKEYS: u8 = 0x42;

/// State the answering side needs, shared with the sending side.
struct Peer {
    store: PreKeyStore,
    bundle: PreKeyBundle,
    in_seq: u64,
    in_prev: Option<[u8; 32]>,
    aad: Vec<u8>,
}

pub async fn run(uri: &str) -> Result<(), Box<dyn std::error::Error>> {
    println!("\n\x1b[1mDUCAT — desktop peer for the Android app\x1b[0m\n");

    let (env, claim_secret) = card_from_uri(uri).map_err(|e| format!("{e:?}"))?;

    // Same order the app uses: read the persona out of the payload, then verify
    // the signature under it. A card that verifies under some *other* key is a
    // card claiming an identity it does not hold.
    let peek = ContactInvite::from_value(
        decode(&peek_body(&env).map_err(|e| format!("{e:?}"))?).map_err(|e| format!("{e:?}"))?,
    )
    .map_err(|e| format!("{e:?}"))?;
    let their_pk = PublicKey::from_bytes(Suite::Ed25519X25519, &peek.persona)
        .map_err(|e| format!("bad persona: {e:?}"))?;
    let (ty, body) = open_env(&env, &their_pk).map_err(|e| format!("signature: {e:?}"))?;
    if ty != ObjectType::ContactOffer {
        return Err("not a contact card".into());
    }
    let invite =
        ContactInvite::from_value(decode(body.bytes()).map_err(|e| format!("{e:?}"))?)
            .map_err(|e| format!("{e:?}"))?;

    let their_hex = hex::encode(&invite.persona);
    println!("  from     {} (unverified — self-asserted)",
             invite.display_name.as_deref().unwrap_or("(no name)"));
    println!("  persona  {}…", &their_hex[..16]);
    let left = invite.expiry.saturating_sub(now());
    if left == 0 {
        return Err("that card has expired — make a fresh one on the phone".into());
    }
    println!("  expires  in {} h {} m\n", left / 3600, (left % 3600) / 60);

    let (api, mut calls) = crate::veilid::start("peer").await?;
    let rc = api.routing_context()?;
    let their_blob = invite.rendezvous.clone();
    let my_route = api.new_custom_private_route(PrivateSpec::default()).await?;

    // Fixed so a restarted peer is the same contact on the phone rather than a
    // second one. A real client keeps this in its store.
    let key = SecretKey::ed25519_from_bytes(&[0x71; 32]);
    let my_hex = hex::encode(key.public().to_bytes());
    let aad = thread_aad(&my_hex, &their_hex);

    // Our own prekeys, so the phone can write to us (§16.11).
    let (signed_sk, signed_pk) = hpke::derive_keypair(&[0x81; 32]);
    let mut store = PreKeyStore::new(signed_sk);
    let mut one_time = Vec::new();
    for id in 1u32..=16 {
        let (sk, pk) = hpke::derive_keypair(&[0x90u8.wrapping_add(id as u8); 32]);
        store.insert_one_time(id, sk);
        one_time.push(PreKey { id, public: pk });
    }
    let bundle = PreKeyBundle {
        version: 1, suite: 1, signed_prekey: signed_pk,
        one_time, expiry: now() + 86_400 * 30,
    };

    let claim = ContactClaim {
        version: 1,
        suite: 1,
        persona: key.public().to_bytes().to_vec(),
        rendezvous: my_route.blob.clone(),
        display_name: Some("desktop".into()),
        claim_secret,
        timestamp: now(),
    };

    println!("  claiming the card…");
    let reply = call(&api, &rc, &their_blob, frame(MSG_CLAIM, &claim.to_value().encode()))
        .await
        .map_err(|e| format!("claim: {e}"))?;
    let text = String::from_utf8_lossy(&reply).to_string();
    if text.starts_with("ok") {
        println!("  \x1b[32mclaimed\x1b[0m — you are now a contact on the phone\n");
    } else if text.contains("Replay") {
        // The card was already claimed, which on a restart means *we* claimed
        // it. Sending still works — the receiver matches an inbound ciphertext
        // by AAD against contacts it already has, and needs nothing from us to
        // do that. What is stale is the route *they* hold for us, so their
        // replies will not arrive until we are re-added from a fresh card.
        println!("  \x1b[33malready claimed\x1b[0m — continuing as an existing contact");
        println!("  \x1b[2mtheir route for us is from the previous run, so their");
        println!("  replies will not arrive; sending still works\x1b[0m\n");
    } else {
        return Err(format!("the phone refused: {text}").into());
    }

    let peer = Arc::new(Mutex::new(Peer {
        store, bundle, in_seq: 0, in_prev: None, aad: aad.clone(),
    }));

    // Answering runs on its own task. The phone calls *us* for prekeys before
    // its first message, and a process blocked reading stdin answers nothing.
    let answer = {
        let peer = peer.clone();
        let api = api.clone();
        tokio::spawn(async move {
            while let Some((id, msg)) = calls.recv().await {
                let out = {
                    let mut p = peer.lock().unwrap();
                    handle(&mut p, &msg)
                };
                api.app_call_reply(id, out).await.ok();
            }
        })
    };

    println!("  \x1b[2mType a message and press enter. Ctrl-C to stop.\x1b[0m");
    println!("  \x1b[2mOpen the 'desktop' contact on the phone to send this way.\x1b[0m\n");

    // Outgoing state. The phone's bundle is fetched lazily, the same way the
    // app does it, because it may not have generated prekeys until first asked.
    let mut their_bundle: Option<PreKeyBundle> = None;
    let mut out_seq = 0u64;
    let mut out_prev = [0u8; 32];
    let stdin = std::io::stdin();

    for line in stdin.lock().lines() {
        let line = line?;
        let body = line.trim();
        if body.is_empty() {
            continue;
        }
        if their_bundle.is_none() {
            match call(&api, &rc, &their_blob, frame(MSG_PREKEYS, b"")).await {
                Ok(raw) => match decode(&raw).ok().and_then(|v| PreKeyBundle::from_value(v).ok()) {
                    Some(b) => {
                        println!("  \x1b[2m{} one-time keys available\x1b[0m", b.one_time.len());
                        their_bundle = Some(b);
                    }
                    None => {
                        println!("  \x1b[31mtheir reply was not a prekey bundle:\x1b[0m {}",
                                 String::from_utf8_lossy(&raw));
                        continue;
                    }
                },
                Err(e) => {
                    println!("  \x1b[31mcould not reach the phone:\x1b[0m {e}");
                    continue;
                }
            }
        }
        let b = their_bundle.as_mut().unwrap();
        let msg = Message {
            version: 1, suite: 1, seq: out_seq, prev: out_prev,
            body: body.to_string(), timestamp: now(),
        };
        let link = msg.link();
        let (chosen, fs) = b.select();
        if !fs {
            println!("  \x1b[33m! their one-time keys are exhausted — no forward secrecy\x1b[0m");
        }
        let mut rng = crate::contact::HarnessRng(0x5A ^ (out_seq as u8));
        let (ek, ct) = hpke::seal(&mut rng, &chosen.public, &hpke::message_info(1), &aad,
                                  &msg.to_value().encode())
            .map_err(|e| format!("seal: {e:?}"))?;
        let sealed = SealedMessage {
            version: 1, suite: 1, prekey_id: chosen.id, enc: ek, ciphertext: ct,
        };
        b.one_time.retain(|k| k.id != chosen.id);

        match call(&api, &rc, &their_blob, frame(MSG_TEXT, &sealed.to_value().encode())).await {
            Ok(r) => {
                let t = String::from_utf8_lossy(&r).to_string();
                if t.starts_with("ok") {
                    println!("  \x1b[35m→\x1b[0m [{out_seq}] {body}");
                    out_seq += 1;
                    out_prev = link;
                } else {
                    println!("  \x1b[31mrefused:\x1b[0m {t}");
                }
            }
            Err(e) => println!("  \x1b[31msend failed:\x1b[0m {e}"),
        }
    }

    answer.abort();
    api.shutdown().await;
    Ok(())
}

/// One request, importing the remote route **fresh**.
///
/// Holding a `RouteId` and reusing it looks like the obvious optimisation and
/// does not survive: the first call succeeded and every later one came back
/// `could not get remote private route`, because veilid drops an imported
/// remote route from its table once it is done with it. The Android bridge
/// imports per call and worked throughout, which is what made the asymmetry
/// visible — phone to desktop fine, desktop to phone dead after the first
/// message.
async fn call(
    api: &VeilidAPI,
    rc: &RoutingContext,
    blob: &[u8],
    payload: Vec<u8>,
) -> Result<Vec<u8>, String> {
    let route = api
        .import_remote_private_route(blob.to_vec())
        .map_err(|e| format!("import route: {e}"))?;
    rc.app_call(Target::RouteId(route), payload)
        .await
        .map_err(|e| format!("{e}"))
}

/// Every branch replies. An `app_call` blocks its caller until answered, so
/// dropping a frame we dislike spends the phone's timeout rather than ours.
fn handle(p: &mut Peer, msg: &[u8]) -> Vec<u8> {
    let Ok((kind, body)) = unframe(msg) else {
        return b"!frame".to_vec();
    };
    match kind {
        MSG_PREKEYS => p.bundle.to_value().encode(),
        MSG_TEXT => {
            let Ok(sealed) = decode(body).map_err(|_| ()).and_then(|v| {
                SealedMessage::from_value(v).map_err(|_| ())
            }) else {
                return b"!malformed".to_vec();
            };
            let info = hpke::message_info(1);
            let opened = p.store.open_and_consume(&sealed, &info, &p.aad);
            let Ok((plain, one_time)) = opened else {
                return format!("!{:?}", opened.unwrap_err().code).into_bytes();
            };
            let Ok(m) = decode(&plain).map_err(|_| ()).and_then(|v| {
                Message::from_value(v).map_err(|_| ())
            }) else {
                return b"!body".to_vec();
            };
            // §16.10's chain, against the link we stored rather than the whole
            // previous message.
            if m.seq != p.in_seq || m.prev != p.in_prev.unwrap_or([0u8; 32]) {
                return b"!out of order".to_vec();
            }
            let mark = if one_time { "" } else { " \x1b[33m(no forward secrecy)\x1b[0m" };
            println!("  \x1b[36m←\x1b[0m [{}] {}{}", m.seq, m.body, mark);
            p.in_seq += 1;
            p.in_prev = Some(m.link());
            b"ok".to_vec()
        }
        _ => b"!unknown".to_vec(),
    }
}
