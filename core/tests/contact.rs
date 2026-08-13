//! Out-of-band contact cards and 1:1 messages (§16.9, §16.10).

use ducat_core::contact::*;

fn invite(expiry: u64, secret: &[u8; 32]) -> ContactInvite {
    ContactInvite {
        version: 1,
        suite: 1,
        persona: vec![0xAA; 32],
        rendezvous: vec![0xBB; 32],
        display_name: Some("kara".into()),
        claim_commit: claim_commitment(secret),
        expiry,
    }
}

fn claim(secret: [u8; 32]) -> ContactClaim {
    ContactClaim {
        version: 1,
        suite: 1,
        persona: vec![0xCC; 32],
        rendezvous: vec![0xDD; 32],
        display_name: Some("sam".into()),
        claim_secret: secret,
        timestamp: 1000,
    }
}

#[test]
fn round_trips_through_canonical_cbor() {
    let s = [7u8; 32];
    let i = invite(9999, &s);
    assert_eq!(ContactInvite::from_value(i.to_value()).unwrap(), i);
    let c = claim(s);
    assert_eq!(ContactClaim::from_value(c.to_value()).unwrap(), c);
}

#[test]
fn a_card_without_a_name_is_still_a_card() {
    let mut i = invite(9999, &[1u8; 32]);
    i.display_name = None;
    assert_eq!(ContactInvite::from_value(i.to_value()).unwrap(), i);
}

#[test]
fn good_claim_is_accepted() {
    let s = [7u8; 32];
    check_claim(&invite(9999, &s), &claim(s), 1000, false).unwrap();
}

/// The screenshot case: someone else saw the card in a group chat.
#[test]
fn a_second_claim_is_refused() {
    let s = [7u8; 32];
    let err = check_claim(&invite(9999, &s), &claim(s), 1000, true).unwrap_err();
    assert!(format!("{err:?}").contains("Replay"), "{err:?}");
}

/// Knowing the persona is not knowing the invitation. Personas are public.
#[test]
fn knowing_the_persona_is_not_enough() {
    let s = [7u8; 32];
    let err = check_claim(&invite(9999, &s), &claim([8u8; 32]), 1000, false).unwrap_err();
    assert!(format!("{err:?}").contains("BadSig"), "{err:?}");
}

#[test]
fn an_expired_card_is_refused() {
    let s = [7u8; 32];
    let err = check_claim(&invite(500, &s), &claim(s), 501, false).unwrap_err();
    assert!(format!("{err:?}").contains("Expired"), "{err:?}");
}

/// Otherwise an attacker who intercepts a card can burn it by claiming it back
/// to its issuer, and the intended recipient silently gets nothing.
#[test]
fn a_card_cannot_be_claimed_by_its_own_issuer() {
    let s = [7u8; 32];
    let i = invite(9999, &s);
    let mut c = claim(s);
    c.persona = i.persona.clone();
    let err = check_claim(&i, &c, 1000, false).unwrap_err();
    assert!(format!("{err:?}").contains("PolicyRefused"), "{err:?}");
}

#[test]
fn a_display_name_cannot_smuggle_a_paragraph() {
    let mut i = invite(9999, &[1u8; 32]);
    i.display_name = Some("x".repeat(MAX_DISPLAY_NAME_CHARS + 1));
    assert!(ContactInvite::from_value(i.to_value()).is_err());
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

/// A message whose sequence fits but whose link does not means something was
/// removed and replaced. That is the case worth catching.
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

/// "" and "key absent" would otherwise be two spellings of the same nothing.
#[test]
fn an_empty_body_is_refused_rather_than_being_a_second_spelling_of_none() {
    let m = msg(0, [0u8; 32], "");
    assert!(Message::from_value(m.to_value()).is_err());
}

#[test]
fn an_empty_display_name_is_refused_too() {
    let mut i = invite(9999, &[1u8; 32]);
    i.display_name = Some(String::new());
    assert!(ContactInvite::from_value(i.to_value()).is_err());
}

#[test]
fn messages_round_trip() {
    let m = msg(3, [4u8; 32], "here's the 20 back");
    assert_eq!(Message::from_value(m.to_value()).unwrap(), m);
}
