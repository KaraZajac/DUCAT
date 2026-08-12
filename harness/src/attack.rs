//! Attacks, fired down a live route at an honest counterparty.
//!
//! Every refusal in this protocol is unit-tested. None had ever been *sent*.
//! That gap is not academic: the `dest` bug (§18.12) was a check that existed,
//! was tested, and rejected every real payment — because the fixtures agreed
//! with the mistake. A check that is never exercised across a process boundary
//! is a check nobody has confirmed is wired in.
//!
//! So this is a hostile payer against the same honest payee the other harness
//! modes use, unmodified. Each attack asserts a **refusal**, and a silent
//! acceptance is the failure.

use ducat_core::cbor::{decode, Value};
use ducat_core::commit::{commit, Purpose};
use ducat_core::sig::{ObjectType, PublicKey, SecretKey, SignedBytes};
use ducat_core::wire::*;
use veilid_core::*;

use crate::flow::*;
use crate::payee::now;

struct Outcome {
    name: &'static str,
    why: &'static str,
    refused: bool,
    detail: String,
}

pub async fn run(tap_path: &str) -> Result<(), Box<dyn std::error::Error>> {
    println!("\n\x1b[1mDUCAT harness — hostile payer against an honest payee\x1b[0m\n");

    let tap_env = std::fs::read(tap_path)?;
    let payee_pk = PublicKey::from_bytes(
        ducat_core::sig::Suite::Ed25519X25519,
        &hex::decode(std::fs::read_to_string(format!("{tap_path}.pk"))?.trim())?,
    )
    .map_err(|e| format!("{e:?}"))?;
    let (_, tb) = open(&tap_env, &payee_pk).map_err(|e| format!("{e:?}"))?;
    let tap = decode_tap(tb.bytes())?;

    let key = SecretKey::ed25519_from_bytes(&[0x11; 32]);
    let wrong = SecretKey::ed25519_from_bytes(&[0x99; 32]);
    let session = SecretKey::ed25519_from_bytes(&[0x12; 32]);
    std::fs::write(format!("{tap_path}.payer"), hex::encode(key.public().to_bytes()))?;

    let (api, _calls) = crate::veilid::start("attacker").await?;
    let rc = api.routing_context()?;
    let route = api.import_remote_private_route(tap.route.clone())?;

    async fn send(rc: &RoutingContext, r: &RouteId, m: Vec<u8>) -> Result<Vec<u8>, String> {
        for attempt in 0..3 {
            match rc.app_call(Target::RouteId(r.clone()), m.clone()).await {
                Ok(v) => return Ok(v),
                Err(_) if attempt < 2 => continue,
                Err(e) => return Err(e.to_string()),
            }
        }
        Err("unreachable".into())
    }

    // Get the genuine offer, so every attack below is a *modification* of a
    // legitimate exchange rather than noise the payee would reject anyway.
    let reply = send(&rc, &route, frame(MSG_REQUEST_OFFER, b"")).await?;
    let (_, ob) = unframe(&reply)?;
    let (_, offer_body) = open(ob, &payee_pk).map_err(|e| format!("{e:?}"))?;
    let offer = decode_offer(offer_body.bytes())?;
    let good_hash = commit(Purpose::Offer, offer_body.bytes());
    println!("  baseline offer: {} pXMR\n", offer.amount_pxmr);

    let mk = |amount: u64, offer_hash: [u8; 32], k: &SecretKey| -> Vec<u8> {
        let a = Accept {
            version: 1, suite: 1, nonce: tap.nonce, offer_hash,
            amount_final: amount, dest: Some(offer.payto.clone()),
            reader_session_pk: session.public().to_bytes(), timestamp: now(),
            chosen_version: 1, chosen_suite: 1,
            refund_to: Some(b"refund-address-placeholder".to_vec()),
        };
        seal(&SignedBytes::from_received(a.to_value().encode()).unwrap(),
             ObjectType::Accept, k)
    };

    let mut results = Vec::new();
    let mut check = |name: &'static str, why: &'static str, reply: Vec<u8>| {
        let (kind, body) = unframe(&reply).unwrap_or((0, b""));
        let refused = kind == MSG_REJECT;
        let detail = String::from_utf8_lossy(body).chars().take(90).collect::<String>();
        println!(
            "  {} {name}\n      {}",
            if refused { "\x1b[32mrefused \x1b[0m" } else { "\x1b[31mACCEPTED\x1b[0m" },
            if refused { detail.clone() } else { "no refusal — the check is not wired in".into() }
        );
        results.push(Outcome { name, why, refused, detail });
    };

    // 1. Underpay: sign an ACCEPT for less than the offer.
    let r = send(&rc, &route, frame(MSG_ACCEPT, &mk(offer.amount_pxmr - 1, good_hash, &key))).await?;
    check("accept_underpays", "a payer signing less than the offer is the cheapest attack there is", r);

    // 2. Overpay is not an attack, but an ACCEPT naming any other number is a
    //    disagreement about price that must not silently resolve in either
    //    direction.
    let r = send(&rc, &route, frame(MSG_ACCEPT, &mk(offer.amount_pxmr + 1, good_hash, &key))).await?;
    check("accept_overpays", "a price disagreement must not resolve silently, even favourably", r);

    // 3. Name a different offer.
    let r = send(&rc, &route, frame(MSG_ACCEPT, &mk(offer.amount_pxmr, [0xAB; 32], &key))).await?;
    check("accept_names_another_offer", "§18.6: the commitment is what binds an ACCEPT to what was shown", r);

    // 4. Forge the signature.
    let r = send(&rc, &route, frame(MSG_ACCEPT, &mk(offer.amount_pxmr, good_hash, &wrong))).await?;
    check("accept_signed_by_a_stranger", "the payee holds the payer's key from the tap exchange", r);

    // 5. Cross-context replay: a RECEIPT-domain signature offered as an ACCEPT.
    let a = Accept {
        version: 1, suite: 1, nonce: tap.nonce, offer_hash: good_hash,
        amount_final: offer.amount_pxmr, dest: Some(offer.payto.clone()),
        reader_session_pk: session.public().to_bytes(), timestamp: now(),
        chosen_version: 1, chosen_suite: 1, refund_to: None,
    };
    let body = SignedBytes::from_received(a.to_value().encode()).unwrap();
    let mislabelled = seal(&body, ObjectType::Receipt, &key);
    let r = send(&rc, &route, frame(MSG_ACCEPT, &mislabelled)).await?;
    check("accept_signed_in_another_context",
          "§18.3: domain separation — a signature harvested from one context must not verify as another", r);

    // 6. Non-canonical encoding, signed correctly.
    let mut m = match a.to_value() { Value::Map(m) => m, _ => unreachable!() };
    m.insert(200u64, Value::Uint(1)); // an unknown field
    let noncanon = Value::Map(m).encode();
    let env = seal(&SignedBytes::from_received(noncanon).unwrap(), ObjectType::Accept, &key);
    let r = send(&rc, &route, frame(MSG_ACCEPT, &env)).await?;
    check("accept_with_an_unknown_field",
          "§18.8: strictness — an object carrying a field the version does not define is refused, not ignored", r);

    // 7. TXID before any ACCEPT has been established.
    let t = ducat_core::escrow::TxId {
        version: 1, suite: 1, accept_link: [0x00; 32], txid: [0x77; 32],
        amount_pxmr: offer.amount_pxmr, timestamp: now(),
    };
    let r = send(&rc, &route, frame(MSG_TXID, &t.to_value().encode())).await?;
    check("txid_before_any_accept", "§18.4: a message out of state is a violation, never a silent ignore", r);

    // Establish a genuine ACCEPT, then attack the settlement leg.
    let good = mk(offer.amount_pxmr, good_hash, &key);
    let (_, gb) = open(&good, &key.public()).map_err(|e| format!("{e:?}"))?;
    let r = send(&rc, &route, frame(MSG_ACCEPT, &good)).await?;
    let (k, _) = unframe(&r)?;
    if k == MSG_REJECT {
        return Err("the honest ACCEPT was refused — the attacks below cannot be trusted".into());
    }
    println!("\n  (honest ACCEPT established; attacking the settlement leg)\n");

    // 8. TXID for a different ACCEPT.
    let t = ducat_core::escrow::TxId {
        version: 1, suite: 1, accept_link: [0xCD; 32], txid: [0x77; 32],
        amount_pxmr: offer.amount_pxmr, timestamp: now(),
    };
    let r = send(&rc, &route, frame(MSG_TXID, &t.to_value().encode())).await?;
    check("txid_for_another_transaction", "a TXID must name the ACCEPT it settles", r);

    // 9. TXID announcing less than was accepted.
    let t = ducat_core::escrow::TxId {
        version: 1, suite: 1,
        accept_link: commit(Purpose::ChainLink, gb.bytes()),
        txid: [0x77; 32], amount_pxmr: offer.amount_pxmr - 1, timestamp: now(),
    };
    let r = send(&rc, &route, frame(MSG_TXID, &t.to_value().encode())).await?;
    check("txid_announces_an_underpayment",
          "an underpayment announced honestly is still an underpayment", r);

    // 10. TXID for a transaction that does not exist. This one costs the payee
    //     a real scan, which is the point: the answer comes from the chain.
    let t = ducat_core::escrow::TxId {
        version: 1, suite: 1,
        accept_link: commit(Purpose::ChainLink, gb.bytes()),
        txid: [0xEE; 32], amount_pxmr: offer.amount_pxmr, timestamp: now(),
    };
    // The TXID is acknowledged (the structural checks pass — it names the right
    // ACCEPT for the right amount), and the refusal arrives when the scan finds
    // nothing. That two-step is the fix for a five-minute freeze: acknowledge
    // cheaply, decide slowly, and never hold the session open on a scan.
    let _ = send(&rc, &route, frame(MSG_TXID, &t.to_value().encode())).await?;
    let mut verdict = frame(MSG_PENDING, b"");
    for _ in 0..30 {
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        let Ok(r) = send(&rc, &route, frame(MSG_RECEIPT_Q, b"")).await else { continue };
        let (k, _) = unframe(&r)?;
        if k != MSG_PENDING {
            verdict = r;
            break;
        }
    }
    check("txid_for_a_transaction_that_does_not_exist",
          "§17.4: the payee is the recipient and scans; a pointer to nothing resolves to nothing", verdict);

    // ---- verdict ----------------------------------------------------------
    let accepted: Vec<_> = results.iter().filter(|r| !r.refused).collect();
    println!("\n\x1b[1m  {} attacks, {} refused\x1b[0m\n", results.len(), results.len() - accepted.len());
    if !accepted.is_empty() {
        for a in &accepted {
            println!("  \x1b[31mNOT REFUSED\x1b[0m {} — {}", a.name, a.why);
        }
        api.shutdown().await;
        return Err(format!("{} attack(s) were accepted", accepted.len()).into());
    }
    println!("  every attack refused over a live route, by the same code path a");
    println!("  real counterparty runs.\n");

    api.shutdown().await;
    Ok(())
}
