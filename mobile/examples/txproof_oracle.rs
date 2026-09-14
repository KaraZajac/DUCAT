//! The oracle check for §9.5's payment proofs: our `OutProofV2` against the
//! reference wallet's, both ways.
//!
//!   cargo run -p ducat-mobile --example txproof_oracle -- check <node_url> <txid> <address> <stagenet:1|0> <message> <proof>
//!       → prints the amount our verifier finds; compare with wallet-rpc's check_tx_proof
//!   cargo run -p ducat-mobile --example txproof_oracle -- make <txid> <address> <stagenet:1|0> <message> <tx_key_hex>[,<additional_hex>...]
//!       → prints a proof for wallet-rpc's check_tx_proof to judge
//!   cargo run -p ducat-mobile --example txproof_oracle -- burn-address <stagenet:1|0>
//!
//! The message is taken as a literal string (ASCII), the same bytes the
//! wallet-rpc was handed in JSON.

use ducat_mobile::txproof::{monero_burn_address, monero_check_out_proof, monero_make_out_proof};

fn fetch_tx_hex(node: &str, txid: &str) -> String {
    let url = format!("{}/get_transactions", node.trim_end_matches('/'));
    let body = format!("{{\"txs_hashes\":[\"{txid}\"]}}");
    let text = ureq::post(&url)
        .set("Content-Type", "application/json")
        .send_string(&body)
        .expect("node unreachable")
        .into_string()
        .expect("node did not answer");
    let resp: serde_json::Value = serde_json::from_str(&text).expect("node did not answer JSON");
    resp["txs"][0]["as_hex"].as_str().expect("no as_hex in the node's answer").to_string()
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("burn-address") => println!("{}", monero_burn_address(args[1] == "1")),
        Some("check") => {
            let (node, txid, address, stagenet, message, proof) = (&args[1], &args[2], &args[3], args[4] == "1", &args[5], &args[6]);
            let tx_hex = fetch_tx_hex(node, txid);
            match monero_check_out_proof(tx_hex, txid.clone(), address.clone(), stagenet, message.as_bytes().to_vec(), proof.clone()) {
                Ok(amount) => println!("received={amount}"),
                Err(e) => {
                    println!("refused: {e}");
                    std::process::exit(2);
                }
            }
        }
        Some("make") => {
            let (txid, address, stagenet, message, keys) = (&args[1], &args[2], args[3] == "1", &args[4], &args[5]);
            let keys: Vec<String> = keys.split(',').map(|s| s.to_string()).collect();
            match monero_make_out_proof(txid.clone(), message.as_bytes().to_vec(), keys, address.clone(), stagenet) {
                Ok(p) => println!("{p}"),
                Err(e) => {
                    println!("refused: {e}");
                    std::process::exit(2);
                }
            }
        }
        _ => {
            eprintln!("usage: txproof_oracle check|make|burn-address ...");
            std::process::exit(1);
        }
    }
}
