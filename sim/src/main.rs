//! DUCAT market simulation.
//!
//! Five participants, real keys, real protocol objects, real state machines.
//! Run with `cargo run` for a summary, `cargo run -- -v` to watch every message.

mod flow;
mod live;
mod persona;
mod transport;
mod wallet;

use ducat_core::state::{Event, Role, SettleMode};
use persona::{Kind, Persona};
use transport::Wire;

const XMR: u64 = 1_000_000_000_000; // piconero per XMR

fn xmr(x: f64) -> u64 {
    // Simulation-only convenience. Note the protocol itself never does this:
    // §18.2 forbids floats on the wire precisely because this conversion is
    // where precision dies.
    (x * XMR as f64).round() as u64
}

fn fmt(pxmr: u64) -> String {
    format!("{:.6} XMR", pxmr as f64 / XMR as f64)
}

fn banner(title: &str) {
    println!("\n\x1b[1m{}\x1b[0m", title);
    println!("{}", "─".repeat(title.len()));
}

fn main() {
    if std::env::args().any(|a| a == "--live") {
        live_main();
        return;
    }
    if std::env::args().any(|a| a == "--drain") {
        drain_main();
        return;
    }
    if std::env::args().any(|a| a == "--scenarios") {
        scenarios_main();
        return;
    }
    let verbose = std::env::args().any(|a| a == "-v" || a == "--verbose");
    let mut wire = Wire::new(verbose);

    // Addresses are placeholders here; setup_wallets.sh creates the real
    // stagenet wallets and prints their addresses.
    let addr = |n: u8| vec![0x40 + n; 69];

    let mut user_01 = Persona::new("user_01", Kind::Consumer, 11, addr(1));
    let mut user_02 = Persona::new("user_02", Kind::Consumer, 12, addr(2));
    let mut taxi_01 = Persona::new("taxi_01", Kind::Taxi, 21, addr(3));
    let mut coffee_01 = Persona::new("coffee_01", Kind::Coffee, 22, addr(4));
    let mut shopkeep_01 = Persona::new("shopkeep_01", Kind::Shopkeeper, 23, addr(5));

    user_01.balance_pxmr = xmr(0.02);
    user_02.balance_pxmr = xmr(0.02);

    banner("DUCAT market simulation");
    println!("  five participants, real signing, real state machines");
    println!("  transport: in-process (Veilid measured separately, Phase 0b)\n");
    for p in [&user_01, &user_02, &taxi_01, &coffee_01, &shopkeep_01] {
        println!(
            "  {:<12} {:<12} persona {}",
            p.name,
            format!("{:?}", p.kind),
            hex8(&p.public().to_bytes())
        );
    }

    let mut failures = Vec::new();
    let mut n = 0u8;
    let mut nonce = || {
        n += 1;
        [n; 16]
    };

    // ---- 1. coffee ----
    banner("1. user_01 buys coffee — pos/1, fixed, direct");
    match flow::transact(&mut wire, &mut coffee_01, &mut user_01, xmr(0.0025), nonce(), None, "coffee") {
        Ok(o) => println!("    settled {}", fmt(o.amount_pxmr)),
        Err(e) => failures.push(e),
    }

    // ---- 2. taxi ride, with a chat during it ----
    banner("2. user_02 takes a ride — ride/1, price derived from route");
    let dest = Some(vec![0x0D; 16]);
    println!("    chat/1 over the session (no settlement, §16.1 tier 1):");
    println!("      user_02 → taxi_01  \"i'm at the side entrance\"");
    println!("      taxi_01 → user_02  \"two minutes\"");
    match flow::transact(&mut wire, &mut taxi_01, &mut user_02, xmr(0.008), nonce(), dest, "ride") {
        Ok(o) => println!("    settled {}", fmt(o.amount_pxmr)),
        Err(e) => failures.push(e),
    }

    // ---- 3. market stall, several items ----
    banner("3. user_01 buys from the market stall — pos/1");
    for (item, price) in [("bread", 0.0018), ("cheese", 0.0032)] {
        match flow::transact(
            &mut wire, &mut shopkeep_01, &mut user_01, xmr(price), nonce(), None, item,
        ) {
            Ok(o) => println!("    {} settled {}", item, fmt(o.amount_pxmr)),
            Err(e) => failures.push(e),
        }
    }

    // ---- 4. person to person ----
    banner("4. user_02 pays user_01 back — xfer/1");
    match flow::transact(&mut wire, &mut user_01, &mut user_02, xmr(0.004), nonce(), None, "payback") {
        Ok(o) => println!("    settled {}", fmt(o.amount_pxmr)),
        Err(e) => failures.push(e),
    }

    // ---- 5. adversarial: the offer is swapped after the tap ----
    banner("5. hostile terminal swaps the offer after committing to it");
    println!("    a terminal presents a 0.002 tap, then delivers a 0.02 offer");
    match swap_attack(&mut wire, &mut coffee_01, &mut user_01) {
        Ok(()) => failures.push("SWAP ATTACK SUCCEEDED — §15.3's commitment failed".into()),
        Err(e) => println!("    \x1b[32mrefused\x1b[0m: {}", e),
    }

    // ---- 6. adversarial: a message out of order ----
    banner("6. out-of-order message — the RetoSwap shape (§2.5)");
    println!("    a party sends ACCEPT before any offer exists");
    user_01.reset();
    match user_01.step(Role::Payer, SettleMode::Direct, &Event::Accept { from: Role::Payer }) {
        Ok(()) => failures.push("OUT-OF-ORDER ACCEPTED — §18.4 failed".into()),
        Err(e) => println!("    \x1b[32mrefused\x1b[0m: {}", e),
    }

    // ---- ledger ----
    banner("Ledger");
    for p in [&user_01, &user_02, &taxi_01, &coffee_01, &shopkeep_01] {
        let earned: u64 = p.receipts.iter().filter(|r| !r.paid).map(|r| r.amount_pxmr).sum();
        let spent: u64 = p.receipts.iter().filter(|r| r.paid).map(|r| r.amount_pxmr).sum();
        println!(
            "  {:<12} {:>2} transcripts   spent {:<14} earned {:<14} balance {}",
            p.name,
            p.receipts.len(),
            fmt(spent),
            fmt(earned),
            fmt(p.balance_pxmr)
        );
    }

    banner("Wire");
    println!("  {} protocol messages, {} bytes total", wire.messages, wire.bytes_sent);
    println!(
        "  average message {} B",
        if wire.messages > 0 { wire.bytes_sent / wire.messages } else { 0 }
    );

    banner("Result");
    if failures.is_empty() {
        println!("  \x1b[32mall scenarios passed\x1b[0m — every transcript verified by both parties,");
        println!("  both attacks refused");
    } else {
        println!("  \x1b[31m{} failure(s)\x1b[0m", failures.len());
        for f in &failures {
            println!("    - {}", f);
        }
        std::process::exit(1);
    }
}

/// Present a tap committing to a cheap offer, then deliver an expensive one.
/// The payer must refuse before any human sees a number (§15.3, §15.5).
fn swap_attack(wire: &mut Wire, payee: &mut Persona, payer: &mut Persona) -> Result<(), String> {
    use ducat_core::commit::{commit, Purpose};
    use ducat_core::sig::{ObjectType, SignedBytes};
    use ducat_core::wire::*;

    payee.reset();
    payer.reset();
    let nonce = [0xEE; 16];

    let honest = FullOffer {
        version: 1, suite: 1, profile: 2,
        payto: payee.payto.clone(), amount_pxmr: xmr(0.002),
        supported_versions: vec![1], supported_suites: vec![1, 2],
        settle_mode: 0, fee_policy: FeePolicy::PayerPays, nonce_echo: nonce,
        terms: Terms::default(),
    };
    let mut dearer = honest.clone();
    dearer.amount_pxmr = xmr(0.02);

    let tap = TapPresent {
        version: 1, suite: 1, profile: 2,
        presenter_role: PresenterRole::Payee,
        amount_authority: AmountAuthority::Fixed,
        intent: Intent::Oneshot, rmode: ReachMode::Token,
        nonce, expiry: 1_800_000_030,
        session_pk: payee.public().to_bytes(),
        route: vec![0x11; 32],
        offer_commit: honest.commitment(),   // commits to the cheap one
        dest: None, session_ref: None,
    };
    let env = seal(&SignedBytes::from_value(tap.to_value()), ObjectType::TapPresent, &payee.persona_key);
    wire.send(&payee.name, &payer.name, "TapPresent", &env);
    let (_, body) = open(&wire.recv(), &payee.public()).map_err(|e| format!("{:?}", e.code))?;
    let tap_seen = TapPresent::from_value(body.value().clone()).map_err(|e| format!("{:?}", e.code))?;

    // ...and now delivers the expensive one.
    let bad = seal(&SignedBytes::from_value(dearer.to_value()), ObjectType::FullOffer, &payee.persona_key);
    wire.send(&payee.name, &payer.name, "FullOffer(swapped)", &bad);
    let (_, offer_body) = open(&wire.recv(), &payee.public()).map_err(|e| format!("{:?}", e.code))?;

    if commit(Purpose::Offer, offer_body.bytes()) != tap_seen.offer_commit {
        return Err("offer does not match the tap's commitment".into());
    }
    Ok(())
}

fn hex8(b: &[u8]) -> String {
    b.iter().take(4).map(|x| format!("{:02x}", x)).collect::<String>() + "…"
}

/// Live mode: the same market, settled on stagenet.
fn live_main() {
    use live::{transact_live, Party};
    use persona::Persona;
    use std::collections::BTreeMap;
    use wallet::Wallet;

    let ports: BTreeMap<String, u16> = [
        ("user_01", 28101u16),
        ("user_02", 28102),
        ("taxi_01", 28103),
        ("coffee_01", 28104),
        ("shopkeep_01", 28105),
    ]
    .iter()
    .map(|(n, p)| (n.to_string(), *p))
    .collect();

    banner("DUCAT market — live settlement on stagenet");
    println!("  five wallets, real sXMR, real transcripts\n");

    let mut parties: BTreeMap<String, Party> = BTreeMap::new();
    let mut seed = 11u8;
    for (name, port) in &ports {
        match Wallet::new(name, *port) {
            Ok(w) => {
                println!("  {:<12} port {}  {}…", name, port, &w.address[..12]);
                let persona =
                    Persona::new(name, live::kind_of(name), seed, w.address.as_bytes().to_vec());
                parties.insert(name.clone(), Party { persona, wallet: w });
                seed += 1;
            }
            Err(e) => {
                println!("  {:<12} UNAVAILABLE: {}", name, e);
                std::process::exit(1);
            }
        }
    }

    banner("Waiting for spendable outputs");
    println!("  §17.2: one output funds one payment per lock interval\n");
    if !live::wait_for_outputs(&ports, 1, 2400) {
        println!("\n  gave up waiting for outputs to unlock");
        std::process::exit(1);
    }

    let mut wire = Wire::new(true);
    let mut settled: Vec<live::Settled> = Vec::new();
    let mut failures: Vec<String> = Vec::new();
    let mut n = 100u8;

    let plan: Vec<(&str, &str, f64, Option<Vec<u8>>, &str)> = vec![
        ("coffee_01", "user_01", 0.0006, None, "coffee"),
        ("taxi_01", "user_02", 0.0009, Some(vec![0x0D; 16]), "ride"),
        ("shopkeep_01", "user_01", 0.0005, None, "bread"),
        ("shopkeep_01", "user_01", 0.0007, None, "cheese"),
        ("user_01", "user_02", 0.0008, None, "payback"),
        ("shopkeep_01", "coffee_01", 0.0005, None, "vendor-to-vendor"),
    ];

    for (i, (payee, payer, amt, dest, label)) in plan.into_iter().enumerate() {
        banner(&format!("{}. {} — {} pays {}", i + 1, label, payer, payee));
        n += 1;
        let nonce = [n; 16];
        let mut a = parties.remove(payee).unwrap();
        let mut b = parties.remove(payer).unwrap();
        match transact_live(&mut wire, &mut a, &mut b, xmr(amt), nonce, dest, label) {
            Ok(s) => settled.push(s),
            Err(e) => {
                println!("    \x1b[31mfailed\x1b[0m: {}", e);
                failures.push(format!("{}: {}", label, e));
            }
        }
        parties.insert(payee.to_string(), a);
        parties.insert(payer.to_string(), b);
    }

    banner("Settled on chain");
    for s in &settled {
        println!(
            "  {:<18} {:>10} XMR  {} → {}  {}…",
            s.label,
            format!("{:.6}", s.amount as f64 / 1e12),
            s.payer,
            s.payee,
            &s.txid[..16]
        );
    }

    banner("Final balances (from the chain)");
    for (name, port) in &ports {
        if let Ok(w) = Wallet::new(name, *port) {
            let _ = w.refresh();
            if let Ok(b) = w.balance() {
                println!(
                    "  {:<12} total {:>10}  unlocked {:>10}  outputs {}",
                    name,
                    format!("{:.6}", b.total as f64 / 1e12),
                    format!("{:.6}", b.unlocked as f64 / 1e12),
                    b.unlocked_outputs
                );
            }
        }
    }

    banner("Result");
    println!("  {} settled, {} failed", settled.len(), failures.len());
    for f in &failures {
        println!("    - {}", f);
    }
}

/// Drain mode: one participant buys until it cannot.
///
/// §17.2 predicts the failure precisely — a payment consumes a whole output and
/// its change returns locked, so consecutive capacity is a count. This exercises
/// the *negative* path, which the happy-path run never reaches: does the client
/// detect exhaustion before promising a fare, or discover it at settlement with
/// a customer waiting?
fn drain_main() {
    use live::{transact_live, Party};
    use persona::Persona;
    use wallet::Wallet;

    banner("DUCAT — spending to output exhaustion");
    println!("  §17.2: consecutive capacity is a count of unlocked outputs,");
    println!("  not a balance. A payment costs a whole output whatever its size.\n");

    let buyer_w = match Wallet::new("user_01", 28101) {
        Ok(w) => w,
        Err(e) => { println!("  user_01 unavailable: {}", e); return; }
    };
    let seller_w = match Wallet::new("coffee_01", 28104) {
        Ok(w) => w,
        Err(e) => { println!("  coffee_01 unavailable: {}", e); return; }
    };

    let _ = buyer_w.refresh();
    let start = match buyer_w.balance() {
        Ok(b) => b,
        Err(e) => { println!("  balance unavailable: {}", e); return; }
    };
    println!(
        "  user_01 starts with {} XMR across {} unlocked output(s)",
        format!("{:.6}", start.unlocked as f64 / 1e12),
        start.unlocked_outputs
    );
    println!("  predicted consecutive purchases: {}\n", start.unlocked_outputs);

    let mut buyer = Party {
        persona: Persona::new("user_01", persona::Kind::Consumer, 11, buyer_w.address.as_bytes().to_vec()),
        wallet: buyer_w,
    };
    let mut seller = Party {
        persona: Persona::new("coffee_01", persona::Kind::Coffee, 22, seller_w.address.as_bytes().to_vec()),
        wallet: seller_w,
    };

    let mut wire = Wire::new(false);
    let mut bought = 0usize;
    let mut n = 200u8;

    for i in 1..=12 {
        n += 1;
        banner(&format!("purchase {}", i));
        match transact_live(&mut wire, &mut seller, &mut buyer, xmr(0.0003), [n; 16], None, "coffee") {
            Ok(_) => bought += 1,
            Err(e) => {
                println!("    \x1b[33mrefused\x1b[0m: {}", e);
                break;
            }
        }
    }

    banner("Result");
    println!("  predicted: {} consecutive purchases", start.unlocked_outputs);
    println!("  actual:    {} consecutive purchases", bought);
    if bought == start.unlocked_outputs {
        println!("\n  \x1b[32mexact match\x1b[0m — capacity is the output count, as §17.2 states.");
        println!("  The refusal came from the client's own pre-check, before any");
        println!("  offer was accepted, rather than from a failed settlement with");
        println!("  a customer waiting.");
    } else {
        println!("\n  \x1b[33mdiverged\x1b[0m — worth understanding before trusting the model.");
    }
}

/// The four situations DUCAT has to actually work in, driven end to end.
///
/// Unit tests check a rule; these check that a rule survives contact with the
/// others. Every bug found in this project so far has lived between two
/// sections that were each correct alone.
fn scenarios_main() {
    use ducat_core::commit::{commit, Purpose};
    use ducat_core::reject::RejectCode;
    use ducat_core::state::{Event, Role, SettleMode, State};
    use ducat_core::wire::*;

    let mut failures: Vec<String> = Vec::new();
    let mut check = |name: &str, ok: bool, why: &str, failures: &mut Vec<String>| {
        if ok {
            println!("    \x1b[32m✓\x1b[0m {}", name);
        } else {
            println!("    \x1b[31m✗\x1b[0m {} — {}", name, why);
            failures.push(name.to_string());
        }
    };

    // ---------------------------------------------------------------- shop --
    banner("A merchant: coffee, a refund, a final-sale item");
    {
        let terms = Terms { refund_window_s: 86_400 * 14, ..Terms::default() };
        let accept_bytes = b"accept-coffee".to_vec();
        let link = commit(Purpose::ChainLink, &accept_bytes);
        let receipt = Receipt {
            version: 1, suite: 1, accept_hash: link, prev: link,
            amount_final: 600_000_000, timestamp: 1_800_000_000, unilateral: false,
        };
        let rb = receipt.to_value().encode();

        let good = Refund {
            version: 1, suite: 1,
            prior_receipt: commit(Purpose::ChainLink, &rb),
            amount_pxmr: 600_000_000, txid: [0x1; 32],
            timestamp: receipt.timestamp + 3600,
        };
        check("refund next day is honoured",
              check_refund(&good, &receipt, &rb, &terms).is_ok(),
              "a merchant must be able to refund", &mut failures);

        let late = Refund { timestamp: receipt.timestamp + 86_400 * 15, ..good.clone() };
        check("refund after the window is refused",
              check_refund(&late, &receipt, &rb, &terms).map_err(|e| e.code)
                  == Err(RejectCode::PolicyRefused),
              "an unbounded liability", &mut failures);

        let final_sale = Terms { refund_window_s: 0, ..Terms::default() };
        check("final sale accepts no refund later",
              check_refund(&good, &receipt, &rb, &final_sale).is_err(),
              "zero window must mean zero", &mut failures);
    }

    // ---------------------------------------------------------------- taxi --
    banner("A taxi: a ride, and a cancellation before it starts");
    {
        let mut s = State::Idle;
        for ev in [Event::TapPresent, Event::FullOffer,
                   Event::Accept { from: Role::Payer }, Event::Fund,
                   Event::Proof, Event::Receipt] {
            s = match ducat_core::state::transition(s, Role::Payer, SettleMode::Direct, &ev) {
                Ok(t) => t.next,
                Err(e) => { failures.push(format!("ride: {:?}", e.code)); State::Aborted }
            };
        }
        check("ride settles", s == State::Closed, "ride did not close", &mut failures);

        // The rider changes their mind after the fare is locked.
        let mut s = State::Idle;
        for ev in [Event::TapPresent, Event::FullOffer, Event::Accept { from: Role::Payer }] {
            s = ducat_core::state::transition(s, Role::Payer, SettleMode::Direct, &ev).unwrap().next;
        }
        let c = ducat_core::state::transition(s, Role::Payer, SettleMode::Direct, &Event::Cancel);
        check("cancel after fare-lock invokes terms",
              c.map(|t| t.next) == Ok(State::Cancelled),
              "a rider must be able to cancel", &mut failures);

        let terms = Terms { cancellation_pxmr: 500_000_000, ..Terms::default() };
        let ab = b"accept-ride".to_vec();
        let cancel = Cancel {
            version: 1, suite: 1,
            prior_accept: commit(Purpose::ChainLink, &ab),
            fee_pxmr: 500_000_000, timestamp: 1,
        };
        check("cancellation fee is the one that was signed",
              check_cancel(&cancel, &ab, &terms).is_ok(), "fee mismatch", &mut failures);
        let inflated = Cancel { fee_pxmr: 9_000_000_000, ..cancel.clone() };
        check("an inflated cancellation fee is refused",
              check_cancel(&inflated, &ab, &terms).is_err(),
              "a driver could invent a penalty", &mut failures);
    }

    // ------------------------------------------------------------- bar tab --
    banner("A bar tab: open, run long, and the customer walks out");
    {
        let mut s = State::Idle;
        for ev in [Event::TapPresent, Event::FullOffer, Event::MeterStart] {
            s = ducat_core::state::transition(s, Role::Payer, SettleMode::Direct, &ev).unwrap().next;
        }
        check("tab opens into METERING", s == State::Metering, "no metering state", &mut failures);

        let three_hours = ducat_core::state::transition(
            s, Role::Payer, SettleMode::Direct,
            &Event::Elapsed(std::time::Duration::from_secs(10_800))).unwrap();
        check("tab survives three hours",
              three_hours.next == State::Metering,
              "the meter died while the customer was still drinking", &mut failures);

        check("a payer cannot abort a running tab",
              ducat_core::state::transition(s, Role::Payer, SettleMode::Direct,
                  &Event::Abort { from: Role::Payer }).is_err(),
              "drink now, abort later", &mut failures);

        let walked = ducat_core::state::transition(
            s, Role::Payee, SettleMode::Direct, &Event::MeterExpired).unwrap();
        check("walking out leaves evidence, not a clean exit",
              walked.effect == ducat_core::state::Effect::EmitSingleSidedReceipt,
              "no record of what was owed", &mut failures);

        let offer = FullOffer {
            version: 1, suite: 1, profile: 2, payto: vec![0x1; 8],
            amount_pxmr: 0, supported_versions: vec![1], supported_suites: vec![1],
            settle_mode: 0, fee_policy: FeePolicy::PayerPays, nonce_echo: [0; 16],
            terms: Terms { meter_cap_pxmr: 20_000_000_000, meter_max_s: 14_400, ..Terms::default() },
        };
        check("an abandoned tab is capped at what was agreed",
              abandoned_meter_claim(&offer, 1_000_000_000, 999_999) == 20_000_000_000,
              "unbounded liability", &mut failures);

        let mut uncapped = offer.clone();
        uncapped.terms.meter_cap_pxmr = 0;
        let mut tap_rated = TapPresent {
            version: 1, suite: 1, profile: 2,
            presenter_role: PresenterRole::Payee,
            amount_authority: AmountAuthority::Rated,
            intent: Intent::Start, rmode: ReachMode::Token,
            nonce: [0; 16], expiry: 0, session_pk: vec![0x2; 32],
            route: vec![0x3; 32], offer_commit: [0; 32], dest: None, session_ref: None,
        };
        tap_rated.intent = Intent::Start;
        check("a tab with no cap cannot be opened",
              check_meter_terms(&tap_rated, &uncapped).is_err(),
              "open-ended obligation", &mut failures);
    }

    // ------------------------------------------------------------- friends --
    banner("Between friends: a payback, and a standing arrangement");
    {
        let mut s = State::Idle;
        for ev in [Event::TapPresent, Event::FullOffer,
                   Event::Accept { from: Role::Payer }, Event::Fund,
                   Event::Proof, Event::Receipt] {
            s = ducat_core::state::transition(s, Role::Payer, SettleMode::Direct, &ev).unwrap().next;
        }
        check("payback settles", s == State::Closed, "xfer failed", &mut failures);

        check("identity exchange is possible only after the money",
              ducat_core::state::transition(s, Role::Payer, SettleMode::Direct,
                  &Event::ContactOffer).is_ok()
              && ducat_core::state::transition(State::Funded, Role::Payer, SettleMode::Direct,
                  &Event::ContactOffer).is_err(),
              "contact must follow closure", &mut failures);

        let flatmate = vec![0xF1; 32];
        let m = Mandate {
            version: 1, suite: 1, payee_persona: flatmate.clone(),
            cap_pxmr: 50_000_000_000, period_s: 86_400 * 30,
            expiry: 1_800_000_000 + 86_400 * 365, nonce: [0x9; 16],
        };
        let u = MandateUsage::default();
        let after = check_mandate_draw(&m, &u, &flatmate, 50_000_000_000, 1_800_000_000).unwrap();
        check("a standing arrangement draws within its cap",
              after.drawn_pxmr == m.cap_pxmr, "draw refused", &mut failures);
        check("and refuses a second draw past it",
              check_mandate_draw(&m, &after, &flatmate, 1, 1_800_000_100).is_err(),
              "cap did not bind", &mut failures);
        check("a stranger cannot draw on it",
              check_mandate_draw(&m, &u, &[0xEE; 32], 1, 1_800_000_000).is_err(),
              "mandate is bearer paper", &mut failures);
    }

    banner("Result");
    if failures.is_empty() {
        println!("  \x1b[32mall four situations behave\x1b[0m");
    } else {
        println!("  \x1b[31m{} failed\x1b[0m: {}", failures.len(), failures.join(", "));
        std::process::exit(1);
    }
}
