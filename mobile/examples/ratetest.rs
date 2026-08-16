fn main() {
    for cur in ["USD", "THB", "AED"] {
        match ducat_mobile::monero::monero_rate(cur.into(), 8000) {
            Ok(r) => println!("{cur}: {} via {}", r.per_xmr, r.source),
            Err(e) => println!("{cur}: FAILED {e}"),
        }
    }
}
