fn main() {
    for cur in ["usd", "eur", "gbp"] {
        match ducat_mobile::monero::monero_rate(cur.into(), 12_000) {
            Ok(r) => println!("{:>4}  {:.2} per XMR  via {}", r.currency, r.per_xmr, r.source),
            Err(e) => println!("{cur:>4}  FAILED {e:?}"),
        }
    }
}
