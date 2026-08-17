fn main() {
    let key = std::fs::read_to_string("research/monero-rs/testwallet.key").unwrap();
    let spend = key.lines().next().unwrap().trim().to_string();
    println!("{}", ducat_mobile::monero::monero_subaddress(spend, 9, true).unwrap());
}
