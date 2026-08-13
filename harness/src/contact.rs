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
use ducat_core::hpke::{self, PreKey, PreKeyBundle, PreKeyStore, SealedMessage};
use ducat_core::sig::{ObjectType, PublicKey, SecretKey, SignedBytes};
use ducat_core::wire::{open, seal};
use veilid_core::*;

use crate::flow::*;
use crate::payee::now;

const MSG_CLAIM: u8 = 0x40;
const MSG_TEXT: u8 = 0x41;
const MSG_PREKEYS: u8 = 0x42;

/// Drop a one-time key from the local copy of a bundle once it has been used.
/// A sender that reuses one gets a `STATE_VIOLATION` from the receiver, which is
/// correct but wasteful — the sender already knows it spent that key.
fn bundle_take(b: &mut PreKeyBundle, id: u32) {
    b.one_time.retain(|k| k.id != id);
}

/// A deterministic CSPRNG stand-in, so a failed harness run reproduces exactly.
/// Never appropriate outside a harness, which is why it lives here and not in
/// `core` — `core` holds no randomness at all and takes it as a parameter.
struct HarnessRng(u8);

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

    // §16.11: the receiver's short-lived keys. Three one-time keys and a signed
    // fallback, so the run can show both the consuming path and what happens
    // after the supply is gone.
    let (signed_sk, signed_pk) = hpke::derive_keypair(&[0x31; 32]);
    let mut store = PreKeyStore::new(signed_sk);
    let mut one_time = Vec::new();
    for id in 1u32..=3 {
        let (sk, pk) = hpke::derive_keypair(&[0x40 + id as u8; 32]);
        store.insert_one_time(id, sk);
        one_time.push(PreKey { id, public: pk });
    }
    let bundle = PreKeyBundle {
        version: 1,
        suite: 1,
        signed_prekey: signed_pk,
        one_time,
        expiry: now() + 86_400,
    };
    println!("  prekeys  {} one-time + 1 signed (§16.11)", store.remaining());
    let info = hpke::message_info(1);

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
            MSG_PREKEYS => {
                api.app_call_reply(id, bundle.to_value().encode()).await.ok();
            }
            MSG_TEXT => {
                let Ok(v) = decode(body) else {
                    api.app_call_reply(id, reject("sealed decode")).await.ok();
                    continue;
                };
                let Ok(sealed) = SealedMessage::from_value(v) else {
                    api.app_call_reply(id, reject("sealed malformed")).await.ok();
                    continue;
                };
                let before = store.remaining();
                let opened = store.open_and_consume(&sealed, &info, b"");
                let Ok((plain, was_one_time)) = opened else {
                    let e = opened.unwrap_err();
                    println!("  \x1b[31mrefused\x1b[0m a ciphertext: {:?} (prekeys still {before})", e.code);
                    api.app_call_reply(id, reject(&format!("{:?}", e.code))).await.ok();
                    continue;
                };
                if was_one_time {
                    println!(
                        "           prekey {} consumed — {} left, that ciphertext is now dead",
                        sealed.prekey_id,
                        store.remaining()
                    );
                } else {
                    println!("           \x1b[33msigned prekey used\x1b[0m — no forward secrecy until rotation");
                }
                let Ok(v) = decode(&plain) else {
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

    // §16.11: fetch the recipient's published prekeys before saying anything.
    let raw = rc
        .app_call(Target::RouteId(route.clone()), frame(MSG_PREKEYS, b""))
        .await?;
    let bundle = PreKeyBundle::from_value(
        decode(&raw).map_err(|e| format!("bundle decode: {e:?}"))?,
    )
    .map_err(|e| format!("bundle malformed: {e:?}"))?;
    println!(
        "  prekeys  {} one-time available from kara",
        bundle.one_time.len()
    );
    let mut bundle_state = bundle.clone();
    let info = hpke::message_info(1);
    // Deterministic only so a failed run reproduces. Real senders use OsRng;
    // `core` takes the CSPRNG as a parameter precisely so this choice is the
    // caller's and never buried in the library.
    let mut rng = HarnessRng(0x5A);
    let mut consumed: Vec<(u32, Vec<u8>)> = Vec::new();

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

        // Seal to a one-time key. `select` reports which kind it handed back,
        // and a fallback is surfaced rather than silently accepted (§16.11).
        let (chosen, is_one_time) = bundle_state.select();
        if !is_one_time {
            println!("  \x1b[33m!\x1b[0m one-time keys exhausted — falling back to the signed prekey");
        }
        let (ek, ct) = hpke::seal(&mut rng, &chosen.public, &info, b"", &enc)
            .map_err(|e| format!("seal: {e:?}"))?;
        let sealed = SealedMessage {
            version: 1,
            suite: 1,
            prekey_id: chosen.id,
            enc: ek,
            ciphertext: ct,
        };
        let sealed_bytes = sealed.to_value().encode();
        consumed.push((chosen.id, sealed_bytes.clone()));
        bundle_take(&mut bundle_state, chosen.id);

        let reply = rc
            .app_call(Target::RouteId(route.clone()), frame(MSG_TEXT, &sealed_bytes))
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

    // §16.11's actual claim, observable from outside: the ciphertext that was
    // already delivered cannot be delivered again, because the key it was sealed
    // to no longer exists on the receiver. This is what "forward-secret" means
    // operationally — not that an attacker fails to decrypt, but that *nobody*
    // can, including the recipient who read it a moment ago.
    println!("\n  replaying a delivered ciphertext (the seized-phone case)");
    let (pid, bytes) = consumed[0].clone();
    let r4 = rc
        .app_call(Target::RouteId(route.clone()), frame(MSG_TEXT, &bytes))
        .await?;
    if decode(&r4).ok().and_then(|v| Message::from_value(v).ok()).is_some() {
        return Err(format!("prekey {pid} was not consumed — no forward secrecy").into());
    }
    println!(
        "  \x1b[32mundecryptable\x1b[0m — prekey {pid} is gone from the receiver: {}",
        String::from_utf8_lossy(&r4)
    );

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
    let (fchosen, _) = bundle_state.select();
    let (fek, fct) = hpke::seal(&mut rng, &fchosen.public, &info, b"", &forged.to_value().encode())
        .map_err(|e| format!("seal: {e:?}"))?;
    let fsealed = SealedMessage {
        version: 1, suite: 1, prekey_id: fchosen.id, enc: fek, ciphertext: fct,
    };
    let r3 = rc
        .app_call(Target::RouteId(route), frame(MSG_TEXT, &fsealed.to_value().encode()))
        .await?;
    if Message::from_value(decode(&r3).unwrap_or(ducat_core::cbor::Value::Uint(0))).is_ok() {
        return Err("a substituted message was accepted".into());
    }
    println!(
        "  \x1b[32mrefused\x1b[0m — {}",
        String::from_utf8_lossy(&r3)
    );

    println!("\n  \x1b[32mfive properties held over a real route\x1b[0m");
    println!("  claim-once, chained thread, replayed claim refused, delivered");
    println!("  ciphertext undecryptable after its prekey was consumed, forged link refused");
    api.shutdown().await;
    Ok(())
}
