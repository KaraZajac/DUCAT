//! What a DHT subkey's sequence says about how many times it was written.
//!
//! `cargo run --release -p ducat-mobile --example seqtest`
//!
//! A card's reply subkey is claim-once, and the check that enforces it reads
//! the sequence: `Some(0)` is one write, anything higher is a second answer
//! (see `Mailbox.claimedOnce`). That rule is only as good as the assumption
//! underneath it — that veilid numbers a subkey's first write zero, and that an
//! honest single claim therefore never looks contested. An assumption about a
//! live network belongs against the live network, so this writes a real record
//! on a real node and reports what comes back.
//!
//! Prints PASS/FAIL and exits non-zero on a mismatch, so it can be run as a
//! check rather than read as a log.

use ducat_mobile::contacts::generate_writer_keys;
use ducat_mobile::node::{
    node_dht_create_shared, node_dht_get_versioned, node_dht_open, node_dht_set, node_start,
    node_status,
};

fn main() {
    let dir = std::env::temp_dir().join(format!("ducat-seqtest-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("state dir");
    node_start(dir.to_string_lossy().into(), true).expect("start");

    // A write needs a route, and a route needs the node to have worked out
    // what sort of network it is on. Attachment alone is not enough.
    print!("waiting for the node");
    for _ in 0 .. 180 {
        let s = node_status();
        if s.public_internet_ready {
            println!("  ready — {} peers", s.peers);
            break;
        }
        print!(".");
        use std::io::Write as _;
        std::io::stdout().flush().ok();
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
    if !node_status().public_internet_ready {
        println!("\nSEQ_SKIP the node never became ready — nothing was proved");
        std::process::exit(2);
    }

    let writer = generate_writer_keys();
    let rec = node_dht_create_shared(writer.public.clone()).expect("create");
    node_dht_open(rec.key.clone(), Some(writer.public.clone()), Some(writer.secret.clone()))
        .expect("open");
    println!("record {}", rec.key);

    let mut fail = false;

    // Never written: no sequence at all, and nothing to read.
    match node_dht_get_versioned(rec.key.clone(), 1, true) {
        Ok(None) => println!("unwritten     -> absent            (expected)"),
        Ok(Some(v)) => println!("unwritten     -> seq {:?}, {} bytes", v.seq, v.data.len()),
        Err(e) => println!("unwritten     -> error {e}"),
    }

    // One honest claim.
    node_dht_set(rec.key.clone(), 1, b"the first answer".to_vec()).expect("first write");
    let after_one = node_dht_get_versioned(rec.key.clone(), 1, true).expect("read").expect("value");
    println!("one write     -> seq {:?}", after_one.seq);
    if after_one.seq != Some(0) {
        println!("  FAIL a single write did not leave Some(0) — claimedOnce would refuse it");
        fail = true;
    }

    // Somebody answering over the top of it.
    node_dht_set(rec.key.clone(), 1, b"the second answer".to_vec()).expect("second write");
    let after_two = node_dht_get_versioned(rec.key.clone(), 1, true).expect("read").expect("value");
    println!("two writes    -> seq {:?}", after_two.seq);
    if after_two.seq == Some(0) || after_two.seq.is_none() {
        println!("  FAIL an overwrite was indistinguishable from a single write");
        fail = true;
    }
    if after_two.data != b"the second answer" {
        println!("  note: the network still serves the older value; the sequence is what matters");
    }

    println!(
        "{}",
        if fail {
            "SEQ_FAIL the sequence does not carry the claim-once property"
        } else {
            "SEQ_OK one write = Some(0), an overwrite is distinguishable"
        }
    );
    std::process::exit(i32::from(fail));
}
