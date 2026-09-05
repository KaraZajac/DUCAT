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
            // What is already in the threads is history, not a prompt.
            let mut seen: std::collections::HashMap<String, usize> =
                app.contacts().into_iter().map(|c| (c.persona_hex.clone(), app.thread(&c.persona_hex).len())).collect();
            let mut answered = 0;
            while t0.elapsed() < Duration::from_secs(1500) {
                let claimed = app.collect_claims(None);
                if claimed > 0 {
                    println!("MB_CLAIM_SEEN {claimed}");
                }
                app.poll();
                for c in app.contacts() {
                    let thread = app.thread(&c.persona_hex);
                    let before = seen.get(&c.persona_hex).copied().unwrap_or(0);
                    for row in thread.iter().skip(before).filter(|r| !r.outgoing) {
                        println!(
                            "MB_GOT {} seq {} kind {} '{}' fs={} re={:?}/{} att={}",
                            c.display_name(),
                            row.seq,
                            row.kind,
                            row.body.replace('\n', "\\n"),
                            row.forward_secret,
                            row.re_seq,
                            row.re_own,
                            row.att_mime.as_deref().unwrap_or("-")
                        );
                        if row.kind != 0 {
                            continue;
                        }
                        // "bill me": a small bill back, so the other side can
                        // pay or decline one of ours.
                        let out = if row.body.trim().eq_ignore_ascii_case("card me") {
                            // "card me": an intro card of ours, the way the
                            // phone shares "a card for me".
                            let card = app.issue_card(Some("Host Desk"), 60 * 60 * 24 * 7, "intro", None).expect("MB_FAIL intro card");
                            Outgoing::text(&format!("🎟 A card for me — pass it to whoever should reach me. One claim, one week:\n{}", card.uri))
                        } else if row.body.trim().eq_ignore_ascii_case("bill me") {
                            let payto = app.ensure_wallet().ok().and_then(|_| app.wallet_address());
                            Outgoing {
                                body: "A small bill".into(),
                                kind: 1,
                                amount_pxmr: Some(100_000_000),
                                payto,
                                items: vec![ducat_app::contacts::BillItem { description: "A thing".into(), amount_pxmr: 100_000_000 }],
                                ..Default::default()
                            }
                        } else {
                            Outgoing::text(&format!("echo: {}", row.body))
                        };
                        match app.send(&c, out) {
                            Ok(_) => {
                                println!("MB_SENT reply to {}", c.display_name());
                                answered += 1;
                            }
                            Err(e) => println!("MB_FAIL reply: {e}"),
                        }
                    }
                    seen.insert(c.persona_hex.clone(), thread.len());
                }
                if answered >= 40 {
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
            // customer [card]: with a card, claim it first; without, the
            // bill is expected from somebody already known.
            let uri = args.get(1).filter(|u| u.starts_with("ducat:card/"));
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
            let known = match uri {
                Some(uri) => match app.claim_card(uri, Some("the till"), false, None) {
                    Ok(c) => Some(c.contact()),
                    Err(e) => {
                        println!("MB_FAIL claim: {e}");
                        return;
                    }
                },
                None => None,
            };
            if let Some(c) = &known {
                println!("MB_CLAIMED {}", c.display_name());
            }
            // The newest bill nobody has paid: no payment notice of ours
            // names it and no receipt of theirs does.
            let unpaid = |app: &App| -> Option<(ducat_app::contacts::Contact, ducat_app::contacts::StoredMessage)> {
                let mut best: Option<(ducat_app::contacts::Contact, ducat_app::contacts::StoredMessage)> = None;
                for c in app.contacts() {
                    if known.as_ref().map_or(false, |k| k.persona_hex != c.persona_hex) {
                        continue;
                    }
                    let thread = app.thread(&c.persona_hex);
                    for b in thread.iter().filter(|m| !m.outgoing && m.kind == 1) {
                        // Seq restarts with every fresh card, so one thread can hold two
                        // bills numbered 0; an answer must also be newer than the bill.
                        let answered = thread.iter().any(|m| m.timestamp >= b.timestamp && ((m.outgoing && m.kind == 2 && m.re_seq == Some(b.seq)) || (!m.outgoing && m.kind == 3 && m.re_seq == Some(b.seq))));
                        if !answered && best.as_ref().map_or(true, |(_, x)| b.timestamp > x.timestamp) {
                            best = Some((c.clone(), b.clone()));
                        }
                    }
                }
                best
            };
            let t0 = Instant::now();
            let (c, bill) = loop {
                app.poll();
                if let Some(found) = unpaid(&app) {
                    break found;
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
                if let Some(r) = app.thread(&c.persona_hex).into_iter().find(|m| !m.outgoing && m.kind == 3 && m.re_seq == Some(bill.seq) && m.timestamp >= bill.timestamp) {
                    println!("MB_RECEIPT {} XMR txid={} re_seq={:?}", ducat_app::wallet::format_xmr(r.amount_pxmr), r.txid_hex.as_deref().unwrap_or("-"), r.re_seq);
                    println!("MB_OK receipt in {:.0}s", t1.elapsed().as_secs_f64());
                    // A publication's bill is answered with its key: wait a
                    // little for one and fetch the issue.
                    if bill.items.iter().any(|i| i.description.contains(" — ")) {
                        let t2 = Instant::now();
                        while t2.elapsed() < Duration::from_secs(240) {
                            app.poll();
                            if let Some(sub) = app.subscription(&c.persona_hex) {
                                if let Some(period) = sub.periods.keys().last().cloned() {
                                    match app.fetch_issue(&c.persona_hex, &period) {
                                        Ok(dir) => println!("MB_ISSUE '{period}' at {}", dir.display()),
                                        Err(e) => println!("MB_FAIL issue: {e}"),
                                    }
                                    return;
                                }
                            }
                            std::thread::sleep(Duration::from_secs(5));
                        }
                        println!("MB_FAIL paid but no key came");
                    }
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
                            println!(
                                "MB_GROUP '{}' {}: kind {} gseq {} re={:?}/{:?} '{}'",
                                g.name,
                                who,
                                r.message.kind,
                                r.message.group_seq,
                                r.message.group_re_sender.as_deref().map(|h| &h[..8.min(h.len())]),
                                r.message.group_re_seq,
                                r.message.body
                            );
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
        Some("callee") => {
            // Claim a card, then wait to be rung; answer with a test tone,
            // talk until the far side hangs up, report the frames.
            app.set_my_name(None, "Callee Desk").expect("MB_FAIL name");
            if let Some(uri) = args.get(1) {
                match app.claim_card(uri, Some("the caller"), false, None) {
                    Ok(c) => println!("MB_CLAIMED {}", c.contact().display_name()),
                    Err(e) => println!("MB_FAIL claim: {e}"),
                }
            }
            let h = app.profile_code(None).expect("MB_FAIL issue");
            println!("MB_CARD {}", h.uri);
            let tone = ducat_app::calls::ToneAudio::new(660.0);
            let played = tone.played.clone();
            let rang = tone.rang.clone();
            ducat_app::calls::calls().set_audio(Box::new(tone));
            let t0 = Instant::now();
            let mut answered = false;
            while t0.elapsed() < Duration::from_secs(900) {
                app.lap_once();
                match ducat_app::calls::calls().state() {
                    ducat_app::calls::CallState::Incoming { .. } if !answered => {
                        println!("MB_RING rang={}", rang.load(std::sync::atomic::Ordering::SeqCst));
                        match app.answer_call() {
                            Ok(()) => {
                                answered = true;
                                println!("MB_ANSWERED");
                            }
                            Err(e) => println!("MB_FAIL answer: {e}"),
                        }
                    }
                    ducat_app::calls::CallState::Active { .. } => {
                        std::thread::sleep(Duration::from_millis(500));
                        continue;
                    }
                    ducat_app::calls::CallState::Idle | ducat_app::calls::CallState::NoAnswer { .. } if answered => {
                        let cs = ducat_app::calls::calls();
                        println!(
                            "MB_CALLEND rx={} tx={} played={}",
                            cs.rx_frames.load(std::sync::atomic::Ordering::SeqCst),
                            cs.tx_frames.load(std::sync::atomic::Ordering::SeqCst),
                            played.load(std::sync::atomic::Ordering::SeqCst)
                        );
                        println!("MB_OK call over");
                        return;
                    }
                    _ => {}
                }
                std::thread::sleep(Duration::from_secs(2));
            }
            println!("MB_FAIL nobody rang");
        }
        Some("caller") => {
            // Claim a card and ring it; talk for a while once answered,
            // then hang up.
            let uri = args.get(1).expect("MB_FAIL caller <card uri>");
            let talk_secs: u64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(10);
            app.set_my_name(None, "Caller Desk").expect("MB_FAIL name");
            let c = match app.claim_card(uri, Some("the callee"), false, None) {
                Ok(c) => c.contact(),
                Err(e) => {
                    println!("MB_FAIL claim: {e}");
                    return;
                }
            };
            println!("MB_CLAIMED {}", c.display_name());
            // Their reply to our claim must be collected before we speak.
            for _ in 0..30 {
                app.poll();
                if app.contact(&c.persona_hex).map_or(false, |k| k.their_bundle.is_some()) {
                    break;
                }
                std::thread::sleep(Duration::from_secs(2));
            }
            let tone = ducat_app::calls::ToneAudio::new(440.0);
            let played = tone.played.clone();
            ducat_app::calls::calls().set_audio(Box::new(tone));
            if let Err(e) = app.place_call(&c.persona_hex) {
                println!("MB_FAIL place: {e}");
                return;
            }
            println!("MB_RINGING");
            let t0 = Instant::now();
            let mut active_at: Option<Instant> = None;
            loop {
                match ducat_app::calls::calls().state() {
                    ducat_app::calls::CallState::Active { .. } => {
                        if active_at.is_none() {
                            active_at = Some(Instant::now());
                            println!("MB_ACTIVE after {:.1}s", t0.elapsed().as_secs_f64());
                        }
                        if active_at.map_or(false, |a| a.elapsed() >= Duration::from_secs(talk_secs)) {
                            app.hang_up();
                            std::thread::sleep(Duration::from_secs(1));
                            let cs = ducat_app::calls::calls();
                            println!(
                                "MB_CALLEND rx={} tx={} played={}",
                                cs.rx_frames.load(std::sync::atomic::Ordering::SeqCst),
                                cs.tx_frames.load(std::sync::atomic::Ordering::SeqCst),
                                played.load(std::sync::atomic::Ordering::SeqCst)
                            );
                            println!("MB_OK hung up");
                            return;
                        }
                    }
                    ducat_app::calls::CallState::NoAnswer { why, .. } => {
                        println!("MB_FAIL no answer: {why:?}");
                        return;
                    }
                    ducat_app::calls::CallState::Idle if active_at.is_some() => {
                        let cs = ducat_app::calls::calls();
                        println!("MB_CALLEND rx={} tx={} (they hung up)", cs.rx_frames.load(std::sync::atomic::Ordering::SeqCst), cs.tx_frames.load(std::sync::atomic::Ordering::SeqCst));
                        println!("MB_OK they hung up");
                        return;
                    }
                    _ => {}
                }
                if t0.elapsed() > Duration::from_secs(300) {
                    println!("MB_FAIL call never connected");
                    return;
                }
                std::thread::sleep(Duration::from_millis(500));
            }
        }
        _ => panic!("MB_FAIL usage: host | guest <card uri> [name] | customer <card uri> | reader <press code> | party <name> [card...] | callee <card> | caller <card> [secs]"),
    }
}
