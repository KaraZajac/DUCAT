//! A wallet the harness controls, so `send` can be verified against the real
//! chain rather than only compiled.
//!
//! Persisted, because `create_wallet` draws from the OS CSPRNG: a fresh address
//! every run would mean funding one and then testing with another.
//!
//!   cargo run -p ducat-mobile --example testwallet          # show / create
//!   cargo run -p ducat-mobile --example testwallet -- send <address> <xmr>

const NODE: &str = "http://xmr-lux.boldsuck.org:38081";
/// Resolved against the crate, not the working directory.
///
/// It was a bare relative path, and that is a wallet-destroying bug waiting for
/// the wrong `cd`: run from elsewhere, the read misses, `load_or_create` decides
/// the wallet does not exist, and generates a new one **over the old key file**
/// if the path happens to resolve there. The funds are not recoverable
/// afterwards. A key file's location must not depend on where you were standing.
fn keyfile() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crate has a parent")
        .join("research/monero-rs/testwallet.key")
}

fn main() {
    let st = ducat_mobile::monero::monero_probe(NODE.into(), 15_000);
    let (spend, address, restore) = load_or_create(st.height);

    let args: Vec<String> = std::env::args().collect();
    if args.len() >= 4 && args[1] == "send" {
        let to = args[2].clone();
        let xmr: f64 = args[3].parse().expect("amount in XMR");
        let pxmr = (xmr * 1e12) as u64;

        // Scan for what we can spend. One request per block, so this walks from
        // the recorded restore height rather than from genesis.
        // Same skip as `balance`: this walks one request per block, and a
        // wallet whose restore height is a thousand blocks back should not
        // re-read all of it to spend an output found ten minutes ago.
        let mut from = std::env::var("SCAN_FROM")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(restore);
        let mut blobs = Vec::new();
        while from < st.height {
            let r = ducat_mobile::monero::monero_scan(NODE.into(), spend.clone(), from, 2_000)
                .expect("scan");
            for o in &r.outputs {
                println!("  found {} pXMR at {}", o.amount_pxmr, o.height);
                blobs.push((o.blob.clone(), o.key_image_hex.clone(), o.amount_pxmr));
            }
            if r.scanned_to <= from { break }
            from = r.scanned_to;
        }
        // **Scanning finds outputs; it does not say which are still there.**
        // Every found blob went straight in as an input, so a wallet that had
        // ever spent anything built a transaction double-spending it, and the
        // daemon's refusal arrived as "no node accepted it" — which reads as a
        // network problem and is arithmetic.
        let kis: Vec<String> = blobs.iter().map(|(_, k, _)| k.clone()).collect();
        let spent = ducat_mobile::monero::monero_spent(NODE.into(), kis).expect("spent check");
        let blobs: Vec<Vec<u8>> = blobs
            .into_iter()
            .zip(&spent)
            .filter(|(_, s)| !**s)
            .map(|((b, _, a), _)| {
                println!("  spendable {a} pXMR");
                b
            })
            .collect();
        if blobs.is_empty() {
            println!("nothing to spend — fund {address} first");
            return;
        }
        let priority: u32 = std::env::var("PRIORITY").ok().and_then(|v| v.parse().ok()).unwrap_or(1);
        match ducat_mobile::monero::monero_send(NODE.into(), spend, blobs, to, pxmr, priority) {
            Ok(r) => println!(
                "\n  txid {}\n  fee {} pXMR\n  accepted by {} node(s)",
                r.txid_hex, r.fee_pxmr, r.accepted_by
            ),
            Err(e) => println!("\n  FAILED {e:?}"),
        }
        return;
    }

    if args.get(1).map(|a| a == "balance").unwrap_or(false) {
        // SCAN_FROM skips ahead when the interesting range is known: this walks
        // one request per block, so a thousand blocks is a thousand round trips.
        let mut from = std::env::var("SCAN_FROM").ok().and_then(|v| v.parse().ok()).unwrap_or(restore);
        let mut kis = Vec::new();
        let mut total = 0u64;
        while from < st.height {
            let r = ducat_mobile::monero::monero_scan(NODE.into(), spend.clone(), from, 2_000)
                .expect("scan");
            for o in &r.outputs {
                println!("  {} pXMR at {}  ki {}", o.amount_pxmr, o.height, o.key_image_hex);
                kis.push(o.key_image_hex.clone());
                total += o.amount_pxmr;
            }
            if r.scanned_to <= from { break }
            from = r.scanned_to;
        }
        let spent = ducat_mobile::monero::monero_spent(NODE.into(), kis.clone()).expect("spent");
        let unspent: u64 = kis.iter().zip(&spent).enumerate()
            .filter(|(_, (_, s))| !**s).map(|(i, _)| i).map(|_| 0u64).sum();
        let _ = unspent;
        println!("\n  address {address}");
        println!("  found {} output(s), {} pXMR gross", kis.len(), total);
        println!("  spent flags {spent:?}");
        return;
    }

    println!("address       {address}");
    println!("restore from  {restore}");
    println!("chain tip     {}", st.height);
    println!("\nfund it, then:\n  cargo run -p ducat-mobile --example testwallet -- send <address> <xmr>");
}

fn load_or_create(tip: u64) -> (String, String, u64) {
    if let Ok(txt) = std::fs::read_to_string(keyfile()) {
        let mut it = txt.lines();
        let spend = it.next().unwrap_or_default().to_string();
        let address = it.next().unwrap_or_default().to_string();
        let restore = it.next().and_then(|v| v.parse().ok()).unwrap_or(0);
        if !spend.is_empty() {
            return (spend, address, restore);
        }
    }
    // A day back, not the tip: starting at the tip skips anything that lands
    // between generating the address and funding it.
    let w = ducat_mobile::create_wallet(tip.saturating_sub(720), true);
    std::fs::write(
        keyfile(),
        format!("{}\n{}\n{}\n", w.spend_key_hex, w.address, w.restore_height),
    )
    .expect("write key file");
    println!("(created a new test wallet)\n");
    (w.spend_key_hex, w.address, w.restore_height)
}
