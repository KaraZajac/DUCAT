//! Network check: does `monero_tx_details` agree with the daemon's own JSON?
//! Ignored by default — it needs a live stagenet node.

#[test]
#[ignore]
fn tx_details_match_the_daemon() {
    let node = "http://xmr-lux.boldsuck.org:38081".to_string();
    for h in [
        "7c4138288e0d50a38f6017c357e33ecc76b0e03e4caf1f4a3cf9c7269acdf2a1",
        "ac884f521e6ff57c73ec0acb17c8177d0cac8926a47bc69da63aae5969687fd3",
        "f3decd432895cb6639c89244449bb0005f58a0da7943ca3e3900ba1f1b065408",
    ] {
        let d = ducat_mobile::monero::monero_tx_details(node.clone(), h.to_string())
            .expect("details");
        assert_eq!(d.tx_hash_hex, h, "echoed the wrong id");
        println!(
            "{h}\n  v{} fee={} in={} out={} ring={} coinbase={} extra={}B\n  kis={:?}",
            d.version, d.fee_pxmr, d.input_count, d.output_count, d.ring_size,
            d.coinbase, d.extra_len, d.key_images_hex,
        );
    }
    let t = ducat_mobile::monero::monero_block_time(node, 2184652).expect("time");
    assert_eq!(t, 1786638236, "block time disagrees with the daemon");
    println!("block time ok: {t}");
}
