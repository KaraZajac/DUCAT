//! Escrow (§8.2) and bonded fast settlement (§17.4, §17.5) — the two paths the
//! manifest admitted had no coverage at all.

use ducat_core::cbor::{decode, Value};
use ducat_core::commit::{commit, Purpose};
use ducat_core::escrow::*;
use ducat_core::reject::RejectCode;
use ducat_core::wire::*;

const T0: u64 = 1_800_000_000;
const FARE: u64 = 21_000_000_000;
const EID: [u8; 32] = [0xE5; 32];

fn accept_pair() -> (Accept, Vec<u8>) {
    let a = Accept {
        version: 1,
        suite: 1,
        nonce: [0x22; 16],
        offer_hash: [0x11; 32],
        amount_final: FARE,
        dest: Some(b"driver-addr".to_vec()),
        reader_session_pk: vec![0x33; 32],
        timestamp: T0,
        chosen_version: 1,
        chosen_suite: 1,
        refund_to: Some(b"payer-refund-addr".to_vec()),
    };
    let b = a.to_value().encode();
    (a, b)
}

fn receipt_pair(accept_bytes: &[u8]) -> (Receipt, Vec<u8>) {
    let r = Receipt {
        version: 1,
        suite: 1,
        accept_hash: commit(Purpose::ChainLink, accept_bytes),
        prev: commit(Purpose::ChainLink, accept_bytes),
        amount_final: FARE,
        timestamp: T0 + 5,
        unilateral: false,
    };
    let b = r.to_value().encode();
    (r, b)
}

fn txid_for(accept_bytes: &[u8], amount: u64) -> TxId {
    TxId {
        version: 1,
        suite: 1,
        accept_link: commit(Purpose::ChainLink, accept_bytes),
        txid: [0x77; 32],
        amount_pxmr: amount,
        timestamp: T0 + 2,
    }
}

// ---------------------------------------------------------------- TXID ----

#[test]
fn txid_round_trips_and_binds_to_its_accept() {
    let (a, ab) = accept_pair();
    let t = txid_for(&ab, FARE);
    let enc = t.to_value().encode();
    assert_eq!(TxId::from_value(decode(&enc).unwrap()).unwrap(), t);
    assert_eq!(check_txid(&t, &a, &ab).unwrap(), FARE);
}

/// The API returns the ACCEPT's amount, never the TXID's. A payee that scans for
/// the figure the payer supplied has verified the payer's arithmetic about the
/// payer's own debt.
#[test]
fn the_amount_to_scan_for_comes_from_the_accept() {
    let (a, ab) = accept_pair();
    let honest = check_txid(&txid_for(&ab, FARE), &a, &ab).unwrap();
    assert_eq!(honest, a.amount_final);
    // An underpayment announced honestly is still an underpayment.
    assert_eq!(
        check_txid(&txid_for(&ab, FARE - 1), &a, &ab).unwrap_err().code,
        RejectCode::PriceMismatch
    );
}

#[test]
fn a_txid_for_another_transaction_is_refused() {
    let (a, ab) = accept_pair();
    let mut t = txid_for(&ab, FARE);
    t.accept_link = [0x99; 32];
    assert_eq!(
        check_txid(&t, &a, &ab).unwrap_err().code,
        RejectCode::CommitMismatch
    );
}

// ------------------------------------------------------------- TXPROOF ----

#[test]
fn a_proof_must_be_bound_to_the_transcript_it_is_offered_for() {
    let (_, ab) = accept_pair();
    let good = TxProof {
        version: 1,
        suite: 1,
        txid: [0x77; 32],
        proof: b"OutProofV2...".to_vec(),
        destination: b"driver-addr".to_vec(),
        proof_message: commit(Purpose::ChainLink, &ab),
        amount_pxmr: FARE,
        timestamp: T0 + 9,
    };
    let enc = good.to_value().encode();
    assert_eq!(TxProof::from_value(decode(&enc).unwrap()).unwrap(), good);
    assert!(check_tx_proof_binding(&good, &ab, b"driver-addr", FARE).is_ok());

    // The obvious implementation leaves the Monero proof message empty. Then
    // any proof the payer ever generated for that transaction replays into an
    // unrelated dispute.
    let mut unbound = good.clone();
    unbound.proof_message = [0u8; 32];
    assert_eq!(
        check_tx_proof_binding(&unbound, &ab, b"driver-addr", FARE).unwrap_err().code,
        RejectCode::CommitMismatch
    );
}

#[test]
fn a_proof_about_a_different_address_proves_nothing_here() {
    let (_, ab) = accept_pair();
    let p = TxProof {
        version: 1, suite: 1, txid: [0x77; 32],
        proof: b"OutProofV2...".to_vec(),
        destination: b"someone-else".to_vec(),
        proof_message: commit(Purpose::ChainLink, &ab),
        amount_pxmr: FARE, timestamp: T0,
    };
    assert_eq!(
        check_tx_proof_binding(&p, &ab, b"driver-addr", FARE).unwrap_err().code,
        RejectCode::PolicyRefused
    );
}

// --------------------------------------------------------- SLASH_CLAIM ----

fn claim(reason: SlashReason, key_image: Option<[u8; 32]>, ab: &[u8], rb: &[u8]) -> SlashClaim {
    SlashClaim {
        version: 1,
        suite: 1,
        accept_link: commit(Purpose::ChainLink, ab),
        receipt_link: commit(Purpose::ChainLink, rb),
        txid: [0x77; 32],
        reason,
        key_image,
        claim_pxmr: FARE,
        timestamp: T0 + 100,
    }
}

#[test]
fn a_cure_window_claim_waits_for_the_cure_window() {
    let (a, ab) = accept_pair();
    let (r, rb) = receipt_pair(&ab);
    let c = claim(SlashReason::CureWindowExpired, None, &ab, &rb);
    assert_eq!(
        check_slash_claim(&c, &a, &ab, &r, &rb, 19, 20).unwrap_err().code,
        RejectCode::PolicyRefused,
        "non-confirmation is usually a fee problem, not fraud"
    );
    assert!(check_slash_claim(&c, &a, &ab, &r, &rb, 20, 20).is_ok());
}

/// The reason that skips the waiting period is the one worth forging, so it is
/// the one that must carry its evidence.
#[test]
fn a_double_spend_claim_must_carry_the_key_image() {
    let (a, ab) = accept_pair();
    let (r, rb) = receipt_pair(&ab);
    let bare = claim(SlashReason::ConflictingKeyImage, None, &ab, &rb);
    assert_eq!(
        check_slash_claim(&bare, &a, &ab, &r, &rb, 0, 20).unwrap_err().code,
        RejectCode::Malformed
    );
    let evidenced = claim(SlashReason::ConflictingKeyImage, Some([0x5A; 32]), &ab, &rb);
    assert!(
        check_slash_claim(&evidenced, &a, &ab, &r, &rb, 0, 20).is_ok(),
        "a conflicting key image is self-authenticating and skips the cure window"
    );
}

#[test]
fn a_claim_cannot_exceed_what_was_agreed() {
    let (a, ab) = accept_pair();
    let (r, rb) = receipt_pair(&ab);
    let mut c = claim(SlashReason::CureWindowExpired, None, &ab, &rb);
    c.claim_pxmr = FARE + 1;
    assert_eq!(
        check_slash_claim(&c, &a, &ab, &r, &rb, 30, 20).unwrap_err().code,
        RejectCode::PriceMismatch
    );
}

#[test]
fn slash_claim_round_trips() {
    let (_, ab) = accept_pair();
    let (_, rb) = receipt_pair(&ab);
    for c in [
        claim(SlashReason::CureWindowExpired, None, &ab, &rb),
        claim(SlashReason::ConflictingKeyImage, Some([0x5A; 32]), &ab, &rb),
    ] {
        let enc = c.to_value().encode();
        assert_eq!(SlashClaim::from_value(decode(&enc).unwrap()).unwrap(), c);
    }
}

// -------------------------------------------------------- THE CEREMONY ----

fn setup(round: u64, from: u8) -> EscrowSetup {
    EscrowSetup {
        version: 1, suite: 1, escrow_id: EID, round,
        info: vec![0xAB; 64], from_index: from, timestamp: T0,
    }
}

#[test]
fn a_two_round_ceremony_converges() {
    let mut t = RoundTracker::new(EID, 2);
    for round in 0..2 {
        for who in [BUYER, SELLER, ARBITER] {
            let closed = t.accept(&setup(round, who)).unwrap();
            assert_eq!(closed, who == ARBITER);
        }
    }
    assert!(t.complete());
    assert_eq!(
        t.accept(&setup(2, BUYER)).unwrap_err().code,
        RejectCode::StateViolation
    );
}

/// §2.5. RetoSwap — this exact structure in production — was drained of ~$2.7M
/// by a forged, out-of-order message that overwrote settled state.
#[test]
fn an_out_of_order_setup_message_is_refused() {
    let mut t = RoundTracker::new(EID, 2);
    assert_eq!(
        t.accept(&setup(1, BUYER)).unwrap_err().code,
        RejectCode::StateViolation,
        "round 1 arriving while round 0 is open is the shape of the exploit"
    );
    t.accept(&setup(0, BUYER)).unwrap();
    assert_eq!(t.accept(&setup(1, SELLER)).unwrap_err().code, RejectCode::StateViolation);
}

#[test]
fn a_participant_cannot_contribute_twice_to_one_round() {
    let mut t = RoundTracker::new(EID, 2);
    t.accept(&setup(0, BUYER)).unwrap();
    assert_eq!(
        t.accept(&setup(0, BUYER)).unwrap_err().code,
        RejectCode::Replay,
        "a second contribution would revise state the ceremony has settled"
    );
}

#[test]
fn a_setup_message_for_another_escrow_is_refused() {
    let mut t = RoundTracker::new(EID, 2);
    let mut s = setup(0, BUYER);
    s.escrow_id = [0x01; 32];
    assert_eq!(t.accept(&s).unwrap_err().code, RejectCode::PolicyRefused);
}

#[test]
fn setup_round_trips_and_rejects_nonsense() {
    let s = setup(0, SELLER);
    let enc = s.to_value().encode();
    assert_eq!(EscrowSetup::from_value(decode(&enc).unwrap()).unwrap(), s);

    let mut bad = match s.to_value() { Value::Map(m) => m, _ => unreachable!() };
    bad.insert(107u64, Value::Uint(3)); // participant index outside the group
    assert_eq!(
        EscrowSetup::from_value(Value::Map(bad)).unwrap_err().code,
        RejectCode::Malformed
    );

    let mut empty = s.clone();
    empty.info.clear();
    let enc = empty.to_value().encode();
    assert_eq!(
        EscrowSetup::from_value(decode(&enc).unwrap()).unwrap_err().code,
        RejectCode::Malformed
    );
}

// ------------------------------------------------------------- READY ------

fn ready(from: u8, addr: &[u8], arbiter: &[u8]) -> EscrowReady {
    EscrowReady {
        version: 1, suite: 1, escrow_id: EID,
        ms_address: addr.to_vec(), threshold: 2, total: 3,
        arbiter: arbiter.to_vec(), from_index: from, timestamp: T0 + 200,
    }
}

fn trusted() -> Vec<Vec<u8>> { vec![b"arbiter-key-1".to_vec()] }

#[test]
fn all_three_must_report_the_same_wallet() {
    let a = b"53multisigaddress";
    let reports: Vec<_> = [BUYER, SELLER, ARBITER]
        .iter().map(|i| ready(*i, a, b"arbiter-key-1")).collect();
    assert_eq!(check_escrow_ready(&reports, &EID, &trusted()).unwrap(), a);

    // Three successful ceremonies, two different groups: the funds go to a
    // wallet the payer holds no share of.
    let mut split = reports.clone();
    split[2].ms_address = b"53someotheraddress".to_vec();
    assert_eq!(
        check_escrow_ready(&split, &EID, &trusted()).unwrap_err().code,
        RejectCode::CommitMismatch
    );
}

/// §2.5's other half: the arbiter must come from the market descriptor, never
/// from a message. The forged message in the real exploit was well-formed.
#[test]
fn an_arbiter_outside_the_market_set_is_refused() {
    let a = b"53multisigaddress";
    let reports: Vec<_> = [BUYER, SELLER, ARBITER]
        .iter().map(|i| ready(*i, a, b"attacker-key")).collect();
    assert_eq!(
        check_escrow_ready(&reports, &EID, &trusted()).unwrap_err().code,
        RejectCode::UntrustedArbiterSet
    );
}

#[test]
fn a_silent_participant_has_not_agreed_to_anything() {
    let a = b"53multisigaddress";
    let two: Vec<_> = [BUYER, SELLER].iter().map(|i| ready(*i, a, b"arbiter-key-1")).collect();
    assert_eq!(
        check_escrow_ready(&two, &EID, &trusted()).unwrap_err().code,
        RejectCode::PolicyRefused
    );
    let dup = vec![ready(BUYER, a, b"arbiter-key-1"), ready(BUYER, a, b"arbiter-key-1"),
                   ready(SELLER, a, b"arbiter-key-1")];
    assert_eq!(
        check_escrow_ready(&dup, &EID, &trusted()).unwrap_err().code,
        RejectCode::Replay
    );
}

#[test]
fn escrow_must_be_two_of_three() {
    let a = b"53multisigaddress";
    let mut reports: Vec<_> = [BUYER, SELLER, ARBITER]
        .iter().map(|i| ready(*i, a, b"arbiter-key-1")).collect();
    for r in reports.iter_mut() { r.threshold = 1; }
    assert_eq!(
        check_escrow_ready(&reports, &EID, &trusted()).unwrap_err().code,
        RejectCode::PolicyRefused
    );
}

// ----------------------------------------------------------- RELEASE ------

#[test]
fn a_release_may_only_pay_a_party_to_the_escrow() {
    let rdy = ready(BUYER, b"53multisigaddress", b"arbiter-key-1");
    let rb = rdy.to_value().encode();
    let dests = vec![b"seller-payout".to_vec(), b"buyer-refund".to_vec()];
    let mut rel = Release {
        version: 1, suite: 1, escrow_id: EID,
        ready_link: commit(Purpose::ChainLink, &rb),
        to: b"seller-payout".to_vec(),
        amount_pxmr: FARE, timestamp: T0 + 300,
    };
    let enc = rel.to_value().encode();
    assert_eq!(Release::from_value(decode(&enc).unwrap()).unwrap(), rel);
    assert!(check_release(&rel, &rdy, &rb, FARE, &dests).is_ok());

    // The check the happy path never exercises: both parties co-signing a
    // release to the seller looks identical whether or not the destination was
    // ever constrained.
    rel.to = b"attacker-addr".to_vec();
    assert_eq!(
        check_release(&rel, &rdy, &rb, FARE, &dests).unwrap_err().code,
        RejectCode::PolicyRefused
    );
}

#[test]
fn a_release_cannot_exceed_or_zero_out_the_escrow() {
    let rdy = ready(BUYER, b"53multisigaddress", b"arbiter-key-1");
    let rb = rdy.to_value().encode();
    let dests = vec![b"seller-payout".to_vec()];
    let base = Release {
        version: 1, suite: 1, escrow_id: EID,
        ready_link: commit(Purpose::ChainLink, &rb),
        to: b"seller-payout".to_vec(), amount_pxmr: FARE, timestamp: T0,
    };
    let mut over = base.clone();
    over.amount_pxmr = FARE + 1;
    assert_eq!(
        check_release(&over, &rdy, &rb, FARE, &dests).unwrap_err().code,
        RejectCode::PriceMismatch
    );
    let mut zero = base.clone();
    zero.amount_pxmr = 0;
    assert_eq!(
        check_release(&zero, &rdy, &rb, FARE, &dests).unwrap_err().code,
        RejectCode::PriceMismatch,
        "a release of zero moves nothing and closes nothing"
    );
    // Partial release is legitimate — a ruling can award less than the whole.
    let mut partial = base.clone();
    partial.amount_pxmr = FARE / 2;
    assert!(check_release(&partial, &rdy, &rb, FARE, &dests).is_ok());
}

#[test]
fn a_release_against_a_different_formation_is_refused() {
    let rdy = ready(BUYER, b"53multisigaddress", b"arbiter-key-1");
    let rb = rdy.to_value().encode();
    let dests = vec![b"seller-payout".to_vec()];
    let mut rel = Release {
        version: 1, suite: 1, escrow_id: EID,
        ready_link: [0xAA; 32],
        to: b"seller-payout".to_vec(), amount_pxmr: FARE, timestamp: T0,
    };
    assert_eq!(
        check_release(&rel, &rdy, &rb, FARE, &dests).unwrap_err().code,
        RejectCode::CommitMismatch
    );
    rel.ready_link = commit(Purpose::ChainLink, &rb);
    rel.escrow_id = [0x01; 32];
    assert_eq!(
        check_release(&rel, &rdy, &rb, FARE, &dests).unwrap_err().code,
        RejectCode::PolicyRefused
    );
}
