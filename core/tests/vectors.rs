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
use ducat_core::state::{deadline, transition, Event, Role, SettleMode, State};
use ducat_core::wire::*;
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
    // Derived from the manifest rather than a list written here. A hardcoded
    // list silently omits any vector file added later, which is how contact.json
    // was briefly uncounted — the exact staleness this test exists to catch.
    let names: Vec<String> = m["counts"].as_object().unwrap().keys().cloned().collect();
    assert!(names.len() >= 9, "manifest lists suspiciously few files: {names:?}");
    for name in &names {
        let n = cases(name).len();
        assert_eq!(
            m["counts"][name.as_str()].as_u64().unwrap() as usize,
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
    //
    // Keyed on the conformance open problem by number, which is O21 — this
    // asserted "O18" until 0.42 and passed anyway, because the manifest text was
    // wrong in the same direction. A self-consistency test that only compares
    // the artifact against itself agrees with its own mistakes.
    assert!(
        m["does_not_yet_cover"]["O21 caveat"].is_string(),
        "the manifest stopped admitting that one implementation cannot validate its own vectors"
    );
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
    // One shape, per `vectors/v1/schema.json#/$defs/event`:
    //   {"name": "...", "from": "Payer"?, "elapsed_s": N?}
    // Until 0.46 this had to accept five spellings of the same concept, which is
    // what a second implementer met and had to reverse-engineer (§18.11).
    let name = v["name"].as_str().expect("event needs a name");
    let from = match v.get("from").and_then(|x| x.as_str()) {
        Some("Payee") => Role::Payee,
        _ => Role::Payer,
    };
    match name {
        "Elapsed" => Event::Elapsed(Duration::from_secs(
            v["elapsed_s"].as_u64().expect("Elapsed needs elapsed_s"),
        )),
        "TapPresent" => Event::TapPresent,
        "FullOffer" => Event::FullOffer,
        "Accept" => Event::Accept { from },
        "Abort" => Event::Abort { from },
        "Fund" => Event::Fund,
        "TxId" => Event::TxId,
        "Proof" => Event::Proof,
        "Receipt" => Event::Receipt,
        "Cancel" => Event::Cancel,
        "Dispute" => Event::Dispute,
        "ContactOffer" => Event::ContactOffer,
        "ContactAccept" => Event::ContactAccept,
        "ConfirmationsReached" => Event::ConfirmationsReached,
        "CureWindowExpired" => Event::CureWindowExpired,
        "MeterStart" => Event::MeterStart,
        "MeterStop" => Event::MeterStop,
        "MeterExpired" => Event::MeterExpired,
        "DeliveryWindowExpired" => Event::DeliveryWindowExpired,
        other => panic!("unknown event {}", other),
    }
}

fn parse_mode(s: &str) -> SettleMode {
    match s {
        "Direct" => SettleMode::Direct,
        "Fast" => SettleMode::Fast,
        "Escrow" => SettleMode::Escrow,
        other => panic!("unknown settlement mode {}", other),
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

        if let Some(d) = c.get("deadline_s") {
            let got = deadline(s, mode).map(|x| x.as_secs());
            assert_eq!(
                got, d.as_u64(),
                "{}: deadline for {:?}/{:?}", name, s, mode
            );
        }

        for step in c["steps"].as_array().expect("steps") {
            let ev = parse_event(&step["event"]);
            let expect = &step["expect"];
            let got = transition(s, role, mode, &ev);
            if expect["ok"].as_bool() == Some(false) {
                let err = got.unwrap_err();
                assert_eq!(
                    err.code as u8,
                    expect["reject_code"].as_u64().unwrap() as u8,
                    "{}: wrong reject code for {:?}", name, ev
                );
                break;
            }
            let t = got.unwrap_or_else(|e| panic!("{}: {:?} rejected: {:?}", name, ev, e));
            assert_eq!(
                format!("{:?}", t.next),
                expect["next"].as_str().unwrap(),
                "{}: wrong next state after {:?}", name, ev
            );
            assert_eq!(
                format!("{:?}", t.effect),
                expect["effect"].as_str().unwrap(),
                "{}: wrong effect after {:?}", name, ev
            );
            s = t.next;
        }
    }
}

#[test]
fn negotiation_downgrade_vector_passes() {
    // Moved out of negotiate.json at 0.46: it is a commitment case, not a
    // negotiation. §18.11 recorded that a second implementer met it there.
    let c = cases("commit")
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
    let c = cases("commit")
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

/// §18.9(4) — replay each published transcript through the verifier.
#[test]
fn transcript_vectors_pass() {
    for c in cases("transcript") {
        let name = c["name"].as_str().unwrap();

        // The tampered case carries only the commitment and the swapped offer.
        let Some(tap_hex) = c["tap_present_hex"].as_str() else {
            let expected: [u8; 32] = unhex(c["tap_offer_commit_hex"].as_str().unwrap())
                .try_into().unwrap();
            let delivered = FullOffer::from_value(
                decode(&unhex(c["delivered_offer_hex"].as_str().unwrap())).unwrap()
            ).unwrap();
            assert_ne!(delivered.commitment(), expected, "{}: swap should not match", name);
            continue;
        };

        let tap = TapPresent::from_value(decode(&unhex(tap_hex)).unwrap()).unwrap();
        let offer = FullOffer::from_value(
            decode(&unhex(c["full_offer_hex"].as_str().unwrap())).unwrap()).unwrap();
        let accept_bytes = unhex(c["accept_hex"].as_str().unwrap());
        let accept = Accept::from_value(decode(&accept_bytes).unwrap()).unwrap();
        let receipt = Receipt::from_value(
            decode(&unhex(c["receipt_hex"].as_str().unwrap())).unwrap()).unwrap();

        verify_transcript(&tap, &offer, &accept, &accept_bytes, &receipt)
            .unwrap_or_else(|e| panic!("{}: {:?}", name, e));

        // The published intermediate digests must match ours, or two clients
        // agree the transcript is valid while disagreeing on what it hashes to.
        assert_eq!(
            hex::encode(offer.commitment()),
            c["expect"]["offer_commit_hex"].as_str().unwrap(),
            "{}: offer commitment differs", name
        );
        assert_eq!(
            accept.amount_final,
            c["expect"]["amount_pxmr"].as_u64().unwrap(),
            "{}: amount differs", name
        );
    }
}

/// §4.3.2 — the backup format, replayed from the published artifact.
///
/// The point is not to re-test the crypto, which `tests/backup.rs` covers. It is
/// to prove the *file* in `vectors/v1/` is the file this implementation actually
/// produces and consumes, so a second client has something real to disagree
/// with. A vector nobody executes is documentation with a `.json` extension.
#[test]
fn backup_vectors_pass() {
    use ducat_core::backup::import;

    let mut saw_canonical = false;
    for c in cases("backup") {
        let name = c["name"].as_str().unwrap();
        let blob = unhex(c["blob_hex"].as_str().unwrap());
        let pass = c["passphrase_utf8"].as_str().unwrap().as_bytes();
        let got = import(&blob, pass);

        if !expects_ok(&c) {
            let want = c["expect"]["reject_code"].as_u64().unwrap() as u8;
            let err = got.unwrap_err();
            assert_eq!(
                err.code as u8, want,
                "{name}: expected {} got {:?}",
                c["expect"]["reject_name"], err.code
            );
            continue;
        }

        let b = got.unwrap_or_else(|e| panic!("{name}: {e:?}"));
        let d = &c["expect"]["decoded"];
        assert_eq!(b.persona_suite as u64, d["persona_suite"].as_u64().unwrap(), "{name}");
        assert_eq!(hexs(&b.persona_secret), d["persona_secret_hex"].as_str().unwrap(), "{name}");
        assert_eq!(b.monero_seed, d["monero_seed"].as_str().unwrap(), "{name}");
        assert_eq!(
            b.monero_restore_height,
            d["monero_restore_height"].as_u64().unwrap(),
            "{name}: the field whose absence costs 106 hours and whose overshoot costs everything"
        );
        let listed = |k: &str| -> Vec<String> {
            d[k].as_array().unwrap().iter().map(|v| v.as_str().unwrap().to_string()).collect()
        };
        assert_eq!(b.rendezvous.iter().map(|r| hexs(r)).collect::<Vec<_>>(), listed("rendezvous_hex"), "{name}");
        assert_eq!(
            b.attestation_records.iter().map(|r| hexs(r)).collect::<Vec<_>>(),
            listed("attestation_records_hex"),
            "{name}"
        );
        assert_eq!(b.mandates.iter().map(|r| hexs(r)).collect::<Vec<_>>(), listed("mandates_hex"), "{name}");
        let v = &d["verification"];
        assert_eq!(b.verification.device_unlock_at, v["device_unlock_at"].as_u64().unwrap(), "{name}");
        assert_eq!(b.verification.app_secret_at, v["app_secret_at"].as_u64().unwrap(), "{name}");
        assert_eq!(b.verification.cumulative_at, v["cumulative_at"].as_u64().unwrap(), "{name}");
        assert_eq!(b.created, d["created"].as_u64().unwrap(), "{name}");
        saw_canonical = true;
    }
    assert!(saw_canonical, "no positive backup vector ran — a suite of only rejections proves nothing");
}

fn hexs(b: &[u8]) -> String {
    b.iter().map(|x| format!("{:02x}", x)).collect()
}

/// Every published case carries a `kind`, and every kind is one a consumer has
/// been told about.
///
/// `vectors/v1/schema.json` is the authority and `conformance/validate_vectors.py`
/// enforces it, but that runs outside `cargo test` and can be skipped. This is
/// the cheap half that cannot be: if the generator invents a kind, or drops the
/// discriminator, or reuses a case name, a third-party client hits it before we
/// do — and their first experience of DUCAT is a file they cannot dispatch on.
#[test]
fn every_case_declares_a_known_kind_and_a_unique_name() {
    const KINDS: &[&str] = &[
        "codec.decode", "signing.verify", "signing.pubkey", "negotiate.select",
        "commit.purposes", "commit.substitution", "state.sequence",
        "transcript.replay", "transcript.substitution", "backup.import",
        "object.roundtrip", "escrow.ceremony", "escrow.ready", "escrow.release",
        "bond.check", "slash.check",
        "contact.card", "contact.details", "log.head", "log.ring", "stand.shard", "stand.epoch", "message.chain",
        "message.payment", "hail.notice", "rental.listing", "board.sealed",
        "board.beacon_window", "board.beacon_verdict", "position.frame",
    ];
    let dir = std::path::Path::new("../vectors/v1");
    let mut seen: std::collections::HashMap<String, String> = Default::default();
    let mut count = 0;
    for entry in std::fs::read_dir(dir).expect("vector dir") {
        let path = entry.expect("dir entry").path();
        let file = path.file_name().unwrap().to_string_lossy().to_string();
        if path.extension().and_then(|e| e.to_str()) != Some("json")
            || file == "manifest.json"
            || file == "schema.json"
        {
            continue;
        }
        let doc: J = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        for c in doc["cases"].as_array().expect("cases array") {
            count += 1;
            let kind = c["kind"].as_str().unwrap_or_else(|| {
                panic!("{}: case {} has no kind", file, c["name"])
            });
            assert!(KINDS.contains(&kind), "{}: unknown kind {}", file, kind);
            let name = c["name"].as_str().expect("case name").to_string();
            assert!(
                c["why"].as_str().map(|w| !w.is_empty()).unwrap_or(false),
                "{}: case {} has no `why` — a case nobody can explain is a case \
                 nobody can safely change",
                file,
                name
            );
            if let Some(prev) = seen.insert(name.clone(), file.clone()) {
                panic!("duplicate case name {} in {} and {}", name, file, prev);
            }
        }
    }
    assert!(count >= 100, "vector set unexpectedly small: {}", count);
}


/// §8.2 / §17.4 / §17.5 object encodings, replayed from the published artifact.
#[test]
fn object_vectors_pass() {
    for c in cases("object") {
        let name = c["name"].as_str().unwrap();
        let raw = unhex(c["object_hex"].as_str().unwrap());
        let got = decode(&raw);
        if c["expect"]["ok"].as_bool() == Some(false) {
            // The type check is the whole point of the negative case.
            let v = got.expect("case should decode as CBOR");
            assert!(
                ducat_core::escrow::EscrowSetup::from_value(v).is_err(),
                "{}: an object declaring another type must not decode",
                name
            );
            continue;
        }
        let v = got.unwrap_or_else(|e| panic!("{}: {:?}", name, e));
        assert_eq!(
            hexs(&v.encode()),
            c["expect"]["reencodes_to_hex"].as_str().unwrap(),
            "{}: re-encoding is not stable",
            name
        );
    }
}


/// §8.2 / §17.4 / §17.5 contract logic, replayed through `core` itself.
///
/// The published cases are executed twice: here against the library, and in
/// `conformance/ducat_check.py` against an implementation written from the spec.
/// Encoding agreement was never the hard part — two clients can serialise
/// identically and still *decide* differently, and these are the decisions money
/// depends on.
/// §16.9 / §16.12 — record-based cards, inbox details, the log ring, and chains.
#[test]
fn contact_vectors_pass() {
    use ducat_core::contact::*;

    for c in cases("contact") {
        let name = c["name"].as_str().unwrap();
        // Lazy, like `want_code` beside it. Not every case in this file is a
        // yes/no: `board.beacon_verdict` has three answers, and reading `ok`
        // eagerly made adding a case with a richer expectation panic in the
        // *other* cases' setup, which is a confusing way to find out.
        let want_ok = || c["expect"]["ok"].as_bool().unwrap();
        let want_code = || c["expect"]["reject"].as_str().unwrap().to_string();
        match c["kind"].as_str().unwrap() {
            "contact.card" => {
                let got = decode(&unhex(c["card_hex"].as_str().unwrap()))
                    .map_err(|e| e.into())
                    .and_then(ContactCard::from_value);
                assert_eq!(got.is_ok(), want_ok(), "{name}: {got:?}");
                match got {
                    Ok(card) => assert_eq!(
                        hexs(&card.to_value().encode()),
                        c["expect"]["reencodes_to_hex"].as_str().unwrap(),
                        "{name}: did not re-encode byte-identically"
                    ),
                    Err(e) => assert_eq!(
                        format!("{:?}", e.code).to_uppercase(), want_code(),
                        "{name}: wrong reject code"
                    ),
                }
            }
            "contact.details" => {
                let got = decode(&unhex(c["details_hex"].as_str().unwrap()))
                    .map_err(|e| e.into())
                    .and_then(ContactDetails::from_value);
                assert_eq!(got.is_ok(), want_ok(), "{name}: {got:?}");
                match got {
                    Ok(d) => assert_eq!(
                        hexs(&d.to_value().encode()),
                        c["expect"]["reencodes_to_hex"].as_str().unwrap(), "{name}"
                    ),
                    Err(e) => assert_eq!(
                        format!("{:?}", e.code).to_uppercase(), want_code(), "{name}"
                    ),
                }
            }
            "log.head" => {
                let got = LogHead::from_value(
                    decode(&unhex(c["head_hex"].as_str().unwrap())).unwrap());
                let ok = c["expect"]["ok"].as_bool().unwrap_or(true);
                match got {
                    Ok(h) => {
                        assert!(ok, "{name}: decoded a head the vector refuses");
                        assert_eq!(
                            hexs(&h.to_value().encode()),
                            c["expect"]["reencodes_to_hex"].as_str().unwrap(), "{name}"
                        );
                    }
                    Err(e) => {
                        assert!(!ok, "{name}: refused a head the vector accepts: {e:?}");
                        assert_eq!(
                            format!("{:?}", e.code).to_uppercase(),
                            c["expect"]["reject"].as_str().unwrap(), "{name}"
                        );
                    }
                }
            }
            "hail.notice" => {
                let got = HailNotice::from_value(
                    decode(&unhex(c["notice_hex"].as_str().unwrap())).unwrap());
                let ok = c["expect"]["ok"].as_bool().unwrap_or(true);
                match got {
                    Ok(n) => {
                        assert!(ok, "{name}: decoded a notice the vector refuses");
                        assert_eq!(
                            hexs(&n.to_value().encode()),
                            c["expect"]["reencodes_to_hex"].as_str().unwrap(), "{name}"
                        );
                    }
                    Err(e) => {
                        assert!(!ok, "{name}: refused a notice the vector accepts: {e:?}");
                        assert_eq!(
                            format!("{:?}", e.code).to_uppercase(),
                            c["expect"]["reject"].as_str().unwrap(), "{name}"
                        );
                    }
                }
            }
            // §16.18 + board.rs: the sealed form, which is what is actually on
            // a board. The notice inside is checked by the cases above; what
            // this pins is the seal — the poster key, a signature over the
            // notice *and the slot*, and a nonce doing board::POW_BITS of work.
            "board.sealed" => {
                let bytes = unhex(c["sealed_hex"].as_str().unwrap());
                let board = c["board"].as_str().unwrap();
                let subkey = c["subkey"].as_u64().unwrap() as u32;
                let got = ducat_core::board::open(
                    decode(&bytes).unwrap(),
                    ducat_core::board::RENTAL,
                    board,
                    subkey,
                );
                let ok = c["expect"]["ok"].as_bool().unwrap_or(true);
                match got {
                    Ok(o) => {
                        assert!(ok, "{name}: opened a notice the vector refuses");
                        assert_eq!(
                            hexs(&o.poster),
                            c["expect"]["poster_hex"].as_str().unwrap(), "{name}"
                        );
                        // The beacon comes back out, and the vector pins it:
                        // a reader that dropped it on the floor would have no
                        // way to judge freshness and would look identical here
                        // to one that carried it.
                        assert_eq!(
                            o.beacon.height,
                            c["expect"]["beacon_height"].as_u64().unwrap(), "{name}"
                        );
                        assert_eq!(
                            hexs(&o.beacon.hash),
                            c["expect"]["beacon_hash"].as_str().unwrap(), "{name}"
                        );
                        // And what is left is a listing this implementation
                        // reads, so the seal and the notice really do compose.
                        RentalNotice::from_value(o.notice)
                            .unwrap_or_else(|e| panic!("{name}: inner listing refused: {e:?}"));
                    }
                    Err(e) => {
                        assert!(!ok, "{name}: refused a notice the vector accepts: {e:?}");
                        assert_eq!(
                            format!("{:?}", e.code).to_uppercase(),
                            c["expect"]["reject"].as_str().unwrap(), "{name}"
                        );
                    }
                }
            }
            // §16.18.1's freshness window. Not `open`'s job — judging a
            // beacon needs a chain and `open` is a pure function of its
            // arguments — so what this pins is the *rule a caller applies*:
            // a tip of zero means this device has no chain view and skips the
            // test, and anything else is the range test. Both edges, both
            // sides, because a bound checked from one side is a bound the
            // next implementation gets to pick.
            "board.beacon_window" => {
                let beacon = ducat_core::board::Beacon {
                    height: c["beacon_height"].as_u64().unwrap(),
                    hash: [0u8; 32],
                };
                let tip = c["tip_height"].as_u64().unwrap();
                let got = tip == 0 || ducat_core::board::beacon_in_window(&beacon, tip);
                assert_eq!(got, c["expect"]["ok"].as_bool().unwrap(), "{name}");
            }
            // §16.18.1's whole freshness rule, three answers and all. The
            // window above pins the range; this pins what a reader may *do*
            // with a notice once it has looked — and in particular that "I
            // cannot check that yet" is its own answer rather than a yes.
            "board.beacon_verdict" => {
                let mut hash = [0u8; 32];
                hash.copy_from_slice(&unhex(c["beacon_hash"].as_str().unwrap()));
                let beacon = ducat_core::board::Beacon {
                    height: c["verdict_height"].as_u64().unwrap(),
                    hash,
                };
                let known = c["known_hash"].as_str().map(|h| {
                    let mut k = [0u8; 32];
                    k.copy_from_slice(&unhex(h));
                    k
                });
                let got = ducat_core::board::beacon_verdict(
                    &beacon,
                    c["verdict_tip"].as_u64().unwrap(),
                    known.as_ref(),
                );
                let want = match c["expect"]["verdict"].as_str().unwrap() {
                    "show" => ducat_core::board::BeaconVerdict::Show,
                    "hold" => ducat_core::board::BeaconVerdict::Hold,
                    "refuse" => ducat_core::board::BeaconVerdict::Refuse,
                    other => panic!("{name}: unknown verdict {other}"),
                };
                assert_eq!(got, want, "{name}");
            }
            // §15.12 — one live-position frame, the encrypted value written
            // to the stream's record. The runner opens the given bytes with
            // the vector's stream key and record, and checks the fields or the
            // refusal — the fixed layout and the record-as-AAD binding are what
            // this pins across implementations.
            "position.frame" => {
                let mut sk = [0u8; 32];
                sk.copy_from_slice(&unhex(c["stream_key_hex"].as_str().unwrap()));
                let rec = c["record_key"].as_str().unwrap();
                let got = ducat_core::position::open(&sk, rec, &unhex(c["frame_sealed_hex"].as_str().unwrap()));
                let ok = c["expect"]["ok"].as_bool().unwrap();
                match got {
                    Ok(fr) => {
                        assert!(ok, "{name}: opened a frame the vector refuses");
                        assert_eq!(fr.counter, c["expect"]["counter"].as_u64().unwrap(), "{name} counter");
                        assert_eq!(fr.lat_e7, c["expect"]["lat_e7"].as_i64().unwrap(), "{name} lat");
                        assert_eq!(fr.lon_e7, c["expect"]["lon_e7"].as_i64().unwrap(), "{name} lon");
                        assert_eq!(fr.captured, c["expect"]["captured"].as_u64().unwrap(), "{name} captured");
                        // Heading is present in some cases and absent in others.
                        match c["expect"]["heading"].as_u64() {
                            Some(h) => assert_eq!(fr.heading, Some(h as u16), "{name} heading"),
                            None => assert_eq!(fr.heading, None, "{name} heading absent"),
                        }
                    }
                    Err(e) => {
                        assert!(!ok, "{name}: refused a frame the vector accepts: {e:?}");
                        assert_eq!(
                            format!("{:?}", e.code).to_uppercase(),
                            c["expect"]["reject"].as_str().unwrap(), "{name}"
                        );
                    }
                }
            }
            "rental.listing" => {
                let got = RentalNotice::from_value(
                    decode(&unhex(c["listing_hex"].as_str().unwrap())).unwrap());
                let ok = c["expect"]["ok"].as_bool().unwrap_or(true);
                match got {
                    Ok(n) => {
                        assert!(ok, "{name}: decoded a listing the vector refuses");
                        assert_eq!(
                            hexs(&n.to_value().encode()),
                            c["expect"]["reencodes_to_hex"].as_str().unwrap(), "{name}"
                        );
                    }
                    Err(e) => {
                        assert!(!ok, "{name}: refused a listing the vector accepts: {e:?}");
                        assert_eq!(
                            format!("{:?}", e.code).to_uppercase(),
                            c["expect"]["reject"].as_str().unwrap(), "{name}"
                        );
                    }
                }
            }
            "stand.epoch" => {
                let base = c["base"].as_str().unwrap();
                let epoch = c["epoch"].as_u64().unwrap();
                match ducat_core::geo::stand_epoch_name(base, epoch) {
                    Ok(got) => {
                        assert!(c["expect"]["ok"].as_bool().unwrap(), "{name}: should reject");
                        assert_eq!(got, c["expect"]["board"].as_str().unwrap(), "{name}: wrong board name");
                    }
                    Err(e) => {
                        assert!(!c["expect"]["ok"].as_bool().unwrap(), "{name}: should accept: {:?}", e.code);
                        assert_eq!(
                            format!("{:?}", e.code).to_uppercase(),
                            c["expect"]["reject"].as_str().unwrap(), "{name}"
                        );
                    }
                }
            }
            "stand.shard" => {
                let base = c["base"].as_str().unwrap();
                let shard = c["shard"].as_u64().unwrap() as u32;
                match ducat_core::geo::stand_shard_name(base, shard) {
                    Ok(got) => {
                        assert!(c["expect"]["ok"].as_bool().unwrap(), "{name}: should reject");
                        assert_eq!(got, c["expect"]["board"].as_str().unwrap(), "{name}: wrong board name");
                    }
                    Err(e) => {
                        assert!(!c["expect"]["ok"].as_bool().unwrap(), "{name}: should accept: {:?}", e.code);
                        assert_eq!(
                            format!("{:?}", e.code).to_uppercase(),
                            c["expect"]["reject"].as_str().unwrap(), "{name}"
                        );
                    }
                }
            }
            "log.ring" => {
                let seq = c["seq"].as_u64().unwrap();
                let count = c["subkey_count"].as_u64().unwrap() as u32;
                assert_eq!(
                    subkey_for(seq, count) as u64,
                    c["expect"]["subkey"].as_u64().unwrap(),
                    "{name}: wrong subkey"
                );
                // Everything from `oldest_readable` up to seq must still be
                // fetchable, and the one before it must not.
                let oldest = c["expect"]["oldest_readable"].as_u64().unwrap();
                assert!(still_in_ring(oldest, seq + 1, count), "{name}: oldest should be readable");
                if oldest > 0 {
                    assert!(!still_in_ring(oldest - 1, seq + 1, count),
                            "{name}: the ring should have passed {}", oldest - 1);
                }
            }
            "message.payment" => {
                let got = decode(&unhex(c["payment_hex"].as_str().unwrap()))
                    .map_err(|e| e.into())
                    .and_then(Message::from_value);
                assert_eq!(got.is_ok(), want_ok(), "{name}: {got:?}");
                match got {
                    Ok(m) => assert_eq!(
                        hexs(&m.to_value().encode()),
                        c["expect"]["reencodes_to_hex"].as_str().unwrap(), "{name}"
                    ),
                    Err(e) => assert_eq!(
                        format!("{:?}", e.code).to_uppercase(), want_code(), "{name}"
                    ),
                }
            }
            "message.chain" => {
                let msgs: Vec<Message> = c["messages_hex"].as_array().unwrap().iter()
                    .map(|h| Message::from_value(
                        decode(&unhex(h.as_str().unwrap())).unwrap()).unwrap())
                    .collect();
                let mut prev: Option<Message> = None;
                let mut failed_at = None;
                for (i, m) in msgs.iter().enumerate() {
                    match check_message(m, i as u64, prev.as_ref()) {
                        Ok(()) => prev = Some(m.clone()),
                        Err(e) => { failed_at = Some((i, format!("{:?}", e.code).to_uppercase())); break }
                    }
                }
                match (want_ok(), failed_at) {
                    (true, None) => {}
                    (true, Some((i, code))) => panic!("{name}: unexpected reject {code} at {i}"),
                    (false, None) => panic!("{name}: expected a reject, chain was accepted"),
                    (false, Some((i, code))) => {
                        assert_eq!(i as u64, c["expect"]["fails_at_index"].as_u64().unwrap(),
                                   "{name}: failed at the wrong message");
                        assert_eq!(code, want_code(), "{name}: wrong reject code");
                    }
                }
            }
            other => panic!("{name}: unhandled contact kind {other}"),
        }
    }
}

#[test]
fn contract_vectors_pass() {
    use ducat_core::escrow::*;

    for c in cases("contract") {
        let name = c["name"].as_str().unwrap();
        match c["kind"].as_str().unwrap() {
            "escrow.ceremony" => {
                let eid: [u8; 32] = unhex(c["escrow_id_hex"].as_str().unwrap()).try_into().unwrap();
                let mut t = RoundTracker::new(eid, c["rounds_required"].as_u64().unwrap());
                for step in c["steps"].as_array().unwrap() {
                    let s = EscrowSetup {
                        version: 1,
                        suite: 1,
                        escrow_id: eid,
                        round: step["round"].as_u64().unwrap(),
                        info: unhex(step["info_hex"].as_str().unwrap_or("ab")),
                        from_index: step["from_index"].as_u64().unwrap() as u8,
                        timestamp: 1_800_000_000,
                    };
                    let got = t.accept(&s);
                    let want_ok = step["expect"]["ok"].as_bool().unwrap_or(true);
                    assert_eq!(got.is_ok(), want_ok, "{name}: round {}", s.round);
                    if let Err(e) = got {
                        assert_eq!(
                            e.code as u8,
                            step["expect"]["reject_code"].as_u64().unwrap() as u8,
                            "{name}: wrong reject code"
                        );
                        break;
                    }
                }
            }
            "escrow.ready" => {
                let eid: [u8; 32] = unhex(c["escrow_id_hex"].as_str().unwrap()).try_into().unwrap();
                let reports: Vec<EscrowReady> = c["reports"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|r| EscrowReady {
                        version: 1,
                        suite: 1,
                        escrow_id: eid,
                        ms_address: r["ms_address"].as_str().unwrap().as_bytes().to_vec(),
                        threshold: r["threshold"].as_u64().unwrap() as u8,
                        total: r["total"].as_u64().unwrap() as u8,
                        arbiter: r["arbiter"].as_str().unwrap().as_bytes().to_vec(),
                        from_index: r["from_index"].as_u64().unwrap() as u8,
                        timestamp: 1_800_000_000,
                    })
                    .collect();
                let trusted: Vec<Vec<u8>> = c["trusted_arbiters"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|a| a.as_str().unwrap().as_bytes().to_vec())
                    .collect();
                let got = check_escrow_ready(&reports, &eid, &trusted);
                check_outcome(name, &c["expect"], got.map(|a| a.to_vec()).map_err(|e| e.code as u8));
            }
            "escrow.release" => {
                let eid: [u8; 32] = unhex(c["escrow_id_hex"].as_str().unwrap()).try_into().unwrap();
                let ready = EscrowReady {
                    version: 1, suite: 1, escrow_id: eid,
                    ms_address: b"53multisigaddress".to_vec(),
                    threshold: 2, total: 3,
                    arbiter: b"arbiter-key-1".to_vec(),
                    from_index: 0, timestamp: 1_800_000_000,
                };
                let rb = ready.to_value().encode();
                let rel = Release {
                    version: 1, suite: 1, escrow_id: eid,
                    ready_link: commit(Purpose::ChainLink, &rb),
                    to: c["to"].as_str().unwrap().as_bytes().to_vec(),
                    amount_pxmr: c["amount_pxmr"].as_u64().unwrap(),
                    timestamp: 1_800_000_000,
                };
                let dests: Vec<Vec<u8>> = c["allowed_destinations"]
                    .as_array().unwrap().iter()
                    .map(|d| d.as_str().unwrap().as_bytes().to_vec()).collect();
                let got = check_release(&rel, &ready, &rb,
                    c["escrowed_pxmr"].as_u64().unwrap(), &dests);
                check_outcome(name, &c["expect"], got.map(|_| vec![]).map_err(|e| e.code as u8));
            }
            "bond.check" => {
                let bond = BondProof {
                    version: 1, suite: 1,
                    bond_ms_address: b"53multisigbondaddress".to_vec(),
                    bond_amount_pxmr: c["bond_amount_pxmr"].as_u64().unwrap(),
                    arbiter_set_id: unhex(c["arbiter_set_id_hex"].as_str().unwrap())
                        .try_into().unwrap(),
                    capacity_bucket: c["capacity_bucket"].as_u64().unwrap(),
                    issued: c["issued"].as_u64().unwrap(),
                };
                let trusted: Vec<[u8; 32]> = c["trusted_arbiter_sets"].as_array().unwrap()
                    .iter().map(|t| unhex(t.as_str().unwrap()).try_into().unwrap()).collect();
                let got = check_bond_proof(
                    &bond,
                    c["fare_pxmr"].as_u64().unwrap(),
                    c["now"].as_u64().unwrap(),
                    c["max_age_s"].as_u64().unwrap(),
                    &trusted,
                );
                check_outcome(name, &c["expect"], got.map(|_| vec![]).map_err(|e| e.code as u8));
            }
            "slash.check" => {
                let accept_bytes = b"accept".to_vec();
                let receipt_bytes = b"receipt".to_vec();
                let agreed = c["agreed_pxmr"].as_u64().unwrap();
                let accept = Accept {
                    version: 1, suite: 1, nonce: [0x22; 16], offer_hash: [0x11; 32],
                    amount_final: agreed, dest: None,
                    reader_session_pk: vec![0x33; 32], timestamp: 1_800_000_000,
                    chosen_version: 1, chosen_suite: 1, refund_to: None,
                    memo: None,
                };
                let receipt = Receipt {
                    version: 1, suite: 1,
                    accept_hash: commit(Purpose::ChainLink, &accept_bytes),
                    prev: commit(Purpose::ChainLink, &accept_bytes),
                    amount_final: agreed, timestamp: 1_800_000_005, unilateral: false,
                };
                // Not "the nearest reason we know".
                //
                // This read `if reason == 1 { cure window } else { double
                // spend }`, and so did the second implementation — both
                // rounding an unrecognised reason to the same known one, which
                // is why no vector ever caught that neither of them refused
                // it. `SlashClaim::from_value` does, and the reason they both
                // rounded to is the one that skips the waiting period.
                let reason = match c["reason"].as_u64().unwrap() {
                    1 => SlashReason::CureWindowExpired,
                    2 => SlashReason::ConflictingKeyImage,
                    _ => {
                        check_outcome(
                            name, &c["expect"],
                            Err(ducat_core::reject::RejectCode::Malformed as u8),
                        );
                        continue;
                    }
                };
                let claim = SlashClaim {
                    version: 1, suite: 1,
                    accept_link: commit(Purpose::ChainLink, &accept_bytes),
                    receipt_link: commit(Purpose::ChainLink, &receipt_bytes),
                    txid: [0x77; 32],
                    reason,
                    key_image: c["key_image_hex"].as_str()
                        .map(|k| unhex(k).try_into().unwrap()),
                    claim_pxmr: c["claim_pxmr"].as_u64().unwrap(),
                    timestamp: 1_800_000_100,
                };
                let got = check_slash_claim(
                    &claim, &accept, &accept_bytes, &receipt, &receipt_bytes,
                    c["elapsed_blocks"].as_u64().unwrap(),
                    c["cure_blocks"].as_u64().unwrap(),
                );
                check_outcome(name, &c["expect"], got.map(|_| vec![]).map_err(|e| e.code as u8));
            }
            other => panic!("{name}: unhandled contract kind {other}"),
        }
    }
}

fn check_outcome(name: &str, expect: &J, got: Result<Vec<u8>, u8>) {
    let want_ok = expect["ok"].as_bool().unwrap_or(true);
    match got {
        Ok(_) => assert!(want_ok, "{name}: accepted where the vector expects a refusal"),
        Err(code) => {
            assert!(!want_ok, "{name}: refused where the vector expects success");
            assert_eq!(
                code,
                expect["reject_code"].as_u64().unwrap() as u8,
                "{name}: wrong reject code"
            );
        }
    }
}
