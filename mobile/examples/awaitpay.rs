//! Watch the test wallet for an incoming payment, then report it.
//!
//! Scans a window near the tip repeatedly rather than from the restore height:
//! the payment being waited for is by definition recent, and walking 800 blocks
//! per poll would spend the whole wait re-reading history.

fn main() {
    const NODE: &str = "http://xmr-lux.boldsuck.org:38081";
    let key = std::fs::read_to_string("research/monero-rs/testwallet.key").expect("key file");
    let mut lines = key.lines();
    let spend = lines.next().unwrap().to_string();
    let address = lines.next().unwrap_or_default();
    println!("watching {}…\n", &address[..24]);

    let mut seen: Vec<String> = Vec::new();
    for round in 0..14 {
        let st = ducat_mobile::monero::monero_probe(NODE.into(), 15_000);
        let from = st.height.saturating_sub(25);
        match ducat_mobile::monero::monero_scan(NODE.into(), spend.clone(), from, 25) {
            Ok(r) => {
                for o in &r.outputs {
                    if seen.contains(&o.key_image_hex) {
                        continue;
                    }
                    seen.push(o.key_image_hex.clone());
                    println!(
                        "  RECEIVED {} pXMR ({:.6} XMR) at height {}",
                        o.amount_pxmr,
                        o.amount_pxmr as f64 / 1e12,
                        o.height
                    );
                }
                if round % 3 == 0 || !r.outputs.is_empty() {
                    println!(
                        "  [{}] tip {} — read {} blocks, {} found so far",
                        round, st.height, r.blocks_read, seen.len()
                    );
                }
                if !seen.is_empty() {
                    println!("\n  payment seen — it needs 10 blocks (~20 min) to unlock");
                    return;
                }
            }
            Err(e) => println!("  [{}] scan failed: {e:?}", round),
        }
        std::thread::sleep(std::time::Duration::from_secs(20));
    }
    println!("\n  nothing yet — it may still be in the pool");
}
