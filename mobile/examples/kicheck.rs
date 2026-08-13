//! Does the key image we derive match the one the network sees?
//!
//! The only way to know is to ask the daemon before and after spending. A wrong
//! key image answers "not spent" forever, which is indistinguishable from a
//! correct one on an unspent output — and is exactly how spent money got
//! counted twice.

fn main() {
    const NODE: &str = "http://xmr-lux.boldsuck.org:38081";
    let key = std::fs::read_to_string("research/monero-rs/testwallet.key").expect("key file");
    let spend = key.lines().next().unwrap().to_string();

    let st = ducat_mobile::monero::monero_probe(NODE.into(), 15_000);
    let from = st.height.saturating_sub(60);
    let r = ducat_mobile::monero::monero_scan(NODE.into(), spend, from, 60).expect("scan");
    println!("tip {} — {} output(s)", st.height, r.outputs.len());
    if r.outputs.is_empty() {
        println!("nothing to check");
        return;
    }
    let kis: Vec<String> = r.outputs.iter().map(|o| o.key_image_hex.clone()).collect();
    for o in &r.outputs {
        println!(
            "  {} pXMR at {} — unlocks at {}",
            o.amount_pxmr, o.height, o.height + 10
        );
        println!("     key image {}", o.key_image_hex);
    }
    match ducat_mobile::monero::monero_spent(NODE.into(), kis) {
        Ok(v) => println!("\n  daemon says spent: {v:?}"),
        Err(e) => println!("\n  spent query failed: {e:?}"),
    }
}
