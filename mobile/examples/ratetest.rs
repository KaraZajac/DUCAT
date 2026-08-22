// Live check of the rate rule: what the venues say, and what gets believed.
//
// `None` for the last-known rate on purpose — this is the first-run case, the
// one where a lone quote has nothing to corroborate it and is refused. A
// currency only one venue carries will print FAILED here, and that is the rule
// working rather than a broken endpoint.
fn main() {
    for cur in ["USD", "THB", "AED"] {
        match ducat_mobile::monero::monero_rate(cur.into(), 8000, None) {
            Ok(r) => println!("{cur}: {} via {}", r.per_xmr, r.source),
            Err(e) => println!("{cur}: FAILED {e}"),
        }
    }
}
