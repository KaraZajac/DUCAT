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
        payto: None,
        avatar: None, email: None, phone: None, signal: None, pronouns: None,
        car_model: None,
        car_color: None,
        plate: None,
        purpose: Some("sale".into()),
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
    let h = LogHead { version: 1, suite: 1, next_seq: 42, prekey_bundle: None, read_up_to: None, ring: None };
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
        timestamp: 1000 + seq, kind: MessageKind::Text, amount_pxmr: None, txid: None,
        payto: None, items: Vec::new(), tax_pxmr: None, re_seq: None, re_own: false, eta_secs: None, payload: None, round: None, ceremony_id: None, attachment: None, position: None, publication: None, group_id: None, group_seq: None, group_re_sender: None, group_re_seq: None,
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

// --- money in a conversation (§16.13) -------------------------------------

fn pay(kind: MessageKind, amount: Option<u64>, txid: Option<Vec<u8>>) -> Message {
    Message {
        version: 1, suite: 1, seq: 0, prev: [0u8; 32],
        body: "for the coffee".into(), timestamp: 1000,
        kind, amount_pxmr: amount, txid, payto: None, items: Vec::new(), tax_pxmr: None, re_seq: None, re_own: false, eta_secs: None, payload: None, round: None, ceremony_id: None, attachment: None, position: None, publication: None, group_id: None, group_seq: None, group_re_sender: None, group_re_seq: None,
    }
}

#[test]
fn payment_messages_round_trip() {
    let req = pay(MessageKind::PaymentRequest, Some(21_000_000_000), None);
    assert_eq!(Message::from_value(req.to_value()).unwrap(), req);
    let sent = pay(MessageKind::PaymentSent, Some(21_000_000_000), Some(vec![7u8; 32]));
    assert_eq!(Message::from_value(sent.to_value()).unwrap(), sent);
}

/// Text is the default, so encoding it explicitly would give one meaning two
/// encodings — the thing §18.1 refuses everywhere else.
#[test]
fn text_is_encoded_by_omission() {
    let t = pay(MessageKind::Text, None, None);
    let enc = t.to_value().encode();
    assert!(!enc.windows(1).any(|_| false)); // shape check below is the real one
    let mut v = t.to_value();
    if let ducat_core::cbor::Value::Map(m) = &mut v {
        assert!(!m.contains_key(&178), "text must not encode a kind");
        m.insert(178, ducat_core::cbor::Value::Uint(0));
    }
    assert!(Message::from_value(v).is_err(), "an explicit text kind must be refused");
    let _ = enc;
}

/// A payment screen with a blank where the amount goes.
#[test]
fn a_payment_without_an_amount_is_refused() {
    for k in [MessageKind::PaymentRequest, MessageKind::PaymentSent] {
        assert!(Message::from_value(pay(k, None, None).to_value()).is_err());
    }
}

/// An amount nothing will honour is worse than no amount at all.
#[test]
fn text_must_not_carry_an_amount() {
    assert!(Message::from_value(pay(MessageKind::Text, Some(1), None).to_value()).is_err());
}

#[test]
fn only_a_notice_carries_a_transaction() {
    let bad = pay(MessageKind::PaymentRequest, Some(1), Some(vec![7u8; 32]));
    assert!(Message::from_value(bad.to_value()).is_err());
}

#[test]
fn an_unknown_kind_is_refused_rather_than_shown_as_text() {
    let mut v = pay(MessageKind::Text, None, None).to_value();
    if let ducat_core::cbor::Value::Map(m) = &mut v {
        m.insert(178, ducat_core::cbor::Value::Uint(99));
    }
    assert!(Message::from_value(v).is_err());
}

/// The chain covers the amount, because a request is only as trustworthy as
/// the thread it arrived in.
#[test]
fn changing_an_amount_breaks_the_chain() {
    let m0 = pay(MessageKind::PaymentRequest, Some(1_000), None);
    let tampered = pay(MessageKind::PaymentRequest, Some(9_999_999), None);
    assert_ne!(m0.link(), tampered.link());
}

/// A request names where to pay; nothing else may.
#[test]
fn only_a_request_names_a_destination() {
    let mut req = pay(MessageKind::PaymentRequest, Some(1_000), None);
    req.payto = Some("5ApJU8bfJ2sb4eGHNSCcSjGH4SxMghLahdFoh3NKpkPYhJ".into());
    assert_eq!(Message::from_value(req.to_value()).unwrap(), req);

    for k in [MessageKind::Text, MessageKind::PaymentSent] {
        let mut bad = pay(k, if k == MessageKind::Text { None } else { Some(1) }, None);
        bad.payto = Some("5ApJU8bf".into());
        assert!(
            Message::from_value(bad.to_value()).is_err(),
            "{k:?} must not carry a destination"
        );
    }
}

/// An address field long enough to hold a payload is a covert channel with a
/// signature on it.
#[test]
fn a_destination_is_bounded() {
    let mut req = pay(MessageKind::PaymentRequest, Some(1), None);
    req.payto = Some("5".repeat(MAX_ADDRESS_CHARS + 1));
    assert!(Message::from_value(req.to_value()).is_err());
}

/// Changing where a request points must break the thread, or a request is only
/// as trustworthy as the last person to touch the record it sits in.
#[test]
fn changing_the_destination_breaks_the_chain() {
    let mut a = pay(MessageKind::PaymentRequest, Some(1_000), None);
    a.payto = Some("5ApJU8bf".into());
    let mut b = a.clone();
    b.payto = Some("5Attacker".into());
    assert_ne!(a.link(), b.link());
}

/// A contact may publish an address so they can be paid without asking first.
/// Optional, because §16.12 makes it a real trade against linkability.
#[test]
fn contact_details_may_carry_a_payout_address() {
    let mut d = details();
    assert_eq!(d.payto, None, "absent is the default");
    assert_eq!(ContactDetails::from_value(d.to_value()).unwrap(), d);

    d.payto = Some("5ApJU8bfJ2sb4eGHNSCcSjGH4SxMghLahdFoh3NKpkPYhJ".into());
    assert_eq!(ContactDetails::from_value(d.to_value()).unwrap(), d);
}

#[test]
fn a_published_address_is_bounded_and_never_empty() {
    let mut d = details();
    d.payto = Some("5".repeat(MAX_ADDRESS_CHARS + 1));
    assert!(ContactDetails::from_value(d.to_value()).is_err());
    d.payto = Some(String::new());
    assert!(ContactDetails::from_value(d.to_value()).is_err());
}
