//! The presenting side: allocate a route, publish a tap, serve the protocol.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ducat_core::cbor::decode;
use ducat_core::commit::{commit, commit_eq, Purpose};
use ducat_core::sig::{ObjectType, PublicKey, SecretKey, SignedBytes};
use ducat_core::wire::*;
use veilid_core::*;

use crate::flow::*;
use crate::wallet::Wallet;

pub async fn run(
    tap_path: &str,
    amount_pxmr: u64,
    fast: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    println!(
        "\n\x1b[1mDUCAT harness — payee ({})\x1b[0m\n",
        if fast { "fast/1" } else { "direct" }
    );

    let w = Wallet::open("coffee_01", 28104)?;
    println!("  wallet   {}…", &w.address[..20]);

    let (api, mut calls) = crate::veilid::start("payee").await?;

    // A route is the presenter's reachability. §15.3 puts its blob inside the
    // tap, which is why the tap is worth its bytes.
    let route = api.new_custom_private_route(PrivateSpec::default()).await?;
    println!("  route    {} B blob", route.blob.len());

    let key = SecretKey::ed25519_from_bytes(&[0x21; 32]);
    let session = SecretKey::ed25519_from_bytes(&[0x22; 32]);

    let offer = FullOffer {
        version: 1,
        suite: 1,
        profile: 2, // pos/1
        payto: w.address.as_bytes().to_vec(),
        amount_pxmr,
        supported_versions: vec![1],
        supported_suites: vec![1, 2],
        settle_mode: if fast { 1 } else { 0 },
        fee_policy: FeePolicy::PayerPays,
        nonce_echo: [0x5A; 16],
        terms: Terms::default(),
    };
    let offer_bytes = offer.to_value().encode();
    let offer_env = seal(
        &SignedBytes::from_received(offer_bytes.clone()).unwrap(),
        ObjectType::FullOffer,
        &key,
    );

    let tap = TapPresent {
        version: 1,
        suite: 1,
        profile: 2,
        presenter_role: PresenterRole::Payee,
        amount_authority: AmountAuthority::Fixed,
        intent: Intent::Oneshot,
        rmode: ReachMode::Inline,
        nonce: [0x5A; 16],
        expiry: now() + 30,
        session_pk: session.public().to_bytes(),
        route: route.blob.clone(),
        offer_commit: commit(Purpose::Offer, &offer_bytes),
        dest: None,
        session_ref: None,
    };
    let tap_bytes = tap.to_value().encode();
    let tap_env = seal(
        &SignedBytes::from_received(tap_bytes.clone()).unwrap(),
        ObjectType::TapPresent,
        &key,
    );

    // The tap file *is* the QR code. Writing the persona key alongside is a
    // harness convenience standing in for the payer learning it from the tap's
    // signature chain; nothing else here is simulated.
    std::fs::write(tap_path, &tap_env)?;
    std::fs::write(format!("{tap_path}.pk"), hex::encode(key.public().to_bytes()))?;
    println!("  tap      {} B written to {tap_path}", tap_env.len());
    println!("  offering {amount_pxmr} pXMR — waiting for a payer\n");

    let mut accept: Option<(Accept, Vec<u8>)> = None;
    // The receipt is produced asynchronously — see the TXID handler.
    let receipt_slot: Arc<Mutex<Option<Result<Vec<u8>, String>>>> = Arc::new(Mutex::new(None));
    // §17.4: under fast/1 the provider will hand over goods before confirmation,
    // so it wants collateral standing behind the payment first.
    let mut bonded = !fast;
    let deadline = Instant::now() + Duration::from_secs(600);

    while Instant::now() < deadline {
        let Ok(Some((id, msg))) =
            tokio::time::timeout(Duration::from_secs(30), calls.recv()).await
        else {
            continue;
        };
        let (kind, body) = match unframe(&msg) {
            Ok(v) => v,
            Err(e) => {
                let _ = api.app_call_reply(id, reject(&e)).await;
                continue;
            }
        };

        match kind {
            MSG_REQUEST_OFFER => {
                println!("  → offer requested; replying with FullOffer");
                api.app_call_reply(id, frame(MSG_FULL_OFFER, &offer_env)).await?;
            }
            MSG_BOND => {
                let parsed = decode(body)
                    .map_err(|e| format!("{e:?}"))
                    .and_then(|v| {
                        ducat_core::escrow::BondProof::from_value(v)
                            .map_err(|e| format!("{e:?}"))
                    });
                match parsed {
                    Ok(b) => {
                        // The arbiter set is supplied here, not read from the
                        // message — §2.5's exploit installed one that arrived in
                        // a message and was well-formed.
                        let trusted = [[0xA5u8; 32]];
                        match ducat_core::escrow::check_bond_proof(
                            &b, amount_pxmr, now(), 300, &trusted,
                        ) {
                            Ok(()) => {
                                println!(
                                    "  → bond accepted: {} pXMR posted, capacity bucket {}",
                                    b.bond_amount_pxmr, b.capacity_bucket
                                );
                                bonded = true;
                                api.app_call_reply(id, frame(MSG_BOND, b"ok")).await?;
                            }
                            Err(e) => {
                                println!("  → bond refused: {e:?}");
                                api.app_call_reply(id, reject(&format!("{e:?}"))).await.ok();
                            }
                        }
                    }
                    Err(e) => {
                        api.app_call_reply(id, reject(&e)).await.ok();
                    }
                }
            }
            MSG_ACCEPT if !bonded => {
                api.app_call_reply(id, reject("fast/1 requires a bond first")).await.ok();
            }
            MSG_ACCEPT => {
                // §18.4.1(1): only the payer may emit ACCEPT, and the guard is on
                // who sent it — established here by the signature, before the
                // machine sees the event.
                let payer_pk_hex = std::fs::read_to_string(format!("{tap_path}.payer"))
                    .unwrap_or_default();
                let payer_pk = PublicKey::from_bytes(
                    ducat_core::sig::Suite::Ed25519X25519,
                    &hex::decode(payer_pk_hex.trim()).unwrap_or_default(),
                )
                .ok();
                let Some(pk) = payer_pk else {
                    api.app_call_reply(id, reject("no payer key")).await?;
                    continue;
                };
                match open(body, &pk) {
                    Ok((ObjectType::Accept, sb)) => {
                        // A `?` here would kill the payee rather than refuse the
                        // message — which is exactly what happened on this
                        // harness's first run, and made a decode bug look like a
                        // network timeout to the payer. A server that dies on
                        // bad input has turned every client error into an
                        // outage.
                        let a = match decode_accept(sb.bytes()) {
                            Ok(a) => a,
                            Err(e) => {
                                api.app_call_reply(id, reject(&e)).await.ok();
                                continue;
                            }
                        };
                        if !commit_eq(&a.offer_hash, &commit(Purpose::Offer, &offer_bytes)) {
                            api.app_call_reply(id, reject("ACCEPT names another offer")).await?;
                            continue;
                        }
                        if a.amount_final != amount_pxmr {
                            api.app_call_reply(id, reject("price mismatch")).await?;
                            continue;
                        }
                        println!("  → ACCEPT verified: {} pXMR", a.amount_final);
                        accept = Some((a, sb.bytes().to_vec()));
                        api.app_call_reply(id, frame(MSG_ACCEPT, b"ok")).await?;
                    }
                    Ok(_) => {
                        api.app_call_reply(id, reject("unexpected object type")).await?;
                    }
                    Err(e) => {
                        api.app_call_reply(id, reject(&format!("{e:?}"))).await?;
                    }
                }
            }
            MSG_TXID => {
                let Some((acc, acc_bytes)) = accept.clone() else {
                    api.app_call_reply(id, reject("no ACCEPT on file")).await?;
                    continue;
                };
                let parsed = decode(body)
                    .map_err(|e| format!("{e:?}"))
                    .and_then(|v| {
                        ducat_core::escrow::TxId::from_value(v).map_err(|e| format!("{e:?}"))
                    })
                    .and_then(|t| {
                        ducat_core::escrow::check_txid(&t, &acc, &acc_bytes)
                            .map(|want| (t, want))
                            .map_err(|e| format!("{e:?}"))
                    });
                let (t, want) = match parsed {
                    Ok(v) => v,
                    Err(e) => {
                        api.app_call_reply(id, reject(&e)).await.ok();
                        continue;
                    }
                };

                // **Acknowledge now, scan after.**
                //
                // The payee's answer depends on a chain scan, and an `app_call`
                // has a transport timeout that has nothing to do with how long
                // Monero takes. Holding the call open until the scan finishes
                // means a legitimate slow confirmation and a fabricated TXID are
                // both delivered to the payer as `Timeout` — indistinguishable,
                // and pointing at the network rather than at the payment.
                //
                // Worse, it is a denial of service: the first version blocked for
                // five minutes on a TXID naming a transaction that does not
                // exist, so one message froze the terminal. Structural checks are
                // synchronous and cheap; anything that waits on the world is not
                // allowed to hold the session.
                let txid = hex::encode(t.txid);
                println!("  → TXID {}… — acknowledged; scanning for {want} pXMR", &txid[..16]);
                api.app_call_reply(id, frame(MSG_TXID, b"scanning")).await?;

                let slot = receipt_slot.clone();
                let port = w.port;
                let wname = w.name.clone();
                let key2 = SecretKey::ed25519_from_bytes(&[0x21; 32]);
                let fastc = fast;
                tokio::task::spawn_blocking(move || {
                    let Ok(w2) = Wallet::open(&wname, port) else {
                        *slot.lock().unwrap() = Some(Err("wallet unavailable".into()));
                        return;
                    };
                    let tries = if fastc { 10 } else { 15 };
                    match w2.scan_for(&txid, tries) {
                        Ok(got) if got >= want => {
                            if fastc {
                                println!("  ✓ observed {got} pXMR — accepting at mempool visibility");
                                println!("    (PROVISIONAL: service proceeds, finality still pending)");
                            } else {
                                println!("  ✓ observed {got} pXMR on chain");
                            }
                            let receipt = Receipt {
                                version: 1,
                                suite: 1,
                                accept_hash: commit(Purpose::ChainLink, &acc_bytes),
                                prev: commit(Purpose::ChainLink, &acc_bytes),
                                amount_final: acc.amount_final,
                                timestamp: now(),
                                unilateral: false,
                            };
                            let rb = receipt.to_value().encode();
                            let env = seal(
                                &SignedBytes::from_received(rb).unwrap(),
                                ObjectType::Receipt,
                                &key2,
                            );
                            *slot.lock().unwrap() = Some(Ok(env));
                        }
                        Ok(got) => {
                            println!("  → underpaid: {got} < {want}");
                            *slot.lock().unwrap() =
                                Some(Err(format!("underpaid: {got} < {want}")));
                        }
                        Err(e) => {
                            println!("  → \x1b[33m{e}\x1b[0m");
                            *slot.lock().unwrap() = Some(Err(e));
                        }
                    }
                });
            }
            MSG_RECEIPT_Q => {
                let current = receipt_slot.lock().unwrap().clone();
                match current {
                    None => {
                        api.app_call_reply(id, frame(MSG_PENDING, b"scanning")).await.ok();
                    }
                    Some(Ok(env)) => {
                        api.app_call_reply(id, frame(MSG_RECEIPT, &env)).await.ok();
                        println!("\n  \x1b[32mCLOSED\x1b[0m — receipt co-signed and returned\n");
                        break;
                    }
                    Some(Err(e)) => {
                        api.app_call_reply(id, reject(&e)).await.ok();
                    }
                }
            }
            other => {
                api.app_call_reply(id, reject(&format!("unexpected message {other}"))).await?;
            }
        }
    }

    api.shutdown().await;
    Ok(())
}

pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}
