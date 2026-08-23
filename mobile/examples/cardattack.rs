//! Contest a card's reply subkey, the way a board reader can.
//!
//! `cargo run --release -p ducat-mobile --example cardattack -- <ducat:card/…>`
//!
//! A card URI carries its own writer secret — that is what lets the person you
//! hand it to answer at all. For a tap or a QR that secret goes to one person;
//! for a hail or a listing the whole URI is public board text, so *everyone*
//! reading the board holds the capability to write the reply slot. A subkey is
//! a mutable slot and the set helper retries against the network's sequence, so
//! a second writer wins and is adopted as the counterparty — payment address
//! and all.
//!
//! This is that attacker: it writes the slot twice and reports the sequence it
//! leaves behind. The issuer is expected to notice and discard the card
//! unclaimed rather than adopt anybody (`Mailbox.claimedOnce`). Run it against
//! a card a phone has just created, then read that phone's log.
//!
//! Junk bytes on purpose — the point is the *slot*, not the payload. An issuer
//! that discards on the sequence never reaches the parse, and one that does not
//! should fail loudly on nonsense rather than quietly adopt a forgery.

use ducat_mobile::contacts::read_contact_card;
use ducat_mobile::node::{node_dht_get_versioned, node_dht_open, node_dht_set, node_start, node_status};

fn main() {
    let uri = std::env::args().nth(1).expect("cardattack <ducat:card/…>");
    let card = read_contact_card(uri).expect("that is not a card");
    println!("inbox   {}", card.inbox_key);
    println!("writer  {} bytes of secret, straight out of the URI", card.writer_secret.len());

    let dir = std::env::temp_dir().join(format!("ducat-cardattack-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("state dir");
    node_start(dir.to_string_lossy().into(), true).expect("start");

    print!("waiting for the node");
    for _ in 0 .. 180 {
        if node_status().public_internet_ready {
            println!("  ready — {} peers", node_status().peers);
            break;
        }
        print!(".");
        use std::io::Write as _;
        std::io::stdout().flush().ok();
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
    if !node_status().public_internet_ready {
        println!("\nATTACK_SKIP the node never became ready");
        std::process::exit(2);
    }

    node_dht_open(
        card.inbox_key.clone(),
        Some(card.writer_public.clone()),
        Some(card.writer_secret.clone()),
    )
    .expect("open as writer — the URI said we could");

    let before = node_dht_get_versioned(card.inbox_key.clone(), 1, true).expect("read");
    println!("before  {:?}", before.as_ref().map(|v| (v.seq, v.data.len())));

    for (n, body) in [(1, b"contested-1".to_vec()), (2, b"contested-2".to_vec())] {
        match node_dht_set(card.inbox_key.clone(), 1, body) {
            Ok(()) => println!("write {n} accepted"),
            Err(e) => println!("write {n} refused: {e}"),
        }
    }

    let after = node_dht_get_versioned(card.inbox_key.clone(), 1, true).expect("read");
    let seq = after.as_ref().and_then(|v| v.seq);
    println!("after   seq {seq:?}");
    println!(
        "{}",
        match seq {
            Some(s) if s > 0 => "ATTACK_WROTE the slot now reads as contested — \
                                 the issuer must discard this card unclaimed",
            _ => "ATTACK_INERT the slot does not read as contested; nothing was proved",
        }
    );
}
