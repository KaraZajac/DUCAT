//! §16.9 / §16.10 over real Veilid routes: a card, a claim, and a thread.
//!
//! Everything else in this harness moves money. This moves an *introduction*,
//! which is the thing that has to work before two people who know each other can
//! use any of the rest of it.
//!
//! The card is written to a file and read by the other process for the same
//! reason the tap blob is: it models an **out-of-band** channel honestly. The
//! difference from a tap is that this channel is not a phone held over another
//! phone — it is Signal, or Discord, or a QR on a screen — and the claimant
//! process genuinely starts knowing nothing but the bytes in that file.
//!
//! # What this proves that the unit tests cannot
//!
//! `core/tests/contact.rs` proves `check_claim` refuses a second claim. It
//! cannot prove that the *issuer* remembers, because single-use is not a
//! property of a function — it is a property of a store that outlives the call.
//! Here the issuer keeps that state across two real round trips from two
//! separate connections, and the second one is refused over the wire.

use std::time::{Duration, Instant};

use ducat_core::cbor::decode;
use ducat_core::contact::*;
use ducat_core::sig::{ObjectType, PublicKey, SecretKey, SignedBytes};
use ducat_core::wire::{open, seal};
use veilid_core::*;

use crate::flow::*;
use crate::payee::now;

const MSG_CLAIM: u8 = 0x40;
const MSG_TEXT: u8 = 0x41;

/// Issuer: mint a card, hand it out, honour exactly one claim, then chat.
pub async fn share(card_path: &str) -> Result<(), Box<dyn std::error::Error>> {
    println!("\n\x1b[1mDUCAT harness — sharing a contact card (§16.9)\x1b[0m\n");

    let (api, mut calls) = crate::veilid::start("issuer").await?;
    let route = api.new_custom_private_route(PrivateSpec::default()).await?;

    let key = SecretKey::ed25519_from_bytes(&[0x21; 32]);
    // A real deployment draws this from the CSPRNG. Fixed here so a failed run
    // is reproducible; the property under test is single use, not entropy.
    let secret = [0x5E; 32];

    let invite = ContactInvite {
        version: 1,
        suite: 1,
        persona: key.public().to_bytes().to_vec(),
        rendezvous: route.blob.clone(),
        display_name: Some("kara".into()),
        claim_commit: claim_commitment(&secret),
        expiry: now() + 600,
    };
    // The secret rides with the card, never with the issuer's record. That is
    // what makes a stolen list of issued invitations useless.
    let env = seal(
        &SignedBytes::from_received(invite.to_value().encode()).unwrap(),
        ObjectType::ContactOffer,
        &key,
    );
    std::fs::write(card_path, &env)?;
    std::fs::write(format!("{card_path}.secret"), hex::encode(secret))?;
    std::fs::write(format!("{card_path}.pk"), hex::encode(key.public().to_bytes()))?;
    println!("  card     {} B written to {card_path}", env.len());
    println!("  name     kara (self-asserted — §16.9 says this is worth what the channel is worth)");
    println!("  waiting for a claim\n");

    // The single-use state. A bool here, a row in the contacts table on a phone.
    let mut claimed = false;
    // The run is only complete once both refusals have actually been exercised.
    // Exiting on message count alone left nobody home for the replay, which is
    // the property this harness exists to demonstrate.
    let mut refused_replay = false;
    let mut refused_forgery = false;
    let mut peer_seq: u64 = 0;
    let mut peer_prev: Option<Message> = None;
    let mut my_seq: u64 = 0;
    let mut my_prev: Option<Message> = None;

    let deadline = Instant::now() + Duration::from_secs(300);
    while Instant::now() < deadline {
        let Ok(Some((id, msg))) =
            tokio::time::timeout(Duration::from_secs(30), calls.recv()).await
        else {
            continue;
        };
        // Never `?` on a decode inside the loop: a malformed frame from anyone
        // who found the route must cost one reply, not the process (§8.7.2).
        let Ok((kind, body)) = unframe(&msg) else {
            api.app_call_reply(id, reject("frame")).await.ok();
            continue;
        };
        match kind {
            MSG_CLAIM => {
                let Ok(v) = decode(body) else {
                    api.app_call_reply(id, reject("claim decode")).await.ok();
                    continue;
                };
                let Ok(claim) = ContactClaim::from_value(v) else {
                    api.app_call_reply(id, reject("claim malformed")).await.ok();
                    continue;
                };
                match check_claim(&invite, &claim, now(), claimed) {
                    Ok(()) => {
                        claimed = true;
                        let who = claim.display_name.as_deref().unwrap_or("(unnamed)");
                        println!("  \x1b[32mclaimed\x1b[0m by {who} — contact is now mutual");
                        println!("           their persona {}…", hex::encode(&claim.persona[..8]));
                        api.app_call_reply(id, b"ok".to_vec()).await.ok();
                    }
                    Err(e) => {
                        refused_replay = true;
                        println!("  \x1b[33mrefused\x1b[0m a claim: {:?} — {}", e.code, e.detail.unwrap_or_default());
                        api.app_call_reply(id, reject(&format!("{:?}", e.code)))
                            .await
                            .ok();
                    }
                }
            }
            MSG_TEXT => {
                let Ok(v) = decode(body) else {
                    api.app_call_reply(id, reject("message decode")).await.ok();
                    continue;
                };
                let Ok(m) = Message::from_value(v) else {
                    api.app_call_reply(id, reject("message malformed")).await.ok();
                    continue;
                };
                match check_message(&m, peer_seq, peer_prev.as_ref()) {
                    Ok(()) => {
                        println!("  \x1b[36m←\x1b[0m [{}] {}", m.seq, m.body);
                        peer_seq += 1;
                        peer_prev = Some(m);
                        // Reply on the same round trip. Veilid gives one reply
                        // per call, so the answer *is* the return channel here.
                        let r = Message {
                            version: 1,
                            suite: 1,
                            seq: my_seq,
                            prev: my_prev.as_ref().map(|p| p.link()).unwrap_or([0u8; 32]),
                            body: match my_seq {
                                0 => "hey — got your card".into(),
                                1 => "yeah, send it whenever".into(),
                                _ => "👍".into(),
                            },
                            timestamp: now(),
                        };
                        println!("  \x1b[35m→\x1b[0m [{}] {}", r.seq, r.body);
                        my_seq += 1;
                        let out = r.to_value().encode();
                        my_prev = Some(r);
                        api.app_call_reply(id, out).await.ok();
                    }
                    Err(e) => {
                        refused_forgery = true;
                        println!("  \x1b[31mrefused\x1b[0m a message: {:?} — {}", e.code, e.detail.unwrap_or_default());
                        api.app_call_reply(id, reject(&format!("{:?}", e.code)))
                            .await
                            .ok();
                    }
                }
            }
            _ => {
                api.app_call_reply(id, reject("unknown kind")).await.ok();
            }
        }
        if claimed && my_seq >= 3 && refused_replay && refused_forgery {
            break;
        }
    }

    if !(claimed && refused_replay && refused_forgery) {
        return Err(format!(
            "incomplete: claimed={claimed} refused_replay={refused_replay} \
             refused_forgery={refused_forgery}"
        )
        .into());
    }
    println!("\n  \x1b[32mdone\x1b[0m — one claim honoured, one replay and one forgery refused");
    api.shutdown().await;
    Ok(())
}

/// Claimant: read a card that arrived out of band, claim it, then talk.
pub async fn claim(card_path: &str) -> Result<(), Box<dyn std::error::Error>> {
    println!("\n\x1b[1mDUCAT harness — claiming a contact card (§16.9)\x1b[0m\n");

    let env = std::fs::read(card_path)?;
    let issuer_pk = PublicKey::from_bytes(
        ducat_core::sig::Suite::Ed25519X25519,
        &hex::decode(std::fs::read_to_string(format!("{card_path}.pk"))?.trim())?,
    )
    .map_err(|e| format!("issuer key: {e:?}"))?;
    // The signature proves the persona key made this card. It does not prove the
    // person who sent it to you is that keyholder — §16.9 is explicit that the
    // channel supplies that, and here the channel is a file.
    let (_, body) = open(&env, &issuer_pk).map_err(|e| format!("card signature: {e:?}"))?;
    let invite = ContactInvite::from_value(
        decode(body.bytes()).map_err(|e| format!("card decode: {e:?}"))?,
    )
    .map_err(|e| format!("card malformed: {e:?}"))?;
    let secret: [u8; 32] = hex::decode(std::fs::read_to_string(format!("{card_path}.secret"))?.trim())?
        .try_into()
        .map_err(|_| "claim secret is not 32 bytes")?;

    println!(
        "  from     {} (unverified — self-asserted)",
        invite.display_name.as_deref().unwrap_or("(unnamed)")
    );
    println!("  expires  in {}s", invite.expiry.saturating_sub(now()));

    let (api, _calls) = crate::veilid::start("claimant").await?;
    let rc = api.routing_context()?;
    let my_route = api.new_custom_private_route(PrivateSpec::default()).await?;
    let route = api.import_remote_private_route(invite.rendezvous.clone())?;

    let key = SecretKey::ed25519_from_bytes(&[0x22; 32]);
    let mk_claim = || ContactClaim {
        version: 1,
        suite: 1,
        persona: key.public().to_bytes().to_vec(),
        rendezvous: my_route.blob.clone(),
        display_name: Some("sam".into()),
        claim_secret: secret,
        timestamp: now(),
    };

    let t0 = Instant::now();
    let r = rc
        .app_call(
            Target::RouteId(route.clone()),
            frame(MSG_CLAIM, &mk_claim().to_value().encode()),
        )
        .await?;
    if r.as_slice() != b"ok" {
        return Err(format!("claim refused: {}", String::from_utf8_lossy(&r)).into());
    }
    println!("  \x1b[32mclaimed\x1b[0m in {} ms\n", t0.elapsed().as_millis());

    // Three round trips of actual conversation, each one a chained message.
    let mut my_seq = 0u64;
    let mut my_prev: Option<Message> = None;
    let mut their_seq = 0u64;
    let mut their_prev: Option<Message> = None;
    for body in ["hey, this is sam", "can you send me the 20 back?", "thanks"] {
        let m = Message {
            version: 1,
            suite: 1,
            seq: my_seq,
            prev: my_prev.as_ref().map(|p| p.link()).unwrap_or([0u8; 32]),
            body: body.into(),
            timestamp: now(),
        };
        println!("  \x1b[35m→\x1b[0m [{}] {}", m.seq, m.body);
        let enc = m.to_value().encode();
        my_seq += 1;
        my_prev = Some(m);

        let reply = rc
            .app_call(Target::RouteId(route.clone()), frame(MSG_TEXT, &enc))
            .await?;
        let rm = Message::from_value(
            decode(&reply).map_err(|e| format!("reply decode: {e:?}"))?,
        )
        .map_err(|e| format!("reply malformed: {e:?}"))?;
        check_message(&rm, their_seq, their_prev.as_ref())
            .map_err(|e| format!("their thread broke: {e:?}"))?;
        println!("  \x1b[36m←\x1b[0m [{}] {}", rm.seq, rm.body);
        their_seq += 1;
        their_prev = Some(rm);
    }

    // The property no unit test can reach: the *issuer* must remember. A fresh
    // claim, well-formed and correctly signed, carrying the right secret — and
    // refused, because the card was already spent.
    println!("\n  replaying the claim (the screenshot case)");
    let r2 = rc
        .app_call(
            Target::RouteId(route.clone()),
            frame(MSG_CLAIM, &mk_claim().to_value().encode()),
        )
        .await?;
    let text = String::from_utf8_lossy(&r2).to_string();
    if r2.as_slice() == b"ok" {
        return Err("second claim was honoured — single use is not enforced".into());
    }
    println!("  \x1b[32mrefused\x1b[0m — {text}");

    // And a substituted message: right sequence, wrong link.
    println!("\n  sending a message with a forged predecessor link");
    let forged = Message {
        version: 1,
        suite: 1,
        seq: my_seq,
        prev: [0x99; 32],
        body: "make that 200".into(),
        timestamp: now(),
    };
    let r3 = rc
        .app_call(
            Target::RouteId(route),
            frame(MSG_TEXT, &forged.to_value().encode()),
        )
        .await?;
    if Message::from_value(decode(&r3).unwrap_or(ducat_core::cbor::Value::Uint(0))).is_ok() {
        return Err("a substituted message was accepted".into());
    }
    println!(
        "  \x1b[32mrefused\x1b[0m — {}",
        String::from_utf8_lossy(&r3)
    );

    println!("\n  \x1b[32mall four properties held over a real route\x1b[0m");
    api.shutdown().await;
    Ok(())
}
