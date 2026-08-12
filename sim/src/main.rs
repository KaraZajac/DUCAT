//! DUCAT market simulation.
//!
//! Five participants, real keys, real protocol objects, real state machines.
//! Run with `cargo run` for a summary, `cargo run -- -v` to watch every message.

mod flow;
mod persona;
mod transport;

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
