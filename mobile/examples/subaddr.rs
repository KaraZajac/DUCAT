fn main() {
    let key = std::fs::read_to_string("research/monero-rs/testwallet.key").unwrap();
    let spend = key.lines().next().unwrap().trim().to_string();
    let a = ducat_mobile::monero::monero_subaddress(spend, 7, true).unwrap();
    println!("{a}");
}
