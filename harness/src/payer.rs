//! The reading side: import a route, verify, confirm, pay.

use std::time::Instant;

use ducat_core::commit::{commit, commit_eq, Purpose};
use ducat_core::sig::{ObjectType, PublicKey, SecretKey, SignedBytes};
use ducat_core::verify::{check_verification, VerificationPolicy, VerificationState};
use ducat_core::wire::*;
use veilid_core::*;

use crate::flow::*;
use crate::payee::now;
use crate::wallet::Wallet;

pub async fn run(tap_path: &str) -> Result<(), Box<dyn std::error::Error>> {
    println!("\n\x1b[1mDUCAT harness — payer\x1b[0m\n");

    let tap_env = std::fs::read(tap_path)?;
    let payee_pk = PublicKey::from_bytes(
        ducat_core::sig::Suite::Ed25519X25519,
        &hex::decode(std::fs::read_to_string(format!("{tap_path}.pk"))?.trim())?,
    )
    .map_err(|e| format!("bad payee key: {e:?}"))?;

    // Everything the payer knows starts here: a signed blob it did not choose.
    let (ty, tap_body) = open(&tap_env, &payee_pk).map_err(|e| format!("{e:?}"))?;
    if ty != ObjectType::TapPresent {
        return Err("tap is not a TapPresent".into());
    }
    let tap = decode_tap(tap_body.bytes())?;
    println!("  tap      verified, {} B route blob, expires in {}s",
        tap.route.len(), tap.expiry.saturating_sub(now()));
    if tap.expiry < now() {
        return Err("tap has expired".into());
    }

    let w = Wallet::open("user_01", 28101)?;
    println!("  wallet   {}…", &w.address[..20]);

    let (api, _calls) = crate::veilid::start("payer").await?;

    // §15.3's budget starts here, not at node startup. A phone app keeps its
    // node attached; the three seconds are the user's wait from presenting the
    // device to seeing a confirm screen, so the clock starts at the tap.
    let t_tap = Instant::now();
    let rc = api.routing_context()?;
    let route_id = api.import_remote_private_route(tap.route.clone())?;
    let d_import = t_tap.elapsed();
    println!("  route    imported ({:.0} ms)\n", d_import.as_secs_f64() * 1000.0);

    // 1. Ask for the offer the tap committed to.
    let t_call = Instant::now();
    let reply = rc.app_call(Target::RouteId(route_id.clone()), frame(MSG_REQUEST_OFFER, b"")).await?;
    let d_roundtrip = t_call.elapsed();
    let (kind, body) = unframe(&reply)?;
    if kind != MSG_FULL_OFFER {
        return Err(format!("expected FullOffer, got {kind}").into());
    }
    let (_, offer_body) = open(body, &payee_pk).map_err(|e| format!("{e:?}"))?;
    let offer = decode_offer(offer_body.bytes())?;

    // §18.6: the commitment is checked BEFORE negotiating. Negotiating first
    // means selecting from an attacker-chosen menu and only then noticing.
    if !commit_eq(&tap.offer_commit, &commit(Purpose::Offer, offer_body.bytes())) {
        return Err("offer does not match the tap's commitment".into());
    }
    let d_to_confirm = t_tap.elapsed();
    println!("  offer    verified against the tap: {} pXMR", offer.amount_pxmr);
    println!(
        "\n  \x1b[1mtap budget (§15.3)\x1b[0m  route import {:.0} ms + round trip {:.0} ms \
         + verify = \x1b[1m{:.2} s\x1b[0m to confirm screen  [{}]\n",
        d_import.as_secs_f64() * 1000.0,
        d_roundtrip.as_secs_f64() * 1000.0,
        d_to_confirm.as_secs_f64(),
        if d_to_confirm.as_secs_f64() <= 3.0 { "within budget" } else { "OVER BUDGET" }
    );

    // §17.4: under fast/1 the provider hands over goods before confirmation, so
    // it wants collateral first. The bond is posted before the ACCEPT, because a
    // provider that learns the bond is inadequate *after* accepting has already
    // made the decision the bond exists to inform.
    if offer.settle_mode == 1 {
        let bond_total = 100_000_000_000u64;              // 0.1 XMR posted
        let remaining = 60_000_000_000u64;                // what is left of it
        let bp = ducat_core::escrow::BondProof {
            version: 1,
            suite: 1,
            bond_ms_address: b"53multisigbondaddress".to_vec(),
            bond_amount_pxmr: bond_total,
            arbiter_set_id: [0xA5; 32],
            // §17.8: the floor of a ladder, never the exact figure — an exact
            // balance shown to every provider is a running meter on spending.
            capacity_bucket: ducat_core::bond::bucket_floor(remaining),
            issued: now(),
        };
        println!(
            "  bond     {} pXMR posted; publishing capacity bucket {} (true remaining {} withheld)",
            bp.bond_amount_pxmr, bp.capacity_bucket, remaining
        );
        let reply = rc
            .app_call(Target::RouteId(route_id.clone()), frame(MSG_BOND, &bp.to_value().encode()))
            .await?;
        let (kind, body) = unframe(&reply)?;
        if kind == MSG_REJECT {
            return Err(format!("bond refused: {}", String::from_utf8_lossy(body)).into());
        }
        println!("  bond     accepted by the provider");
    }

    // §15.5.1 — is the person holding this device entitled to spend?
    let policy = VerificationPolicy::default();
    let state = VerificationState { device_unlocked: true, app_secret_age_s: Some(5) };
    // The harness values a piconero amount in reference minor units crudely;
    // the tier ladder is what is being exercised, not the rate.
    let ref_minor = offer.amount_pxmr / 10_000_000;
    match check_verification(&policy, &state, ref_minor, 0, true) {
        Ok(tier) => println!("  verify   satisfied at {tier:?} (§15.5.1)"),
        Err(need) => return Err(format!("verification required: {need:?}").into()),
    }

    // 2. ACCEPT — the payer's signature is the human checkpoint (§15.5).
    let key = SecretKey::ed25519_from_bytes(&[0x11; 32]);
    let session = SecretKey::ed25519_from_bytes(&[0x12; 32]);
    std::fs::write(format!("{tap_path}.payer"), hex::encode(key.public().to_bytes()))?;

    let accept = Accept {
        version: 1,
        suite: 1,
        nonce: tap.nonce,
        offer_hash: commit(Purpose::Offer, offer_body.bytes()),
        amount_final: offer.amount_pxmr,
        dest: Some(offer.payto.clone()),
        reader_session_pk: session.public().to_bytes(),
        timestamp: now(),
        chosen_version: 1,
        chosen_suite: 1,
        refund_to: Some(w.address.as_bytes().to_vec()),
    };
    let accept_bytes = accept.to_value().encode();
    let accept_env = seal(
        &SignedBytes::from_received(accept_bytes.clone()).unwrap(),
        ObjectType::Accept,
        &key,
    );
    let reply = rc.app_call(Target::RouteId(route_id.clone()), frame(MSG_ACCEPT, &accept_env)).await?;
    let (kind, body) = unframe(&reply)?;
    if kind == MSG_REJECT {
        return Err(format!("payee refused ACCEPT: {}", String::from_utf8_lossy(body)).into());
    }
    println!("  accept   acknowledged");

    // 3. FUND on chain, and confirm it actually went somewhere (§8.7.2).
    let payto = String::from_utf8(offer.payto.clone())?;
    let txid = w.pay(&payto, offer.amount_pxmr)?;
    println!("  fund     {}…", &txid[..16]);
    let seen = w.confirm_propagated(&txid)?;
    println!("  relayed  visible on {seen}");

    // 4. TXID — a pointer, not evidence. The payee scans.
    let mut raw = [0u8; 32];
    raw.copy_from_slice(&hex::decode(&txid)?);
    let t = ducat_core::escrow::TxId {
        version: 1,
        suite: 1,
        accept_link: commit(Purpose::ChainLink, &accept_bytes),
        txid: raw,
        amount_pxmr: offer.amount_pxmr,
        timestamp: now(),
    };
    let reply = rc
        .app_call(Target::RouteId(route_id.clone()), frame(MSG_TXID, &t.to_value().encode()))
        .await?;
    let (kind, body) = unframe(&reply)?;
    if kind == MSG_REJECT {
        return Err(format!("payee refused: {}", String::from_utf8_lossy(body)).into());
    }

    // 5. Collect the receipt. Separate from the TXID call on purpose: the
    //    payee's answer waits on a chain scan, and an `app_call` timeout has
    //    nothing to do with how long Monero takes. Folding the two together
    //    delivers a slow confirmation and a fabricated TXID to the payer as the
    //    same `Timeout`, pointing at the network rather than at the payment.
    let mut body_owned = Vec::new();
    let mut have = false;
    for _ in 0..25 {
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        let Ok(reply) = rc
            .app_call(Target::RouteId(route_id.clone()), frame(MSG_RECEIPT_Q, b""))
            .await
        else {
            continue; // a lost poll is a transport event (§8.7.2)
        };
        let (kind, b) = unframe(&reply)?;
        match kind {
            MSG_RECEIPT => {
                body_owned = b.to_vec();
                have = true;
                break;
            }
            MSG_REJECT => {
                return Err(format!("payee refused: {}", String::from_utf8_lossy(b)).into())
            }
            _ => continue, // still scanning
        }
    }
    if !have {
        return Err("no receipt within the window".into());
    }
    let body = body_owned.as_slice();

    // 6. RECEIPT — and the whole transcript verified as one artifact.
    let (_, receipt_body) = open(body, &payee_pk).map_err(|e| format!("{e:?}"))?;
    let receipt = decode_receipt(receipt_body.bytes())?;
    verify_transcript(&tap, &offer, &accept, &accept_bytes, &receipt)
        .map_err(|e| format!("transcript failed: {e:?}"))?;

    println!(
        "\n  total    {:.2} s from tap to CLOSED (settlement included)",
        t_tap.elapsed().as_secs_f64()
    );
    println!("  \x1b[32mCLOSED\x1b[0m — transcript verified end to end");
    println!("  {} pXMR settled to {}…", receipt.amount_final, &payto[..20]);
    println!("  txid {txid}\n");

    api.shutdown().await;
    Ok(())
}
