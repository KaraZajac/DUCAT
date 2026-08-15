//! Does the scanner actually find money? (§17.2)
//!
//! A scan that returns nothing is indistinguishable from a scan that is broken,
//! so this checks against **known ground truth**: the FROSTLASS spike funded a
//! stagenet group address and later spent from it, and both amounts and heights
//! were recorded at the time.
//!
//!   cargo run -p ducat-mobile --example scantest

const NODE: &str = "http://xmr-lux.boldsuck.org:38081";
const FUNDING_HEIGHT: u64 = 2_183_921;
const FUNDING_PXMR: u64 = 800_000_000;

fn main() {
    // The keys this checked against are no longer in the repository: they were
    // real Monero secrets, and a public repository is not where those live. The
    // check still runs for anyone holding them locally, and skips cleanly for
    // everyone else rather than failing in a way that reads like a broken
    // scanner.
    let dir = std::path::Path::new("research/monero-rs/frostlass-spike/group");
    let (addr, view) = match (
        std::fs::read_to_string(dir.join("address.txt")),
        std::fs::read_to_string(dir.join("view.hex")),
    ) {
        (Ok(a), Ok(v)) => (a.trim().to_string(), v.trim().to_string()),
        _ => {
            println!("skipped — no local key material for the known-funded group.");
            println!("Fund examples/testwallet.rs instead; it verifies the same path.");
            return;
        }
    };

    let st = ducat_mobile::monero::monero_probe(NODE.into(), 15_000);
    println!("node    height={} synced={} net={}", st.height, st.synced, st.nettype);
    assert!(st.synced && st.nettype == "stagenet", "need a synced stagenet node");

    let from = FUNDING_HEIGHT - 6;
    let r = ducat_mobile::monero::monero_scan_view_only(NODE.into(), addr, view, from, 20, 0)
        .expect("scan");
    println!("scan    {}..{} — {} output(s)", from, r.scanned_to, r.outputs.len());
    for o in &r.outputs {
        println!("        {} pXMR at height {}", o.amount_pxmr, o.height);
    }

    // The funding is the anchor. The second output is the change from the 3-of-5
    // spend, which is only there because the spend really happened.
    assert!(
        r.outputs.iter().any(|o| o.height == FUNDING_HEIGHT && o.amount_pxmr == FUNDING_PXMR),
        "the known funding output was not found — the scanner is not working"
    );
    println!("\n  found the known funding output — positive detection confirmed");
}
