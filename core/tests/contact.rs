//! Record-based contact cards and 1:1 message chains (§16.9, §16.10, §16.12).

use ducat_core::contact::*;

fn card(expiry: u64) -> ContactCard {
    ContactCard {
        version: 1,
        suite: 1,
        persona: vec![0xAA; 32],
        inbox_key: "VLD0:Qaq9to-SDiN5usgs4gxdmFDo-V8OH_xztMIcv8QWqPA:qqSJ7OtADw5pvvKc1Z3uISLsq-uOl1gmmL3r_8vhvkA".into(),
        writer_public: vec![0xBB; 32],
        display_name: Some("kara".into()),
        expiry,
    }
}

#[test]
fn cards_round_trip_through_canonical_cbor() {
    let c = card(9999);
    assert_eq!(ContactCard::from_value(c.to_value()).unwrap(), c);
}

#[test]
fn a_card_without_a_name_is_still_a_card() {
    let mut c = card(9999);
    c.display_name = None;
    assert_eq!(ContactCard::from_value(c.to_value()).unwrap(), c);
}

#[test]
fn a_card_must_carry_an_inbox() {
    let mut v = card(9999).to_value();
    if let ducat_core::cbor::Value::Map(m) = &mut v {
        m.remove(&168);
    }
    assert!(ContactCard::from_value(v).is_err());
}

#[test]
fn an_inbox_key_is_bounded() {
    let mut c = card(9999);
    c.inbox_key = "V".repeat(MAX_RECORD_KEY_CHARS + 1);
    assert!(ContactCard::from_value(c.to_value()).is_err());
}

#[test]
fn a_display_name_cannot_smuggle_a_paragraph() {
    let mut c = card(9999);
    c.display_name = Some("x".repeat(MAX_DISPLAY_NAME_CHARS + 1));
    assert!(ContactCard::from_value(c.to_value()).is_err());
}

#[test]
fn an_empty_display_name_is_refused() {
    let mut c = card(9999);
    c.display_name = Some(String::new());
    assert!(ContactCard::from_value(c.to_value()).is_err());
}

/// The URI carries the writer secret beside the card, never inside it — so the
/// signed object can be logged without becoming answerable.
#[test]
fn a_card_round_trips_through_its_uri() {
    let env = b"pretend this is a signed envelope".to_vec();
    let secret = [0x5E; 32];
    let uri = card_to_uri(&env, &secret);
    assert!(uri.starts_with(CARD_URI_PREFIX));
    let (back_env, back_secret) = card_from_uri(&uri).unwrap();
    assert_eq!(back_env, env);
    assert_eq!(back_secret, secret);
}

#[test]
fn a_truncated_uri_is_refused_rather_than_half_parsed() {
    let uri = card_to_uri(b"envelope", &[1u8; 32]);
    let cut = &uri[..uri.len() - 20];
    assert!(card_from_uri(cut).is_err());
    assert!(card_from_uri("ducat:card/nodotseparator").is_err());
    assert!(card_from_uri("https://example.com/x.y").is_err());
}

// --- inbox details --------------------------------------------------------

fn details() -> ContactDetails {
    ContactDetails {
        version: 1,
        suite: 1,
        persona: vec![0xCC; 32],
        outbox_key: "VLD0:abc:def".into(),
        prekey_bundle: vec![0x01, 0x02, 0x03],
        display_name: Some("sam".into()),
    }
}

#[test]
fn details_round_trip() {
    let d = details();
    assert_eq!(ContactDetails::from_value(d.to_value()).unwrap(), d);
}

#[test]
fn details_must_name_an_outbox() {
    let mut v = details().to_value();
    if let ducat_core::cbor::Value::Map(m) = &mut v {
        m.remove(&173);
    }
    assert!(ContactDetails::from_value(v).is_err());
}

// --- the log ring ---------------------------------------------------------

#[test]
fn heads_round_trip() {
    let h = LogHead { version: 1, suite: 1, next_seq: 42, prekey_bundle: None };
    assert_eq!(LogHead::from_value(h.to_value()).unwrap(), h);
}

/// Subkey 0 is the head, so messages start at 1 and wrap without ever landing
/// on it. An off-by-one here overwrites the head with a message, which loses
/// the whole log rather than one entry.
#[test]
fn messages_never_land_on_the_head_subkey() {
    for count in [2u32, 3, 8, 64] {
        for seq in 0u64..200 {
            let sk = subkey_for(seq, count);
            assert!(sk >= 1, "seq {seq} landed on the head with {count} subkeys");
            assert!(sk < count, "seq {seq} ran past subkey {count}");
        }
    }
}

#[test]
fn the_ring_wraps_at_the_slot_count() {
    // 8 subkeys = 1 head + 7 slots.
    assert_eq!(subkey_for(0, 8), 1);
    assert_eq!(subkey_for(6, 8), 7);
    assert_eq!(subkey_for(7, 8), 1);
    assert_eq!(subkey_for(14, 8), 1);
}

/// A reader that was away long enough has genuinely lost messages, and must be
/// able to tell — silently showing a thread with a hole in it is §16.10's
/// "conversation that did not happen".
#[test]
fn a_reader_can_tell_when_the_ring_has_passed_it() {
    // 8 subkeys, 7 slots. Writer is at 10, so 3..9 are still fetchable.
    assert!(still_in_ring(9, 10, 8));
    assert!(still_in_ring(3, 10, 8));
    assert!(!still_in_ring(2, 10, 8), "seq 2 was overwritten by seq 9");
    assert!(!still_in_ring(10, 10, 8), "seq 10 has not been written yet");
    assert!(!still_in_ring(11, 10, 8));
}

// --- messages -------------------------------------------------------------

fn msg(seq: u64, prev: [u8; 32], body: &str) -> Message {
    Message {
        version: 1,
        suite: 1,
        seq,
        prev,
        body: body.into(),
        timestamp: 1000 + seq,
    }
}

#[test]
fn messages_chain() {
    let m0 = msg(0, [0u8; 32], "hey");
    check_message(&m0, 0, None).unwrap();
    let m1 = msg(1, m0.link(), "you around?");
    check_message(&m1, 1, Some(&m0)).unwrap();
}

#[test]
fn a_gap_is_refused_rather_than_papered_over() {
    let m0 = msg(0, [0u8; 32], "hey");
    let m2 = msg(2, m0.link(), "third");
    let err = check_message(&m2, 1, Some(&m0)).unwrap_err();
    assert!(format!("{err:?}").contains("StateViolation"), "{err:?}");
}

#[test]
fn a_substituted_message_is_caught_by_the_link() {
    let m0 = msg(0, [0u8; 32], "hey");
    let forged = msg(1, msg(0, [0u8; 32], "different").link(), "you around?");
    let err = check_message(&forged, 1, Some(&m0)).unwrap_err();
    assert!(format!("{err:?}").contains("CommitMismatch"), "{err:?}");
}

#[test]
fn the_first_message_must_link_to_nothing() {
    let err = check_message(&msg(0, [9u8; 32], "hey"), 0, None).unwrap_err();
    assert!(format!("{err:?}").contains("CommitMismatch"), "{err:?}");
}

#[test]
fn a_message_body_is_bounded() {
    let m = msg(0, [0u8; 32], &"x".repeat(MAX_MESSAGE_CHARS + 1));
    assert!(Message::from_value(m.to_value()).is_err());
}

#[test]
fn an_empty_body_is_refused_rather_than_being_a_second_spelling_of_none() {
    assert!(Message::from_value(msg(0, [0u8; 32], "").to_value()).is_err());
}

#[test]
fn messages_round_trip() {
    let m = msg(3, [4u8; 32], "here's the 20 back");
    assert_eq!(Message::from_value(m.to_value()).unwrap(), m);
}
