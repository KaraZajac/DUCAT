//! The wallet without the window: mint, watch, and pay from a desk state.
//!
//!   DUCAT_DESK_STATE=<dir> cargo run -p ducat-app --example wallet -- address
//!   DUCAT_DESK_STATE=<dir> cargo run -p ducat-app --example wallet -- sync        (scan until caught up, print balances)
//!   DUCAT_DESK_STATE=<dir> cargo run -p ducat-app --example wallet -- send <addr> <xmr> [note]
//!
//! Markers: WL_ADDR, WL_BAL, WL_SENT, WL_FAIL. No Veilid node is started —
//! the wallet only needs a Monero node.

use std::time::{Duration, Instant};

use ducat_app::wallet::{format_xmr, parse_xmr};
use ducat_app::App;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let app = App::open_default().expect("WL_FAIL open");
    eprintln!("state: {}", app.root().display());
    let node = app.last_good_node().or_else(|| app.pick_node()).expect("WL_FAIL no monero node");
    eprintln!("node: {node}");
    let address = app.ensure_wallet().expect("WL_FAIL mint");
    println!("WL_ADDR {address}");
    match args.first().map(String::as_str) {
        Some("address") => {}
        Some("sync") => {
            let t0 = Instant::now();
            loop {
                let moved = app.scan_step(&node);
                let b = app.balances();
                eprintln!("scanned {} / {} ({:.0}%)", b.scanned_to, b.tip, b.progress * 100.0);
                if !moved || !b.syncing || t0.elapsed() > Duration::from_secs(1800) {
                    break;
                }
            }
            app.refresh_spent(&node);
            let b = app.balances();
            println!(
                "WL_BAL spendable {} XMR locked {} XMR notes {} tip {} scanned {}{}",
                format_xmr(b.spendable_pxmr),
                format_xmr(b.locked_pxmr),
                b.spendable_outputs,
                b.tip,
                b.scanned_to,
                b.error.map(|e| format!(" error={e}")).unwrap_or_default()
            );
        }
        Some("send") => {
            let to = args.get(1).expect("WL_FAIL send <addr> <xmr>");
            let amount = parse_xmr(args.get(2).expect("WL_FAIL send <addr> <xmr>")).expect("WL_FAIL amount");
            let note = args.get(3).map(String::as_str);
            let q = app.quote(amount, 1);
            eprintln!("quote: fee {} XMR over {} note(s), affordable={}", format_xmr(q.fee_pxmr), q.notes, q.affordable);
            match app.send_xmr(to, amount, None, note, 1, false) {
                Ok(r) => println!("WL_SENT {} fee {} XMR accepted_by {}", r.txid_hex, format_xmr(r.fee_pxmr), r.accepted_by),
                Err(e) => println!("WL_FAIL send: {e}"),
            }
        }
        _ => println!("WL_FAIL usage: address | sync | send <addr> <xmr> [note]"),
    }
}
