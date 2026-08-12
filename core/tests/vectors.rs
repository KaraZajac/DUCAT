//! Execute the exported conformance vectors (§18.9) against this implementation.
//!
//! Two jobs. First, it proves the published artifact is actually runnable —
//! a vector set nobody executes is decoration. Second, it catches drift: the
//! in-tree tests and the exported vectors are generated from the same code but
//! checked independently, so a change that updates one without the other fails
//! here.
//!
//! This does **not** close O18. A vector set validated only by the
//! implementation that produced it encodes that implementation's bugs as the
//! specification. It closes when a second, independent client runs these files.

use ducat_core::cbor::decode;
use ducat_core::commit::{commit, Purpose};
use ducat_core::sig::{ObjectType, PublicKey, SignedBytes, Suite};
use ducat_core::state::{transition, Event, Role, SettleMode, State};
use serde_json::Value as J;
use std::time::Duration;

fn load(name: &str) -> J {
    let path = format!("../vectors/v1/{}.json", name);
    let raw = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{} — run `cargo run --example gen_vectors`: {}", path, e));
    serde_json::from_str(&raw).expect("vector file is not valid JSON")
}

fn cases(name: &str) -> Vec<J> {
    load(name)["cases"].as_array().cloned().unwrap()
}

fn unhex(s: &str) -> Vec<u8> {
    hex::decode(s).expect("bad hex in vector file")
}

fn expects_ok(c: &J) -> bool {
    c["expect"]["ok"].as_bool().unwrap_or(false)
}

#[test]
fn manifest_is_self_consistent() {
    let m = load("manifest");
    let total = m["total_cases"].as_u64().unwrap() as usize;
    let mut sum = 0;
    for name in ["codec", "signing", "state", "negotiate"] {
        let n = cases(name).len();
        assert_eq!(
            m["counts"][name].as_u64().unwrap() as usize,
            n,
            "manifest count for {} is stale",
            name
        );
        sum += n;
    }
    assert_eq!(total, sum, "manifest total is stale");
    assert!(sum >= 80, "vector set unexpectedly small: {}", sum);

    // The manifest must keep admitting what it does not cover. If this ever
    // disappears, someone has quietly claimed more coverage than exists.
    assert!(m["does_not_yet_cover"]["18.9(4) full per-profile transcripts"].is_string());
    assert!(m["does_not_yet_cover"]["O18 caveat"].is_string());
}

#[test]
fn codec_vectors_pass() {
    for c in cases("codec") {
        let name = c["name"].as_str().unwrap();
        let input = unhex(c["input_hex"].as_str().unwrap());
        let got = decode(&input);

        if expects_ok(&c) {
            let v = got.unwrap_or_else(|e| panic!("{}: expected ok, got {:?}", name, e));
            // Canonicality means a successful decode re-encodes byte-identically.
            let expect_re = unhex(c["expect"]["reencodes_to_hex"].as_str().unwrap());
            assert_eq!(v.encode(), expect_re, "{}: re-encoding differs", name);

            // Money cases carry the expected integer so a float client fails here.
            if let Some(amount) = c["expect"]["amount_at_key_1"].as_u64() {
                let got_amount = v.as_map().unwrap().get(&1).unwrap().as_uint().unwrap();
                assert_eq!(got_amount, amount, "{}: amount lost precision", name);
            }
        } else {
            assert!(got.is_err(), "{}: expected rejection, decoded fine", name);
        }
    }
}

#[test]
fn signing_vectors_pass() {
    for c in cases("signing") {
        let name = c["name"].as_str().unwrap();
        let suite = match c["suite"].as_u64().unwrap() {
            1 => Suite::Ed25519X25519,
            2 => Suite::P256,
            other => panic!("{}: unknown suite {}", name, other),
        };
        let pk_bytes = unhex(c["pubkey_hex"].as_str().unwrap());
        let pk = PublicKey::from_bytes(suite, &pk_bytes);

        // Key-encoding cases carry no object; the key parse *is* the assertion.
        let Some(obj_hex) = c["object_hex"].as_str() else {
            assert!(pk.is_err(), "{}: malformed key was accepted", name);
            continue;
        };

        let pk = pk.unwrap_or_else(|e| panic!("{}: key rejected: {:?}", name, e));
        let obj = SignedBytes::from_received(unhex(obj_hex))
            .unwrap_or_else(|e| panic!("{}: object not canonical: {:?}", name, e));
        let sig_v = unhex(c["sig_hex"].as_str().unwrap());
        let sig: [u8; 64] = sig_v.try_into().expect("signature must be 64 bytes");

        let verify_as = match c["verify_as"].as_str().unwrap() {
            "ACCEPT" => ObjectType::Accept,
            "TapPresent" => ObjectType::TapPresent,
            "RECEIPT" => ObjectType::Receipt,
            "bond_proof" => ObjectType::BondProof,
            "CONTACT_OFFER" => ObjectType::ContactOffer,
            other => panic!("{}: unknown object type {}", name, other),
        };

        let got = obj.verify(verify_as, &pk, &sig);
        if expects_ok(&c) {
            assert!(got.is_ok(), "{}: expected valid, got {:?}", name, got);
        } else {
            assert!(got.is_err(), "{}: signature should not have verified", name);
        }
    }
}

fn parse_state(s: &str) -> State {
    match s {
        "Idle" => State::Idle,
        "Offered" => State::Offered,
        "Quoted" => State::Quoted,
        "Accepted" => State::Accepted,
        "Funded" => State::Funded,
        "Provisional" => State::Provisional,
        "Delivered" => State::Delivered,
        "Closed" => State::Closed,
        "Settled" => State::Settled,
        "Claimed" => State::Claimed,
        "Aborted" => State::Aborted,
        "Cancelled" => State::Cancelled,
        "Disputed" => State::Disputed,
        other => panic!("unknown state {}", other),
    }
}

fn parse_event(v: &J) -> Event {
    // Timeouts arrive as {"Elapsed": secs}; everything else as a bare string.
    if let Some(secs) = v.get("Elapsed").and_then(|x| x.as_u64()) {
        return Event::Elapsed(Duration::from_secs(secs));
    }
    let s = v.as_str().expect("event must be a string or {Elapsed}");
    if let Some(rest) = s.strip_prefix("Elapsed(") {
        // Debug form "Elapsed(30s)" produced by the generator.
        let secs: u64 = rest
            .trim_end_matches(')')
            .trim_end_matches('s')
            .parse()
            .unwrap_or_else(|_| panic!("cannot parse duration from {}", s));
        return Event::Elapsed(Duration::from_secs(secs));
    }
    match s {
        "TapPresent" => Event::TapPresent,
        "FullOffer" => Event::FullOffer,
        "Accept" => Event::Accept,
        "Fund" => Event::Fund,
        "TxProof" => Event::TxProof,
        "Proof" => Event::Proof,
        "Receipt" => Event::Receipt,
        "Cancel" => Event::Cancel,
        "Dispute" => Event::Dispute,
        "Abort" => Event::Abort,
        "ContactOffer" => Event::ContactOffer,
        "ContactAccept" => Event::ContactAccept,
        "ConfirmationsReached" => Event::ConfirmationsReached,
        "CureWindowExpired" => Event::CureWindowExpired,
        other => panic!("unknown event {}", other),
    }
}

fn parse_mode(s: &str) -> SettleMode {
    match s {
        "Direct" => SettleMode::Direct,
        "Fast" => SettleMode::Fast,
        "Escrow" => SettleMode::Escrow,
        other => panic!("unknown mode {}", other),
    }
}

fn parse_role(s: &str) -> Role {
    match s {
        "Payer" => Role::Payer,
        "Payee" => Role::Payee,
        other => panic!("unknown role {}", other),
    }
}

#[test]
fn state_vectors_pass() {
    for c in cases("state") {
        let name = c["name"].as_str().unwrap();
        let mode = parse_mode(c["mode"].as_str().unwrap());
        let role = parse_role(c["role"].as_str().unwrap());
        let mut s = parse_state(c["from"].as_str().unwrap());

        // Multi-step sequences.
        if let Some(steps) = c["steps"].as_array() {
            for step in steps {
                let ev = parse_event(&step["event"]);
                let t = transition(s, role, mode, &ev)
                    .unwrap_or_else(|e| panic!("{}: {:?} rejected: {:?}", name, ev, e));
                assert_eq!(
                    format!("{:?}", t.next),
                    step["next"].as_str().unwrap(),
                    "{}: wrong next state after {:?}",
                    name,
                    ev
                );
                assert_eq!(
                    format!("{:?}", t.effect),
                    step["effect"].as_str().unwrap(),
                    "{}: wrong effect after {:?}",
                    name,
                    ev
                );
                s = t.next;
            }
            continue;
        }

        // Single transitions.
        let ev = parse_event(&c["event"]);
        let got = transition(s, role, mode, &ev);
        if c["expect"]["ok"].as_bool() == Some(false) {
            let err = got.expect_err(&format!("{}: expected rejection", name));
            assert_eq!(
                err.code as u8,
                c["expect"]["reject_code"].as_u64().unwrap() as u8,
                "{}: wrong reject code",
                name
            );
        } else {
            let t = got.unwrap_or_else(|e| panic!("{}: unexpected rejection {:?}", name, e));
            assert_eq!(
                format!("{:?}", t.next),
                c["expect"]["next"].as_str().unwrap(),
                "{}: wrong next state",
                name
            );
            assert_eq!(
                format!("{:?}", t.effect),
                c["expect"]["effect"].as_str().unwrap(),
                "{}: wrong effect",
                name
            );
        }
    }
}

#[test]
fn negotiation_downgrade_vector_passes() {
    let c = cases("negotiate")
        .into_iter()
        .find(|c| c["name"] == "downgrade_stripped_suite_fails_commitment")
        .expect("downgrade vector missing");

    let expected: [u8; 32] = unhex(c["offer_commit_hex"].as_str().unwrap())
        .try_into()
        .unwrap();
    let genuine = unhex(c["genuine_offer_hex"].as_str().unwrap());
    let stripped = unhex(c["stripped_offer_hex"].as_str().unwrap());

    assert_eq!(
        commit(Purpose::Offer, &genuine),
        expected,
        "genuine offer must reproduce the published commitment"
    );
    assert_ne!(
        commit(Purpose::Offer, &stripped),
        expected,
        "a stripped suite list must not satisfy the commitment"
    );
}

#[test]
fn commitment_domain_separation_vector_passes() {
    let c = cases("negotiate")
        .into_iter()
        .find(|c| c["name"] == "commitments_are_domain_separated_by_purpose")
        .expect("domain separation vector missing");

    let input = unhex(c["input_hex"].as_str().unwrap());
    let digests = &c["expect"]["digests_by_purpose"];

    for (purpose, label) in [
        (Purpose::Offer, "offer_commit"),
        (Purpose::Receipt, "receipt"),
        (Purpose::ChainLink, "chain"),
        (Purpose::MarketGenesis, "market_genesis"),
    ] {
        let published = unhex(digests[label].as_str().unwrap());
        assert_eq!(
            commit(purpose, &input).to_vec(),
            published,
            "{} digest differs from the published vector",
            label
        );
    }

    // And the four must be mutually distinct, which is the whole point.
    let all: Vec<&str> = ["offer_commit", "receipt", "chain", "market_genesis"]
        .iter()
        .map(|k| digests[*k].as_str().unwrap())
        .collect();
    for i in 0..all.len() {
        for j in (i + 1)..all.len() {
            assert_ne!(all[i], all[j], "two purposes produced the same digest");
        }
    }
}
