//! One complete transaction, run for real between two personas.
//!
//! Every object is built, signed, sealed, transmitted as bytes, opened,
//! verified, and fed through the state machine. The only thing simulated is the
//! wire — messages move through a `Vec<u8>` rather than a Veilid private route,
//! which is the part Phase 0b already proved works.
//!
//! Notably this runs the *payer's* checks the way §15.5 requires: the payer
//! recomputes the commitment itself and refuses on mismatch rather than
//! trusting what the presenter claims.

use ducat_core::cbor::decode;
use ducat_core::commit::{commit, Purpose};
use ducat_core::sig::{ObjectType, SignedBytes};
use ducat_core::state::{Event, Role, SettleMode};
use ducat_core::wire::*;

use crate::persona::{CompletedTransaction, Persona};
use crate::transport::Wire;

pub struct TxOutcome {
    pub amount_pxmr: u64,
    pub transcript: Vec<Vec<u8>>,
}

/// Run a tap-to-receipt transaction. `payee` presents, `payer` reads.
pub fn transact(
    wire: &mut Wire,
    payee: &mut Persona,
    payer: &mut Persona,
    amount_pxmr: u64,
    nonce: [u8; 16],
    dest: Option<Vec<u8>>,
    label: &str,
) -> Result<TxOutcome, String> {
    let profile = payee.kind.profile();
    payee.reset();
    payer.reset();

    // ---- payee builds the offer, then the bootstrap that commits to it ----
    let offer = FullOffer {
        version: 1,
        suite: 1,
        profile,
        payto: payee.payto.clone(),
        amount_pxmr,
        supported_versions: vec![1],
        supported_suites: vec![1, 2],
        settle_mode: 0, // direct
        fee_policy: FeePolicy::PayerPays,
        nonce_echo: nonce,
        terms: Terms::default(),
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
        session_pk: payee.public().to_bytes(),
        route: vec![0x11; 32],
        offer_commit: offer.commitment(),
        dest: dest.clone(),
        session_ref: None,
    };

    let tap_env = seal(
        &SignedBytes::from_value(tap.to_value()),
        ObjectType::TapPresent,
        &payee.persona_key,
    );
    wire.send(&payee.name, &payer.name, "TapPresent", &tap_env);

    // ---- payer receives the bootstrap ----
    let (kind, tap_body) = open(&wire.recv(), &payee.public()).map_err(|e| {
        format!("{} rejected the tap: {:?}", payer.name, e.code)
    })?;
    if kind != ObjectType::TapPresent {
        return Err(format!("{}: wrong object type on the tap", payer.name));
    }
    let tap_seen = TapPresent::from_value(tap_body.value().clone())
        .map_err(|e| format!("{} could not parse the tap: {:?}", payer.name, e.code))?;
    payer.step(Role::Payer, SettleMode::Direct, &Event::TapPresent)?;

    // ---- payee delivers the offer over the channel the tap opened ----
    let offer_env = seal(
        &SignedBytes::from_value(offer.to_value()),
        ObjectType::FullOffer,
        &payee.persona_key,
    );
    wire.send(&payee.name, &payer.name, "FullOffer", &offer_env);

    let (_, offer_body) = open(&wire.recv(), &payee.public())
        .map_err(|e| format!("{} rejected the offer: {:?}", payer.name, e.code))?;
    let offer_seen = FullOffer::from_value(offer_body.value().clone())
        .map_err(|e| format!("{} could not parse the offer: {:?}", payer.name, e.code))?;

    // §15.5: the payer verifies the commitment ITSELF before anything is shown
    // to a human, and refuses a mismatch. This is the swap defence.
    let recomputed = commit(Purpose::Offer, offer_body.bytes());
    if recomputed != tap_seen.offer_commit {
        return Err(format!(
            "{}: offer does not match the tap's commitment — refusing",
            payer.name
        ));
    }
    payer.step(Role::Payer, SettleMode::Direct, &Event::FullOffer)?;

    // ---- confirm screen: the payer signs only what it derived ----
    let accept = Accept {
        version: 1,
        suite: 1,
        nonce: tap_seen.nonce,
        offer_hash: recomputed,
        amount_final: offer_seen.amount_pxmr,
        dest: dest.clone(),
        reader_session_pk: payer.public().to_bytes(),
        timestamp: 1_800_000_005,
        chosen_version: 1,
        chosen_suite: 1,
    };
    let accept_bytes = accept.to_value().encode();
    let accept_env = seal(
        &SignedBytes::from_value(accept.to_value()),
        ObjectType::Accept,
        &payer.persona_key,
    );
    payer.step(Role::Payer, SettleMode::Direct, &Event::Accept { from: Role::Payer })?;
    wire.send(&payer.name, &payee.name, "ACCEPT", &accept_env);

    let (_, accept_body) = open(&wire.recv(), &payer.public())
        .map_err(|e| format!("{} rejected the accept: {:?}", payee.name, e.code))?;
    let accept_seen = Accept::from_value(accept_body.value().clone())
        .map_err(|e| format!("{} could not parse the accept: {:?}", payee.name, e.code))?;
    payee.step(Role::Payee, SettleMode::Direct, &Event::TapPresent)?;
    payee.step(Role::Payee, SettleMode::Direct, &Event::FullOffer)?;
    payee.step(Role::Payee, SettleMode::Direct, &Event::Accept { from: Role::Payer })?;

    // ---- settlement, then delivery, then closure ----
    payer.step(Role::Payer, SettleMode::Direct, &Event::Fund)?;
    payee.step(Role::Payee, SettleMode::Direct, &Event::Fund)?;
    wire.note(&payer.name, &payee.name, &format!("FUND {} pxmr", amount_pxmr));

    payer.step(Role::Payer, SettleMode::Direct, &Event::Proof)?;
    payee.step(Role::Payee, SettleMode::Direct, &Event::Proof)?;

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
    let receipt_bytes = receipt.to_value().encode();
    let receipt_env = seal(
        &SignedBytes::from_value(receipt.to_value()),
        ObjectType::Receipt,
        &payee.persona_key,
    );
    wire.send(&payee.name, &payer.name, "RECEIPT", &receipt_env);

    let (_, receipt_body) = open(&wire.recv(), &payee.public())
        .map_err(|e| format!("{} rejected the receipt: {:?}", payer.name, e.code))?;
    let receipt_seen = Receipt::from_value(receipt_body.value().clone())
        .map_err(|e| format!("{} could not parse the receipt: {:?}", payer.name, e.code))?;

    payer.step(Role::Payer, SettleMode::Direct, &Event::Receipt)?;
    payee.step(Role::Payee, SettleMode::Direct, &Event::Receipt)?;

    // ---- both sides verify the whole chain, as §6 promises they can ----
    verify_transcript(&tap_seen, &offer_seen, &accept_seen, &accept_bytes, &receipt_seen)
        .map_err(|e| format!("transcript failed verification: {:?}", e.code))?;

    // Sanity: decoding is canonical, so re-encoding must be byte-identical.
    for (n, b) in [
        ("tap", tap.to_value().encode()),
        ("offer", offer_bytes.clone()),
        ("accept", accept_bytes.clone()),
        ("receipt", receipt_bytes.clone()),
    ] {
        if decode(&b).map(|v| v.encode()).as_deref() != Ok(b.as_slice()) {
            return Err(format!("{} is not canonical", n));
        }
    }

    let transcript = vec![
        tap.to_value().encode(),
        offer_bytes,
        accept_bytes,
        receipt_bytes,
    ];

    payer.balance_pxmr = payer.balance_pxmr.saturating_sub(amount_pxmr);
    payee.balance_pxmr += amount_pxmr;

    payer.receipts.push(CompletedTransaction {
        counterparty: payee.name.clone(),
        profile,
        amount_pxmr,
        paid: true,
        transcript: transcript.clone(),
    });
    payee.receipts.push(CompletedTransaction {
        counterparty: payer.name.clone(),
        profile,
        amount_pxmr,
        paid: false,
        transcript: transcript.clone(),
    });

    wire.note("", "", &format!("✓ {} complete, transcript verified by both", label));

    Ok(TxOutcome {
        amount_pxmr,
        transcript,
    })
}
