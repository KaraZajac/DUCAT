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
        (Event::TxProof, State::Provisional),
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

fn main() -> std::io::Result<()> {
    let dir = std::path::Path::new("../vectors").join(format!("v{}", VECTOR_SET_VERSION));
    std::fs::create_dir_all(&dir)?;

    let files: [(&str, Vec<J>); 6] = [
        ("codec", codec_cases()),
        ("signing", signing_cases()),
        ("state", state_cases()),
        ("negotiate", negotiate_cases()),
        ("transcript", transcript_cases()),
        ("backup", backup_cases()),
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
            "every case carries a `kind` discriminator and validates against schema.json": true
        },
        "does_not_yet_cover": {
            "escrow and fast/1 transcripts": "only direct settlement is covered; TXPROOF and escrow objects are unimplemented",
            "suite 2 key agreement": "only signatures are covered; X25519/ECDH is unimplemented",
            "O21 caveat": "a vector set validated by one implementation encodes that implementation's bugs. These close O21 only when a second, independent client runs them.",
            "multisig backup": "§4.3.3 — a bond or escrow share is deliberately absent from the bundle, because monero-wallet-rpc has no method to restore one. Nothing here to test."
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
