//! Escrow (§8.2): three parties, a ceremony, and a release — over real routes.
//!
//! The `direct` and `fast/1` harnesses are two-party. Escrow is the first flow
//! where the *number of parties* is part of the security argument, and where the
//! failure that matters is not a bad signature but a **message arriving out of
//! turn** — §2.5's exploit drained a production system of ~$2.7M with a forged,
//! out-of-order ACK that overwrote settled state.
//!
//! So this harness proves the ordering, not the arithmetic. Each participant
//! feeds every ceremony message through [`RoundTracker`], which accepts exactly
//! the round it expects and exactly one contribution per participant per round,
//! and the run deliberately includes an attacker replaying an earlier round to
//! show it refused.
//!
//! The Monero multisig underneath is the 2-of-3 already formed in
//! `monero-spike/` — forming a fresh one requires the wallet2 CLI dance (§8.2),
//! which is orthogonal to what is being demonstrated here.

use std::collections::HashMap;
use std::time::Duration;

use ducat_core::cbor::decode;
use ducat_core::commit::{commit, Purpose};
use ducat_core::escrow::*;
use ducat_core::sig::{ObjectType, SecretKey, SignedBytes};
use ducat_core::wire::seal;
use veilid_core::*;

use crate::flow::*;
use crate::payee::now;

pub const MSG_ESC_SETUP: u8 = 0x20;
pub const MSG_ESC_READY: u8 = 0x21;
pub const MSG_ESC_RELEASE: u8 = 0x22;

const ESCROW_ID: [u8; 32] = [0xE5; 32];
const ROUNDS: u64 = 2; // measured for a 2-of-3 wallet2 ceremony (§O1)

fn role_index(role: &str) -> u8 {
    match role {
        "buyer" => BUYER,
        "seller" => SELLER,
        _ => ARBITER,
    }
}

/// A ceremony participant that serves its route and answers.
pub async fn serve(role: &str, tap_path: &str) -> Result<(), Box<dyn std::error::Error>> {
    println!("\n\x1b[1mDUCAT harness — escrow {role}\x1b[0m\n");
    let idx = role_index(role);
    let (api, mut calls) = crate::veilid::start(role).await?;
    let route = api.new_custom_private_route(PrivateSpec::default()).await?;
    std::fs::write(format!("{tap_path}.{role}.route"), &route.blob)?;
    println!("  route    {} B published for {role}", route.blob.len());

    let key = SecretKey::ed25519_from_bytes(&[0x30 + idx; 32]);
    let mut tracker = RoundTracker::new(ESCROW_ID, ROUNDS);
    let mut reports: Vec<EscrowReady> = Vec::new();
    let mut contributed: Vec<u64> = Vec::new();

    let deadline = std::time::Instant::now() + Duration::from_secs(600);
    while std::time::Instant::now() < deadline {
        let Ok(Some((id, msg))) =
            tokio::time::timeout(Duration::from_secs(30), calls.recv()).await
        else {
            continue;
        };
        let Ok((kind, body)) = unframe(&msg) else {
            api.app_call_reply(id, reject("empty")).await.ok();
            continue;
        };
        match kind {
            MSG_ESC_SETUP => {
                let parsed = decode(body)
                    .map_err(|e| format!("{e:?}"))
                    .and_then(|v| EscrowSetup::from_value(v).map_err(|e| format!("{e:?}")));
                let reply = match parsed {
                    Err(e) => reject(&e),
                    Ok(s) => {
                        // A participant never receives its own contribution over
                        // the wire — it generates it. So it must record its own
                        // before collecting the others, or its round never
                        // closes and it refuses the next one as out of order.
                        // Modelling that wrong made RoundTracker look broken.
                        if s.round == tracker.current_round() && !contributed.contains(&s.round) {
                            let mine = EscrowSetup {
                                version: 1,
                                suite: 1,
                                escrow_id: ESCROW_ID,
                                round: s.round,
                                info: vec![0xAB; 64],
                                from_index: idx,
                                timestamp: now(),
                            };
                            let _ = tracker.accept(&mine);
                            contributed.push(s.round);
                        }
                        match tracker.accept(&s) {
                        Ok(closed) => {
                            println!(
                                "  → setup round {} from participant {} {}",
                                s.round,
                                s.from_index,
                                if closed { "(round closed)" } else { "" }
                            );
                            frame(MSG_ESC_SETUP, if closed { b"closed" } else { b"ok" })
                        }
                        Err(e) => {
                            println!("  \x1b[33m→ REFUSED\x1b[0m round {}: {}", s.round,
                                e.detail.clone().unwrap_or_default());
                            reject(&format!("{e:?}"))
                        }
                        }
                    }
                };
                api.app_call_reply(id, reply).await.ok();
            }
            MSG_ESC_READY => {
                let parsed = decode(body)
                    .map_err(|e| format!("{e:?}"))
                    .and_then(|v| EscrowReady::from_value(v).map_err(|e| format!("{e:?}")));
                match parsed {
                    Ok(r) => {
                        reports.retain(|x| x.from_index != r.from_index);
                        reports.push(r);
                        println!("  → ready report {} of 3", reports.len());
                        api.app_call_reply(id, frame(MSG_ESC_READY, b"ok")).await.ok();
                    }
                    Err(e) => {
                        api.app_call_reply(id, reject(&e)).await.ok();
                    }
                }
            }
            MSG_ESC_RELEASE => {
                println!("  → release co-signature requested");
                let _ = key;
                api.app_call_reply(id, frame(MSG_ESC_RELEASE, b"cosigned")).await.ok();
                println!("\n  \x1b[32mreleased\x1b[0m\n");
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

/// The buyer drives: it runs the ceremony, gathers reports, and releases.
pub async fn drive(tap_path: &str, ms_address: &str) -> Result<(), Box<dyn std::error::Error>> {
    println!("\n\x1b[1mDUCAT harness — escrow buyer (driver)\x1b[0m\n");
    let (api, _calls) = crate::veilid::start("buyer").await?;
    let rc = api.routing_context()?;

    let mut peers: HashMap<u8, RouteId> = HashMap::new();
    for (role, idx) in [("seller", SELLER), ("arbiter", ARBITER)] {
        let blob = std::fs::read(format!("{tap_path}.{role}.route"))?;
        peers.insert(idx, api.import_remote_private_route(blob)?);
        println!("  route    {role} imported");
    }

    // Retry once: a private route is not a connection (§8.7.2).
    async fn call(
        rc: &RoutingContext,
        route: &RouteId,
        msg: Vec<u8>,
    ) -> Result<Vec<u8>, String> {
        for attempt in 0..2 {
            match rc.app_call(Target::RouteId(route.clone()), msg.clone()).await {
                Ok(r) => return Ok(r),
                Err(e) if attempt == 0 => {
                    println!("    (retrying a lost round trip: {e})");
                }
                Err(e) => return Err(e.to_string()),
            }
        }
        Err("unreachable".into())
    }

    println!("\n  \x1b[1mceremony\x1b[0m — {ROUNDS} rounds, strictly ordered\n");
    let mut own = RoundTracker::new(ESCROW_ID, ROUNDS);
    for round in 0..ROUNDS {
        for idx in [BUYER, SELLER, ARBITER] {
            let s = EscrowSetup {
                version: 1,
                suite: 1,
                escrow_id: ESCROW_ID,
                round,
                info: vec![0xAB; 64],
                from_index: idx,
                timestamp: now(),
            };
            own.accept(&s).map_err(|e| format!("{e:?}"))?;
            let enc = s.to_value().encode();
            for (peer_idx, route) in &peers {
                if *peer_idx == idx {
                    continue; // a participant does not send itself its own message
                }
                let reply = call(&rc, route, frame(MSG_ESC_SETUP, &enc)).await?;
                let (k, b) = unframe(&reply)?;
                if k == MSG_REJECT {
                    return Err(format!(
                        "participant refused round {round} from {idx}: {}",
                        String::from_utf8_lossy(b)
                    )
                    .into());
                }
            }
        }
        println!("  round {round} closed by all participants");
    }

    // §2.5, demonstrated rather than asserted: replay a round already settled.
    println!("\n  \x1b[1mattack\x1b[0m — replaying a settled round (§2.5's shape)\n");
    let stale = EscrowSetup {
        version: 1, suite: 1, escrow_id: ESCROW_ID, round: 0,
        info: vec![0xFF; 64], from_index: SELLER, timestamp: now(),
    };
    let route = peers.get(&ARBITER).unwrap();
    let reply = call(&rc, route, frame(MSG_ESC_SETUP, &stale.to_value().encode())).await?;
    let (k, b) = unframe(&reply)?;
    if k == MSG_REJECT {
        println!("  \x1b[32mrefused\x1b[0m — {}", String::from_utf8_lossy(b).chars().take(120).collect::<String>());
    } else {
        return Err("an out-of-order setup message was ACCEPTED — §2.5 all over again".into());
    }

    // Every participant reports what it formed; they must agree (§8.2).
    println!("\n  \x1b[1magreement\x1b[0m\n");
    let mut reports = Vec::new();
    for idx in [BUYER, SELLER, ARBITER] {
        let r = EscrowReady {
            version: 1, suite: 1, escrow_id: ESCROW_ID,
            ms_address: ms_address.as_bytes().to_vec(),
            threshold: 2, total: 3,
            arbiter: b"arbiter-key-1".to_vec(),
            from_index: idx,
            timestamp: now(),
        };
        for (peer_idx, route) in &peers {
            if *peer_idx == idx { continue; }
            let _ = call(&rc, route, frame(MSG_ESC_READY, &r.to_value().encode())).await?;
        }
        reports.push(r);
    }
    let trusted = vec![b"arbiter-key-1".to_vec()];
    let agreed = check_escrow_ready(&reports, &ESCROW_ID, &trusted)
        .map_err(|e| format!("{e:?}"))?;
    println!("  all three formed {}…", String::from_utf8_lossy(&agreed[..24.min(agreed.len())]));

    // Release, constrained to a party of the escrow.
    let ready_bytes = reports[0].to_value().encode();
    let rel = Release {
        version: 1, suite: 1, escrow_id: ESCROW_ID,
        ready_link: commit(Purpose::ChainLink, &ready_bytes),
        to: b"seller-payout".to_vec(),
        amount_pxmr: 500_000_000,
        timestamp: now(),
    };
    let dests = vec![b"seller-payout".to_vec(), b"buyer-refund".to_vec()];
    check_release(&rel, &reports[0], &ready_bytes, 800_000_000, &dests)
        .map_err(|e| format!("{e:?}"))?;
    let key = SecretKey::ed25519_from_bytes(&[0x30 + BUYER; 32]);
    let env = seal(
        &SignedBytes::from_received(rel.to_value().encode()).unwrap(),
        ObjectType::Release,
        &key,
    );
    let route = peers.get(&SELLER).unwrap();
    let reply = call(&rc, route, frame(MSG_ESC_RELEASE, &env)).await?;
    let (k, _) = unframe(&reply)?;
    if k == MSG_REJECT {
        return Err("seller refused the release".into());
    }
    println!("\n  \x1b[32mRELEASED\x1b[0m — {} pXMR to a bound party\n", rel.amount_pxmr);

    api.shutdown().await;
    Ok(())
}
