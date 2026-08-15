fn main() {
    let node = "http://xmr-lux.boldsuck.org:38081".to_string();
    let st = ducat_mobile::monero::monero_probe(node.clone(), 15000);
    let from = 2183900u64;
    let r = ducat_mobile::monero::monero_scan_view_only(
        node, "525uzvqwmLVVTGiMhqwizL3dA2hR759wedHo45r8mRRyTTypBS8WHDqarqUDtU8ZyK8fxuz7SxTJyipMtrnB9VxgBEEBqFZ".into(), "7fd37f2640fd7539fb456e138973b48ee4efaae164974002a805f6860cf4ae03".into(), from, (st.height - from) as u32, 0).unwrap();
    let total: u64 = r.outputs.iter().map(|o| o.amount_pxmr).sum();
    println!("tip {} — scanned {}..{}", st.height, from, r.scanned_to);
    for o in &r.outputs { println!("  {} pXMR at {}", o.amount_pxmr, o.height); }
    println!("total seen: {} pXMR ({:.6} XMR)", total, total as f64 / 1e12);
}
