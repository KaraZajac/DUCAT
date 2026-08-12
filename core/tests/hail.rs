//! Remote hail (§5.2), rewritten wholesale in draft 0.19 and never executed.
//!
//! The section's entire safety argument is that matching runs over DHT reads,
//! which import nothing, and that the one route import happens after mutual
//! selection. These tests check the property that argument rests on.

use ducat_core::cbor::{decode, Value};
use ducat_core::reject::RejectCode;
use ducat_core::wire::*;

const T0: u64 = 1_800_000_000;

fn hail() -> Hail {
    Hail {
        version: 1,
        suite: 1,
        profile: 3, // ride/1
        geocell: b"u4pru".to_vec(), // 5 chars ~ district
        nonce: [0x5A; 16],
        ephemeral_pk: vec![0x11; 32],
        expiry: T0 + 300,
    }
}

fn reply(h: &Hail) -> HailReply {
    HailReply {
        version: 1,
        suite: 1,
        nonce_echo: h.nonce,
        session_pk: vec![0x22; 32],
        quote_pxmr: 8_000_000_000,
    }
}

/// The property the whole inversion depends on: neither object can carry a
/// route. §5.2 forbids it in prose; here it is unrepresentable, which is
/// stronger — a rule can go unimplemented, a missing field cannot be populated.
///
/// If a provider could attach a route to a reply, a consumer that helpfully
/// imported it would be deanonymised by Veilid #395 — the exact harvesting the
/// section was rewritten to eliminate.
#[test]
fn neither_hail_nor_reply_can_carry_a_route() {
    let h = hail();
    let encoded = h.to_value().encode();
    let decoded = decode(&encoded).unwrap();
    let map = decoded.as_map().unwrap();

    // `ROUTE` is field 11, used by TapPresent. It must not appear here.
    assert!(!map.contains_key(&11), "a hail must not carry a route");

    let r = reply(&h);
    let rmap = decode(&r.to_value().encode()).unwrap();
    assert!(!rmap.as_map().unwrap().contains_key(&11), "a reply must not carry a route");

    // And an object that smuggles one in is refused outright, rather than
    // being parsed with the extra field quietly ignored (§18.8).
    let mut v = h.to_value();
    if let Value::Map(m) = &mut v {
        m.insert(11, Value::Bytes(vec![0xFF; 32]));
    }
    assert_eq!(Hail::from_value(v).unwrap_err().code, RejectCode::UnknownField);
}

/// §5.2.3's disclosure ladder starts at a district. An over-precise cell turns
/// the first rung into a position fix, so precision is bounded by the parser
/// rather than left to a client's discretion.
#[test]
fn an_over_precise_geocell_is_refused() {
    let mut h = hail();
    h.geocell = b"u4pruydqqvj".to_vec(); // building-level precision
    assert_eq!(
        Hail::from_value(h.to_value()).unwrap_err().code,
        RejectCode::PolicyRefused
    );

    // District-level is fine.
    let ok = hail();
    assert!(Hail::from_value(ok.to_value()).is_ok());
}

#[test]
fn a_reply_must_echo_the_hail_it_answers() {
    let h = hail();
    assert!(check_hail_reply(&h, &reply(&h), T0).is_ok());

    // A provider's stale reply replayed against a fresh hail: the consumer
    // would otherwise choose from quotes nobody currently stands behind.
    let mut stale = reply(&h);
    stale.nonce_echo = [0xFF; 16];
    assert_eq!(
        check_hail_reply(&h, &stale, T0).unwrap_err().code,
        RejectCode::Replay
    );
}

#[test]
fn an_expired_hail_accepts_no_replies() {
    let h = hail();
    assert!(check_hail_reply(&h, &reply(&h), h.expiry - 1).is_ok());
    assert_eq!(
        check_hail_reply(&h, &reply(&h), h.expiry).unwrap_err().code,
        RejectCode::Expired
    );
}

/// Two hails from one consumer must be unlinkable to a watcher, or the
/// ephemeral key is decoration. Nothing in a hail identifies its author.
#[test]
fn two_hails_share_no_identifying_field() {
    let mut a = hail();
    let mut b = hail();
    a.nonce = [0x01; 16];
    b.nonce = [0x02; 16];
    a.ephemeral_pk = vec![0xAA; 32];
    b.ephemeral_pk = vec![0xBB; 32];

    let am = decode(&a.to_value().encode()).unwrap();
    let bm = decode(&b.to_value().encode()).unwrap();
    let (am, bm) = (am.as_map().unwrap(), bm.as_map().unwrap());

    // The only fields that match are the ones every hail in a market shares:
    // version, suite, profile, cell, expiry. Nothing persona-scoped.
    for k in [f::NONCE, f::HAIL_EPHEMERAL_PK] {
        assert_ne!(am.get(&k), bm.get(&k), "field {} links two hails", k);
    }
}

#[test]
fn hail_objects_round_trip() {
    let h = hail();
    let enc = h.to_value().encode();
    assert_eq!(Hail::from_value(decode(&enc).unwrap()).unwrap(), h);
    assert_eq!(decode(&enc).unwrap().encode(), enc);

    let r = reply(&h);
    let renc = r.to_value().encode();
    assert_eq!(HailReply::from_value(decode(&renc).unwrap()).unwrap(), r);
    assert_eq!(decode(&renc).unwrap().encode(), renc);
}
