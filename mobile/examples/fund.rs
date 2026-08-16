//! Fund an address from the research stagenet wallet, from the host.
//!
//!   fund <dest_address> <amount_pxmr>
//!
//! Reads research/monero-rs/testwallet.key (spend hex / address / birth
//! height) and drives the same scan+send path the phone uses, so nothing
//! here is a second implementation that can drift.

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let dest = args.get(1).expect("fund <dest> <amount_pxmr>").clone();
    let amount: u64 = args.get(2).expect("fund <dest> <amount_pxmr>").parse().expect("amount");

    let keyfile = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("research/monero-rs/testwallet.key");
    let content = std::fs::read_to_string(keyfile).expect("testwallet.key");
    let mut lines = content.lines();
    let spend_hex = lines.next().expect("spend key line").trim().to_string();
    let _addr = lines.next().expect("address line");
    let birth: u64 = lines.next().expect("height line").trim().parse().expect("height");

    let node = "http://xmr-lux.boldsuck.org:38081".to_string();
    println!("scanning testwallet from {birth}…");
    let mut from = birth;
    let mut outs = Vec::new();
    let mut tip;
    loop {
        let r = ducat_mobile::monero::monero_scan(node.clone(), spend_hex.clone(), from, 100_000, 0)
            .expect("scan");
        outs.extend(r.outputs);
        tip = r.tip;
        if r.scanned_to >= r.tip {
            break;
        }
        from = r.scanned_to;
    }
    // `fund CHECKONLY 0` — report holdings and exit; the funds watcher polls
    // this while a top-up is in flight.
    if dest == "CHECKONLY" {
        let total: u64 = outs.iter().map(|o| o.amount_pxmr).sum();
        let newest = outs.iter().map(|o| o.height).max().unwrap_or(0);
        println!("CHECK {} output(s) {} pXMR newest_height {} tip {}",
            outs.len(), total, newest, tip);
        return;
    }

    // Spendable = deep enough to be unlocked, and not already spent.
    let candidates: Vec<_> = outs.iter().filter(|o| o.height + 10 <= tip).collect();
    let images: Vec<String> = candidates.iter().map(|o| o.key_image_hex.clone()).collect();
    let spent = ducat_mobile::monero::monero_spent(node.clone(), images).expect("spent check");
    let mut picked = Vec::new();
    let mut total = 0u64;
    for (o, s) in candidates.iter().zip(spent) {
        if s {
            continue;
        }
        picked.push(o.blob.clone());
        total += o.amount_pxmr;
        if total > amount + 400_000_000 {
            break;
        }
    }
    println!("  {} spendable inputs, {total} pXMR gathered (need {amount} + fee)", picked.len());
    assert!(total > amount + 200_000_000, "not enough unlocked funds");

    let r = ducat_mobile::monero::monero_send(node, spend_hex, picked, dest, amount, 1).expect("send");
    println!("  txid {} fee {} accepted_by {}", r.txid_hex, r.fee_pxmr, r.accepted_by);
}
