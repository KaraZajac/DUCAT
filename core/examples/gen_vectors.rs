//! Generate the DUCAT conformance vector set (§18.9).
//!
//! Run with `cargo run --example gen_vectors`. Output is deterministic — all
//! keys come from fixed seeds and nothing consults a clock or an RNG — so
//! regenerating on an unchanged implementation produces byte-identical files
//! and `git diff` shows real changes only.
//!
//! Vectors state expectations in terms of **§18.5 wire reject codes**, never
//! this implementation's internal error enum. Two clients must agree that some
//! input is `MALFORMED`; they need not agree on which internal variant said so.
//! Each case additionally carries a non-normative `hint` naming the rule, to
//! save an implementer from bisecting their decoder.

use ducat_core::cbor::Value;
use ducat_core::cbor_map;
use ducat_core::commit::{commit, Purpose};
use ducat_core::negotiate::{negotiate, Policy, Supported};
use ducat_core::reject::RejectCode;
use ducat_core::sig::{ObjectType, SecretKey, SignedBytes, Suite};
use ducat_core::state::{deadline, transition, Event, Role, SettleMode, State};
use ducat_core::wire::*;
use serde_json::{json, Map, Value as J};
use std::time::Duration;

const VECTOR_SET_VERSION: &str = "1";
const PROTOCOL_DRAFT: &str = "0.42";

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{:02x}", x)).collect()
}

fn ok_case(name: &str, why: &str, input: &[u8]) -> J {
    json!({
        "name": name,
        "why": why,
        "input_hex": hex(input),
        "expect": { "ok": true, "reencodes_to_hex": hex(input) }
    })
}

fn reject_case(name: &str, why: &str, input: &[u8], code: RejectCode, hint: &str) -> J {
    json!({
        "name": name,
        "why": why,
        "input_hex": hex(input),
        "expect": { "ok": false, "reject_code": code as u8, "reject_name": format!("{:?}", code) },
        "hint": hint
    })
}

// ------------------------------------------------------------------ codec --

fn codec_cases() -> Vec<J> {
    let mut v = Vec::new();

    // §18.1 text. No object carried a string until memos (§7.5), so nothing
    // exercised the NFC rule — and the reference decoder did not enforce it
    // while the second implementation did. A divergence no vector could reveal
    // is a divergence the suite was not testing for.
    //
    // "café" composed and decomposed are the same string to a reader and
    // different bytes to a hash, which is §18.3's transcript-divergence bug
    // arriving through a display field.
    v.push(ok_case(
        "text_nfc_composed",
        "composed form is the canonical one and decodes",
        &Value::Text("caf\u{e9}".into()).encode(),
    ));
    v.push(reject_case(
        "text_nfd_decomposed_refused",
        "the same string in decomposed form is a second encoding of one value, and two \
         encodings of one value is a transcript-divergence bug whatever the signatures say",
        &Value::Text("cafe\u{301}".into()).encode(),
        RejectCode::Malformed,
        "NFC normalization",
    ));

    // §18.1 negative integers. Added at 0.45 because two implementations
    // disagreed here and *neither was wrong*: the spec said nothing, so one
    // accepted them and one refused. An unspecified behaviour with no vector is
    // exactly how a conformance suite certifies a divergence.
    for (name, enc) in [
        ("nint_minus_1_refused", vec![0x20u8]),
        ("nint_minus_256_refused", vec![0x38, 0xFF]),
    ] {
        v.push(reject_case(
            name,
            "no object in the protocol carries a negative number — money is unsigned \
             piconero, map keys are unsigned, every timestamp is a count. Refusal is the \
             reversible choice: accepting a type later extends the format, while refusing \
             what was once accepted breaks every peer relying on it.",
            &enc,
            RejectCode::Malformed,
            "CBOR major type 1",
        ));
    }

    // §18.9(1) — integer boundaries, both directions.
    for (n, enc) in [
        (0u64, vec![0x00]),
        (23, vec![0x17]),
        (24, vec![0x18, 0x18]),
        (255, vec![0x18, 0xFF]),
        (256, vec![0x19, 0x01, 0x00]),
        (65535, vec![0x19, 0xFF, 0xFF]),
        (65536, vec![0x1A, 0x00, 0x01, 0x00, 0x00]),
        (4294967295, vec![0x1A, 0xFF, 0xFF, 0xFF, 0xFF]),
        (4294967296, vec![0x1B, 0, 0, 0, 1, 0, 0, 0, 0]),
        (u64::MAX, vec![0x1B, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]),
    ] {
        assert_eq!(Value::Uint(n).encode(), enc, "generator disagrees with itself");
        v.push(ok_case(
            &format!("uint_{}_shortest_form", n),
            "each integer must use the shortest head that can carry it",
            &enc,
        ));
    }

    for (name, enc, hint) in [
        ("overlong_uint_0", vec![0x18, 0x00], "0 must use the immediate form"),
        ("overlong_uint_23", vec![0x18, 0x17], "23 is the last immediate-form value"),
        ("overlong_uint_24", vec![0x19, 0x00, 0x18], "24 belongs in the 1-byte form"),
        ("overlong_uint_255", vec![0x19, 0x00, 0xFF], "255 belongs in the 1-byte form"),
        ("overlong_uint_256", vec![0x1A, 0, 0, 0x01, 0x00], "256 belongs in the 2-byte form"),
        ("overlong_uint_65535", vec![0x1B, 0, 0, 0, 0, 0, 0, 0xFF, 0xFF], "65535 belongs in the 2-byte form"),
    ] {
        v.push(reject_case(
            name,
            "a longer-than-necessary integer encoding is not canonical",
            &enc,
            RejectCode::Malformed,
            hint,
        ));
    }

    // Map ordering and duplicates.
    v.push(ok_case(
        "map_keys_ascending",
        "canonical map key order is ascending and distinct",
        &[0xA2, 0x01, 0x00, 0x02, 0x00],
    ));
    v.push(reject_case(
        "map_keys_descending",
        "map keys out of ascending order are not canonical",
        &[0xA2, 0x02, 0x00, 0x01, 0x00],
        RejectCode::Malformed,
        "keys must strictly ascend",
    ));
    v.push(reject_case(
        "map_keys_duplicate",
        "a repeated key must be an error, never a last-wins overwrite",
        &[0xA2, 0x01, 0x00, 0x01, 0x00],
        RejectCode::Malformed,
        "duplicate map key",
    ));
    v.push(reject_case(
        "map_key_not_integer",
        "map keys are unsigned integers only (§18.1)",
        &[0xA1, 0x61, 0x61, 0x00],
        RejectCode::Malformed,
        "text key",
    ));

    // §18.2 — floats never appear.
    for (name, enc) in [
        ("float_half", vec![0xF9, 0x00, 0x00]),
        ("float_single", vec![0xFA, 0, 0, 0, 0]),
        ("float_double", vec![0xFB, 0, 0, 0, 0, 0, 0, 0, 0]),
        ("float_nested_in_map", vec![0xA1, 0x01, 0xF9, 0x00, 0x00]),
    ] {
        v.push(reject_case(
            name,
            "money is integers; a float anywhere is malformed (§18.2)",
            &enc,
            RejectCode::Malformed,
            "float encountered",
        ));
    }

    for (name, enc, hint) in [
        ("tag", vec![0xC0, 0x00], "the tag allowlist is empty"),
        ("indefinite_bytes", vec![0x5F], "indefinite lengths are forbidden"),
        ("indefinite_array", vec![0x9F], "indefinite lengths are forbidden"),
        ("indefinite_map", vec![0xBF], "indefinite lengths are forbidden"),
        ("trailing_bytes", vec![0x00, 0x00], "a signed object is exactly its bytes"),
        ("truncated_head", vec![0x19, 0x01], "input ended mid-item"),
        ("truncated_bytes", vec![0x42, 0xFF], "input ended mid-item"),
        ("invalid_utf8", vec![0x62, 0xFF, 0xFE], "text must be valid UTF-8"),
        (
            "oversized_array_header",
            vec![0x9A, 0xFF, 0xFF, 0xFF, 0xFF],
            "a length header must not drive allocation before data arrives",
        ),
        (
            "oversized_bytes_header",
            vec![0x5A, 0xFF, 0xFF, 0xFF, 0xFF],
            "a length header must not drive allocation before data arrives",
        ),
    ] {
        v.push(reject_case(
            name,
            "restricted CBOR profile (§18.1)",
            &enc,
            RejectCode::Malformed,
            hint,
        ));
    }

    // Depth bound. 18 nested arrays exceeds the 16-deep limit.
    let mut deep = vec![0x81u8; 18];
    deep.push(0x00);
    v.push(reject_case(
        "nesting_too_deep",
        "a small payload must not be able to exhaust the stack during decode",
        &deep,
        RejectCode::Malformed,
        "exceeds the 16-level nesting bound",
    ));

    // §18.9(7) — money. Present specifically to fail a float implementation.
    for (name, amount) in [
        ("piconero_one", 1u64),
        ("piconero_2_5_xmr", 2_500_000_000_000),
        ("piconero_2pow53_plus_1", 9_007_199_254_740_993),
        ("piconero_u64_max", u64::MAX),
    ] {
        let enc = cbor_map! { 1 => Value::Uint(amount) }.encode();
        v.push(json!({
            "name": name,
            "why": "amounts are integer piconero; 2^53+1 is the first integer an f64 cannot represent, so a float-based client fails here",
            "input_hex": hex(&enc),
            "expect": { "ok": true, "reencodes_to_hex": hex(&enc), "amount_at_key_1": amount }
        }));
    }

    v
}

// ---------------------------------------------------------------- signing --

fn signing_cases() -> Vec<J> {
    let mut v = Vec::new();
    let body = cbor_map! {
        1 => Value::Uint(1),
        2 => Value::Uint(2_500_000_000_000u64),
    };
    let obj = SignedBytes::from_value(body);

    for (suite, sk) in [
        (Suite::Ed25519X25519, SecretKey::ed25519_from_bytes(&[9u8; 32])),
        (Suite::P256, SecretKey::p256_from_bytes(&[9u8; 32]).unwrap()),
    ] {
        let pk = sk.public();
        let sig = obj.sign(ObjectType::Accept, &sk);

        v.push(json!({
            "name": format!("suite{}_accept_valid", suite as u8),
            "why": "baseline: a correctly domain-separated signature verifies",
            "suite": suite as u8,
            "object_type": "ACCEPT",
            "object_hex": hex(obj.bytes()),
            "pubkey_hex": hex(&pk.to_bytes()),
            "sig_hex": hex(&sig),
            "verify_as": "ACCEPT",
            "expect": { "ok": true }
        }));

        // §18.9(3) — cross-context replay.
        for other in ["TapPresent", "RECEIPT", "bond_proof", "CONTACT_OFFER"] {
            v.push(json!({
                "name": format!("suite{}_accept_replayed_as_{}", suite as u8, other),
                "why": "a signature must not transfer between object types: same key, same bytes, different domain",
                "suite": suite as u8,
                "object_type": "ACCEPT",
                "object_hex": hex(obj.bytes()),
                "pubkey_hex": hex(&pk.to_bytes()),
                "sig_hex": hex(&sig),
                "verify_as": other,
                "expect": { "ok": false, "reject_code": RejectCode::BadSig as u8, "reject_name": "BadSig" }
            }));
        }

        // Byte-level tampering.
        let mut tampered = obj.bytes().to_vec();
        let last = tampered.len() - 1;
        tampered[last] ^= 0x01;
        v.push(json!({
            "name": format!("suite{}_tampered_amount", suite as u8),
            "why": "flipping the lowest bit of the amount must invalidate the signature",
            "suite": suite as u8,
            "object_type": "ACCEPT",
            "object_hex": hex(&tampered),
            "pubkey_hex": hex(&pk.to_bytes()),
            "sig_hex": hex(&sig),
            "verify_as": "ACCEPT",
            "expect": { "ok": false, "reject_code": RejectCode::BadSig as u8, "reject_name": "BadSig" }
        }));
    }

    // P-256 malleability: the high-s twin of a valid signature.
    let sk = SecretKey::p256_from_bytes(&[9u8; 32]).unwrap();
    let pk = sk.public();
    let sig = obj.sign(ObjectType::Accept, &sk);
    v.push(json!({
        "name": "suite2_high_s_twin_refused",
        "why": "ECDSA admits (r, n-s) as an equally valid signature. Accepting both would give one message two valid encodings and diverge the transcript hash, so the high-s form is refused rather than normalized.",
        "suite": Suite::P256 as u8,
        "object_type": "ACCEPT",
        "object_hex": hex(obj.bytes()),
        "pubkey_hex": hex(&pk.to_bytes()),
        "sig_hex": hex(&flip_s(&sig)),
        "verify_as": "ACCEPT",
        "expect": { "ok": false, "reject_code": RejectCode::BadSig as u8, "reject_name": "BadSig" },
        "hint": "reject high-s; do not normalize on receipt"
    }));

    // SEC1 key encoding uniqueness.
    let compressed = pk.to_bytes();
    for bad in [0x04u8, 0x05, 0x06, 0x07] {
        let mut variant = compressed.clone();
        variant[0] = bad;
        v.push(json!({
            "name": format!("suite2_pubkey_tag_{:#04x}_refused", bad),
            "why": "exactly one public key encoding is legal (compressed, 33 bytes). Common parsers accept 0x05 by reading y-parity from the low bit, yielding the same key as 0x03 — a second encoding of one key is a second canonical object and a second hash.",
            "suite": Suite::P256 as u8,
            "pubkey_hex": hex(&variant),
            "expect": { "ok": false, "reject_code": RejectCode::Malformed as u8, "reject_name": "Malformed" },
            "hint": "check the SEC1 tag explicitly rather than delegating to the parser"
        }));
    }

    v
}

fn flip_s(sig: &[u8; 64]) -> [u8; 64] {
    const N: [u8; 32] = [
        0xFF, 0xFF, 0xFF, 0xFF, 0x00, 0x00, 0x00, 0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
        0xFF, 0xBC, 0xE6, 0xFA, 0xAD, 0xA7, 0x17, 0x9E, 0x84, 0xF3, 0xB9, 0xCA, 0xC2, 0xFC, 0x63,
        0x25, 0x51,
    ];
    let mut out = *sig;
    let mut borrow = 0i16;
    for i in (0..32).rev() {
        let d = N[i] as i16 - sig[32 + i] as i16 - borrow;
        if d < 0 {
            out[32 + i] = (d + 256) as u8;
            borrow = 1;
        } else {
            out[32 + i] = d as u8;
            borrow = 0;
        }
    }
    out
}

// ------------------------------------------------------------------ state --

fn state_name(s: State) -> String {
    format!("{:?}", s)
}

fn state_cases() -> Vec<J> {
    let mut v = Vec::new();

    // Happy paths.
    let direct = [
        (Event::TapPresent, State::Offered),
        (Event::FullOffer, State::Quoted),
        (Event::Accept { from: Role::Payer }, State::Accepted),
        (Event::Fund, State::Funded),
        (Event::Proof, State::Delivered),
        (Event::Receipt, State::Closed),
    ];
    v.push(sequence_case(
        "happy_path_direct",
        "the ordinary course of a direct-settlement transaction",
        SettleMode::Direct,
        Role::Payer,
        &direct,
    ));

    let fast = [
        (Event::TapPresent, State::Offered),
        (Event::FullOffer, State::Quoted),
        (Event::Accept { from: Role::Payer }, State::Accepted),
        (Event::Fund, State::Funded),
        (Event::TxId, State::Provisional),
        (Event::Proof, State::Delivered),
        (Event::Receipt, State::Closed),
        (Event::ConfirmationsReached, State::Settled),
    ];
    v.push(sequence_case(
        "happy_path_fast",
        "fast/1 adds TXPROOF before delivery and reaches SETTLED on finality",
        SettleMode::Fast,
        Role::Payer,
        &fast,
    ));

    // §18.9(5) — every deadline in §6.2, checked from both sides.
    for (state, mode) in [
        (State::Offered, SettleMode::Direct),
        (State::Quoted, SettleMode::Direct),
        (State::Accepted, SettleMode::Direct),
        (State::Accepted, SettleMode::Escrow),
        (State::Funded, SettleMode::Fast),
        (State::Delivered, SettleMode::Direct),
        (State::Closed, SettleMode::Direct),
    ] {
        let d = deadline(state, mode).expect("state should have a deadline");
        let secs = d.as_secs();

        let early = transition(state, Role::Payer, mode, &Event::Elapsed(Duration::from_secs(secs - 1))).unwrap();
        v.push(json!({
            "name": format!("{}_{:?}_holds_before_deadline", state_name(state).to_lowercase(), mode),
            "why": "a timeout must not fire early",
            "from": state_name(state), "role": "Payer", "mode": format!("{:?}", mode),
            "event": { "Elapsed": secs - 1 },
            "expect": { "next": state_name(early.next), "effect": format!("{:?}", early.effect) }
        }));

        let due = transition(state, Role::Payer, mode, &Event::Elapsed(d)).unwrap();
        v.push(json!({
            "name": format!("{}_{:?}_fires_at_deadline", state_name(state).to_lowercase(), mode),
            "why": "§6.2 deadline, inclusive at the boundary",
            "from": state_name(state), "role": "Payer", "mode": format!("{:?}", mode),
            "event": { "Elapsed": secs },
            "expect": { "next": state_name(due.next), "effect": format!("{:?}", due.effect) },
            "deadline_secs": secs
        }));
    }

    // Named failure paths worth calling out individually.
    v.push(single_case(
        "abandoned_tap_discards_silently",
        "a tap that never delivers its offer must leave no trace and must never put a screen in front of the human — the confirm screen is the security boundary (§15.5)",
        State::Offered, Role::Payer, SettleMode::Direct, Event::Elapsed(Duration::from_secs(10)),
    ));
    v.push(single_case(
        "vanishing_counterparty_single_sided_receipt",
        "money gone, no co-signed record: the payer keeps signed evidence of what it paid, which proves payment and not delivery",
        State::Delivered, Role::Payer, SettleMode::Direct, Event::Elapsed(Duration::from_secs(120)),
    ));
    v.push(single_case(
        "escrow_setup_timeout_recovers_funds",
        "multisig setup is multi-round; its expiry must run the recovery path rather than a bare abort",
        State::Accepted, Role::Payer, SettleMode::Escrow, Event::Elapsed(Duration::from_secs(300)),
    ));
    v.push(single_case(
        "cure_window_expiry_enables_slash",
        "fast/1: unconfirmed past the cure window makes a slash claim fileable (§17.5)",
        State::Closed, Role::Payee, SettleMode::Fast, Event::CureWindowExpired,
    ));

    // §18.4: unlisted pairings are refusals, never silent ignores.
    for (state, ev, why) in [
        (State::Idle, Event::Accept { from: Role::Payer }, "ACCEPT before any offer exists"),
        (State::Quoted, Event::Fund, "funding before the price is locked"),
        (State::Funded, Event::Cancel, "cancellation after funds have moved"),
        (State::Closed, Event::Fund, "funding a closed transaction"),
        (State::Delivered, Event::ContactOffer, "identity exchange must follow closure, never precede it (§16.3)"),
    ] {
        let err = transition(state, Role::Payer, SettleMode::Direct, &ev).unwrap_err();
        v.push(json!({
            "name": format!("violation_{}_{:?}", state_name(state).to_lowercase(), ev),
            "why": why,
            "from": state_name(state), "role": "Payer", "mode": "Direct",
            "event": format!("{:?}", ev),
            "expect": { "ok": false, "reject_code": err.code as u8, "reject_name": format!("{:?}", err.code) }
        }));
    }

    // Direction constrains the ORIGINATOR, not the evaluator. A payee-originated
    // ACCEPT is refused by both parties; a payer-originated one is accepted by
    // both. Guarding on the local role instead means a payee refuses every
    // ACCEPT it receives and no transaction can complete — a bug the market
    // simulator caught on its first run.
    for who in [Role::Payer, Role::Payee] {
        let t = transition(State::Quoted, who, SettleMode::Direct,
                           &Event::Accept { from: Role::Payer }).unwrap();
        v.push(json!({
            "name": format!("payer_originated_accept_processed_by_{:?}", who).to_lowercase(),
            "why": "both parties must reach the same verdict about the same message; the payee has to process the ACCEPT it receives",
            "from": "Quoted", "role": format!("{:?}", who), "mode": "Direct",
            "event": { "Accept": { "from": "Payer" } },
            "expect": { "next": format!("{:?}", t.next), "effect": format!("{:?}", t.effect) }
        }));
        let err = transition(State::Quoted, who, SettleMode::Direct,
                             &Event::Accept { from: Role::Payee }).unwrap_err();
        v.push(json!({
            "name": format!("payee_originated_accept_refused_by_{:?}", who).to_lowercase(),
            "why": "a payee able to accept its own offer drives the whole flow with no human checkpoint (§18.4.1)",
            "from": "Quoted", "role": format!("{:?}", who), "mode": "Direct",
            "event": { "Accept": { "from": "Payee" } },
            "expect": { "ok": false, "reject_code": err.code as u8, "reject_name": format!("{:?}", err.code) }
        }));
    }

    v
}

fn sequence_case(name: &str, why: &str, mode: SettleMode, role: Role, steps: &[(Event, State)]) -> J {
    let mut s = State::Idle;
    let mut out = Vec::new();
    for (ev, expect) in steps {
        let t = transition(s, role, mode, ev).unwrap();
        assert_eq!(t.next, *expect, "generator disagrees with itself at {:?}", ev);
        out.push(json!({
            "event": format!("{:?}", ev),
            "next": state_name(t.next),
            "effect": format!("{:?}", t.effect)
        }));
        s = t.next;
    }
    json!({ "name": name, "why": why, "mode": format!("{:?}", mode), "role": format!("{:?}", role),
            "from": "Idle", "steps": out })
}

fn single_case(name: &str, why: &str, from: State, role: Role, mode: SettleMode, ev: Event) -> J {
    let t = transition(from, role, mode, &ev).unwrap();
    json!({
        "name": name, "why": why,
        "from": state_name(from), "role": format!("{:?}", role), "mode": format!("{:?}", mode),
        "event": format!("{:?}", ev),
        "expect": { "next": state_name(t.next), "effect": format!("{:?}", t.effect) }
    })
}

// ------------------------------------------------------------- negotiation --

fn negotiate_cases() -> Vec<J> {
    let mut v = Vec::new();
    let ed = Suite::Ed25519X25519;
    let p = Suite::P256;

    let policy = Policy::new(vec![ed, p], vec![1]);
    let offered = Supported { versions: vec![1], suites: vec![p, ed] };
    let sel = negotiate(&offered, &policy).unwrap();
    v.push(json!({
        "name": "suite_selection_follows_payer_preference_not_identifier",
        "why": "suite identifiers encode no preference. P-256 (2) exists only because iOS's Secure Enclave holds no Ed25519 key — a hardware-forced fallback, not an upgrade — so 'highest wins' would select the weaker option on every dual-capable pair.",
        "offered": { "versions": offered.versions, "suites": offered.suites.iter().map(|s| *s as u8).collect::<Vec<_>>() },
        "payer_preference": [ed as u8, p as u8],
        // Every negotiation case carries all three inputs. A case that omits one
        // forces the consumer to invent a default, which is exactly how two
        // implementations diverge — the schema now requires them.
        "local_versions": [1],
        "expect": { "ok": true, "version": sel.version, "suite": sel.suite as u8 }
    }));

    let offered_hi = Supported { versions: vec![1, 2, 3], suites: vec![ed] };
    let policy_hi = Policy::new(vec![ed], vec![1, 2]);
    let sel_hi = negotiate(&offered_hi, &policy_hi).unwrap();
    v.push(json!({
        "name": "version_selection_is_highest_mutual",
        "why": "versions, unlike suites, are ordered by construction: higher means newer",
        "offered": { "versions": [1, 2, 3], "suites": [ed as u8] },
        "local_versions": [1, 2],
        "payer_preference": [ed as u8],
        "expect": { "ok": true, "version": sel_hi.version, "suite": sel_hi.suite as u8 }
    }));

    for (name, offered, why, code) in [
        ("no_common_version",
         Supported { versions: vec![7], suites: vec![ed] },
         "no mutually supported protocol version",
         RejectCode::UnsupportedVersion),
        ("no_common_suite",
         Supported { versions: vec![1], suites: vec![p] },
         "the only offered suite is one this client will not speak",
         RejectCode::UnsupportedSuite),
    ] {
        let strict = Policy::new(vec![ed], vec![1]);
        let err = negotiate(&offered, &strict).unwrap_err();
        assert_eq!(err.code, code);
        v.push(json!({
            "name": name, "why": why,
            "offered": { "versions": offered.versions, "suites": offered.suites.iter().map(|s| *s as u8).collect::<Vec<_>>() },
            "payer_preference": [ed as u8],
            "local_versions": [1],
            "expect": { "ok": false, "reject_code": code as u8, "reject_name": format!("{:?}", code) }
        }));
    }

    // §18.9(6) — the downgrade attempt.
    let genuine = cbor_map! {
        1 => Value::Uint(1),
        2 => Value::Uint(2_500_000_000_000u64),
        3 => Value::Array(vec![Value::Uint(1)]),
        4 => Value::Array(vec![Value::Uint(ed as u64), Value::Uint(p as u64)]),
    }.encode();
    let stripped = cbor_map! {
        1 => Value::Uint(1),
        2 => Value::Uint(2_500_000_000_000u64),
        3 => Value::Array(vec![Value::Uint(1)]),
        4 => Value::Array(vec![Value::Uint(p as u64)]),
    }.encode();
    let c = commit(Purpose::Offer, &genuine);

    v.push(json!({
        "name": "downgrade_stripped_suite_fails_commitment",
        "why": "a MITM removes the strong suite so the payer sees a menu of one. offer_commit covers the whole FullOffer, so the substitution is caught BEFORE any suite is chosen — negotiating first would mean selecting from an attacker-chosen menu and only then noticing.",
        "offer_commit_hex": hex(&c),
        "genuine_offer_hex": hex(&genuine),
        "stripped_offer_hex": hex(&stripped),
        "expect": {
            "genuine": { "ok": true },
            "stripped": { "ok": false, "reject_code": RejectCode::CommitMismatch as u8, "reject_name": "CommitMismatch" }
        },
        "hint": "commit = SHA-256(\"DUCAT-v1\" 0x00 purpose 0x00 canonical_bytes)"
    }));

    // Commitment domain separation.
    let mut purposes = Map::new();
    for (p_, label) in [
        (Purpose::Offer, "offer_commit"),
        (Purpose::Receipt, "receipt"),
        (Purpose::ChainLink, "chain"),
        (Purpose::MarketGenesis, "market_genesis"),
    ] {
        purposes.insert(label.to_string(), json!(hex(&commit(p_, &genuine))));
    }
    v.push(json!({
        "name": "commitments_are_domain_separated_by_purpose",
        "why": "the same canonical bytes must produce a different digest per role, so a commitment computed for one purpose cannot be presented as another",
        "input_hex": hex(&genuine),
        "expect": { "digests_by_purpose": purposes }
    }));

    v
}


// ------------------------------------------------------------- transcripts --

/// §18.9(4) — a full transcript per buildable-now profile, chained end to end.
fn transcript_cases() -> Vec<J> {
    const FARE: u64 = 2_500_000_000_000;
    const NONCE: [u8; 16] = [0xA5; 16];
    let payee = SecretKey::ed25519_from_bytes(&[1u8; 32]);

    let mut v = Vec::new();
    for (profile_id, profile_name, dest) in [
        (1u64, "xfer/1", None),
        (2, "pos/1", None),
        (3, "ride/1", Some(vec![0x0Du8; 16])),
    ] {
        let offer = FullOffer {
            version: 1, suite: 1, profile: profile_id,
            payto: vec![0x42; 69], amount_pxmr: FARE,
            supported_versions: vec![1], supported_suites: vec![1, 2],
            settle_mode: 0, fee_policy: FeePolicy::PayerPays, nonce_echo: NONCE,
        terms: Terms::default(),
            memo: None,
        };
        let tap = TapPresent {
            version: 1, suite: 1, profile: profile_id,
            presenter_role: PresenterRole::Payee,
            amount_authority: AmountAuthority::Fixed,
            intent: Intent::Oneshot, rmode: ReachMode::Token,
            nonce: NONCE, expiry: 1_800_000_030,
            session_pk: payee.public().to_bytes(),
            route: vec![0x11; 32],
            offer_commit: offer.commitment(),
            dest: dest.clone(), session_ref: None,
        };
        let accept = Accept {
            version: 1, suite: 1, nonce: NONCE,
            offer_hash: offer.commitment(), amount_final: FARE,
            dest: dest.clone(),
            reader_session_pk: SecretKey::ed25519_from_bytes(&[2u8; 32]).public().to_bytes(),
            timestamp: 1_800_000_005, chosen_version: 1, chosen_suite: 1,
        refund_to: Some(b"payer-refund-addr".to_vec()),
            memo: None,
        };
        let accept_bytes = accept.to_value().encode();
        let link = commit(Purpose::ChainLink, &accept_bytes);
        let receipt = Receipt {
            version: 1, suite: 1, accept_hash: link, prev: link,
            amount_final: FARE, timestamp: 1_800_000_010, unilateral: false,
        };

        verify_transcript(&tap, &offer, &accept, &accept_bytes, &receipt)
            .expect("generator produced an invalid transcript");

        v.push(json!({
            "name": format!("transcript_{}", profile_name.replace('/', "_")),
            "why": format!("a complete {} transaction, verifiable end to end from the tap through the receipt by the two parties who hold it", profile_name),
            "profile": profile_name,
            "tap_present_hex": hex(&tap.to_value().encode()),
            "full_offer_hex": hex(&offer.to_value().encode()),
            "accept_hex": hex(&accept_bytes),
            "receipt_hex": hex(&receipt.to_value().encode()),
            "expect": {
                "ok": true,
                "offer_commit_hex": hex(&offer.commitment()),
                "accept_chain_link_hex": hex(&link),
                "amount_pxmr": FARE
            },
            "chain": [
                "TapPresent.offer_commit == commit(Offer, FullOffer)",
                "ACCEPT.offer_hash == the same commitment",
                "ACCEPT.amount_final == FullOffer.amount_pxmr",
                "RECEIPT.accept_hash == RECEIPT.prev == commit(ChainLink, ACCEPT)",
                "RECEIPT.amount_final == ACCEPT.amount_final"
            ]
        }));
    }

    // A tampered transcript: the offer is swapped after the tap commits to it.
    let offer = FullOffer {
        version: 1, suite: 1, profile: 2,
        payto: vec![0x42; 69], amount_pxmr: FARE,
        supported_versions: vec![1], supported_suites: vec![1, 2],
        settle_mode: 0, fee_policy: FeePolicy::PayerPays, nonce_echo: NONCE,
        terms: Terms::default(),
        memo: None,
    };
    let mut dearer = offer.clone();
    dearer.amount_pxmr = FARE * 10;
    v.push(json!({
        "name": "transcript_offer_swapped_after_tap",
        "why": "the attack §15.3's commitment exists to stop: a hostile terminal delivers a different offer than the one the tap advertised",
        "tap_offer_commit_hex": hex(&offer.commitment()),
        "delivered_offer_hex": hex(&dearer.to_value().encode()),
        "expect": { "ok": false, "reject_code": RejectCode::CommitMismatch as u8, "reject_name": "CommitMismatch" }
    }));

    v
}

// ------------------------------------------------------------------- main --

/// §4.3 backup format.
///
/// The one place in this vector set where the artifact under test is not a wire
/// object. It is here for the same reason §18.9 exists: two implementations that
/// disagree about Argon2 parameters, CBOR field numbering, or what is covered by
/// the AEAD will each produce a file the other cannot open, and neither will see
/// an error more informative than "wrong passphrase". That is not a bug a user
/// can report usefully.
fn backup_cases() -> Vec<J> {
    use ducat_core::backup::{export, Backup};
    use ducat_core::verify::VerificationPolicy;

    let seed = "abbey abducts ability able abnormal abort about above absurd abyss \
                academy accent acid acoustic acquire across actress acute adapt \
                addicted adept adhesive adjust adopt abbey";
    let base = Backup {
        avatar: None, email: None, phone: None, signal: None, pronouns: None,
        persona_suite: 1,
        persona_secret: vec![0x11; 32],
        monero_seed: seed.to_string(),
        monero_restore_height: 2_183_500,
        rendezvous: vec![vec![0xAA; 32], vec![0xBB; 32]],
        attestation_records: vec![vec![0xDD; 32]],
        mandates: vec![vec![0xCC; 48]],
        verification: VerificationPolicy::default(),
        escrow_shares: vec![ducat_core::backup::EscrowShare {
            escrow_id: vec![0xEE; 16],
            key_file: vec![0x9F; 64],
            restore_height: 2_183_000,
        }],
        display_name: None,
        publish_payto: false,
        created: 1_800_000_000,
    };
    let pass = b"a fixed passphrase";
    let salt = [0x42u8; 16];
    let nonce = [0x37u8; 24];
    let blob = export(&base, pass, salt, nonce).expect("export");

    let mut cases = vec![json!({
        "name": "canonical_bundle",
        "why": "the whole format at once — Argon2id parameters, XChaCha20-Poly1305, the AAD, \
                and every CBOR field number. An implementation that differs anywhere produces \
                a file no other client can open, reporting only 'wrong passphrase'.",
        "passphrase_utf8": String::from_utf8_lossy(pass),
        "salt_hex": hex(&salt),
        "nonce_hex": hex(&nonce),
        "kdf": {"algorithm": "argon2id", "version": 19, "memory_kib": 65536, "iterations": 3, "lanes": 1, "output_len": 32},
        "aead": "xchacha20poly1305",
        "aad_utf8": "DUCAT-BACKUP-v1",
        "blob_hex": hex(&blob),
        "expect": {
            "ok": true,
            "decoded": {
                "persona_suite": 1,
                "persona_secret_hex": hex(&base.persona_secret),
                "monero_seed": seed,
                "monero_restore_height": 2_183_500u64,
                "rendezvous_hex": base.rendezvous.iter().map(|r| hex(r)).collect::<Vec<_>>(),
                "attestation_records_hex": base.attestation_records.iter().map(|r| hex(r)).collect::<Vec<_>>(),
                "mandates_hex": base.mandates.iter().map(|r| hex(r)).collect::<Vec<_>>(),
                "verification": {
                    "device_unlock_at": base.verification.device_unlock_at,
                    "app_secret_at": base.verification.app_secret_at,
                    "app_secret_validity_s": base.verification.app_secret_validity_s,
                    "cumulative_at": base.verification.cumulative_at,
                    "cumulative_window_s": base.verification.cumulative_window_s
                },
                "created": 1_800_000_000u64
            }
        },
        "hint": "the layout is MAGIC(15) || salt(16) || nonce(24) || AEAD ciphertext"
    })];

    cases.push(json!({
        "name": "wrong_passphrase",
        "why": "must be indistinguishable from a tampered file — reporting them differently \
                leaks whether a guess was close",
        "passphrase_utf8": "not the passphrase",
        "blob_hex": hex(&blob),
        "expect": {"ok": false, "reject_code": RejectCode::BadSig as u8, "reject_name": "BadSig"}
    }));

    // One flipped bit in the ciphertext body.
    let mut flipped = blob.clone();
    let last = flipped.len() - 1;
    flipped[last] ^= 0x01;
    cases.push(json!({
        "name": "tampered_ciphertext",
        "why": "the AEAD tag covers the payload; a single flipped bit must not decrypt",
        "passphrase_utf8": String::from_utf8_lossy(pass),
        "blob_hex": hex(&flipped),
        "expect": {"ok": false, "reject_code": RejectCode::BadSig as u8, "reject_name": "BadSig"}
    }));

    // One flipped bit in the nonce, which is outside the ciphertext.
    let mut bad_nonce = blob.clone();
    bad_nonce[20] ^= 0x01;
    cases.push(json!({
        "name": "tampered_nonce",
        "why": "the nonce is stored in the clear and is not secret, but altering it must still \
                fail closed rather than decrypting to something else",
        "passphrase_utf8": String::from_utf8_lossy(pass),
        "blob_hex": hex(&bad_nonce),
        "expect": {"ok": false, "reject_code": RejectCode::BadSig as u8, "reject_name": "BadSig"}
    }));

    let mut bad_magic = blob.clone();
    bad_magic[0] ^= 0x01;
    cases.push(json!({
        "name": "foreign_format",
        "why": "the magic is authenticated as AAD, so a file of another format cannot be \
                coerced into decrypting as this one. Rejected before any key derivation, so a \
                wrong file does not cost the user an Argon2 pass.",
        "passphrase_utf8": String::from_utf8_lossy(pass),
        "blob_hex": hex(&bad_magic),
        "expect": {"ok": false, "reject_code": RejectCode::Malformed as u8, "reject_name": "Malformed"}
    }));

    cases.push(json!({
        "name": "truncated",
        "why": "a file shorter than header plus AEAD tag cannot be a bundle; length must be \
                checked before slicing",
        "passphrase_utf8": String::from_utf8_lossy(pass),
        "blob_hex": hex(&blob[..40]),
        "expect": {"ok": false, "reject_code": RejectCode::Malformed as u8, "reject_name": "Malformed"}
    }));

    // Contents that decrypt cleanly and are nonsense.
    let mut inverted = base.clone();
    inverted.verification = VerificationPolicy {
        device_unlock_at: 50_000,
        app_secret_at: 1_000,
        ..VerificationPolicy::default()
    };
    let inv_blob = export(&inverted, pass, [0x43; 16], [0x38; 24]).expect("export");
    cases.push(json!({
        "name": "inverted_verification_ladder",
        "why": "an import is a trust boundary. This file decrypts perfectly and carries a \
                policy where a larger payment demands less than a smaller one. Authenticity is \
                not sanity: fields with construction rules are re-validated on the way in.",
        "passphrase_utf8": String::from_utf8_lossy(pass),
        "blob_hex": hex(&inv_blob),
        "expect": {"ok": false, "reject_code": RejectCode::PolicyRefused as u8, "reject_name": "PolicyRefused"},
        "hint": "app_secret_at (1000) is below device_unlock_at (50000)"
    }));

    cases
}

/// Rewrite an authored case into the published shape.
///
/// §18.11 recorded that the vector files were neither uniform nor documented: a
/// second implementer met a non-uniform signing schema, two non-negotiation
/// cases inside `negotiate.json`, and three spellings of one event. Documenting
/// that in a schema would have formalised the mess. This normalises it instead,
/// at emit time, so the authoring code stays readable and the published artifact
/// stays uniform.
///
/// Returns the file the case belongs in and the rewritten case.
fn normalize(category: &str, mut c: J) -> (&'static str, J) {
    let obj = c.as_object_mut().unwrap();

    let (file, kind): (&str, &str) = match category {
        "codec" => ("codec", "codec.decode"),
        "signing" => {
            if obj.contains_key("object_hex") {
                ("signing", "signing.verify")
            } else {
                // Four cases test public-key parsing alone and carry no object
                // or signature. That is legitimate and was undiscoverable.
                ("signing", "signing.pubkey")
            }
        }
        "negotiate" => {
            if obj.get("expect").and_then(|e| e.get("digests_by_purpose")).is_some() {
                ("commit", "commit.purposes")
            } else if obj.contains_key("genuine_offer_hex") {
                ("commit", "commit.substitution")
            } else {
                ("negotiate", "negotiate.select")
            }
        }
        "state" => ("state", "state.sequence"),
        "transcript" => {
            if obj.contains_key("tap_present_hex") {
                ("transcript", "transcript.replay")
            } else {
                // An offer substituted after the tap. Filed under transcripts
                // and not one: it exercises a single commitment link rather
                // than replaying a whole chain, and it carries neither the tap
                // nor the receipt.
                ("transcript", "transcript.substitution")
            }
        }
        "backup" => ("backup", "backup.import"),
        "contact" => {
            if obj.contains_key("messages_hex") {
                ("contact", "message.chain")
            } else if obj.contains_key("details_hex") {
                ("contact", "contact.details")
            } else if obj.contains_key("payment_hex") {
                ("contact", "message.payment")
            } else if obj.contains_key("head_hex") {
                ("contact", "log.head")
            } else if obj.contains_key("subkey_count") {
                ("contact", "log.ring")
            } else {
                ("contact", "contact.card")
            }
        }
        "object" => ("object", "object.roundtrip"),
        "contract" => {
            if obj.contains_key("rounds_required") {
                ("contract", "escrow.ceremony")
            } else if obj.contains_key("reports") {
                ("contract", "escrow.ready")
            } else if obj.contains_key("allowed_destinations") {
                ("contract", "escrow.release")
            } else if obj.contains_key("bond_amount_pxmr") {
                ("contract", "bond.check")
            } else {
                ("contract", "slash.check")
            }
        }
        other => panic!("no normalization rule for category {}", other),
    };
    obj.insert("kind".into(), json!(kind));

    if kind == "state.sequence" {
        // One event spelling, self-describing. Previously a step could say
        // "Fund", "Accept { from: Payer }", "Elapsed(60s)", {"Elapsed": 60}, or
        // {"Accept": {"from": "Payer"}} — five encodings of one concept.
        let steps = match obj.remove("steps") {
            Some(J::Array(a)) => a,
            _ => {
                let ev = obj.remove("event").expect("state case needs an event");
                let expect = obj.remove("expect").expect("state case needs an expect");
                vec![json!({ "event": ev, "expect": expect })]
            }
        };
        let steps: Vec<J> = steps
            .into_iter()
            .map(|s| {
                let sm = s.as_object().unwrap();
                let ev = normalize_event(sm.get("event").unwrap());
                // Older cases put next/effect beside the event; move them under
                // `expect` so every case has exactly one place for assertions.
                let expect = match sm.get("expect") {
                    Some(e) => e.clone(),
                    None => {
                        let mut m = Map::new();
                        for k in ["next", "effect", "ok", "reject_code", "reject_name"] {
                            if let Some(val) = sm.get(k) {
                                m.insert(k.into(), val.clone());
                            }
                        }
                        J::Object(m)
                    }
                };
                json!({ "event": ev, "expect": expect })
            })
            .collect();
        obj.insert("steps".into(), json!(steps));

        // A deadline is an assertion about (from, mode), not about any one step.
        if let Some(d) = obj.remove("deadline_secs") {
            obj.insert("deadline_s".into(), d);
        }
    }

    (file, c)
}

/// `{"name": …, "from": …?, "elapsed_s": …?}` — the one event encoding.
fn normalize_event(ev: &J) -> J {
    if let Some(s) = ev.as_str() {
        if let Some(rest) = s.strip_prefix("Elapsed(") {
            let n: u64 = rest.trim_end_matches("s)").parse().expect("elapsed secs");
            return json!({ "name": "Elapsed", "elapsed_s": n });
        }
        if let Some((name, rest)) = s.split_once('{') {
            let from = rest.split(':').nth(1).unwrap().trim().trim_end_matches('}').trim();
            return json!({ "name": name.trim(), "from": from });
        }
        return json!({ "name": s.trim() });
    }
    let m = ev.as_object().expect("event must be a string or an object");
    let (name, arg) = m.iter().next().expect("event object is empty");
    if name == "Elapsed" {
        return json!({ "name": "Elapsed", "elapsed_s": arg.as_u64().unwrap() });
    }
    if let Some(from) = arg.get("from") {
        return json!({ "name": name, "from": from });
    }
    json!({ "name": name })
}

/// §8.2 and §17.4/§17.5 objects, round-tripped.
///
/// The manifest said escrow and `fast/1` had no coverage at all. Full
/// transcripts for them still need `FUND`/`RELEASE` orchestration, but the part
/// a second implementer needs first is smaller and sharper: **do we encode these
/// objects to the same bytes?** If not, nothing downstream can agree, because
/// every commitment in the protocol hashes canonical objects.
fn object_cases() -> Vec<J> {
    use ducat_core::escrow::*;
    let mut v = Vec::new();

    let accept_link = commit(Purpose::ChainLink, &[0xA1u8; 16]);

    let txid = TxId {
        version: 1, suite: 1, accept_link,
        txid: [0x77; 32], amount_pxmr: 21_000_000_000, timestamp: 1_800_000_000,
    };
    let proof = TxProof {
        version: 1, suite: 1, txid: [0x77; 32],
        proof: b"OutProofV2gtxRYPBZJN5AfGH6LsGyFTemrmHYKbukYQ".to_vec(),
        destination: b"driver-addr".to_vec(),
        proof_message: accept_link,
        amount_pxmr: 21_000_000_000, timestamp: 1_800_000_009,
    };
    let claim = SlashClaim {
        version: 1, suite: 1, accept_link,
        receipt_link: commit(Purpose::ChainLink, &[0xB2u8; 16]),
        txid: [0x77; 32], reason: SlashReason::ConflictingKeyImage,
        key_image: Some([0x5A; 32]),
        claim_pxmr: 21_000_000_000, timestamp: 1_800_000_100,
    };
    let claim_cure = SlashClaim { reason: SlashReason::CureWindowExpired, key_image: None, ..claim.clone() };
    let setup = EscrowSetup {
        version: 1, suite: 1, escrow_id: [0xE5; 32], round: 0,
        info: vec![0xAB; 64], from_index: BUYER, timestamp: 1_800_000_000,
    };
    let ready = EscrowReady {
        version: 1, suite: 1, escrow_id: [0xE5; 32],
        ms_address: b"53multisigaddress".to_vec(), threshold: 2, total: 3,
        arbiter: b"arbiter-key-1".to_vec(), from_index: BUYER, timestamp: 1_800_000_200,
    };
    let release = Release {
        version: 1, suite: 1, escrow_id: [0xE5; 32],
        ready_link: commit(Purpose::ChainLink, &ready.to_value().encode()),
        to: b"seller-payout".to_vec(), amount_pxmr: 21_000_000_000, timestamp: 1_800_000_300,
    };

    let items: Vec<(&str, &str, Vec<u8>)> = vec![
        ("TXID", "fast/1's mempool pointer. Carries no evidence: the payee scans with its own \
                  view key, and this object only says what to look for.", txid.to_value().encode()),
        ("TXPROOF", "arbitration evidence only. `proof_message` MUST be the transcript chain \
                     link — Monero enforces the binding, so an unbound proof can be replayed \
                     into an unrelated dispute.", proof.to_value().encode()),
        ("SLASH_CLAIM", "a double-spend claim skips the cure window, which makes it the one \
                         worth forging, which is why it must carry the conflicting key image.",
         claim.to_value().encode()),
        ("SLASH_CLAIM", "the cure-window variant carries no key image: non-confirmation is \
                         usually a fee problem, not fraud.", claim_cure.to_value().encode()),
        ("ESCROW_SETUP", "one contribution to one ceremony round. Rounds are strictly \
                          sequential — §2.5's exploit was a forged out-of-order message.",
         setup.to_value().encode()),
        ("ESCROW_READY", "one participant's report of what the ceremony formed. All three must \
                          match, or the funds land in a wallet the payer holds no share of.",
         ready.to_value().encode()),
        ("RELEASE", "an escrow payout. The destination must be a party to the escrow — the \
                     check a rushed implementation drops, because the happy path never \
                     exercises it.", release.to_value().encode()),
    ];
    for (i, (ty, why, enc)) in items.into_iter().enumerate() {
        v.push(json!({
            "name": format!("object_{}_{}", ty.to_lowercase(), i),
            "why": why,
            "object_type": ty,
            "object_hex": hex(&enc),
            "expect": { "ok": true, "reencodes_to_hex": hex(&enc) }
        }));
    }

    // The type field is checked, not discarded — five objects got this wrong
    // until 0.47 and two byte strings decoded to one object.
    let mut m = match setup.to_value() { Value::Map(m) => m, _ => unreachable!() };
    m.insert(0u64, Value::Uint(3));
    v.push(json!({
        "name": "object_wrong_type_code_refused",
        "why": "an object declaring another type must not decode. Until 0.47 five objects read \
                the type field and threw it away, so two byte strings differing only in their \
                declared type produced one object — both verifying, both hashing differently.",
        "object_type": "ESCROW_SETUP",
        "object_hex": hex(&Value::Map(m).encode()),
        "expect": { "ok": false, "reject_code": RejectCode::Malformed as u8, "reject_name": "Malformed" },
        "hint": "type field says 3 (ACCEPT)"
    }));
    v
}

/// §8.2 and §17.4/§17.5 contract logic, made language-neutral.
///
/// `object.roundtrip` proved two implementations encode these the same. It says
/// nothing about whether they *decide* the same, and the decisions are where the
/// money is: an out-of-order ceremony message, an arbiter nobody vouched for, a
/// release to an address that is not a party, a bond attestation from the
/// future, a double-spend claim with no evidence.
/// §16.9 / §16.10 / §16.12 — record-based cards, inbox details, and message
/// chains.
///
/// These exist because §7.5's memos shipped with *no* cross-implementation
/// coverage: the object runner is generic over fields, so no vector had ever
/// put text on the wire and asked two decoders to agree. Cards now carry a
/// record key, which is text, so that gap would have reopened here.
fn contact_cases() -> Vec<J> {
    use ducat_core::contact::*;
    let mut v = Vec::new();

    const KEY: &str = "VLD0:Qaq9to-SDiN5usgs4gxdmFDo-V8OH_xztMIcv8QWqPA:qqSJ7OtADw5pvvKc1Z3uISLsq-uOl1gmmL3r_8vhvkA";

    let base = ContactCard {
        version: 1, suite: 1,
        persona: vec![0xAA; 32],
        inbox_key: KEY.into(),
        writer_public: vec![0xBB; 32],
        display_name: Some("kara".into()),
        expiry: 1_800_000_000,
    };

    let mut card = |name: &str, why: &str, c: &ContactCard, bad: Option<(RejectCode, &str)>| {
        let hex_body = hex(&c.to_value().encode());
        v.push(match bad {
            None => json!({ "name": name, "why": why, "card_hex": hex_body,
                            "expect": { "ok": true, "reencodes_to_hex": hex_body } }),
            Some((code, hint)) => json!({ "name": name, "why": why, "card_hex": hex_body,
                            "expect": { "ok": false, "reject": format!("{:?}", code).to_uppercase(), "hint": hint } }),
        });
    };
    card("card_valid", "A card with every field present decodes and re-encodes byte-identically.", &base, None);
    card("card_no_display_name", "A name is optional; omitting the key is how you decline to assert one.",
        &ContactCard { display_name: None, ..base.clone() }, None);
    card("card_display_name_empty",
        "A present-but-empty name is a second encoding of `absent`. §18.1 admits one encoding per meaning, so this is MALFORMED rather than a name of zero length.",
        &ContactCard { display_name: Some(String::new()), ..base.clone() },
        Some((RejectCode::Malformed, "empty text field")));
    card("card_display_name_too_long",
        "Names are bounded in characters, not bytes, so the bound does not silently shorten scripts needing more than one byte per character.",
        &ContactCard { display_name: Some("x".repeat(MAX_DISPLAY_NAME_CHARS + 1)), ..base.clone() },
        Some((RejectCode::Malformed, "text over bound")));
    card("card_display_name_multibyte",
        "Thirty-two characters of Japanese is 96 bytes. A byte bound would reject a name within the stated limit, which is why the limit counts characters.",
        &ContactCard { display_name: Some("あ".repeat(MAX_DISPLAY_NAME_CHARS)), ..base.clone() }, None);
    card("card_display_name_not_nfc",
        "A decomposed name is valid UTF-8 and non-canonical. Two encodings of one name would make H(object) depend on the sender's keyboard, so §18.1 requires NFC.",
        &ContactCard { display_name: Some("e\u{301}".into()), ..base.clone() },
        Some((RejectCode::Malformed, "text is not NFC-normalized")));
    card("card_inbox_key_too_long",
        "A record key is an address, and an address field long enough to hold a payload is a covert channel with a signature on it.",
        &ContactCard { inbox_key: "V".repeat(MAX_RECORD_KEY_CHARS + 1), ..base.clone() },
        Some((RejectCode::Malformed, "text over bound")));
    card("card_inbox_key_empty",
        "A card whose inbox is the empty string names nothing. Absent and empty must not both mean `no inbox`.",
        &ContactCard { inbox_key: String::new(), ..base.clone() },
        Some((RejectCode::Malformed, "empty text field")));

    let det = ContactDetails {
        version: 1, suite: 1,
        persona: vec![0xCC; 32],
        outbox_key: KEY.into(),
        prekey_bundle: vec![0xDD; 48],
        display_name: Some("sam".into()),
        payto: None,
        avatar: None, email: None, phone: None, signal: None, pronouns: None,
    };
    let mut detail = |name: &str, why: &str, d: &ContactDetails, bad: Option<(RejectCode, &str)>| {
        let hex_body = hex(&d.to_value().encode());
        v.push(match bad {
            None => json!({ "name": name, "why": why, "details_hex": hex_body,
                            "expect": { "ok": true, "reencodes_to_hex": hex_body } }),
            Some((code, hint)) => json!({ "name": name, "why": why, "details_hex": hex_body,
                            "expect": { "ok": false, "reject": format!("{:?}", code).to_uppercase(), "hint": hint } }),
        });
    };
    // §16.9's profile. Everything here rides the record, never the card: the
    // card is a QR code someone has to scan across a counter.
    const PNG1: &[u8] = &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 13];
    let profiled = ContactDetails {
        avatar: Some(PNG1.to_vec()),
        email: Some("sam.oconnor+ducat@example.co.uk".into()),
        phone: Some("14155550123".into()),
        signal: Some("sam_oc.42".into()),
        pronouns: Some(Pronouns::TheyThem),
        ..det.clone()
    };
    detail("details_with_profile",
        "A whole profile: a picture, ways to be reached, and how to be referred to. None of it is in the card — the card is a QR code scanned across a counter, and a profile does not fit in one. It arrives on the record afterwards, which is also why it can change without reissuing anything.",
        &profiled, None);
    detail("details_avatar_not_an_image",
        "Bytes handed to an image decoder on someone else's phone are one of the most reliably exploitable surfaces there is. A decoder should never have to guess what it was given.",
        &ContactDetails { avatar: Some(vec![0x00, 0x01, 0x02, 0x03]), ..profiled.clone() },
        Some((RejectCode::Malformed, "avatar must be PNG, JPEG or WebP")));
    detail("details_avatar_too_large",
        "An avatar is a thumbnail, not a file transfer, and it has to fit in the record beside everything else — a profile that does not fit is a contact nobody can reach.",
        &ContactDetails {
            avatar: Some(PNG1.iter().copied().cycle().take(MAX_AVATAR_BYTES + 1).collect()),
            ..profiled.clone()
        },
        Some((RejectCode::Malformed, "avatar over bound")));
    detail("details_avatar_empty",
        "Present-but-empty is a second spelling of 'no picture', and omitting the key is the first.",
        &ContactDetails { avatar: Some(Vec::new()), ..profiled.clone() },
        Some((RejectCode::Malformed, "empty avatar; omit the key")));
    detail("details_email_not_an_email",
        "These render as identity. An address with no domain is a claim about who someone is that no client checked.",
        &ContactDetails { email: Some("sam@localhost".into()), ..profiled.clone() },
        Some((RejectCode::Malformed, "not the shape of an email")));
    detail("details_email_with_control_characters",
        "The reason to validate rather than escape at the screen: a field that can hold control characters is a field that renders differently in every client that draws it.",
        &ContactDetails { email: Some("sam\u{202E}@example.com".into()), ..profiled.clone() },
        Some((RejectCode::Malformed, "not the shape of an email")));
    detail("details_phone_not_digits",
        "One number has a dozen spellings. Accepting all of them means two clients render it two ways and neither matches when somebody searches.",
        &ContactDetails { phone: Some("+1 (415) 555-0123".into()), ..profiled.clone() },
        Some((RejectCode::Malformed, "phone is digits only")));
    detail("details_signal_without_digits",
        "Signal's own shape is name.digits, and a username that cannot exist points at nobody.",
        &ContactDetails { signal: Some("sam_oc".into()), ..profiled.clone() },
        Some((RejectCode::Malformed, "signal is name.digits")));
    detail("details_without_pronouns",
        "A closed set, because this is drawn next to a name on a stranger's screen and free text there is a place to put a message. Absence is not a failure state — a person with none set renders like anyone else.",
        &ContactDetails { pronouns: None, ..profiled.clone() },
        None);
    detail("details_valid", "What each side writes into the contact inbox: who they are, where to leave things, and the keys to seal with.", &det, None);
    detail("details_no_name", "The name is optional here for the same reason it is on the card.",
        &ContactDetails { display_name: None, ..det.clone() }, None);
    detail("details_with_payto",
        "A contact may publish an address so they can be paid without asking first. Optional, and a real trade: a stored address is a reused address, and a reused address links every payment to that person on a public ledger.",
        &ContactDetails { payto: Some("5ApJU8bfJ2sb4eGHNSCcSjGH4SxMghLahdFoh3NKpkPYhJE3AC56oxFEFcX4Nj7DTD873X3pnwnWSfp1YUCsg6veAAvkwm9".into()), ..det.clone() }, None);
    detail("details_payto_empty",
        "Absent is how a contact declines to publish one. An empty string would be a second spelling of that.",
        &ContactDetails { payto: Some(String::new()), ..det.clone() },
        Some((RejectCode::Malformed, "empty text field")));

    detail("details_outbox_empty",
        "An empty outbox key would leave the other side with a contact it can never write to, reported as success.",
        &ContactDetails { outbox_key: String::new(), ..det.clone() },
        Some((RejectCode::Malformed, "empty text field")));

    for (name, why, seq) in [
        ("head_zero", "A log nobody has written to yet: the next message will be sequence 0.", 0u64),
        ("head_mid", "next_seq doubles as the count of messages ever written, which is what makes a gap detectable.", 42u64),
    ] {
        let h = LogHead { version: 1, suite: 1, next_seq: seq, prekey_bundle: None };
        let hex_body = hex(&h.to_value().encode());
        v.push(json!({ "name": name, "why": why, "head_hex": hex_body,
                       "expect": { "ok": true, "reencodes_to_hex": hex_body } }));
    }

    // The ring. Subkey 0 is the head, so an off-by-one here overwrites it with a
    // message and loses the whole log rather than one entry.
    for (name, why, seq, count, subkey, reachable_from) in [
        ("ring_first", "The first message goes to subkey 1, immediately after the head.", 0u64, 8u32, 1u32, 0u64),
        ("ring_last_slot", "Seven slots in an eight-subkey record, so sequence 6 is the last before wrapping.", 6, 8, 7, 0),
        ("ring_wraps", "Sequence 7 wraps back to subkey 1 and overwrites sequence 0.", 7, 8, 1, 1),
        ("ring_wraps_twice", "Wrapping is modular, not a one-off.", 14, 8, 1, 8),
    ] {
        v.push(json!({
            "name": name, "why": why,
            "seq": seq, "subkey_count": count,
            "expect": { "ok": true, "subkey": subkey, "oldest_readable": reachable_from }
        }));
    }

    let m0 = Message { version: 1, suite: 1, seq: 0, prev: [0u8; 32], body: "hey".into(), timestamp: 1_700_000_000, kind: MessageKind::Text, amount_pxmr: None, txid: None, payto: None, items: Vec::new(), tax_pxmr: None };
    let m1 = Message { version: 1, suite: 1, seq: 1, prev: m0.link(), body: "you around?".into(), timestamp: 1_700_000_060, kind: MessageKind::Text, amount_pxmr: None, txid: None, payto: None, items: Vec::new(), tax_pxmr: None };
    let m2 = Message { version: 1, suite: 1, seq: 2, prev: m1.link(), body: "here's the 20 back".into(), timestamp: 1_700_000_120, kind: MessageKind::Text, amount_pxmr: None, txid: None, payto: None, items: Vec::new(), tax_pxmr: None };

    let mut chain = |name: &str, why: &str, msgs: &[&Message], fail_at: Option<(usize, RejectCode, &str)>| {
        v.push(json!({
            "name": name, "why": why,
            "messages_hex": msgs.iter().map(|m| hex(&m.to_value().encode())).collect::<Vec<_>>(),
            "expect": match fail_at {
                None => json!({ "ok": true }),
                Some((i, code, hint)) => json!({ "ok": false, "fails_at_index": i,
                    "reject": format!("{:?}", code).to_uppercase(), "hint": hint }),
            }
        }));
    };
    chain("chain_three_messages", "A well-formed thread: each message links to the one before and the sequence has no gaps.", &[&m0, &m1, &m2], None);
    chain("chain_first_must_link_to_nothing",
        "The opening message has no predecessor, so its link is thirty-two zero bytes. Anything else claims a history that does not exist.",
        &[&Message { prev: [0x99; 32], ..m0.clone() }],
        Some((0, RejectCode::CommitMismatch, "first message links to nothing")));
    chain("chain_gap_refused",
        "A missing message is refused rather than stored around it: a thread that silently skips one shows a conversation that did not happen.",
        &[&m0, &m2],
        Some((1, RejectCode::StateViolation, "sequence gap")));
    chain("chain_substituted_message",
        "The sequence fits but the link does not, which is what a removed-and-replaced message looks like. This is the case the chain exists to catch.",
        &[&m0, &Message { body: "different".into(), ..m1.clone() }, &m2],
        Some((2, RejectCode::CommitMismatch, "link does not follow")));
    chain("chain_replayed_message",
        "Re-delivering a message already accepted is a stale sequence number, not a fresh one.",
        &[&m0, &m1, &m1],
        Some((2, RejectCode::StateViolation, "sequence replay")));

    // §16.13 — money in a conversation.
    let mut money = |name: &str, why: &str, m: &Message, bad: Option<(RejectCode, &str)>| {
        let hex_body = hex(&m.to_value().encode());
        v.push(match bad {
            None => json!({ "name": name, "why": why, "payment_hex": hex_body,
                            "expect": { "ok": true, "reencodes_to_hex": hex_body } }),
            Some((code, hint)) => json!({ "name": name, "why": why, "payment_hex": hex_body,
                            "expect": { "ok": false, "reject": format!("{:?}", code).to_uppercase(), "hint": hint } }),
        });
    };
    let base_pay = Message {
        version: 1, suite: 1, seq: 0, prev: [0u8; 32],
        body: "for the coffee".into(), timestamp: 1_700_000_000,
        kind: MessageKind::PaymentRequest, amount_pxmr: Some(21_000_000_000), txid: None,
        payto: None, items: Vec::new(), tax_pxmr: None,
    };
    money("payment_request", "Asking a contact for an exact amount. It carries no authority — the payer still decides at §15.5's confirm screen.", &base_pay, None);
    money("payment_sent",
        "A notice that money was sent, with a transaction to look for. Advisory: §17.5 verifies by finding the output, never by believing the note.",
        &Message { kind: MessageKind::PaymentSent, txid: Some(vec![0x77; 32]), ..base_pay.clone() }, None);
    money("payment_without_amount",
        "A payment screen with a blank where the number goes. Refused rather than rendered.",
        &Message { amount_pxmr: None, ..base_pay.clone() },
        Some((RejectCode::Malformed, "payment needs an amount")));
    money("text_with_amount",
        "An amount on a text message is a number nothing will honour, which is worse than no number.",
        &Message { kind: MessageKind::Text, amount_pxmr: Some(1), ..base_pay.clone() },
        Some((RejectCode::Malformed, "text must not carry an amount")));
    money("request_with_payto",
        "A request names where to pay, so the payer needs nothing from a contact record that may be stale — and so the address is not stored and reused, which would link every payment to that person on a public ledger.",
        &Message { payto: Some("5ApJU8bfJ2sb4eGHNSCcSjGH4SxMghLahdFoh3NKpkPYhJE3AC56oxFEFcX4Nj7DTD873X3pnwnWSfp1YUCsg6veAAvkwm9".into()), ..base_pay.clone() }, None);
    money("notice_with_payto",
        "Only a request names a destination. A notice doing so would be describing a payment it claims to have already made.",
        &Message { kind: MessageKind::PaymentSent, payto: Some("5ApJU8bf".into()), ..base_pay.clone() },
        Some((RejectCode::Malformed, "only a request names where to pay")));
    money("payto_too_long",
        "An address field long enough to hold a payload is a covert channel with a signature on it.",
        &Message { payto: Some("5".repeat(MAX_ADDRESS_CHARS + 1)), ..base_pay.clone() },
        Some((RejectCode::Malformed, "text over bound")));

    // Itemisation (§16.13). A bill, and the receipt for one.
    let drink = LineItem { description: "flat white".into(), amount_pxmr: 50_000_000_000 };
    let shoes = LineItem { description: "2 × shoes".into(), amount_pxmr: 300_000_000_000 };
    let billed = Message {
        amount_pxmr: Some(352_000_000_000),
        items: vec![drink.clone(), shoes.clone()],
        tax_pxmr: Some(2_000_000_000),
        ..base_pay.clone()
    };
    money("itemised_bill",
        "A bill that says what the money is for. The items plus tax add up to the amount, which is the property that makes an itemisation worth carrying — a breakdown nobody can check is a breakdown that can say anything.",
        &billed, None);
    money("itemised_receipt",
        "The same lines on a notice rather than a request: a receipt for a payment already made. The vendor issues it, and it remains their claim about what was bought — the chain records the amount and never the reason.",
        &Message { kind: MessageKind::PaymentSent, txid: Some(vec![0x77; 32]), payto: None, ..billed.clone() }, None);
    money("items_do_not_add_up",
        "The one way an itemised bill is worse than none: a breakdown next to a total it disagrees with looks like a check that was performed. It was not, so the message is refused.",
        &Message { amount_pxmr: Some(999_000_000_000), ..billed.clone() },
        Some((RejectCode::Malformed, "items and tax must equal the amount")));
    money("items_without_tax",
        "Tax is optional. With none, the items alone must equal the amount.",
        &Message { amount_pxmr: Some(350_000_000_000), tax_pxmr: None, ..billed.clone() }, None);
    money("tax_without_items",
        "Tax on nothing states a split of a total the message never breaks down, so the recipient has to take it on faith. Itemisation is only worth having when it is always arithmetic.",
        &Message { items: Vec::new(), tax_pxmr: Some(1), amount_pxmr: Some(1), ..base_pay.clone() },
        Some((RejectCode::Malformed, "tax needs items")));
    money("empty_item_list",
        "Present-but-empty is a second spelling of 'not itemised' and omitting the key is the first. §18.1 allows one.",
        &Message { items: Vec::new(), ..billed.clone() },
        Some((RejectCode::Malformed, "empty item list; omit the key")));
    money("item_without_description",
        "A line with an amount and no words is a number on a receipt with nothing to say what it bought.",
        &Message {
            amount_pxmr: Some(1),
            items: vec![LineItem { description: String::new(), amount_pxmr: 1 }],
            tax_pxmr: None,
            ..base_pay.clone()
        },
        Some((RejectCode::Malformed, "line item needs a description")));
    money("item_description_too_long",
        "A description field long enough to hold a payload is a covert channel on a receipt.",
        &Message {
            amount_pxmr: Some(1),
            items: vec![LineItem { description: "x".repeat(MAX_ITEM_CHARS + 1), amount_pxmr: 1 }],
            tax_pxmr: None,
            ..base_pay.clone()
        },
        Some((RejectCode::Malformed, "text over bound")));
    money("too_many_items",
        "A receipt is rendered on someone else's phone, so the length of the list it has to draw is not the sender's to choose without bound.",
        &Message {
            amount_pxmr: Some((MAX_ITEMS as u64) + 1),
            items: (0..=MAX_ITEMS)
                .map(|i| LineItem { description: format!("item {i}"), amount_pxmr: 1 })
                .collect(),
            tax_pxmr: None,
            ..base_pay.clone()
        },
        Some((RejectCode::Malformed, "too many items")));
    money("text_with_items",
        "A text message has no bill to itemise.",
        &Message { kind: MessageKind::Text, amount_pxmr: None, items: vec![drink.clone()], tax_pxmr: None, ..base_pay.clone() },
        Some((RejectCode::Malformed, "text has no bill")));
    money("items_overflow_the_amount",
        "Two lines that sum past u64 must not wrap into a total that matches.",
        &Message {
            amount_pxmr: Some(0),
            items: vec![
                LineItem { description: "a".into(), amount_pxmr: u64::MAX },
                LineItem { description: "b".into(), amount_pxmr: 1 },
            ],
            tax_pxmr: None,
            ..base_pay.clone()
        },
        Some((RejectCode::Malformed, "item amounts overflow")));

    money("itemised_receipt_from_the_payee",
        "A receipt is a different claim from a notice, and neither existing kind can make it: a vendor sending PAYMENT_SENT would be stating they sent money. This says 'I have your payment, and here is the breakdown it settles', and points at the transaction it acknowledges.",
        &Message {
            kind: MessageKind::Receipt,
            txid: Some(vec![0x77; 32]),
            payto: None,
            body: "thanks!".into(),
            ..billed.clone()
        }, None);
    money("receipt_without_amount",
        "A receipt for an unstated amount settles nothing.",
        &Message { kind: MessageKind::Receipt, amount_pxmr: None, txid: None, payto: None, items: Vec::new(), tax_pxmr: None, ..base_pay.clone() },
        Some((RejectCode::Malformed, "payment needs an amount")));
    money("receipt_with_payto",
        "Only a request names where to pay. A receipt doing so is asking again for money it says it already has.",
        &Message { kind: MessageKind::Receipt, payto: Some("5ApJU8bf".into()), ..base_pay.clone() },
        Some((RejectCode::Malformed, "only a request names where to pay")));

    money("request_with_txid",
        "A notice points at the transaction it made and a receipt at the one it acknowledges. A request pointing at either is claiming the payment it is simultaneously asking for.",
        &Message { txid: Some(vec![0x77; 32]), ..base_pay.clone() },
        Some((RejectCode::Malformed, "only a notice or receipt carries a txid")));


    v
}

fn contract_cases() -> Vec<J> {
    use ducat_core::bond::bucket_floor;
    use ducat_core::escrow::*;
    let mut v = Vec::new();
    const T0: u64 = 1_800_000_000;
    let eid = [0xE5u8; 32];

    // -- the ceremony (§2.5) ------------------------------------------------
    let setup = |round: u64, from: u8| {
        json!({ "round": round, "from_index": from, "info_hex": hex(&[0xAB; 64]) })
    };
    let ceremony = |name: &str, why: &str, steps: Vec<J>| {
        json!({ "name": name, "why": why, "escrow_id_hex": hex(&eid),
                "rounds_required": 2, "steps": steps })
    };
    let ok = |s: J| { let mut m = s.as_object().unwrap().clone();
        m.insert("expect".into(), json!({"ok": true})); J::Object(m) };
    let no = |s: J, code: RejectCode| { let mut m = s.as_object().unwrap().clone();
        m.insert("expect".into(), json!({"ok": false, "reject_code": code as u8,
            "reject_name": format!("{:?}", code)})); J::Object(m) };

    v.push(ceremony(
        "ceremony_two_rounds_converge",
        "a 2-of-3 ceremony closes in two rounds, each participant contributing once per round",
        vec![ok(setup(0, 0)), ok(setup(0, 1)), ok(setup(0, 2)),
             ok(setup(1, 0)), ok(setup(1, 1)), ok(setup(1, 2))],
    ));
    v.push(ceremony(
        "ceremony_out_of_order_round_refused",
        "§2.5: RetoSwap — this exact structure in production — was drained of ~$2.7M by a \
         forged, out-of-order message overwriting settled state. Round 1 arriving while \
         round 0 is open has that shape whatever the payload says.",
        vec![ok(setup(0, 0)), no(setup(1, 1), RejectCode::StateViolation)],
    ));
    v.push(ceremony(
        "ceremony_duplicate_contribution_refused",
        "a second contribution from one participant in one round would revise state the \
         ceremony has already settled",
        vec![ok(setup(0, 0)), no(setup(0, 0), RejectCode::Replay)],
    ));
    v.push(ceremony(
        "ceremony_rejects_contributions_after_completion",
        "a finished ceremony has nothing left to contribute to",
        vec![ok(setup(0, 0)), ok(setup(0, 1)), ok(setup(0, 2)),
             ok(setup(1, 0)), ok(setup(1, 1)), ok(setup(1, 2)),
             no(setup(2, 0), RejectCode::StateViolation)],
    ));

    // -- agreement (§8.2) ---------------------------------------------------
    let addr = "53multisigaddress";
    let rdy = |from: u8, address: &str, arbiter: &str| {
        json!({"from_index": from, "ms_address": address, "threshold": 2, "total": 3,
               "arbiter": arbiter})
    };
    let ready_case = |name: &str, why: &str, reports: Vec<J>, expect: J| {
        json!({"name": name, "why": why, "escrow_id_hex": hex(&eid),
               "trusted_arbiters": ["arbiter-key-1"], "reports": reports, "expect": expect})
    };
    v.push(ready_case(
        "ready_all_three_agree",
        "every participant must report what it formed, and the reports must match",
        vec![rdy(0, addr, "arbiter-key-1"), rdy(1, addr, "arbiter-key-1"), rdy(2, addr, "arbiter-key-1")],
        json!({"ok": true, "agreed_address": addr}),
    ));
    v.push(ready_case(
        "ready_divergent_address_refused",
        "three ceremonies can each succeed and form two different groups — the funds then \
         land in a wallet the payer holds no share of",
        vec![rdy(0, addr, "arbiter-key-1"), rdy(1, addr, "arbiter-key-1"),
             rdy(2, "53someotheraddress", "arbiter-key-1")],
        json!({"ok": false, "reject_code": RejectCode::CommitMismatch as u8,
               "reject_name": "CommitMismatch"}),
    ));
    v.push(ready_case(
        "ready_untrusted_arbiter_refused",
        "§2.5's other half: the arbiter comes from the market descriptor, never from a \
         message — the forged message in the real exploit was well-formed",
        vec![rdy(0, addr, "attacker-key"), rdy(1, addr, "attacker-key"), rdy(2, addr, "attacker-key")],
        json!({"ok": false, "reject_code": RejectCode::UntrustedArbiterSet as u8,
               "reject_name": "UntrustedArbiterSet"}),
    ));
    v.push(ready_case(
        "ready_silent_participant_refused",
        "a participant that reported nothing has agreed to nothing",
        vec![rdy(0, addr, "arbiter-key-1"), rdy(1, addr, "arbiter-key-1")],
        json!({"ok": false, "reject_code": RejectCode::PolicyRefused as u8,
               "reject_name": "PolicyRefused"}),
    ));

    // -- release (§8.2) -----------------------------------------------------
    let rel_case = |name: &str, why: &str, to: &str, amount: u64, expect: J| {
        json!({"name": name, "why": why, "escrow_id_hex": hex(&eid),
               "escrowed_pxmr": 800_000_000u64,
               "allowed_destinations": ["seller-payout", "buyer-refund"],
               "to": to, "amount_pxmr": amount, "expect": expect})
    };
    v.push(rel_case("release_to_a_party_is_allowed",
        "the ordinary close of an escrow", "seller-payout", 800_000_000, json!({"ok": true})));
    v.push(rel_case("release_partial_is_allowed",
        "a ruling can award less than the whole", "buyer-refund", 400_000_000, json!({"ok": true})));
    v.push(rel_case("release_to_a_stranger_refused",
        "the check a rushed implementation drops, because the happy path never exercises it: \
         both parties co-signing a release to the seller looks identical whether or not the \
         destination was ever constrained",
        "attacker-addr", 800_000_000,
        json!({"ok": false, "reject_code": RejectCode::PolicyRefused as u8,
               "reject_name": "PolicyRefused"})));
    v.push(rel_case("release_over_the_balance_refused",
        "an escrow cannot pay out more than it holds", "seller-payout", 900_000_000,
        json!({"ok": false, "reject_code": RejectCode::PriceMismatch as u8,
               "reject_name": "PriceMismatch"})));
    v.push(rel_case("release_of_zero_refused",
        "a release of zero moves nothing and closes nothing", "seller-payout", 0,
        json!({"ok": false, "reject_code": RejectCode::PriceMismatch as u8,
               "reject_name": "PriceMismatch"})));

    // -- bond_proof (§17.4, §17.8) -----------------------------------------
    let bond_case = |name: &str, why: &str, bucket: u64, amount: u64, issued: u64,
                     arbiter_set: &str, fare: u64, now: u64, expect: J| {
        json!({"name": name, "why": why, "capacity_bucket": bucket,
               "bond_amount_pxmr": amount, "issued": issued,
               "arbiter_set_id_hex": arbiter_set, "fare_pxmr": fare, "now": now,
               "max_age_s": 300, "trusted_arbiter_sets": [hex(&[0xA5u8; 32])],
               "expect": expect})
    };
    let good_set = hex(&[0xA5u8; 32]);
    v.push(bond_case("bond_fresh_and_sufficient", "the ordinary case",
        bucket_floor(60_000_000_000), 100_000_000_000, T0, &good_set,
        20_000_000_000, T0 + 30, json!({"ok": true})));
    v.push(bond_case("bond_stale_refused",
        "a bond proof is a claim about a balance that moves, so an old one says nothing",
        bucket_floor(60_000_000_000), 100_000_000_000, T0, &good_set,
        1_000, T0 + 400, json!({"ok": false, "reject_code": RejectCode::Expired as u8,
            "reject_name": "Expired"})));
    v.push(bond_case("bond_from_the_future_refused",
        "a proof dated ahead of now is not fresh, it is wrong — skew is tolerated in one \
         direction only",
        bucket_floor(60_000_000_000), 100_000_000_000, T0 + 10_000, &good_set,
        1_000, T0, json!({"ok": false, "reject_code": RejectCode::Expired as u8,
            "reject_name": "Expired"})));
    v.push(bond_case("bond_exact_balance_is_not_a_bucket",
        "§17.8: an arbitrary integer here defeats bucketing entirely — a rider could publish \
         their balance exactly and call it a ladder value",
        49_999_999_999, 100_000_000_000, T0, &good_set,
        1_000, T0, json!({"ok": false, "reject_code": RejectCode::Malformed as u8,
            "reject_name": "Malformed"})));
    v.push(bond_case("bond_capacity_above_the_bond_refused",
        "capacity is what remains of the bond, so a capacity above it is incoherent — and it \
         is the direction a liar benefits from",
        100_000_000_000, 50_000_000_000, T0, &good_set,
        1_000, T0, json!({"ok": false, "reject_code": RejectCode::InsufficientCapacity as u8,
            "reject_name": "InsufficientCapacity"})));
    v.push(bond_case("bond_untrusted_arbiter_set_refused",
        "§2.5: the arbiter set is named by the market, not by the party who benefits from a \
         friendly one",
        bucket_floor(60_000_000_000), 100_000_000_000, T0, &hex(&[0xFFu8; 32]),
        1_000, T0, json!({"ok": false, "reject_code": RejectCode::UntrustedArbiterSet as u8,
            "reject_name": "UntrustedArbiterSet"})));

    // -- slash claims (§17.5) ----------------------------------------------
    let slash = |name: &str, why: &str, reason: u8, key_image: Option<&str>,
                 elapsed: u64, claim: u64, expect: J| {
        let mut m = serde_json::Map::new();
        m.insert("name".into(), json!(name));
        m.insert("why".into(), json!(why));
        m.insert("reason".into(), json!(reason));
        if let Some(k) = key_image { m.insert("key_image_hex".into(), json!(k)); }
        m.insert("elapsed_blocks".into(), json!(elapsed));
        m.insert("cure_blocks".into(), json!(20));
        m.insert("claim_pxmr".into(), json!(claim));
        m.insert("agreed_pxmr".into(), json!(21_000_000_000u64));
        m.insert("expect".into(), expect);
        J::Object(m)
    };
    v.push(slash("slash_cure_window_not_expired",
        "non-confirmation is usually a fee or propagation problem, not fraud — the window \
         exists so an honest payer can re-broadcast",
        1, None, 19, 21_000_000_000,
        json!({"ok": false, "reject_code": RejectCode::PolicyRefused as u8,
               "reject_name": "PolicyRefused"})));
    v.push(slash("slash_cure_window_expired",
        "the boundary is inclusive", 1, None, 20, 21_000_000_000, json!({"ok": true})));
    v.push(slash("slash_double_spend_needs_its_evidence",
        "this reason skips the waiting period, which makes it the one worth forging — so it \
         is the one that must carry the conflicting key image",
        2, None, 0, 21_000_000_000,
        json!({"ok": false, "reject_code": RejectCode::Malformed as u8,
               "reject_name": "Malformed"})));
    v.push(slash("slash_double_spend_with_evidence_skips_the_cure_window",
        "a conflicting key image is on-chain and self-authenticating",
        2, Some(&hex(&[0x5Au8; 32])), 0, 21_000_000_000, json!({"ok": true})));
    v.push(slash("slash_claim_over_the_agreed_amount_refused",
        "a claim exceeding what was agreed is a claimant helping themselves",
        1, None, 30, 21_000_000_001,
        json!({"ok": false, "reject_code": RejectCode::PriceMismatch as u8,
               "reject_name": "PriceMismatch"})));
    v
}

fn main() -> std::io::Result<()> {
    // Anchored to the crate, not to wherever this was run from. As a bare
    // `../vectors` it silently wrote a complete, valid vector set *outside the
    // repository* when invoked from the workspace root — and then the suite
    // passed, because it was still checking the stale files it had not
    // overwritten. A generator that can write the right bytes to the wrong
    // place is worse than one that fails.
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crate has a parent")
        .join("vectors")
        .join(format!("v{}", VECTOR_SET_VERSION));
    std::fs::create_dir_all(&dir)?;

    let files: [(&str, Vec<J>); 9] = [
        ("contact", contact_cases()),
        ("codec", codec_cases()),
        ("signing", signing_cases()),
        ("state", state_cases()),
        ("negotiate", negotiate_cases()),
        ("transcript", transcript_cases()),
        ("backup", backup_cases()),
        ("object", object_cases()),
        ("contract", contract_cases()),
    ];

    // Normalize and route. A case's authored category is not necessarily the
    // file it belongs in — the two commitment cases were living inside
    // negotiate.json, which is one of the things §18.11 recorded.
    let mut by_file: std::collections::BTreeMap<&str, Vec<J>> = Default::default();
    for (category, cases) in files {
        for c in cases {
            let (file, norm) = normalize(category, c);
            by_file.entry(file).or_default().push(norm);
        }
    }

    let mut counts = Map::new();
    for (name, cases) in &by_file {
        counts.insert(name.to_string(), json!(cases.len()));
        let body = json!({ "category": name, "cases": cases });
        std::fs::write(
            dir.join(format!("{}.json", name)),
            serde_json::to_string_pretty(&body)? + "\n",
        )?;
    }

    let total: usize = by_file.values().map(|c| c.len()).sum();
    let manifest = json!({
        "vector_set": VECTOR_SET_VERSION,
        "protocol_draft": PROTOCOL_DRAFT,
        "generated_by": "ducat-core examples/gen_vectors.rs",
        "deterministic": "all keys derive from fixed seeds; no clock or RNG is consulted, so regeneration is byte-identical on an unchanged implementation",
        "reject_codes_are": "§18.5 wire codes. Two clients must agree an input is MALFORMED; they need not agree on which internal decoder rule said so. The non-normative `hint` field names the rule.",
        "total_cases": total,
        "counts": counts,
        "covers": {
            "18.9(1) encoding round-trips and integer boundaries": true,
            "18.9(2) per-object-type valid plus invalid mutations": true,
            "18.9(4) full per-profile transcripts": "xfer/1, pos/1, ride/1",
            "18.9(3) cross-context signature replay": true,
            "18.9(5) failure paths and the single-sided receipt": true,
            "18.9(6) negotiation including a downgrade attempt": true,
            "18.9(7) piconero amounts that defeat a float implementation": true,
            "4.3 backup format known-answer and rejection cases": true,
            "8.2 / 17.4 / 17.5 escrow and fast-settle object encodings": true,
            "8.2 / 17.4 / 17.5 contract logic": "ceremony ordering (§2.5's out-of-order and duplicate cases), participant agreement, release destinations, bond freshness and ladder membership, slash-claim cure windows — decided by core and by a second implementation written from the spec",
            "every case carries a `kind` discriminator and validates against schema.json": true
        },
        "does_not_yet_cover": {
            "escrow and fast/1 end-to-end transcripts": "no *vector* drives these, so they remain outside the language-neutral suite. Both now run end to end in harness/ against real Veilid routes and real stagenet settlement — fast/1 including a bond_proof and mempool-visibility acceptance, escrow including an ordered three-party ceremony, a refused replay, address agreement, and a destination-constrained release. Contract logic is in core/tests/escrow.rs.",
            "suite 2 key agreement": "only signatures are covered; X25519/ECDH is unimplemented",
            "O21 caveat": "a vector set validated by one implementation encodes that implementation's bugs. A second implementation (conformance/ducat_check.py) now runs these and agrees at 104/104, having found three spec defects on its first pass — but it shares an author with the reference, so O21 stays open until someone who has never read core/ runs them.",
            "multisig backup": "§4.3.3 — escrow shares are carried in the bundle as opaque key-file bytes, but no vector exercises one: a share is a Monero wallet key file, not a DUCAT object, so there is nothing language-neutral to assert. Verified against stagenet instead (monero-spike/REPORT.md)."
        }
    });
    std::fs::write(
        dir.join("manifest.json"),
        serde_json::to_string_pretty(&manifest)? + "\n",
    )?;

    println!("wrote {} cases to {}", total, dir.display());
    for (name, cases) in &by_file {
        println!("  {:<12} {}", name, cases.len());
    }
    Ok(())
}
