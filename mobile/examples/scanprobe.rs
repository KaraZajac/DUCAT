fn main() {
    // Proves the ureq-backed daemon transport reads real blocks, which is the
    // thing that failed on the phone while the plain probe beside it worked.
    let node = "http://xmr-lux.boldsuck.org:38081".to_string();
    let st = ducat_mobile::monero::monero_probe(node.clone(), 15_000);
    println!("probe   height={} synced={}", st.height, st.synced);
    let key = std::fs::read_to_string("research/monero-rs/testwallet.key").unwrap();
    let spend = key.lines().next().unwrap().to_string();
    let from = st.height.saturating_sub(30);
    match ducat_mobile::monero::monero_scan(node, spend, from, 30, 0) {
        Ok(r) => println!(
            "scan    {}..{} — read {} block(s), {} failed, {} output(s)",
            from, r.scanned_to, r.blocks_read, r.blocks_failed, r.outputs.len()
        ),
        Err(e) => println!("scan    FAILED {e:?}"),
    }
}
