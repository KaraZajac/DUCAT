//! The paths where nobody co-signs: abandonment, meters, refunds.
//!
//! §6.2 calls post-`FUND`, pre-`RECEIPT` "the dangerous window" — the payer's
//! money is gone and the co-signed record does not exist yet. Every harness
//! before this one ran a flow where both parties stayed. These are the ones
//! where somebody leaves.
//!
//! The machinery under test is §6.2's two unilateral receipts, which **assert
//! opposite things** and are easy to conflate:
//!
//! - **Payment evidence** — the *payer* saying "I paid and hold no
//!   co-signature". Emitted when the payee vanishes after funding.
//! - **Debt evidence** — the *payee* saying "you owe me and never stopped the
//!   meter". Emitted when a payer walks out on a running tab.
//!
//! Conflating them has a merchant filing a payment it never received, or a
//! payer recording a debt it does not owe. The state machine decides which,
//! precisely so the decision is not made by whoever is writing the UI.

use std::time::Duration;

use ducat_core::commit::{commit, Purpose};
use ducat_core::sig::{ObjectType, SecretKey, SignedBytes};
use ducat_core::state::{transition, Effect, Event, Role, SettleMode, State};
use ducat_core::wire::*;

use crate::payee::now;

fn line(label: &str, s: State, e: Effect) {
    println!("  {label:<28} → {s:?}  effect {e:?}");
}

/// Drive both sides of each edge case through the real state machine, and build
/// the artifact each one is supposed to leave behind.
pub async fn run() -> Result<(), Box<dyn std::error::Error>> {
    println!("\n\x1b[1mDUCAT harness — the paths where nobody co-signs\x1b[0m\n");
    let key = SecretKey::ed25519_from_bytes(&[0x21; 32]);
    let mut failures = Vec::new();

    // ---- 1. The payee vanishes after the payer funds ----------------------
    println!("\x1b[1m  1. payee vanishes after FUND (§6.2)\x1b[0m");
    {
        let mut s = State::Funded;
        let t = transition(s, Role::Payer, SettleMode::Direct, &Event::DeliveryWindowExpired)
            .map_err(|e| format!("{e:?}"))?;
        line("delivery window expires", t.next, t.effect);
        s = t.next;
        if t.effect != Effect::EmitPaymentEvidence {
            failures.push("a funded payer left waiting must emit payment evidence");
        }
        if s != State::Closed {
            failures.push("the transaction must close rather than hang");
        }

        // The artifact: a receipt the payer signs alone.
        let accept_bytes = b"the-accept-that-was-signed".to_vec();
        let r = Receipt {
            version: 1,
            suite: 1,
            accept_hash: commit(Purpose::ChainLink, &accept_bytes),
            prev: commit(Purpose::ChainLink, &accept_bytes),
            amount_final: 300_000_000,
            timestamp: now(),
            // The flag is the whole point: this proves what the payer signed and
            // paid. It cannot prove delivery and must not claim to.
            unilateral: true,
        };
        let env = seal(
            &SignedBytes::from_received(r.to_value().encode()).unwrap(),
            ObjectType::Receipt,
            &key,
        );
        let back = Receipt::from_value(
            ducat_core::cbor::decode(
                &open(&env, &key.public()).map_err(|e| format!("{e:?}"))?.1.bytes().to_vec(),
            )
            .map_err(|e| format!("{e:?}"))?,
        )
        .map_err(|e| format!("{e:?}"))?;
        if !back.unilateral {
            failures.push("a single-sided receipt must survive the round trip flagged");
        }
        println!("    payment evidence: {} B, unilateral = {}", env.len(), back.unilateral);
        println!("    proves what the payer signed and paid; proves nothing about delivery\n");
    }

    // ---- 2. The payer walks out on a running meter ------------------------
    println!("\x1b[1m  2. payer abandons a running meter (§15.7)\x1b[0m");
    {
        let mut s = State::Quoted;
        let t = transition(s, Role::Payee, SettleMode::Direct, &Event::MeterStart)
            .map_err(|e| format!("{e:?}"))?;
        line("meter start", t.next, t.effect);
        s = t.next;
        if s != State::Metering {
            failures.push("a started meter must reach METERING");
        }

        // §18.4.1(8): METERING is deliberately not wall-clock bounded — a tab
        // that died sixty seconds after opening was a real bug. Elapsed time
        // here must be a no-op.
        let t = transition(s, Role::Payee, SettleMode::Direct, &Event::Elapsed(Duration::from_secs(3600)))
            .map_err(|e| format!("{e:?}"))?;
        line("one hour elapses", t.next, t.effect);
        if t.next != State::Metering {
            failures.push("METERING must not be closed by a wall clock — a bar tab lasts hours");
        }

        // §18.4.1(7): a payer cannot abort a live meter. Consuming and then
        // leaving owing nothing is exactly what that rule forbids.
        match transition(s, Role::Payer, SettleMode::Direct, &Event::Abort { from: Role::Payer }) {
            Err(e) => println!("    payer abort refused        → {:?}", e.code),
            Ok(_) => failures.push("a payer must not be able to abort a running meter"),
        }
        // The operator may void cleanly: comping a drink is ordinary commerce.
        let t = transition(s, Role::Payee, SettleMode::Direct, &Event::Abort { from: Role::Payee })
            .map_err(|e| format!("{e:?}"))?;
        line("operator voids", t.next, t.effect);

        // Abandonment routes through expiry, leaving evidence rather than a
        // clean exit with no record.
        let t = transition(s, Role::Payee, SettleMode::Direct, &Event::MeterExpired)
            .map_err(|e| format!("{e:?}"))?;
        line("meter expires unstopped", t.next, t.effect);
        if t.effect != Effect::EmitDebtEvidence {
            failures.push("an abandoned meter must emit debt evidence, not payment evidence");
        }
        println!("    debt evidence: the payee saying \"you owe me\" — the opposite claim\n");
    }

    // ---- 3. A refund, and where it is allowed to go -----------------------
    println!("\x1b[1m  3. refund destination (§7.3)\x1b[0m");
    {
        let accept = Accept {
            version: 1, suite: 1, nonce: [0x22; 16], offer_hash: [0x11; 32],
            amount_final: 300_000_000, dest: None,
            reader_session_pk: vec![0x33; 32], timestamp: now(),
            chosen_version: 1, chosen_suite: 1,
            refund_to: Some(b"payer-refund-address".to_vec()),
        };
        let ab = accept.to_value().encode();
        let receipt = Receipt {
            version: 1, suite: 1,
            accept_hash: commit(Purpose::ChainLink, &ab),
            prev: commit(Purpose::ChainLink, &ab),
            amount_final: 300_000_000, timestamp: now(), unilateral: false,
        };
        let rb = receipt.to_value().encode();
        // **The default refund window is zero**, so `Terms::default()` offers no
        // refunds at all. That is a defensible default — a merchant opts into
        // accepting returns — but it is silent, and a client that ships default
        // terms has quietly made every sale final. Worth asserting so nobody
        // discovers it from a customer.
        let default_terms = Terms::default();
        assert_eq!(default_terms.refund_window_s, 0, "default terms grant no refund window");
        let terms = Terms { refund_window_s: 3600, ..Terms::default() };

        let good = Refund {
            version: 1, suite: 1,
            prior_receipt: commit(Purpose::ChainLink, &rb),
            amount_pxmr: 300_000_000,
            txid: [0x88; 32],
            paid_to: b"payer-refund-address".to_vec(),
            timestamp: now() + 10,
        };
        match check_refund(&good, &receipt, &rb, &default_terms, &accept) {
            Err(e) => println!(
                "    default terms ({}s window)        → refused {:?} — every sale final unless \
                 a merchant opts in",
                default_terms.refund_window_s, e.code
            ),
            Ok(()) => failures.push("a zero refund window must not permit a refund"),
        }
        match check_refund(&good, &receipt, &rb, &terms, &accept) {
            Ok(()) => println!("    refund to the signed address      → accepted"),
            Err(e) => {
                println!("    unexpected refusal: {e:?}");
                failures.push("a refund to the address the payer signed must be accepted");
            }
        }

        // The hole BIP-70 shipped: a refund redirected after the fact.
        let mut redirected = good.clone();
        redirected.paid_to = b"somewhere-else".to_vec();
        match check_refund(&redirected, &receipt, &rb, &terms, &accept) {
            Err(e) => println!("    refund redirected elsewhere       → refused {:?}", e.code),
            Ok(()) => failures.push("a redirected refund must be refused — this is BIP-70's published hole"),
        }

        // More than was paid.
        let mut over = good.clone();
        over.amount_pxmr = 300_000_001;
        match check_refund(&over, &receipt, &rb, &terms, &accept) {
            Err(e) => println!("    refund exceeding the payment      → refused {:?}", e.code),
            Ok(()) => failures.push("a refund larger than the payment must be refused"),
        }

        // A payer that supplied no address cannot be refunded, and the protocol
        // must say so rather than invent a destination.
        let mut anon = accept.clone();
        anon.refund_to = None;
        match check_refund(&good, &receipt, &rb, &terms, &anon) {
            Err(e) => println!("    refund with no signed address     → refused {:?}", e.code),
            Ok(()) => failures.push("a refund must not be payable when the payer named no address"),
        }
        let mut late = good.clone();
        late.timestamp = now() + 7200; // past the hour the merchant granted
        match check_refund(&late, &receipt, &rb, &terms, &accept) {
            Err(e) => println!("    refund after the window closed    → refused {:?}", e.code),
            Ok(()) => failures.push("a refund past the granted window must be refused"),
        }
        println!();
    }

    if failures.is_empty() {
        println!("\x1b[32m  every abandonment path leaves the artifact §6.2 requires\x1b[0m\n");
        Ok(())
    } else {
        for f in &failures {
            println!("\x1b[31m  FAILED\x1b[0m {f}");
        }
        Err(format!("{} edge case(s) failed", failures.len()).into())
    }
}
