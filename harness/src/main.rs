//! DUCAT end to end: two parties, real Veilid routes, real Monero settlement.
//!
//! Everything before this exercised the protocol against an in-process queue
//! (`sim`) or the transport against synthetic payloads (`phase0`). Neither
//! answers the question the spec is actually making claims about: **does a
//! DUCAT transaction complete between two nodes that have never met, over an
//! anonymous route, ending in money moving on chain?**
//!
//! The two halves are deliberately separate processes. A tap is an out-of-band
//! channel — a QR code or an NFC exchange (§15.3) — and modelling it as a file
//! written by one process and read by the other is more faithful than passing a
//! struct between threads. It also means the payer genuinely starts knowing
//! nothing but the bytes in that file.
//!
//!   ducat-harness --payee   [amount_pxmr]   writes tap.blob, then serves
//!   ducat-harness --payer   [tap.blob]      reads it, transacts, settles
//!
//! Settlement uses `monero-wallet-rpc` on the ports `monero-spike/` sets up.

mod escrow_role;
mod attack;
mod dht;
mod stand;
mod mailbox;
mod edges;
mod flow;
mod inverted;
mod payee;
mod payer;
mod veilid;
mod wallet;

#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "error".into()),
        )
        .compact()
        .init();

    let args: Vec<String> = std::env::args().collect();
    let tap_path = std::env::var("DUCAT_TAP").unwrap_or_else(|_| "/tmp/ducat-tap.blob".into());

    if let Some(i) = args.iter().position(|a| a == "--escrow-serve") {
        let role = args.get(i + 1).cloned().unwrap_or_else(|| "seller".into());
        return escrow_role::serve(&role, &tap_path).await;
    }
    if args.iter().any(|a| a == "--escrow-drive") {
        let ms = std::env::var("DUCAT_MS_ADDRESS").unwrap_or_else(|_| {
            "53hUxmYTwGtR44fhL8f7JLATagSwjtdLB6y4Q3wQQnbtUsDiLTLCzwnKr2gtBRAAUdgWmD22pJ3GK5Z52sJpgiK624iqtKh".into()
        });
        return escrow_role::drive(&tap_path, &ms).await;
    }
    if args.iter().any(|a| a == "--card-issue") {
        return mailbox::issue().await;
    }
    if let Some(i) = args.iter().position(|a| a == "--card-claim") {
        return mailbox::claim(args.get(i + 1).map(|s| s.as_str()).unwrap_or("")).await;
    }
    if args.iter().any(|a| a == "--peek-own") {
        return mailbox::peek_own().await;
    }
    if let Some(i) = args.iter().position(|a| a == "--card-watch") {
        return mailbox::watch(args.get(i + 1).map(|s| s.as_str()).unwrap_or("")).await;
    }
    if args.iter().any(|a| a == "--refresh-keys") {
        return mailbox::refresh_keys().await;
    }
    if let Some(i) = args.iter().position(|a| a == "--say") {
        return mailbox::say(args.get(i + 1).map(|s| s.as_str()).unwrap_or("")).await;
    }
    if let Some(i) = args.iter().position(|a| a == "--bill") {
        return mailbox::bill(
            args.get(i + 1).map(|s| s.as_str()).unwrap_or(""),
            args.get(i + 2).map(|s| s.as_str()).unwrap_or(""),
            args.get(i + 3).map(|s| s.as_str()).unwrap_or(""),
        ).await;
    }
    if let Some(i) = args.iter().position(|a| a == "--receipt") {
        return mailbox::receipt(
            args.get(i + 1).map(|s| s.as_str()).unwrap_or(""),
            args.get(i + 2).map(|s| s.as_str()).unwrap_or(""),
        ).await;
    }
    if args.iter().any(|a| a == "--contacts") {
        return mailbox::contacts_list();
    }
    if let Some(i) = args.iter().position(|a| a == "--contact-save") {
        return mailbox::contact_save(args.get(i + 1).map(|s| s.as_str()).unwrap_or(""));
    }
    if let Some(i) = args.iter().position(|a| a == "--geo") {
        // The helper the field kept needing: where am I, in board names.
        let lat: f64 = args.get(i + 1).and_then(|s| s.parse().ok()).ok_or("--geo <lat> <lon>")?;
        let lon: f64 = args.get(i + 2).and_then(|s| s.parse().ok()).ok_or("--geo <lat> <lon>")?;
        let (la, lo) = ((lat * 1e7) as i64, (lon * 1e7) as i64);
        let six = ducat_core::geo::geohash_encode(la, lo, 6).map_err(|e| format!("{e:?}"))?;
        let five = ducat_core::geo::geohash_encode(la, lo, 5).map_err(|e| format!("{e:?}"))?;
        println!("\n  cell (6)   {six}   → board geo:{six}");
        println!("  cell (5)   {five}    → board geo:{five}");
        println!("  drive:     --hail-watch geo:{six}\n");
        return Ok(());
    }
    if let Some(i) = args.iter().position(|a| a == "--ride-offer") {
        return mailbox::ride_offer(
            args.get(i + 1).map(|s| s.as_str()).unwrap_or(""),
            args.get(i + 2).map(|s| s.as_str()).unwrap_or("0"),
            args.get(i + 3).map(|s| s.as_str()).unwrap_or(""),
        ).await;
    }
    if let Some(i) = args.iter().position(|a| a == "--ride-accept") {
        return mailbox::ride_accept(
            args.get(i + 1).map(|s| s.as_str()).unwrap_or(""),
            args.get(i + 2).map(|s| s.as_str()).unwrap_or(""),
        ).await;
    }
    if let Some(i) = args.iter().position(|a| a == "--retract") {
        return mailbox::retract(
            args.get(i + 1).map(|s| s.as_str()).unwrap_or(""),
            args.get(i + 2).map(|s| s.as_str()).unwrap_or("0"),
            args.get(i + 3).map(|s| s.as_str()).unwrap_or(""),
        ).await;
    }
    if let Some(i) = args.iter().position(|a| a == "--card-collect") {
        return mailbox::collect(args.get(i + 1).map(|s| s.as_str()).unwrap_or("")).await;
    }
    if args.iter().any(|a| a == "--inbox-create") {
        return dht::inbox_create().await;
    }
    if let Some(i) = args.iter().position(|a| a == "--inbox-reply") {
        return dht::inbox_reply(
            args.get(i + 1).map(|s| s.as_str()).unwrap_or(""),
            args.get(i + 2).map(|s| s.as_str()).unwrap_or(""),
            args.get(i + 3).map(|s| s.as_str()).unwrap_or(""),
        ).await;
    }
    if let Some(i) = args.iter().position(|a| a == "--inbox-collect") {
        return dht::inbox_collect(args.get(i + 1).map(|s| s.as_str()).unwrap_or("")).await;
    }
    if let Some(i) = args.iter().position(|a| a == "--stand-post") {
        return stand::post(
            args.get(i + 1).map(|s| s.as_str()).unwrap_or(""),
            args.get(i + 2).map(|s| s.as_str()).unwrap_or("a notice"),
        ).await;
    }
    if let Some(i) = args.iter().position(|a| a == "--stand-read") {
        return stand::read(args.get(i + 1).map(|s| s.as_str()).unwrap_or("")).await;
    }
    if args.iter().any(|a| a == "--peek-seals") {
        return stand::peek_seals().await;
    }
    if let Some(i) = args.iter().position(|a| a == "--hail-watch") {
        return stand::hail_watch(args.get(i + 1).map(|s| s.as_str()).unwrap_or("")).await;
    }
    if args.iter().any(|a| a == "--dht-write") {
        return dht::write().await;
    }
    if let Some(i) = args.iter().position(|a| a == "--dht-read") {
        let key = args.get(i + 1).cloned().unwrap_or_default();
        if key.is_empty() {
            eprintln!("usage: ducat-harness --dht-read <record key>");
            std::process::exit(2);
        }
        return dht::read(&key).await;
    }
    if args.iter().any(|a| a == "--edges") {
        return edges::run().await;
    }
    if args.iter().any(|a| a == "--attack") {
        return attack::run(&tap_path).await;
    }
    if args.iter().any(|a| a == "--present") {
        return inverted::present(&tap_path).await;
    }
    if let Some(i) = args.iter().position(|a| a == "--scan") {
        let amount = args.get(i + 1).and_then(|v| v.parse().ok()).unwrap_or(300_000_000);
        return inverted::scan(&tap_path, amount).await;
    }
    if args.iter().any(|a| a == "--payee") {
        let amount = args
            .iter()
            .position(|a| a == "--payee")
            .and_then(|i| args.get(i + 1))
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(600_000_000); // 0.0006 XMR, the coffee from the market run
        let fast = args.iter().any(|a| a == "--fast");
        payee::run(&tap_path, amount, fast).await
    } else if args.iter().any(|a| a == "--payer") {
        payer::run(&tap_path).await
    } else {
        eprintln!("usage: ducat-harness --payee [amount_pxmr] [--fast] | --payer");
        eprintln!("       DUCAT_TAP=<path> selects the tap file (default /tmp/ducat-tap.blob)");
        std::process::exit(2);
    }
}
