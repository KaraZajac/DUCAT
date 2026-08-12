//! The market, settled on stagenet.
//!
//! Same protocol path as the offline simulation — same objects, signatures,
//! state machine and transcript verification — but the FUND step moves real
//! sXMR between five real wallets, and the ledger at the end is checked against
//! the chain rather than against the simulator's own arithmetic.
//!
//! This is where §17.2 stops being a claim. A payment consumes a whole output
//! and its change returns locked for ten blocks, so a participant's capacity for
//! *consecutive* payments is a count of unlocked outputs. The run reports that
//! count before each transaction and explains any refusal in those terms.

use std::collections::BTreeMap;
use std::thread::sleep;
use std::time::Duration;

use ducat_core::commit::{commit, Purpose};
use ducat_core::sig::{ObjectType, SignedBytes};
use ducat_core::state::{Event, Role, SettleMode};
use ducat_core::wire::*;

use crate::persona::{Kind, Persona};
use crate::transport::Wire;
use crate::wallet::Wallet;

pub struct Party {
    pub persona: Persona,
    pub wallet: Wallet,
}

pub struct Settled {
    pub label: String,
    pub payer: String,
    pub payee: String,
    pub amount: u64,
    pub txid: String,
}

fn fmt(p: u64) -> String {
    format!("{:.6}", p as f64 / 1e12)
}

/// One transaction, protocol and settlement both real.
#[allow(clippy::too_many_arguments)]
pub fn transact_live(
    wire: &mut Wire,
    payee: &mut Party,
    payer: &mut Party,
    amount_pxmr: u64,
    nonce: [u8; 16],
    dest: Option<Vec<u8>>,
    label: &str,
) -> Result<Settled, String> {
    let profile = payee.persona.kind.profile();
    payee.persona.reset();
    payer.persona.reset();

    // §17.2: capacity for consecutive payments is a count of unlocked outputs,
    // never a balance. Check it before promising anything.
    payer.wallet.refresh()?;
    let b = payer.wallet.balance()?;
    println!(
        "    {} has {} XMR unlocked across {} output(s){}",
        payer.persona.name,
        fmt(b.unlocked),
        b.unlocked_outputs,
        if b.blocks_to_unlock > 0 {
            format!(", {} blocks to next unlock", b.blocks_to_unlock)
        } else {
            String::new()
        }
    );
    if b.unlocked_outputs == 0 {
        return Err(format!(
            "{} has no unlocked outputs — §17.2's predicted failure, not a bug",
            payer.persona.name
        ));
    }

    // ---- payee: offer, then a bootstrap committing to it ----
    let offer = FullOffer {
        version: 1,
        suite: 1,
        profile,
        payto: payee.wallet.address.as_bytes().to_vec(),
        amount_pxmr,
        supported_versions: vec![1],
        supported_suites: vec![1, 2],
        settle_mode: 0,
        fee_policy: FeePolicy::PayerPays,
        nonce_echo: nonce,
    };
    let offer_bytes = offer.to_value().encode();
    let tap = TapPresent {
        version: 1,
        suite: 1,
        profile,
        presenter_role: PresenterRole::Payee,
        amount_authority: AmountAuthority::Fixed,
        intent: Intent::Oneshot,
        rmode: ReachMode::Token,
        nonce,
        expiry: 1_800_000_030,
        session_pk: payee.persona.public().to_bytes(),
        route: vec![0x11; 32],
        offer_commit: offer.commitment(),
        dest: dest.clone(),
        session_ref: None,
    };

    let env = seal(
        &SignedBytes::from_value(tap.to_value()),
        ObjectType::TapPresent,
        &payee.persona.persona_key,
    );
    wire.send(&payee.persona.name, &payer.persona.name, "TapPresent", &env);
    let (_, tap_body) = open(&wire.recv(), &payee.persona.public())
        .map_err(|e| format!("tap rejected: {:?}", e.code))?;
    let tap_seen = TapPresent::from_value(tap_body.value().clone())
        .map_err(|e| format!("tap unparseable: {:?}", e.code))?;
    payer
        .persona
        .step(Role::Payer, SettleMode::Direct, &Event::TapPresent)?;

    let env = seal(
        &SignedBytes::from_value(offer.to_value()),
        ObjectType::FullOffer,
        &payee.persona.persona_key,
    );
    wire.send(&payee.persona.name, &payer.persona.name, "FullOffer", &env);
    let (_, offer_body) = open(&wire.recv(), &payee.persona.public())
        .map_err(|e| format!("offer rejected: {:?}", e.code))?;
    let offer_seen = FullOffer::from_value(offer_body.value().clone())
        .map_err(|e| format!("offer unparseable: {:?}", e.code))?;

    // §15.5 — the payer verifies the commitment itself.
    let recomputed = commit(Purpose::Offer, offer_body.bytes());
    if recomputed != tap_seen.offer_commit {
        return Err("offer does not match the tap's commitment".into());
    }
    payer
        .persona
        .step(Role::Payer, SettleMode::Direct, &Event::FullOffer)?;

    let accept = Accept {
        version: 1,
        suite: 1,
        nonce: tap_seen.nonce,
        offer_hash: recomputed,
        amount_final: offer_seen.amount_pxmr,
        dest: dest.clone(),
        reader_session_pk: payer.persona.public().to_bytes(),
        timestamp: 1_800_000_005,
        chosen_version: 1,
        chosen_suite: 1,
    };
    let accept_bytes = accept.to_value().encode();
    let env = seal(
        &SignedBytes::from_value(accept.to_value()),
        ObjectType::Accept,
        &payer.persona.persona_key,
    );
    payer.persona.step(
        Role::Payer,
        SettleMode::Direct,
        &Event::Accept { from: Role::Payer },
    )?;
    wire.send(&payer.persona.name, &payee.persona.name, "ACCEPT", &env);
    let (_, accept_body) = open(&wire.recv(), &payer.persona.public())
        .map_err(|e| format!("accept rejected: {:?}", e.code))?;
    let accept_seen = Accept::from_value(accept_body.value().clone())
        .map_err(|e| format!("accept unparseable: {:?}", e.code))?;

    for ev in [
        Event::TapPresent,
        Event::FullOffer,
        Event::Accept { from: Role::Payer },
    ] {
        payee.persona.step(Role::Payee, SettleMode::Direct, &ev)?;
    }

    // ---- FUND: real sXMR moves here ----
    let payto = String::from_utf8(offer_seen.payto.clone())
        .map_err(|_| "payto is not a valid address string".to_string())?;
    let txid = payer.wallet.pay(&payto, accept_seen.amount_final)?;
    wire.note(
        &payer.persona.name,
        &payee.persona.name,
        &format!("FUND {} XMR  txid {}…", fmt(amount_pxmr), &txid[..16]),
    );
    payer
        .persona
        .step(Role::Payer, SettleMode::Direct, &Event::Fund)?;
    payee
        .persona
        .step(Role::Payee, SettleMode::Direct, &Event::Fund)?;

    for ev in [Event::Proof] {
        payer.persona.step(Role::Payer, SettleMode::Direct, &ev)?;
        payee.persona.step(Role::Payee, SettleMode::Direct, &ev)?;
    }

    let link = commit(Purpose::ChainLink, accept_body.bytes());
    let receipt = Receipt {
        version: 1,
        suite: 1,
        accept_hash: link,
        prev: link,
        amount_final: accept_seen.amount_final,
        timestamp: 1_800_000_010,
        unilateral: false,
    };
    let env = seal(
        &SignedBytes::from_value(receipt.to_value()),
        ObjectType::Receipt,
        &payee.persona.persona_key,
    );
    wire.send(&payee.persona.name, &payer.persona.name, "RECEIPT", &env);
    let (_, receipt_body) = open(&wire.recv(), &payee.persona.public())
        .map_err(|e| format!("receipt rejected: {:?}", e.code))?;
    let receipt_seen = Receipt::from_value(receipt_body.value().clone())
        .map_err(|e| format!("receipt unparseable: {:?}", e.code))?;
    payer
        .persona
        .step(Role::Payer, SettleMode::Direct, &Event::Receipt)?;
    payee
        .persona
        .step(Role::Payee, SettleMode::Direct, &Event::Receipt)?;

    verify_transcript(
        &tap_seen,
        &offer_seen,
        &accept_seen,
        &accept_bytes,
        &receipt_seen,
    )
    .map_err(|e| format!("transcript failed verification: {:?}", e.code))?;

    let _ = offer_bytes;
    println!("    ✓ {} settled, transcript verified by both", label);

    Ok(Settled {
        label: label.to_string(),
        payer: payer.persona.name.clone(),
        payee: payee.persona.name.clone(),
        amount: amount_pxmr,
        txid,
    })
}

/// Wait until every named party has at least one unlocked output, so the run
/// is not measuring the block clock instead of the protocol.
pub fn wait_for_outputs(parties: &BTreeMap<String, u16>, need: usize, max_wait_s: u64) -> bool {
    let deadline = std::time::Instant::now() + Duration::from_secs(max_wait_s);
    loop {
        let mut all = true;
        let mut line = Vec::new();
        for (name, port) in parties {
            let w = match Wallet::new(name, *port) {
                Ok(w) => w,
                Err(_) => {
                    all = false;
                    continue;
                }
            };
            let _ = w.refresh();
            let b = w.balance().unwrap_or(crate::wallet::Balance {
                total: 0,
                unlocked: 0,
                blocks_to_unlock: 0,
                unlocked_outputs: 0,
            });
            line.push(format!("{} {}o", name, b.unlocked_outputs));
            if b.unlocked_outputs < need {
                all = false;
            }
        }
        println!("    {}", line.join("  "));
        if all {
            return true;
        }
        if std::time::Instant::now() > deadline {
            return false;
        }
        sleep(Duration::from_secs(45));
    }
}

pub fn kind_of(name: &str) -> Kind {
    match name {
        "taxi_01" => Kind::Taxi,
        "coffee_01" => Kind::Coffee,
        "shopkeep_01" => Kind::Shopkeeper,
        _ => Kind::Consumer,
    }
}
