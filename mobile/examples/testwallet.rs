//! A wallet the harness controls, so `send` can be verified against the real
//! chain rather than only compiled.
//!
//! Persisted, because `create_wallet` draws from the OS CSPRNG: a fresh address
//! every run would mean funding one and then testing with another.
//!
//!   cargo run -p ducat-mobile --example testwallet          # show / create
//!   cargo run -p ducat-mobile --example testwallet -- send <address> <xmr>

const NODE: &str = "http://xmr-lux.boldsuck.org:38081";
const KEYFILE: &str = "research/monero-rs/testwallet.key";

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
        let mut from = restore;
        let mut blobs = Vec::new();
        while from < st.height {
            let r = ducat_mobile::monero::monero_scan(NODE.into(), spend.clone(), from, 2_000)
                .expect("scan");
            for o in &r.outputs {
                println!("  found {} pXMR at {}", o.amount_pxmr, o.height);
                blobs.push(o.blob.clone());
            }
            if r.scanned_to <= from { break }
            from = r.scanned_to;
        }
        if blobs.is_empty() {
            println!("nothing to spend — fund {address} first");
            return;
        }
        match ducat_mobile::monero::monero_send(NODE.into(), spend, blobs, to, pxmr) {
            Ok(r) => println!(
                "\n  txid {}\n  fee {} pXMR\n  accepted by {} node(s)",
                r.txid_hex, r.fee_pxmr, r.accepted_by
            ),
            Err(e) => println!("\n  FAILED {e:?}"),
        }
        return;
    }

    println!("address       {address}");
    println!("restore from  {restore}");
    println!("chain tip     {}", st.height);
    println!("\nfund it, then:\n  cargo run -p ducat-mobile --example testwallet -- send <address> <xmr>");
}

fn load_or_create(tip: u64) -> (String, String, u64) {
    if let Ok(txt) = std::fs::read_to_string(KEYFILE) {
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
        KEYFILE,
        format!("{}\n{}\n{}\n", w.spend_key_hex, w.address, w.restore_height),
    )
    .expect("write key file");
    println!("(created a new test wallet)\n");
    (w.spend_key_hex, w.address, w.restore_height)
}
