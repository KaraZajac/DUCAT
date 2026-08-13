//! The other direction: the **payer** presents, the merchant scans (§15.2).
//!
//! Every other harness flow has the payee presenting — the cab, the counter, the
//! busker — which §15.2 calls the normal case and which is what a POS terminal
//! is. This is the inversion: the customer holds out their phone and the till
//! reads it, the shape Alipay and WeChat made familiar.
//!
//! It is not a curiosity. **§15.3.2 leans on it as the iOS escape hatch**: an
//! iPhone cannot present over NFC (O19), so a merchant on iOS either inverts the
//! roles or falls back to QR. An escape hatch the specification depends on for an
//! entire platform had no test and no harness path until 0.56 — the enum variant
//! existed and nothing had ever built one.
//!
//! # What inverts and what does not
//!
//! The presenter supplies **reachability**, so here the tap carries the payer's
//! route and the merchant drives every round trip. `amount_authority` becomes
//! `open`: the reader types the number, because the customer's phone does not
//! know the price of a coffee.
//!
//! **The human checkpoint does not move.** §18.4.1(1) still permits only the
//! payer to emit `ACCEPT`, and §15.5's confirm screen is still the payer's — the
//! party whose money is at risk decides, regardless of who held out a phone.
//! That invariant is the reason this inversion is safe to offer at all.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ducat_core::commit::{commit, commit_eq, Purpose};
use ducat_core::sig::{ObjectType, PublicKey, SecretKey, SignedBytes};
use ducat_core::wire::*;
use veilid_core::*;

use crate::flow::*;
use crate::payee::now;
use crate::wallet::Wallet;

const MSG_READY_Q: u8 = 0x30;

/// Customer side: publish a route, wait to be charged, decide, pay.
pub async fn present(tap_path: &str) -> Result<(), Box<dyn std::error::Error>> {
    println!("\n\x1b[1mDUCAT harness — payer presenting (customer shows, till scans)\x1b[0m\n");
    // Selectable, because §17.2's output lock is real: a wallet that just paid
    // has nothing unlocked for ten blocks, and hardcoding one persona means the
    // harness fails for a reason that has nothing to do with the protocol.
    let name = std::env::var("DUCAT_PAYER_WALLET").unwrap_or_else(|_| "user_01".into());
    let port: u16 = std::env::var("DUCAT_PAYER_PORT")
        .ok().and_then(|v| v.parse().ok()).unwrap_or(28101);
    let w = Wallet::open(&name, port)?;
    println!("  wallet   {}…", &w.address[..20]);

    let (api, mut calls) = crate::veilid::start("presenter").await?;
    let route = api.new_custom_private_route(PrivateSpec::default()).await?;

    let key = SecretKey::ed25519_from_bytes(&[0x11; 32]);
    let session = SecretKey::ed25519_from_bytes(&[0x12; 32]);

    // No amount, and no offer to commit to: the customer does not know the
    // price. `amount_authority: Open` says the reader supplies it.
    let tap = TapPresent {
        version: 1,
        suite: 1,
        profile: 2,
        presenter_role: PresenterRole::Payer,
        amount_authority: AmountAuthority::Open,
        intent: Intent::Oneshot,
        rmode: ReachMode::Inline,
        nonce: [0x7C; 16],
        expiry: now() + 120,
        session_pk: session.public().to_bytes(),
        route: route.blob.clone(),
        // Nothing to commit to yet — the offer does not exist until the till
        // makes one. This is the structural difference between the directions.
        offer_commit: [0u8; 32],
        dest: Some(w.address.as_bytes().to_vec()),
        session_ref: None,
    };
    let env = seal(
        &SignedBytes::from_received(tap.to_value().encode()).unwrap(),
        ObjectType::TapPresent,
        &key,
    );
    std::fs::write(tap_path, &env)?;
    std::fs::write(format!("{tap_path}.pk"), hex::encode(key.public().to_bytes()))?;
    println!("  tap      {} B — presenting, waiting to be charged\n", env.len());

    let mut accept_bytes: Option<Vec<u8>> = None;
    // Shared, because settlement must not happen on the message loop. In this
    // direction the till *polls* — the customer holds the route, so the customer
    // cannot call out — and a loop blocked paying (including up to 40s of
    // propagation retries) answers nothing. The till then times out on a
    // transaction that has in fact been broadcast. A presenter's loop must stay
    // responsive for exactly as long as someone is asking it questions.
    let txid: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let deadline = Instant::now() + Duration::from_secs(600);

    while Instant::now() < deadline {
        let Ok(Some((id, msg))) =
            tokio::time::timeout(Duration::from_secs(30), calls.recv()).await
        else {
            continue;
        };
        let Ok((kind, body)) = unframe(&msg) else { continue };
        match kind {
            MSG_FULL_OFFER => {
                let merchant_pk = PublicKey::from_bytes(
                    ducat_core::sig::Suite::Ed25519X25519,
                    &hex::decode(
                        std::fs::read_to_string(format!("{tap_path}.merchant"))
                            .unwrap_or_default()
                            .trim(),
                    )
                    .unwrap_or_default(),
                );
                let Ok(mpk) = merchant_pk else {
                    api.app_call_reply(id, reject("no merchant key")).await.ok();
                    continue;
                };
                let Ok((_, ob)) = open(body, &mpk) else {
                    api.app_call_reply(id, reject("offer signature")).await.ok();
                    continue;
                };
                let Ok(offer) = decode_offer(ob.bytes()) else {
                    api.app_call_reply(id, reject("offer decode")).await.ok();
                    continue;
                };
                // §15.5: the confirm screen is the payer's, whoever presented.
                println!("  → charged {} pXMR — confirming (§15.5)", offer.amount_pxmr);

                let accept = Accept {
                    version: 1,
                    suite: 1,
                    nonce: tap.nonce,
                    offer_hash: commit(Purpose::Offer, ob.bytes()),
                    amount_final: offer.amount_pxmr,
                    dest: Some(offer.payto.clone()),
                    reader_session_pk: session.public().to_bytes(),
                    timestamp: now(),
                    chosen_version: 1,
                    chosen_suite: 1,
                    refund_to: Some(w.address.as_bytes().to_vec()),
                    memo: None,
                };
                let ab = accept.to_value().encode();
                let aenv = seal(
                    &SignedBytes::from_received(ab.clone()).unwrap(),
                    ObjectType::Accept,
                    &key,
                );
                accept_bytes = Some(ab);
                api.app_call_reply(id, frame(MSG_ACCEPT, &aenv)).await.ok();

                // Settle off the loop.
                let payto = String::from_utf8(offer.payto.clone()).unwrap_or_default();
                let amount = offer.amount_pxmr;
                let slot = txid.clone();
                let wname = name.clone();
                tokio::task::spawn_blocking(move || {
                    let w = match Wallet::open(&wname, port) {
                        Ok(w) => w,
                        Err(e) => {
                            println!("  → wallet unavailable: {e}");
                            return;
                        }
                    };
                    match w.pay(&payto, amount) {
                        Ok(t) => {
                            println!("  → funded {}…", &t[..16]);
                            match w.confirm_propagated(&t) {
                                Ok(seen) => println!("  → propagated, visible on {seen}"),
                                Err(e) => println!("  → \x1b[33m{e}\x1b[0m"),
                            }
                            *slot.lock().unwrap() = Some(t);
                        }
                        Err(e) => println!("  → payment failed: {e}"),
                    }
                });
            }
            MSG_READY_Q => {
                let current = txid.lock().unwrap().clone();
                let reply = match (&current, &accept_bytes) {
                    (Some(t), Some(ab)) => {
                        let mut raw = [0u8; 32];
                        raw.copy_from_slice(&hex::decode(t).unwrap_or(vec![0; 32]));
                        let obj = ducat_core::escrow::TxId {
                            version: 1,
                            suite: 1,
                            accept_link: commit(Purpose::ChainLink, ab),
                            txid: raw,
                            amount_pxmr: 0, // filled below
                            timestamp: now(),
                        };
                        let mut obj = obj;
                        obj.amount_pxmr = Accept::from_value(
                            ducat_core::cbor::decode(ab).unwrap(),
                        )
                        .unwrap()
                        .amount_final;
                        frame(MSG_TXID, &obj.to_value().encode())
                    }
                    _ => reject("not funded yet"),
                };
                api.app_call_reply(id, reply).await.ok();
            }
            MSG_RECEIPT => {
                println!("\n  \x1b[32mCLOSED\x1b[0m — receipt received from the till\n");
                api.app_call_reply(id, frame(MSG_RECEIPT, b"ok")).await.ok();
                break;
            }
            _ => {
                api.app_call_reply(id, reject("unexpected")).await.ok();
            }
        }
    }
    api.shutdown().await;
    Ok(())
}

/// Till side: scan the customer's tap, charge them, verify, receipt.
pub async fn scan(tap_path: &str, amount_pxmr: u64) -> Result<(), Box<dyn std::error::Error>> {
    println!("\n\x1b[1mDUCAT harness — till scanning (POS)\x1b[0m\n");
    let w = Wallet::open("coffee_01", 28104)?;

    let env = std::fs::read(tap_path)?;
    let payer_pk = PublicKey::from_bytes(
        ducat_core::sig::Suite::Ed25519X25519,
        &hex::decode(std::fs::read_to_string(format!("{tap_path}.pk"))?.trim())?,
    )
    .map_err(|e| format!("{e:?}"))?;
    let (ty, tb) = open(&env, &payer_pk).map_err(|e| format!("{e:?}"))?;
    if ty != ObjectType::TapPresent {
        return Err("not a tap".into());
    }
    let tap = decode_tap(tb.bytes())?;
    if tap.presenter_role != PresenterRole::Payer {
        return Err("this tap is not payer-presented".into());
    }
    if tap.amount_authority != AmountAuthority::Open {
        return Err("a payer-presented tap must leave the amount to the reader".into());
    }
    println!("  tap      payer-presented, amount left to the till — charging {amount_pxmr} pXMR");

    let (api, _calls) = crate::veilid::start("till").await?;
    let rc = api.routing_context()?;
    let route = api.import_remote_private_route(tap.route.clone())?;

    let key = SecretKey::ed25519_from_bytes(&[0x21; 32]);
    std::fs::write(format!("{tap_path}.merchant"), hex::encode(key.public().to_bytes()))?;

    let offer = FullOffer {
        version: 1,
        suite: 1,
        profile: 2,
        payto: w.address.as_bytes().to_vec(),
        amount_pxmr,
        supported_versions: vec![1],
        supported_suites: vec![1, 2],
        settle_mode: 0,
        fee_policy: FeePolicy::PayerPays,
        nonce_echo: tap.nonce,
        terms: Terms::default(),
        memo: None,
    };
    let ob = offer.to_value().encode();
    let oenv = seal(
        &SignedBytes::from_received(ob.clone()).unwrap(),
        ObjectType::FullOffer,
        &key,
    );

    let reply = rc
        .app_call(Target::RouteId(route.clone()), frame(MSG_FULL_OFFER, &oenv))
        .await?;
    let (k, b) = unframe(&reply)?;
    if k == MSG_REJECT {
        return Err(format!("customer refused: {}", String::from_utf8_lossy(b)).into());
    }
    let (_, ab) = open(b, &payer_pk).map_err(|e| format!("{e:?}"))?;
    let accept = decode_accept(ab.bytes())?;
    if !commit_eq(&accept.offer_hash, &commit(Purpose::Offer, &ob)) {
        return Err("ACCEPT names another offer".into());
    }
    if accept.amount_final != amount_pxmr {
        return Err("price mismatch".into());
    }
    println!("  accept   verified: {} pXMR", accept.amount_final);

    // Poll for the txid; the customer cannot call us, because the route is
    // theirs. In the payee-presented direction this asymmetry runs the other
    // way and neither side needs to poll — worth knowing before building a UI
    // that assumes symmetry.
    let mut got = None;
    for _ in 0..40 {
        // A lost round trip is a transport event, not a refusal (§8.7.2). A till
        // that abandons the sale on one dropped poll abandons a customer who has
        // already paid.
        let Ok(reply) = rc
            .app_call(Target::RouteId(route.clone()), frame(MSG_READY_Q, b""))
            .await
        else {
            tokio::time::sleep(Duration::from_secs(3)).await;
            continue;
        };
        let (k, b) = unframe(&reply)?;
        if k == MSG_TXID {
            got = Some(ducat_core::escrow::TxId::from_value(
                ducat_core::cbor::decode(b).map_err(|e| format!("{e:?}"))?,
            )
            .map_err(|e| format!("{e:?}"))?);
            break;
        }
        tokio::time::sleep(Duration::from_secs(3)).await;
    }
    let t = got.ok_or("customer never funded")?;
    let want = ducat_core::escrow::check_txid(&t, &accept, ab.bytes())
        .map_err(|e| format!("{e:?}"))?;
    let txid = hex::encode(t.txid);
    println!("  txid     {}… — scanning with my own view key", &txid[..16]);
    let seen = w.scan_for(&txid, 30)?;
    if seen < want {
        return Err(format!("underpaid: {seen} < {want}").into());
    }
    println!("  ✓ observed {seen} pXMR on chain");

    let receipt = Receipt {
        version: 1,
        suite: 1,
        accept_hash: commit(Purpose::ChainLink, ab.bytes()),
        prev: commit(Purpose::ChainLink, ab.bytes()),
        amount_final: accept.amount_final,
        timestamp: now(),
        unilateral: false,
    };
    let renv = seal(
        &SignedBytes::from_received(receipt.to_value().encode()).unwrap(),
        ObjectType::Receipt,
        &key,
    );
    rc.app_call(Target::RouteId(route), frame(MSG_RECEIPT, &renv)).await?;
    println!("\n  \x1b[32mCLOSED\x1b[0m — {} pXMR received, receipt issued\n", accept.amount_final);

    api.shutdown().await;
    Ok(())
}
