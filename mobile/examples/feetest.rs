fn main() {
    let node = "http://xmr-lux.boldsuck.org:38081".to_string();
    for (i, o) in [(1u32, 2u32), (2, 2), (5, 2)] {
        for p in 0u32..4 {
            let f = ducat_mobile::monero::monero_fee_estimate(node.clone(), i, o, p).unwrap();
            if p == 0 {
                println!("{i} in / {o} out — {} bytes", f.estimated_bytes);
            }
            println!(
                "   tier {p}: {:>12} pXMR ({:.6} XMR) at {} per byte, ~{} min",
                f.fee_pxmr, f.fee_pxmr as f64 / 1e12, f.per_byte, f.minutes_to_confirm
            );
        }
    }
}
