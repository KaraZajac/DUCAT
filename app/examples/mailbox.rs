//! Two desks talking over the live network: one cuts a card, the other
//! claims it and says hello, the first answers.
//!
//!   DUCAT_DESK_STATE=<dir A> cargo run -p ducat-app --example mailbox -- host
//!   DUCAT_DESK_STATE=<dir B> cargo run -p ducat-app --example mailbox -- guest <card uri> [name]
//!
//! Markers: MB_CARD, MB_CLAIMED, MB_SENT, MB_GOT, MB_REPLY, MB_OK, MB_FAIL.

use std::time::{Duration, Instant};

use ducat_app::mailbox::{Claim, Outgoing};
use ducat_app::App;

fn ready(app: &App) {
    app.start_node().expect("MB_FAIL node start");
    let t0 = Instant::now();
    while t0.elapsed() < Duration::from_secs(240) {
        let s = app.node_status();
        if s.public_internet_ready {
            eprintln!("node ready — {} peers", s.peers);
            return;
        }
        std::thread::sleep(Duration::from_secs(2));
    }
    panic!("MB_FAIL node never became ready");
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let app = App::open_default().expect("MB_FAIL open");
    eprintln!("state: {}", app.root().display());
    ready(&app);
    match args.first().map(String::as_str) {
        Some("host") => {
            app.set_my_name(None, "Host Desk").expect("MB_FAIL name");
            let handle = app.profile_code(None).expect("MB_FAIL issue");
            println!("MB_CARD {}", handle.uri);
            let t0 = Instant::now();
            let mut seen: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
            let mut answered = 0;
            while t0.elapsed() < Duration::from_secs(600) {
                let claimed = app.collect_claims(None);
                if claimed > 0 {
                    println!("MB_CLAIM_SEEN {claimed}");
                }
                app.poll();
                for c in app.contacts() {
                    let thread = app.thread(&c.persona_hex);
                    let before = seen.get(&c.persona_hex).copied().unwrap_or(0);
                    for row in thread.iter().skip(before).filter(|r| !r.outgoing) {
                        println!("MB_GOT {} seq {} '{}' fs={}", c.display_name(), row.seq, row.body, row.forward_secret);
                        let reply = format!("echo: {}", row.body);
                        match app.send(&c, Outgoing::text(&reply)) {
                            Ok(_) => {
                                println!("MB_SENT reply to {}", c.display_name());
                                answered += 1;
                            }
                            Err(e) => println!("MB_FAIL reply: {e}"),
                        }
                    }
                    seen.insert(c.persona_hex.clone(), thread.len());
                }
                if answered >= 2 {
                    println!("MB_OK host answered {answered}");
                    return;
                }
                std::thread::sleep(Duration::from_secs(5));
            }
            println!("MB_FAIL host timed out");
        }
        Some("guest") => {
            let uri = args.get(1).expect("MB_FAIL guest <card uri>");
            let name = args.get(2).cloned().unwrap_or_else(|| "Guest Desk".into());
            app.set_my_name(None, &name).expect("MB_FAIL name");
            let t0 = Instant::now();
            let c = match app.claim_card(uri, Some("the host"), false, None) {
                Ok(Claim::New(c)) => c,
                Ok(Claim::Known(c)) => {
                    println!("MB_KNOWN already claimed here");
                    c
                }
                Err(e) => {
                    println!("MB_FAIL claim: {e}");
                    return;
                }
            };
            println!("MB_CLAIMED {} ({}) in {:.1}s", c.display_name(), c.asserted_name.clone().unwrap_or_default(), t0.elapsed().as_secs_f64());
            let c = app.send(&c, Outgoing::text("hello from the guest")).expect("MB_FAIL send 1");
            println!("MB_SENT seq {}", c.out_seq - 1);
            let c = app.send(&c, Outgoing::text("and a second line")).expect("MB_FAIL send 2");
            println!("MB_SENT seq {}", c.out_seq - 1);
            let t1 = Instant::now();
            while t1.elapsed() < Duration::from_secs(300) {
                app.poll();
                let inbound: Vec<_> = app.thread(&c.persona_hex).into_iter().filter(|r| !r.outgoing).collect();
                if inbound.len() >= 2 {
                    for r in &inbound {
                        println!("MB_REPLY seq {} '{}' fs={}", r.seq, r.body, r.forward_secret);
                    }
                    println!("MB_OK guest got {} replies in {:.1}s", inbound.len(), t1.elapsed().as_secs_f64());
                    return;
                }
                std::thread::sleep(Duration::from_secs(5));
            }
            println!("MB_FAIL guest never got a reply");
        }
        Some("customer") => {
            // Claim a till's sale code, wait for the bill, pay it, wait
            // for the receipt.
            let uri = args.get(1).expect("MB_FAIL customer <card uri>");
            app.set_my_name(None, "Customer Desk").expect("MB_FAIL name");
            let node = app.last_good_node().or_else(|| app.pick_node()).expect("MB_FAIL no monero node");
            app.ensure_wallet().expect("MB_FAIL wallet");
            // Catch the wallet up first: the bill is paid from unlocked notes.
            for _ in 0..200 {
                if !app.scan_step(&node) {
                    break;
                }
            }
            app.refresh_spent(&node);
            let b = app.balances();
            println!("MB_BAL spendable {} XMR", ducat_app::wallet::format_xmr(b.spendable_pxmr));
            let c = match app.claim_card(uri, Some("the till"), false, None) {
                Ok(c) => c.contact(),
                Err(e) => {
                    println!("MB_FAIL claim: {e}");
                    return;
                }
            };
            println!("MB_CLAIMED {}", c.display_name());
            let t0 = Instant::now();
            let bill = loop {
                app.poll();
                if let Some(b) = app.thread(&c.persona_hex).into_iter().find(|m| !m.outgoing && m.kind == 1) {
                    break b;
                }
                if t0.elapsed() > Duration::from_secs(300) {
                    println!("MB_FAIL no bill arrived");
                    return;
                }
                std::thread::sleep(Duration::from_secs(5));
            };
            println!("MB_BILL seq {} {} XMR payto={} items={}", bill.seq, ducat_app::wallet::format_xmr(bill.amount_pxmr), bill.payto.as_deref().map(|p| &p[..12]).unwrap_or("-"), bill.items.len());
            // The notes may still be locked: a fresh top-up unlocks ten
            // blocks after it lands. Keep the wallet moving and try again.
            let mut paid = false;
            for attempt in 0..60 {
                match app.pay_bill(&c.persona_hex, Some(bill.seq), bill.amount_pxmr, None, 1) {
                    Ok(tx) => {
                        println!("MB_PAID {tx}");
                        paid = true;
                        break;
                    }
                    Err(e) if e.to_string().contains("not enough unlocked") => {
                        if attempt % 4 == 0 {
                            println!("MB_WAIT {e}");
                        }
                        app.scan_step(&node);
                        app.refresh_spent(&node);
                        std::thread::sleep(Duration::from_secs(30));
                    }
                    Err(e) => {
                        println!("MB_FAIL pay: {e}");
                        return;
                    }
                }
            }
            if !paid {
                println!("MB_FAIL pay: still locked after half an hour");
                return;
            }
            let t1 = Instant::now();
            loop {
                app.poll();
                if let Some(r) = app.thread(&c.persona_hex).into_iter().find(|m| !m.outgoing && m.kind == 3) {
                    println!("MB_RECEIPT {} XMR txid={} re_seq={:?}", ducat_app::wallet::format_xmr(r.amount_pxmr), r.txid_hex.as_deref().unwrap_or("-"), r.re_seq);
                    println!("MB_OK receipt in {:.0}s", t1.elapsed().as_secs_f64());
                    return;
                }
                if t1.elapsed() > Duration::from_secs(1800) {
                    println!("MB_FAIL no receipt after 30 min");
                    return;
                }
                std::thread::sleep(Duration::from_secs(10));
            }
        }
        Some("reader") => {
            // Answer a press's subscribe code, wait for the issue's key,
            // fetch the issue.
            let uri = args.get(1).expect("MB_FAIL reader <press code>");
            app.set_my_name(None, "Reader Desk").expect("MB_FAIL name");
            let c = match app.claim_card(uri, Some("the press"), false, None) {
                Ok(c) => c.contact(),
                Err(e) => {
                    println!("MB_FAIL claim: {e}");
                    return;
                }
            };
            println!("MB_CLAIMED {}", c.display_name());
            let t0 = Instant::now();
            let period = loop {
                app.poll();
                if let Some(sub) = app.subscription(&c.persona_hex) {
                    if let Some(p) = sub.periods.keys().next().cloned() {
                        break p;
                    }
                }
                if t0.elapsed() > Duration::from_secs(600) {
                    println!("MB_FAIL no key arrived");
                    return;
                }
                std::thread::sleep(Duration::from_secs(5));
            };
            println!("MB_KEY '{period}' after {:.0}s", t0.elapsed().as_secs_f64());
            let sub = app.subscription(&c.persona_hex).unwrap();
            println!("MB_SUB shelf={} ships={}", sub.record.is_some(), sub.ships.len());
            let t1 = Instant::now();
            match app.fetch_issue(&c.persona_hex, &period) {
                Ok(dir) => {
                    let files: Vec<String> = std::fs::read_dir(&dir).map(|rd| rd.flatten().map(|e| format!("{} ({} B)", e.file_name().to_string_lossy(), e.metadata().map(|m| m.len()).unwrap_or(0))).collect()).unwrap_or_default();
                    println!("MB_ISSUE {} in {:.1}s: {}", dir.display(), t1.elapsed().as_secs_f64(), files.join(", "));
                    println!("MB_OK issue fetched");
                }
                Err(e) => println!("MB_FAIL fetch: {e}"),
            }
        }
        Some("party") => {
            // One member of a group: claim every card given, cut a card of
            // its own, then keep the lap turning and print what the group
            // says. Args: party <name> [card...]
            let name = args.get(1).cloned().unwrap_or_else(|| "Party".into());
            app.set_my_name(None, &name).expect("MB_FAIL name");
            for uri in args.iter().skip(2) {
                match app.claim_card(uri, None, false, None) {
                    Ok(c) => println!("MB_CLAIMED {}", c.contact().display_name()),
                    Err(e) => println!("MB_FAIL claim: {e}"),
                }
            }
            let h = app.profile_code(None).expect("MB_FAIL issue");
            println!("MB_CARD {}", h.uri);
            let mut seen: std::collections::HashSet<(String, String, u64)> = std::collections::HashSet::new();
            let t0 = Instant::now();
            while t0.elapsed() < Duration::from_secs(900) {
                app.lap_once();
                for g in app.groups() {
                    for r in app.group_thread(&g.id_hex) {
                        let key = (g.id_hex.clone(), r.sender_hex.clone(), r.message.group_seq);
                        if seen.insert(key) {
                            let who = app.contact(&r.sender_hex).map(|c| c.display_name()).unwrap_or_else(|| "me".into());
                            println!("MB_GROUP '{}' {}: {}", g.name, who, r.message.body);
                            if !app.persona_hexes().contains(&r.sender_hex) && r.message.body.starts_with("ping") {
                                match app.send_group(&g.id_hex, &format!("pong from {name}"), 0, None, None) {
                                    Ok(all) => println!("MB_PONG all={all}"),
                                    Err(e) => println!("MB_FAIL pong: {e}"),
                                }
                            }
                        }
                    }
                }
                std::thread::sleep(Duration::from_secs(5));
            }
        }
        _ => panic!("MB_FAIL usage: host | guest <card uri> [name] | customer <card uri> | reader <press code> | party <name> [card...]"),
    }
}
