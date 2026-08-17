//! Verify the split release landed: scan the testwallet with subaddress
//! coverage and print each output's amount and receiving minor.
fn main() {
    let from: u64 = std::env::args().nth(1).expect("splitcheck <from_height>").parse().unwrap();
    let key = std::fs::read_to_string("research/monero-rs/testwallet.key").unwrap();
    let spend = key.lines().next().unwrap().trim().to_string();
    let node = "http://xmr-lux.boldsuck.org:38081".to_string();
    let r = ducat_mobile::monero::monero_scan(node, spend, from, 10_000, 8).expect("scan");
    println!("scanned to {} (tip {})", r.scanned_to, r.tip);
    for o in r.outputs {
        println!("  {} pXMR  minor {}  tx {}", o.amount_pxmr, o.minor, &o.tx_hash_hex[..16]);
    }
}
