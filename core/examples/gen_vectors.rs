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
/// The draft these vectors describe, read from the document itself.
///
/// This was the string "0.42", written once and never again — so every
/// manifest published since has told a third-party implementer that the
/// vectors they are about to write code against describe a protocol
/// forty-six drafts old. The one artifact this project points outsiders at
/// was the one thing not checking itself.
fn protocol_draft() -> String {
    let spec = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crate has a parent")
        .join("ducat-protocol.md");
    let text = std::fs::read_to_string(spec).expect("the spec is beside the crate");
    let at = text.find("**Draft ").expect("the spec states its draft");
    // Up to the first whitespace, so "1.0.0-rc1" survives whole — a digit
    // split dropped the rc suffix, and an rc that publishes itself as the
    // final version is lying about the one thing an rc exists to say.
    text[at + "**Draft ".len()..]
        .split_whitespace()
        .next()
        .expect("a draft number follows")
        .trim_end_matches("**")
        .to_string()
}

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
        contacts: Vec::new(), personas: Vec::new(), prekey_signed_secret: None, prekey_one_time: Vec::new(), prekey_next_id: 0, app_state: None,
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
            if obj.contains_key("sealed_hex") {
                ("contact", "board.sealed")
            } else if obj.contains_key("tip_height") {
                ("contact", "board.beacon_window")
            } else if obj.contains_key("verdict_tip") {
                ("contact", "board.beacon_verdict")
            } else if obj.contains_key("frame_sealed_hex") {
                ("contact", "position.frame")
            } else if obj.contains_key("messages_hex") {
                ("contact", "message.chain")
            } else if obj.contains_key("details_hex") {
                ("contact", "contact.details")
            } else if obj.contains_key("payment_hex") {
                ("contact", "message.payment")
            } else if obj.contains_key("head_hex") {
                ("contact", "log.head")
            } else if obj.contains_key("notice_hex") {
                ("contact", "hail.notice")
            } else if obj.contains_key("listing_hex") {
                ("contact", "rental.listing")
            } else if obj.contains_key("subkey_count") {
                ("contact", "log.ring")
            } else if obj.contains_key("shard") {
                ("contact", "stand.shard")
            } else if obj.contains_key("epoch") {
                ("contact", "stand.epoch")
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
        car_model: None,
        car_color: None,
        plate: None,
        purpose: None,
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
        car_model: None,
        car_color: None,
        plate: None,
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
        &ContactDetails { pronouns: None,
        car_model: None,
        car_color: None,
        plate: None, ..profiled.clone() },
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

    detail("details_with_car",
        "A driver's identity at the curb (§15.12): model, colour, plate — claims like the rest of the profile, and the rider's check is the bumper.",
        &ContactDetails {
            car_model: Some("Toyota Corolla".into()),
            car_color: Some("blue".into()),
            plate: Some("KAR-4242".into()),
            ..det.clone()
        }, None);
    detail("details_plate_control_chars",
        "A plate renders beside a name on a stranger's screen; control characters in it are a message, not a plate.",
        &ContactDetails { plate: Some("AB\u{7}123".into()), ..det.clone() },
        Some((RejectCode::Malformed, "plate is short plain text")));
    detail("details_car_model_too_long",
        "Twenty-four characters names any car; more is advertising space.",
        &ContactDetails { car_model: Some("x".repeat(25)), ..det.clone() },
        Some((RejectCode::Malformed, "text too long")));

    detail("details_outbox_empty",
        "An empty outbox key would leave the other side with a contact it can never write to, reported as success.",
        &ContactDetails { outbox_key: String::new(), ..det.clone() },
        Some((RejectCode::Malformed, "empty text field")));

    // A pronouns code nobody has a word for.
    //
    // Built by hand, because the field is an enum in this implementation and
    // cannot hold one — which is precisely why it needed a vector: the closed
    // set had an "absent" case and no "wrong" case, so its *size* was never
    // pinned and a second implementation could have picked its own.
    {
        let ducat_core::cbor::Value::Map(mut m) = det.to_value() else { unreachable!() };
        m.insert(ducat_core::wire::f::DET_PRONOUNS, ducat_core::cbor::Value::Uint(7));
        v.push(json!({ "name": "details_unknown_pronouns",
            "why": "Seven. The set is closed at six on purpose — this is drawn beside a name on a stranger's screen, and free text there is a place to put a message — so a code outside it is refused rather than rendered as a number.",
            "details_hex": hex(&ducat_core::cbor::Value::Map(m).encode()),
            "expect": { "ok": false, "reject": "MALFORMED", "hint": "unknown pronouns code" } }));
    }

    for (name, why, seq) in [
        ("head_zero", "A log nobody has written to yet: the next message will be sequence 0.", 0u64),
        ("head_mid", "next_seq doubles as the count of messages ever written, which is what makes a gap detectable.", 42u64),
    ] {
        let h = LogHead { version: 1, suite: 1, next_seq: seq, prekey_bundle: None, read_up_to: None, ring: None };
        let hex_body = hex(&h.to_value().encode());
        v.push(json!({ "name": name, "why": why, "head_hex": hex_body,
                       "expect": { "ok": true, "reencodes_to_hex": hex_body } }));
    }

    // §16.16's watermark and §16.12's variable ring.
    {
        let mut hcase = |name: &str, why: &str, h: &LogHead, bad: Option<(RejectCode, &str)>| {
            let hex_body = hex(&h.to_value().encode());
            v.push(match bad {
                None => json!({ "name": name, "why": why, "head_hex": hex_body,
                                "expect": { "ok": true, "reencodes_to_hex": hex_body } }),
                Some((code, hint)) => json!({ "name": name, "why": why, "head_hex": hex_body,
                                "expect": { "ok": false, "reject": format!("{:?}", code).to_uppercase(), "hint": hint } }),
            });
        };
        let base = LogHead { version: 1, suite: 1, next_seq: 9, prekey_bundle: None, read_up_to: None, ring: None };
        hcase("head_with_read_watermark",
            "A read receipt as a head field rather than a message: the head is rewritten constantly anyway, so the watermark costs no ring slot, no prekey and no chain entry. Absent means receipts are off, which is the default — when a message was read is behavioural data and leaves the device only by opt-in.",
            &LogHead { read_up_to: Some(7), ..base.clone() }, None);
        hcase("head_with_ring",
            "The ring size travels on the head so it can change: eight was sized for text, and reactions and receipts multiply message count. Readers take the ring from the head, never from a constant — the failure of a mismatch is reading the wrong slot and refusing a valid thread.",
            &LogHead { ring: Some(32), ..base.clone() }, None);
        hcase("head_ring_default_must_be_omitted",
            "Eight is the default and is encoded by omission — one meaning, one encoding (§18.1).",
            &LogHead { ring: Some(8), ..base.clone() },
            Some((RejectCode::Malformed, "default ring is omitted")));
        hcase("head_ring_too_small",
            "A ring needs a head and at least one slot.",
            &LogHead { ring: Some(1), ..base.clone() },
            Some((RejectCode::Malformed, "ring out of range")));
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

    // §15.12's overflow ladder: the shard names both sides construct
    // independently. A writer and a reader disagreeing on a name are standing
    // at different corners, so the format is a vector, not a convention.
    for (name, why, base, shard, expect) in [
        ("shard_zero_is_bare",
         "Shard 0 is the bare stand name — deployed boards from 0.83 stay valid, and a quiet cell costs exactly one read.",
         "geo:u4pruy", 0u32, Some("geo:u4pruy")),
        ("shard_one",
         "The first overflow board: decimal suffix, no padding — a padded and an unpadded spelling would be two different record keys for one name.",
         "geo:u4pruy", 1, Some("geo:u4pruy-1")),
        ("shard_top",
         "The tallest the ladder goes: 16 shards of 8 slots bounds a cell at 128 concurrent notices and a sweep at 16 reads.",
         "geo:u4pruy", 15, Some("geo:u4pruy-15")),
        ("shard_past_cap",
         "Past the cap, density has outgrown the cell; the answer is a finer geohash, not a taller ladder.",
         "geo:u4pruy", 16, None),
        ("shard_of_nothing",
         "A stand needs a name; a shard of the empty string is a board nobody meant to make.",
         "", 0, None),
    ] {
        v.push(json!({
            "name": name, "why": why,
            "base": base, "shard": shard,
            "expect": match expect {
                Some(n) => json!({ "ok": true, "board": n }),
                None => json!({ "ok": false, "reject": "MALFORMED",
                                "hint": "ladder cap or empty base" }),
            }
        }));
    }

    // §15.12's generation: the same argument as the ladder, one level up. A
    // board's write key is public, so anyone can freeze a slot for ever by
    // writing it at the maximum sequence; the epoch in the name is what lets a
    // poisoned cell be abandoned instead of lost. Both sides compute it from a
    // clock and a cell, so the spelling is a vector rather than a convention.
    for (name, why, base, epoch, expect) in [
        ("epoch_zero",
         "The first generation is spelled like any other — no special case for the beginning of time.",
         "geo:u4pruy", 0u64, Some("geo:u4pruy@0")),
        ("epoch_named",
         "Decimal, unpadded, `@` before the shard suffix — a full board name reads `<cell>@<epoch>-<shard>`.",
         "geo:u4pruy", 3021, Some("geo:u4pruy@3021")),
        ("epoch_listing_board",
         "Listings rotate on the same clock as hails; the prefix in front of the cell changes nothing.",
         "local:u4pru", 3021, Some("local:u4pru@3021")),
        ("epoch_of_nothing",
         "A stand needs a name before it can have a generation.",
         "", 3021, None),
        ("epoch_already_stamped",
         "Re-stamping a name that already names a generation would compute a board nobody else does — and would move a poster off the board its own notice is on.",
         "geo:u4pruy@3021", 3022, None),
    ] {
        v.push(json!({
            "name": name, "why": why,
            "base": base, "epoch": epoch,
            "expect": match expect {
                Some(n) => json!({ "ok": true, "board": n }),
                None => json!({ "ok": false, "reject": "MALFORMED",
                                "hint": "empty base or already stamped" }),
            }
        }));
    }

    let m0 = Message { version: 1, suite: 1, seq: 0, prev: [0u8; 32], body: "hey".into(), timestamp: 1_700_000_000, kind: MessageKind::Text, amount_pxmr: None, txid: None, payto: None, items: Vec::new(), tax_pxmr: None, re_seq: None, re_own: false, eta_secs: None, payload: None, round: None, ceremony_id: None, attachment: None, position: None, group_id: None, group_seq: None, group_re_sender: None, group_re_seq: None };
    let m1 = Message { version: 1, suite: 1, seq: 1, prev: m0.link(), body: "you around?".into(), timestamp: 1_700_000_060, kind: MessageKind::Text, amount_pxmr: None, txid: None, payto: None, items: Vec::new(), tax_pxmr: None, re_seq: None, re_own: false, eta_secs: None, payload: None, round: None, ceremony_id: None, attachment: None, position: None, group_id: None, group_seq: None, group_re_sender: None, group_re_seq: None };
    let m2 = Message { version: 1, suite: 1, seq: 2, prev: m1.link(), body: "here's the 20 back".into(), timestamp: 1_700_000_120, kind: MessageKind::Text, amount_pxmr: None, txid: None, payto: None, items: Vec::new(), tax_pxmr: None, re_seq: None, re_own: false, eta_secs: None, payload: None, round: None, ceremony_id: None, attachment: None, position: None, group_id: None, group_seq: None, group_re_sender: None, group_re_seq: None };

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
        payto: None, items: Vec::new(), tax_pxmr: None, re_seq: None, re_own: false, eta_secs: None, payload: None, round: None, ceremony_id: None, attachment: None, position: None, group_id: None, group_seq: None, group_re_sender: None, group_re_seq: None,
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
        &Message { kind: MessageKind::Receipt, amount_pxmr: None, txid: None, payto: None, items: Vec::new(), tax_pxmr: None, re_seq: None, re_own: false, eta_secs: None, payload: None, round: None, ceremony_id: None, attachment: None, ..base_pay.clone() },
        Some((RejectCode::Malformed, "payment needs an amount")));
    money("receipt_with_payto",
        "Only a request names where to pay. A receipt doing so is asking again for money it says it already has.",
        &Message { kind: MessageKind::Receipt, payto: Some("5ApJU8bf".into()), ..base_pay.clone() },
        Some((RejectCode::Malformed, "only a request names where to pay")));

    // §16.14 — reactions.
    let react = Message {
        kind: MessageKind::Reaction, body: "🔥".into(),
        amount_pxmr: None, re_seq: Some(4),
        ..base_pay.clone()
    };
    money("reaction_valid",
        "An emoji about a message, named by sequence in the recipient's log. A message like any other — sealed, chained, sequenced — because a side-channel for reactions would be a second delivery path with its own bugs.",
        &react, None);
    money("reaction_to_own_message",
        "Reacting to one's own earlier message: the target sequence is in the sender's log, flagged by presence (§18.1 — one meaning, one encoding).",
        &Message { re_own: true, ..react.clone() }, None);
    money("reaction_without_target",
        "An emoji about nothing is a text message wearing the wrong kind.",
        &Message { re_seq: None, ..react.clone() },
        Some((RejectCode::Malformed, "a reaction names its target")));
    money("reaction_body_too_long",
        "The body is the emoji. Sixteen characters holds any emoji sequence; a paragraph is a message and should be one.",
        &Message { body: "x".repeat(17), ..react.clone() },
        Some((RejectCode::Malformed, "reaction body too long")));
    money("reaction_with_amount",
        "A reaction carries no money: an amount on an emoji is a payment nothing will match.",
        &Message { amount_pxmr: Some(1), ..react.clone() },
        Some((RejectCode::Malformed, "reaction carries no money")));
    // §16.14's reference, carrying a reply and the two money messages that
    // answer something.
    money("reply_to_a_message",
        "A text naming the message it answers. The field has carried \"this, about that one\" since reactions; a reply is the same claim with words in it. Nothing of the target is quoted — the reader holds the thread and resolves the sequence itself, so an unsend cannot be undone by the reply that followed it.",
        &Message { kind: MessageKind::Text, body: "the second one".into(), amount_pxmr: None, re_seq: Some(1), ..base_pay.clone() },
        None);
    money("reply_to_own_message",
        "Answering one's own earlier message, flagged the way a reaction flags it. People do this; only an accept forbids it, because accepting your own offer is a soliloquy.",
        &Message { kind: MessageKind::Text, body: "— meant the blue one".into(), amount_pxmr: None, re_seq: Some(1), re_own: true, ..base_pay.clone() },
        None);
    money("payment_names_its_request",
        "A payment saying which request it settles. Without this the only thread from a payment back to its bill was the amount, so two identical requests answered by one payment both read as paid. Still advisory — §17.5 verifies by finding the output; what the reference settles is which request the sender says it was for, a question the chain has never been able to answer.",
        &Message { kind: MessageKind::PaymentSent, txid: Some(vec![7u8; 32]), re_seq: Some(3), ..base_pay.clone() },
        None);
    money("receipt_names_its_request",
        "A receipt naming the request it receipts — `re_own`, because the party issuing the receipt is the party that sent the bill. Request, payment and receipt then form a stated chain rather than three messages a reader has to infer a relationship between.",
        &Message { kind: MessageKind::Receipt, payto: None, re_seq: Some(3), re_own: true, ..base_pay.clone() },
        None);
    money("target_on_a_ride_offer",
        "The allow-list is still an allow-list: a reaction, a retract and an accept must name a target, a reply and the two money messages may, and everything else may not.",
        &Message { kind: MessageKind::RideOffer, amount_pxmr: Some(1), re_seq: Some(1), ..base_pay.clone() },
        Some((RejectCode::Malformed, "this kind does not target another")));

    // §16.19 — small groups over pairwise threads.
    let in_group = Message {
        kind: MessageKind::Text, body: "who's bringing the ladder?".into(),
        amount_pxmr: None,
        group_id: Some(vec![0xAB; 16]), group_seq: Some(4),
        ..base_pay.clone()
    };
    money("group_text",
        "A word to the group: the same sealed body fans out into every member's pairwise thread, marked with the group and the sender's own counter — (sender, group_seq) is the one name every member can resolve, because the pairwise sequence differs in every thread the copy lands in.",
        &in_group, None);
    money("group_reply",
        "A reply inside a group names its target by the group reference — the sender's persona and their counter — never by thread sequence, which names a slot in one thread out of N.",
        &Message { body: "the tall one".into(), group_re_sender: Some(vec![0xCD; 32]), group_re_seq: Some(2), ..in_group.clone() },
        None);
    money("group_reaction",
        "An emoji about a group message, targeted the same way. The pairwise re_seq stays forbidden here: one meaning, one encoding (§18.1).",
        &Message { kind: MessageKind::Reaction, body: "🔥".into(), group_re_sender: Some(vec![0xCD; 32]), group_re_seq: Some(2), ..in_group.clone() },
        None);
    money("group_roster",
        "The membership, stated: the member list rides the payload, opaque at this layer like a ceremony round's. The creator's first roster is the invitation, and a member who adds someone sends the grown set to everyone — rosters only grow, so every view converges by union in any order.",
        &Message { kind: MessageKind::GroupRoster, body: "roster".into(), payload: Some(vec![0xEE; 40]), ..in_group.clone() },
        None);
    money("group_id_without_counter",
        "The group and the sender's counter travel together or not at all: a group message with no counter has no name any member can refer to.",
        &Message { group_seq: None, ..in_group.clone() },
        Some((RejectCode::Malformed, "group id and counter travel together")));
    money("group_ref_outside_group",
        "A group reference on a pairwise message points into a room the thread is not in.",
        &Message { kind: MessageKind::Text, amount_pxmr: None, group_re_sender: Some(vec![0xCD; 32]), group_re_seq: Some(2), ..base_pay.clone() },
        Some((RejectCode::Malformed, "a group reference rides only a group message")));
    money("thread_seq_in_group",
        "In a group the pairwise sequence is meaningless — the same message sits at a different seq in every thread it fanned into — so carrying one is refused rather than quietly misread.",
        &Message { re_seq: Some(1), ..in_group.clone() },
        Some((RejectCode::Malformed, "a group targets by group reference")));
    money("bill_in_group",
        "Money stays pairwise: a bill to a group is N debts wearing one number, and every settlement rail — requests, receipts, escrow — is pairwise or a ceremony.",
        &Message { kind: MessageKind::PaymentRequest, amount_pxmr: Some(120_000_000_000), payto: Some("5ApJU8bf".into()), ..in_group.clone() },
        Some((RejectCode::Malformed, "this kind does not travel in a group")));
    money("roster_without_group",
        "A roster names its group; one that does not is a member list of nowhere.",
        &Message { kind: MessageKind::GroupRoster, body: "roster".into(), amount_pxmr: None, payload: Some(vec![0xEE; 40]), ..base_pay.clone() },
        Some((RejectCode::Malformed, "a roster names its group")));
    money("roster_without_members",
        "A roster with no payload states no membership at all.",
        &Message { kind: MessageKind::GroupRoster, body: "roster".into(), payload: None, ..in_group.clone() },
        Some((RejectCode::Malformed, "a roster carries its member list")));

    // §16.15 — attachments.
    let att = Attachment {
        record_key: KEY.into(),
        key: [0xAA; 32], nonce: [0xBB; 24], len: 200_000,
        ct_hash: [0xCC; 32], mime: "image/jpeg".into(), name: Some("cat.jpg".into()),
    };
    let with_att = Message {
        kind: MessageKind::Text, amount_pxmr: None,
        body: "📷".into(), attachment: Some(att.clone()),
        ..base_pay.clone()
    };
    money("attachment_valid",
        "A picture by reference: the bytes live in their own record as XChaCha-sealed chunks, and the key travels here, inside the sealed message — so the record on the network is noise to everyone but the thread.",
        &with_att, None);
    money("attachment_too_large",
        "One record is the unit: 32 chunks of 32 KiB is Veilid's measured 1 MiB cap, and a larger file is a different design, not a larger number.",
        &Message { attachment: Some(Attachment { len: 2_000_000, ..att.clone() }), ..with_att.clone() },
        Some((RejectCode::Malformed, "attachment over bound")));
    money("attachment_on_a_request",
        "Attachments ride ordinary messages. A bill with a file in it is two features fused at their least-tested corner.",
        &Message { kind: MessageKind::PaymentRequest, amount_pxmr: Some(1), ..with_att.clone() },
        Some((RejectCode::Malformed, "only text carries an attachment")));


    money("request_with_txid",
        "A notice points at the transaction it made and a receipt at the one it acknowledges. A request pointing at either is claiming the payment it is simultaneously asking for.",
        &Message { txid: Some(vec![0x77; 32]), ..base_pay.clone() },
        Some((RejectCode::Malformed, "only a notice or receipt carries a txid")));

    // §15.12's ceremony: the claim opens a channel, these three close a deal.
    money("ride_offer",
        "A driver's terms for a claimed hail: the fare, and how far away they are. The claim was the application; nothing is owed until the accept.",
        &Message { kind: MessageKind::RideOffer, body: "be there in 6".into(),
                   amount_pxmr: Some(4_200_000_000), eta_secs: Some(360), ..base_pay.clone() }, None);
    money("ride_offer_without_fare",
        "An offer without a fare offers nothing; the rider would be accepting a blank.",
        &Message { kind: MessageKind::RideOffer, amount_pxmr: None, ..base_pay.clone() },
        Some((RejectCode::Malformed, "a ride message must carry the fare")));
    money("ride_accept",
        "The rider's yes: names the offer it answers and echoes its fare, binding the acceptance to a price neither side can later dispute into a different number.",
        &Message { kind: MessageKind::RideAccept, body: "see you there".into(),
                   amount_pxmr: Some(4_200_000_000), re_seq: Some(0), ..base_pay.clone() }, None);
    money("ride_accept_without_target",
        "An accept that names no offer accepts nothing in particular; with two offers in a thread, which one it answers must never be inferred.",
        &Message { kind: MessageKind::RideAccept, amount_pxmr: Some(4_200_000_000), ..base_pay.clone() },
        Some((RejectCode::Malformed, "a retract or accept names its target")));
    money("retract_a_bill",
        "The sender withdraws their own earlier bill: re_own names their side of the thread, and the button on the other phone goes dead instead of paying into a sale nobody is watching.",
        &Message { kind: MessageKind::Retract, body: "cancelled — wrong table".into(),
                   amount_pxmr: None, re_seq: Some(3), re_own: true, ..base_pay.clone() }, None);
    money("retract_with_amount",
        "A retract withdraws a message; it does not transact. An amount on one is a number nothing should honour.",
        &Message { kind: MessageKind::Retract, re_seq: Some(3), ..base_pay.clone() },
        Some((RejectCode::Malformed, "a retract carries no amount")));
    money("eta_on_a_text",
        "An eta is a ride offer's courtesy figure; on anything else it is a field with no meaning to act on.",
        &Message { kind: MessageKind::Text, amount_pxmr: None, eta_secs: Some(300), ..base_pay.clone() },
        Some((RejectCode::Malformed, "only a ride offer carries an eta")));

    // §17.9's ceremony carries opaque threshold bytes; DUCAT checks the
    // envelope, never the payload.
    money("dkg_round",
        "A DKG round: the threshold library's commitment bytes, a round tag, and the per-escrow context that binds the message to one multisig. DUCAT carries the payload; it does not parse it.",
        &Message { kind: MessageKind::DkgRound, amount_pxmr: None,
                   payload: Some(vec![0xd1; 96]), round: Some(0), ceremony_id: Some([0x11; 32]),
                   ..base_pay.clone() }, None);
    money("frost_round",
        "A FROST signing round: a preprocess or signature share, tagged and bound to its escrow. The release it builds pays a destination the signer verifies before co-signing (§15.5 into escrow).",
        &Message { kind: MessageKind::FrostRound, amount_pxmr: None,
                   payload: Some(vec![0xf2; 128]), round: Some(1), ceremony_id: Some([0x22; 32]),
                   ..base_pay.clone() }, None);
    money("frost_round_with_claimed_return",
        "A release proposal (round 0) MAY state the amount the funder gets back — the consent screen shows it beside the signed payload (§15.12's settlement). A statement, not authority: nothing verifies it but the chain.",
        &Message { kind: MessageKind::FrostRound, amount_pxmr: Some(200_000_000),
                   payload: Some(vec![0xf3; 128]), round: Some(0), ceremony_id: Some([0x22; 32]),
                   ..base_pay.clone() }, None);
    money("ceremony_abort",
        "An abort names the ceremony it ends and carries no round payload — 'nothing happens' is never safe, so a dead build says so (§9.3.4).",
        &Message { kind: MessageKind::CeremonyAbort, amount_pxmr: None,
                   ceremony_id: Some([0x33; 32]), ..base_pay.clone() }, None);
    money("dkg_round_without_payload",
        "A ceremony round with no bytes is an envelope around nothing; refused.",
        &Message { kind: MessageKind::DkgRound, amount_pxmr: None,
                   round: Some(0), ceremony_id: Some([0x11; 32]), ..base_pay.clone() },
        Some((RejectCode::Malformed, "a ceremony round carries a payload")));
    money("dkg_round_without_context",
        "A round that names no escrow could replay into any of them; refused.",
        &Message { kind: MessageKind::DkgRound, amount_pxmr: None,
                   payload: Some(vec![0xd1; 96]), round: Some(0), ..base_pay.clone() },
        Some((RejectCode::Malformed, "a ceremony round names its round and its escrow")));
    money("payload_on_a_text",
        "A ceremony payload on an ordinary message is a field with no meaning to act on; refused.",
        &Message { kind: MessageKind::Text, amount_pxmr: None,
                   payload: Some(vec![0xd1; 8]), ..base_pay.clone() },
        Some((RejectCode::Malformed, "only a ceremony message carries ceremony fields")));
    money("abort_with_payload",
        "An abort withdraws a ceremony; a round payload on it is a contradiction.",
        &Message { kind: MessageKind::CeremonyAbort, amount_pxmr: None,
                   payload: Some(vec![0xd1; 8]), ceremony_id: Some([0x33; 32]), ..base_pay.clone() },
        Some((RejectCode::Malformed, "an abort withdraws a ceremony; it carries no round payload")));

    // §15.12 — the live-position reference (kind 11). The message only hands
    // over the pointer; the stream itself is board.beacon's sibling below,
    // pinned as its own frame kind. Both fields together or neither.
    let pos_ref = Message {
        kind: MessageKind::PositionRef, amount_pxmr: None,
        body: "sharing my position for the ride".into(),
        position: Some(ducat_core::contact::PositionRef {
            record_key: "VLD0:AbCdEfGhIjKlMnOpQrStUvWxYz0123456789aBcDeF".into(),
            stream_key: [0x5au8; 32],
        }),
        ..base_pay.clone()
    };
    money("position_ref",
        "A reference to a live-position stream after a RideAccept: a DHT record and the key to read it, sealed into the thread once. The stream is a record overwritten in place (a now with no past), so this message only carries the pointer.",
        &pos_ref, None);
    money("position_ref_on_a_text",
        "The stream reference is a PositionRef's whole content and nothing else's — on a text message it is a field with no meaning to act on.",
        &Message { kind: MessageKind::Text, body: "hi".into(), ..pos_ref.clone() },
        Some((RejectCode::Malformed, "only a position message carries a stream reference")));
    money("position_kind_without_a_reference",
        "A PositionRef whose reference is absent hands over nothing.",
        &Message { kind: MessageKind::PositionRef, position: None, group_id: None, group_seq: None, group_re_sender: None, group_re_seq: None,
                   body: "empty".into(), ..base_pay.clone() },
        Some((RejectCode::Malformed, "a position message carries a reference to the stream")));
    // The half-reference cannot be built from the struct (both fields or none),
    // so it is made by deleting one from the encoding — same trick as the
    // half-attachment below.
    {
        let mut v2 = pos_ref.to_value();
        if let ducat_core::cbor::Value::Map(ref mut m) = v2 {
            m.remove(&ducat_core::wire::f::MSG_POS_STREAM);
        }
        v.push(json!({ "name": "position_ref_without_key",
            "why": "A record with no key cannot be opened. Both halves of a position reference travel together or not at all (§16.15's rule).",
            "payment_hex": hex(&v2.encode()),
            "expect": { "ok": false, "reject": "MALFORMED", "hint": "a position reference carries its record and its key together" } }));
        let mut v3 = pos_ref.to_value();
        if let ducat_core::cbor::Value::Map(ref mut m) = v3 {
            m.remove(&ducat_core::wire::f::MSG_POS_RECORD);
        }
        v.push(json!({ "name": "position_ref_without_record",
            "why": "A key pointing at no record points nowhere.",
            "payment_hex": hex(&v3.encode()),
            "expect": { "ok": false, "reject": "MALFORMED", "hint": "a position reference carries its record and its key together" } }));
    }

    // The stream itself (§15.12): the encrypted value written to the record's
    // subkey each cadence. A fixed-length primitive, not CBOR — so the
    // ciphertext sequence leaks its heartbeat and nothing else. Deterministic
    // in every part (fixed key, record, nonce, frame), which is what lets it
    // be a vector at all.
    {
        use ducat_core::position::{seal, PositionFrame};
        let stream_key = [0x5au8; 32];
        let record = "VLD0:positionstreamrecordkeyexample000000000";
        let nonce = [0x11u8; ducat_core::position::NONCE_LEN];
        let frame = PositionFrame {
            counter: 42, lat_e7: 525_200_000, lon_e7: 133_760_000,
            heading: Some(270), captured: 1_800_000_000,
        };
        let sealed = seal(&stream_key, record, &nonce, &frame);
        let stream_key_hex: String = stream_key.iter().map(|b| format!("{b:02x}")).collect();
        v.push(json!({ "name": "position_frame",
            "why": "One live-position update, sealed XChaCha20-Poly1305 under the stream key with the record key as associated data, padded to a constant length. A now with no past; the receiver drops a non-increasing counter as a replay.",
            "frame_sealed_hex": hex(&sealed),
            "stream_key_hex": stream_key_hex,
            "record_key": record,
            "expect": { "ok": true, "counter": 42, "lat_e7": 525_200_000,
                        "lon_e7": 133_760_000, "heading": 270, "captured": 1_800_000_000i64 } }));

        // No heading: the same length on the wire (the whole point of the pad).
        let mut noh = frame;
        noh.heading = None;
        let sealed_noh = seal(&stream_key, record, &nonce, &noh);
        v.push(json!({ "name": "position_frame_no_heading",
            "why": "A frame without a heading is the same length as one with it — every update is identical in size, so the sequence carries only its cadence.",
            "frame_sealed_hex": hex(&sealed_noh),
            "stream_key_hex": stream_key_hex,
            "record_key": record,
            "expect": { "ok": true, "counter": 42, "lat_e7": 525_200_000,
                        "lon_e7": 133_760_000, "captured": 1_800_000_000i64 } }));

        // Lifted into another record: the AAD no longer matches, so it fails to
        // authenticate rather than returning a position from the wrong ride.
        v.push(json!({ "name": "position_frame_wrong_record",
            "why": "The record key is the AEAD's associated data, so a value lifted from one ride's record cannot authenticate in another's — which is what stops a fresh key from silently linking two rides.",
            "frame_sealed_hex": hex(&sealed),
            "stream_key_hex": stream_key_hex,
            "record_key": "VLD0:a-different-record-entirely-00000000000000",
            "expect": { "ok": false, "reject": "BADSIG", "hint": "did not authenticate" } }));

        // Truncated: a sealed frame is a fixed length; a short one is refused
        // before the AEAD, so a reader never feeds a runt to decrypt.
        v.push(json!({ "name": "position_frame_truncated",
            "why": "A sealed frame is a constant length; anything else did not come from this construction.",
            "frame_sealed_hex": hex(&sealed[..sealed.len() - 4]),
            "stream_key_hex": stream_key_hex,
            "record_key": record,
            "expect": { "ok": false, "reject": "MALFORMED", "hint": "fixed length" } }));
    }


    // Handcrafted: the struct cannot express a half-attachment, which is the
    // point — so the case is built by deleting the hash from the encoding.
    {
        let mut v2 = with_att.to_value();
        if let ducat_core::cbor::Value::Map(ref mut m) = v2 {
            m.remove(&ducat_core::wire::f::MSG_ATT_HASH);
        }
        let hex_body = hex(&v2.encode());
        v.push(json!({ "name": "attachment_partial",
            "why": "All or nothing: every subset of {record, key, nonce, length, hash, mime} is a trap — fetchable but not decryptable, or decryptable but not verifiable.",
            "payment_hex": hex_body,
            "expect": { "ok": false, "reject": "MALFORMED", "hint": "attachment fields travel together" } }));
    }

    // §16.18: the verifying key a sealed notice carries, for the vector's
    // expectation. Pulled back out rather than recomputed, so the vector
    // asserts what the sealer actually wrote.
    fn sealed_poster(v: &ducat_core::cbor::Value) -> Vec<u8> {
        let ducat_core::cbor::Value::Map(m) = v else { unreachable!() };
        match m.get(&ducat_core::wire::f::RN_POSTER) {
            Some(ducat_core::cbor::Value::Bytes(b)) => b.clone(),
            _ => unreachable!("a sealed notice carries its poster"),
        }
    }

    // §16.17: the hail notice, the one object that lives on a public board.
    {
        let mut ncase = |name: &str, why: &str, n: &HailNotice, bad: Option<(RejectCode, &str)>| {
            let hex_body = hex(&n.to_value().encode());
            v.push(match bad {
                None => json!({ "name": name, "why": why, "notice_hex": hex_body,
                                "expect": { "ok": true, "reencodes_to_hex": hex_body } }),
                Some((code, hint)) => json!({ "name": name, "why": why, "notice_hex": hex_body,
                                "expect": { "ok": false, "reject": format!("{:?}", code).to_uppercase(), "hint": hint } }),
            });
        };
        let base = HailNotice {
            version: 2,
            card: "ducat:2m1CQVCAiPjIfW5EX7ja1i8dRAsCLZ3nSCEgKHZBHZY".into(),
            dest: "airport, terminal B".into(),
            fare_pxmr: Some(5_000_000_000),
            expiry: 1_800_000_000,
            origin_cell: None,
            dest_cell: None,
        };
        ncase("hail_valid",
            "A rider on a public board: a claim-once card, sixty-four bytes of destination, an offer, an expiry. The card is the only field with teeth — claiming it is what §16.9 verifies.",
            &base, None);
        ncase("hail_quote_me",
            "An absent fare is a real posture: name your price. Distinct from zero, which is refused below.",
            &HailNotice { fare_pxmr: None, ..base.clone() }, None);
        ncase("hail_fare_zero",
            "A zero fare offer is a missing one wearing a number — two encodings of one meaning (§18.1).",
            &HailNotice { fare_pxmr: Some(0), ..base.clone() },
            Some((RejectCode::Malformed, "zero fare")));
        ncase("hail_card_wrong_scheme",
            "The board is hostile input; a notice whose card is not a ducat: URI is bait for whatever parses the link.",
            &HailNotice { card: "https://example.com/ride".into(), ..base.clone() },
            Some((RejectCode::Malformed, "card must be a ducat: URI")));
        ncase("hail_dest_empty",
            "An empty destination says nothing and renders as something.",
            &HailNotice { dest: String::new(), ..base.clone() },
            Some((RejectCode::Malformed, "empty destination")));
        ncase("hail_dest_too_long",
            "Sixty-four bytes of human words is the cap by construction: the place a notice can say is the cell it is pinned to. More is a channel, and the channel is the thread.",
            &HailNotice { dest: "x".repeat(65), ..base.clone() },
            Some((RejectCode::Malformed, "text too long")));
        ncase("hail_wrong_version",
            "A version this reader does not speak, refused rather than guessed at.",
            &HailNotice { version: 1, ..base.clone() },
            Some((RejectCode::Malformed, "unknown version")));
        // §15.12's geocells: coarse place on the board, capped by construction.
        ncase("hail_with_geocells",
            "An Uber-shaped hail: origin and destination as geocells no finer than ~1.2 km, so a driver reads the job — distance to the fare, length of the ride — before claiming. The cells are the only location the board ever carries.",
            &HailNotice {
                origin_cell: Some("dqcjq8".into()),
                dest_cell: Some("dqcjnb".into()),
                ..base.clone()
            }, None);
        ncase("hail_origin_only",
            "Either cell may travel alone; a rider may name where they are and keep where they are going for the thread.",
            &HailNotice { origin_cell: Some("u4pruy".into()), ..base.clone() }, None);
        ncase("hail_cell_too_precise",
            "Precision 7 is ~150 m — a location, not an area. The cap is what makes 'no precise location on the board' true by construction rather than by good manners.",
            &HailNotice { origin_cell: Some("dqcjq8h".into()), ..base.clone() },
            Some((RejectCode::Malformed, "cell too precise")));
        ncase("hail_cell_bad_alphabet",
            "'a' is not in the geohash alphabet; a cell that cannot name a place is bait for whatever parses it.",
            &HailNotice { dest_cell: Some("dqcja".into()), ..base.clone() },
            Some((RejectCode::Malformed, "not a geohash")));
    }

    // A message kind nobody has a name for.
    //
    // Forged, because the field is an enum here and cannot hold one — which
    // is why it had no vector: eleven kinds are each exercised positively and
    // the *edge* of the set never was. A reader that renders an unknown kind
    // as text shows a payment request as a chat line, or the reverse.
    {
        let ducat_core::cbor::Value::Map(mut m) = base_pay.to_value() else { unreachable!() };
        m.insert(ducat_core::wire::f::MSG_KIND, ducat_core::cbor::Value::Uint(11));
        v.push(json!({ "name": "message_unknown_kind",
            "why": "Kind 11. The set is closed at ten, and a kind decides what every other field on the message *means* — an amount, a target sequence, a ceremony payload. Falling back to text would render a request for money as something to read.",
            "payment_hex": hex(&ducat_core::cbor::Value::Map(m).encode()),
            "expect": { "ok": false, "reject": "MALFORMED", "hint": "unknown message kind" } }));
    }

    // §16.18: the listing — the other object that lives on a public board,
    // and the one that stays there for days.
    {
        let mut lcase = |name: &str, why: &str, n: &RentalNotice, bad: Option<(RejectCode, &str)>| {
            let hex_body = hex(&n.to_value().encode());
            v.push(match bad {
                None => json!({ "name": name, "why": why, "listing_hex": hex_body,
                                "expect": { "ok": true, "reencodes_to_hex": hex_body } }),
                Some((code, hint)) => json!({ "name": name, "why": why, "listing_hex": hex_body,
                                "expect": { "ok": false, "reject": format!("{:?}", code).to_uppercase(), "hint": hint } }),
            });
        };
        let car = RentalNotice {
            version: 2,
            card: "ducat:2m1CQVCAiPjIfW5EX7ja1i8dRAsCLZ3nSCEgKHZBHZY".into(),
            kind: 2,
            title: "2019 Corolla, automatic".into(),
            area: "north side".into(),
            cell: Some("dqcjq".into()),
            price_pxmr: 40_000_000_000,
            deposit_pxmr: 12_000_000_000,
            expiry: 1_800_000_000,
            make: Some("Toyota".into()),
            model: Some("Corolla".into()),
            year: Some(2019),
            gearbox: Some(2),
            fuel: Some(1),
            seats: Some(5),
            color: Some("silver".into()),
            trim: Some("Hybrid LE".into()),
            rooms: None, sleeps: None, size_m2: None,
            subtype: Some(1),
            features: vec!["child seat".into()],
            quantity: 1,
        };
        let room = RentalNotice {
            kind: 1,
            title: "Sunny room near the park".into(),
            area: "Kreuzberg".into(),
            cell: Some("u33db".into()),
            price_pxmr: 25_000_000_000,
            deposit_pxmr: 5_000_000_000,
            make: None, model: None, year: None, gearbox: None, fuel: None,
            seats: None, color: None, trim: None,
            rooms: Some(1), sleeps: Some(2), size_m2: Some(28), subtype: Some(2),
            features: vec!["wifi".into()],
            ..car.clone()
        };
        lcase("listing_vehicle",
            "A car on a public board: the shape a stranger needs to decide whether to ask, and nothing they would need to arrive. No plate, no address — those pass through the sealed thread after both sides agree.",
            &car, None);
        lcase("listing_place",
            "A room, the same way. Bedrooms and sleeps where the car had a gearbox and a fuel.",
            &room, None);
        lcase("listing_place_with_gearbox",
            "A place with a gearbox is describing two things, and a reader would have to guess which half to believe. Refused rather than reconciled (§18.1).",
            &RentalNotice { gearbox: Some(2), ..room.clone() },
            Some((RejectCode::Malformed, "a place has no gearbox")));
        lcase("listing_vehicle_with_bedrooms",
            "The mirror of it: a car with bedrooms.",
            &RentalNotice { rooms: Some(3), ..car.clone() },
            Some((RejectCode::Malformed, "a vehicle has no bedrooms")));
        lcase("listing_vehicle_with_floor_area",
            "Floor area is a room's number, and a car quoting it is describing something else.",
            &RentalNotice { size_m2: Some(90), ..car.clone() },
            Some((RejectCode::Malformed, "a vehicle has no floor area")));
        lcase("listing_place_with_trim",
            "Trim is a car's word. A flat with a trim level is the place/vehicle split failing quietly.",
            &RentalNotice { trim: Some("Sport".into()), ..room.clone() },
            Some((RejectCode::Malformed, "a place has no trim")));
        lcase("listing_cell_too_precise",
            "A hail's precision 6 (~1.2 km) is legal for a person standing at a kerb for ten minutes and wrong for a home that will still be there next week. A listing is capped at precision 5.",
            &RentalNotice { cell: Some("u33dbc".into()), ..room.clone() },
            Some((RejectCode::Malformed, "listing cell no finer than precision 5")));
        lcase("listing_no_price",
            "A listing with no price is not an offer; it is an invitation to negotiate in the open, which is what the thread is for.",
            &RentalNotice { price_pxmr: 0, ..car.clone() },
            Some((RejectCode::Malformed, "a listing needs a price")));
        lcase("listing_card_wrong_scheme",
            "The board is hostile input, and the card is the one field with teeth.",
            &RentalNotice { card: "https://example.com/car".into(), ..car.clone() },
            Some((RejectCode::Malformed, "card must be a ducat: URI")));
        lcase("listing_unknown_kind",
            "Neither a place nor a vehicle: a reader would have no idea which fields it may believe.",
            &RentalNotice { kind: 7, ..car.clone() },
            Some((RejectCode::Malformed, "a listing is a place or a vehicle")));
        lcase("listing_bad_gearbox",
            "Manual or automatic. A third value is a claim this reader cannot render, so it is refused rather than shown as a number.",
            &RentalNotice { gearbox: Some(3), ..car.clone() },
            Some((RejectCode::Malformed, "gearbox is manual or automatic")));
        lcase("listing_implausible_year",
            "A car from 1750 is a typo or a joke, and either way not a rental.",
            &RentalNotice { year: Some(1750), ..car.clone() },
            Some((RejectCode::Malformed, "implausible year")));
        lcase("listing_too_many_features",
            "Features are a summary, not a description — the description belongs in the conversation, where it is not being broadcast to strangers.",
            &RentalNotice { features: (0..12).map(|i| format!("f{i}")).collect(), ..room.clone() },
            Some((RejectCode::Malformed, "too many features")));
        lcase("listing_no_deposit",
            "A stake of zero is legitimate: the floor rule (§15.12) zeroes a stake worth less than the fee to return it, and an owner may simply not ask for one.",
            &RentalNotice { deposit_pxmr: 0, ..room.clone() }, None);

        // The stall with six of something. Almost every listing is one thing,
        // so one is the *absent* case and the only spelling of it.
        let six = RentalNotice {
            kind: 4,
            title: "Sea kayak, single".into(),
            subtype: Some(1),
            features: vec!["paddle".into()],
            rooms: None, sleeps: None, size_m2: None,
            make: None, model: None, year: None, gearbox: None, fuel: None,
            seats: None, color: None, trim: None,
            quantity: 6,
            ..car.clone()
        };
        lcase("listing_quantity_six",
            "Six identical kayaks on one slot. A board slot is scarce and somebody deciding whether to ask wants to know they are not competing for the last one.",
            &six, None);
        lcase("listing_quantity_one_is_absent",
            "The same listing with one of them: the field is not written at all. One is the default, so the ordinary listing costs nothing on a board that is expensive to read — and there is exactly one encoding of \"I have one of these\", which matters because the signature is over these bytes.",
            &RentalNotice { quantity: 1, ..six.clone() }, None);
        // These two cannot be built by encoding a notice: `to_value` will not
        // write a quantity of one or zero, which is the property under test.
        // So the field is put into the map by hand — which is also exactly
        // what a careless third implementation would do.
        lcase("listing_quantity_a_warehouse",
            "Past the ceiling. A listing is an advertisement for what somebody has to hand, and a board slot is a scarce shared thing.",
            &RentalNotice { quantity: 100_000, ..six.clone() },
            Some((RejectCode::Malformed, "more than a listing is for")));
        lcase("listing_skill_with_a_quantity",
            "An hourly rate for one person's time, offered three at a time. Somebody's time is not stock, and the number would be describing staffing the listing does not have.",
            &RentalNotice { kind: 5, subtype: Some(1), quantity: 3, ..six.clone() },
            Some((RejectCode::Malformed, "somebody's time is not stock")));

        // The text bounds, which had none. A listing sits in the open for days
        // and every one of these is a field a stranger's screen has to lay
        // out; two implementations disagreeing about where the limit falls is
        // one of them rendering a notice the other refuses.
        lcase("listing_title_too_long",
            "One human line. Sixty characters is a headline; past that it is a description, and a description belongs in the conversation where it is not being broadcast.",
            &RentalNotice { title: "x".repeat(61), ..car.clone() },
            Some((RejectCode::Malformed, "text too long")));
        lcase("listing_area_too_long",
            "Human words for a neighbourhood. Forty characters holds one; more is an address being smuggled into the field that exists so an address does not have to be.",
            &RentalNotice { area: "x".repeat(41), ..car.clone() },
            Some((RejectCode::Malformed, "text too long")));
        lcase("listing_word_too_long",
            "Make, model, colour, trim: single words for filtering on, not free text. The bound is the same for all four, and this is the one that pins it.",
            &RentalNotice { model: Some("x".repeat(25)), ..car.clone() },
            Some((RejectCode::Malformed, "text too long")));
        lcase("listing_feature_too_long",
            "A tag is a short word. The count had a vector and the length did not — so an implementation could have taken a sentence per tag and still agreed with this one on everything tested.",
            &RentalNotice { features: vec!["x".repeat(17)], ..room.clone() },
            Some((RejectCode::Malformed, "a feature is a short word")));

        // The version, on the object that lives longest in the open. A hail
        // had this vector and a listing did not, and a listing is the one that
        // sits on a public board for days.
        lcase("listing_wrong_version",
            "Version 1 was never shipped on a board — the notice carries 2 because the sealed form around it does (board.rs). A reader that guessed at an older shape would be reading fields nobody wrote.",
            &RentalNotice { version: 1, ..car.clone() },
            Some((RejectCode::Malformed, "unknown rental notice version")));

        // Every kind's subtype ceiling, from both sides.
        //
        // The table — 2, 3, 9, 5, 12 — had no vector at all, and a second
        // implementation was carrying the two-kind version of it from before
        // draft 0.89 without anything noticing. A reject past the top pins it
        // from above and an accept at the top pins it from below; either alone
        // leaves an implementation free to pick a different number.
        for kind in RENTAL_PLACE..=RENTAL_SKILL {
            let top = rental_subtype_top(kind);
            // No typed extras, so one shape serves every kind: a place and a
            // vehicle refuse each other's fields, not the absence of them.
            let base = RentalNotice { kind, quantity: 1, subtype: Some(top), ..six.clone() };
            lcase(
                &format!("listing_subtype_top_kind_{kind}"),
                &format!(
                    "The last category kind {kind} recognises. The set is deliberately \
                     small and flat — a coarse filter on a board that is expensive to \
                     read, translated everywhere this ships — so its size is part of \
                     the wire and not an implementation's own idea.",
                ),
                &base, None,
            );
            lcase(
                &format!("listing_subtype_past_top_kind_{kind}"),
                &format!(
                    "One past it. A reader cannot render a category it has no name \
                     for, and showing the raw number instead would be a listing \
                     claiming something nobody can read.",
                ),
                &RentalNotice { subtype: Some(top + 1), ..base.clone() },
                Some((RejectCode::Malformed, "unknown subtype")),
            );
        }
        lcase("listing_subtype_zero",
            "Subtypes are one-based; zero is the absence of one, and the way to say that is to omit the field.",
            &RentalNotice { subtype: Some(0), quantity: 1, ..six.clone() },
            Some((RejectCode::Malformed, "unknown subtype")));
        lcase("listing_bad_fuel",
            "Petrol, diesel, electric or hybrid. A fifth value is a claim this reader cannot render — the same rule as the gearbox beside it, which had a vector while this did not.",
            &RentalNotice { fuel: Some(5), ..car.clone() },
            Some((RejectCode::Malformed, "unknown fuel")));

        let forge = |q: u64| {
            let ducat_core::cbor::Value::Map(mut m) = six.to_value() else { unreachable!() };
            m.insert(ducat_core::wire::f::RN_QUANTITY, ducat_core::cbor::Value::Uint(q));
            hex(&ducat_core::cbor::Value::Map(m).encode())
        };
        for (name, why, q) in [
            ("listing_quantity_stated_as_one",
             "One, written down. Refused: it is a second spelling of the listing above, and two byte-strings that mean the same thing is the seam a signature is supposed to close.",
             1u64),
            ("listing_quantity_zero",
             "A listing of nothing. An owner who has run out stops refreshing the notice; they do not advertise the absence.",
             0),
        ] {
            v.push(json!({ "name": name, "why": why, "listing_hex": forge(q),
                "expect": { "ok": false, "reject": "MALFORMED",
                            "hint": "a quantity is written only when it is more than one" } }));
        }

        // §16.18 + board.rs: the sealed form, which is what actually goes on a
        // board. Everything above is the notice *inside* the seal; another
        // implementation needs this to know how the two fit together.
        //
        // Deterministic in every part: Ed25519 signs deterministically, and
        // the nonce search starts at zero and walks up, so the same inputs
        // give the same bytes on any machine. Which is what makes it a vector.
        {
            let seed = ducat_core::board::listing_seed(b"vector-persona", "listing-1");
            let board = "geo:u33db";
            let subkey = 3u32;
            // A pinned block, never a real one and never the clock's: §18.9
            // wants a case decided today to decide the same way in a year, and
            // a vector that reached for a chain tip would expire.
            let beacon = ducat_core::board::Beacon {
                height: 3_210_000,
                hash: [0x5au8; 32],
            };
            let ducat_core::cbor::Value::Map(m) = room.to_value() else { unreachable!() };
            let sealed = ducat_core::board::seal(
                m, ducat_core::board::RENTAL, &seed, board, subkey, &beacon,
            );
            let sealed_hex = hex(&sealed.encode());
            v.push(json!({ "name": "listing_sealed",
                "why": "What a listing looks like on the board: the notice, the listing's own verifying key, a signature over the notice, the slot *and the block it was stamped against*, and a nonce whose Argon2id output shows board::POW_BITS leading zero bits. A board's write key is the cell name hashed, so anyone can write any slot — the signature says who wrote the bytes and the work says it was not free.",
                "sealed_hex": sealed_hex,
                "board": board, "subkey": subkey,
                "expect": { "ok": true, "poster_hex": hex(&sealed_poster(&sealed)),
                            "beacon_height": beacon.height,
                            "beacon_hash": hex(&beacon.hash) } }));
            v.push(json!({ "name": "listing_sealed_wrong_slot",
                "why": "The same bytes offered as slot 4. The slot is inside the signature, so a valid notice cannot be lifted onto another one — without which an attacker holding the public write key could paper a whole cell with somebody else's signed listing.",
                "sealed_hex": sealed_hex,
                "board": board, "subkey": subkey + 1,
                "expect": { "ok": false, "reject": "MALFORMED", "hint": "signed for another slot" } }));

            // §16.18.1: the beacon is inside the signature, so neither half of
            // it can be restated after the work is done. Both halves, because
            // they fail for different reasons to a reader — the hash is what
            // the work is bound to, and the height is the cheap test a reader
            // runs before it looks anything up. An implementation that signed
            // only one would pass a vector for the other.
            let ducat_core::cbor::Value::Map(base) = sealed.clone() else { unreachable!() };
            let mut swapped = base.clone();
            swapped.insert(
                ducat_core::wire::f::RN_BEACON_HASH,
                ducat_core::cbor::Value::Bytes(vec![0x11u8; 32]),
            );
            v.push(json!({ "name": "listing_sealed_beacon_hash_swapped",
                "why": "A different block hash against the same work. Without a beacon in the preimage every stamp in the protocol's future is mineable this afternoon — cell, slot, body and signature are all the poster's own and the board epoch is a floor division — so the block is what makes the work perishable, and it has to be as unforgeable as the rest of the notice.",
                "sealed_hex": hex(&ducat_core::cbor::Value::Map(swapped).encode()),
                "board": board, "subkey": subkey,
                "expect": { "ok": false, "reject": "MALFORMED", "hint": "signed for another slot" } }));

            let mut moved = base.clone();
            moved.insert(
                ducat_core::wire::f::RN_BEACON_HEIGHT,
                ducat_core::cbor::Value::Uint(9_999_999),
            );
            v.push(json!({ "name": "listing_sealed_beacon_height_moved",
                "why": "The same block hash re-labelled with a newer height. A reader tests the height first because it is free, and looks the hash up only for heights that survive — so a height that could be moved would let one mined notice claim any tip, and the cheap test would be worth nothing.",
                "sealed_hex": hex(&ducat_core::cbor::Value::Map(moved).encode()),
                "board": board, "subkey": subkey,
                "expect": { "ok": false, "reject": "MALFORMED", "hint": "signed for another slot" } }));

            let mut short = base.clone();
            short.insert(
                ducat_core::wire::f::RN_BEACON_HASH,
                ducat_core::cbor::Value::Bytes(vec![0x5au8; 31]),
            );
            v.push(json!({ "name": "listing_sealed_beacon_hash_short",
                "why": "Thirty-one bytes where a block hash goes. Pinned because a reader that accepted a short hash would be reading a different preimage from the one the poster signed, and would then disagree with every other implementation about which notices are valid.",
                "sealed_hex": hex(&ducat_core::cbor::Value::Map(short).encode()),
                "board": board, "subkey": subkey,
                "expect": { "ok": false, "reject": "MALFORMED", "hint": "a block hash is 32 bytes" } }));

            // §16.18.1's window, pinned at both edges and from both sides.
            //
            // A bound tested from one side only is a bound the other
            // implementation gets to choose — the lesson §18.9 already learned
            // about enumerations, applied to a range. The freshness test lives
            // outside `open` because it needs a chain and `open` must not, so
            // it needs cases of its own or it is agreed by nobody.
            let tip = 3_210_000u64;
            for (name, height, tip_height, ok, why) in [
                ("beacon_window_at_the_tip", tip, tip, true,
                 "A notice stamped against the reader's own tip. The ordinary case, and the one an implementation cannot get wrong by accident — it is here so the two that follow have something to be one past."),
                ("beacon_window_oldest_accepted", tip - 720, tip, true,
                 "Exactly 720 blocks back, which is the oldest block still inside the window: about a day. A day rather than the hour a precomputation argument alone would want, because the limit is the reader — a phone that has been in a drawer would otherwise show an empty marketplace and no reason for it."),
                ("beacon_window_one_too_old", tip - 721, tip, false,
                 "One block past the edge. Without this the top of the range is unpinned and a second implementation could pick any number it liked, agreeing with the first on every case anybody wrote down."),
                ("beacon_window_reader_two_behind", tip + 2, tip, true,
                 "A poster whose node is two blocks ahead of the reader's. Ordinary — nodes lag — and refusing it would make freshness a race between daemons rather than a property of the notice."),
                ("beacon_window_ahead_of_the_chain", tip + 3, tip, false,
                 "Three blocks ahead of the reader's tip: a height nobody could have mined against yet. The other edge of the same range, pinned for the same reason."),
                ("beacon_window_no_chain_view", tip, 0, true,
                 "A reader that does not know the height. Zero is a real answer and means *skip the test*, not *everything is stale*: reading a board has never needed a Monero node, and a marketplace that goes dark because a daemon is unreachable is a worse answer than the spam it was avoiding."),
            ] {
                v.push(json!({ "name": name, "why": why,
                    "beacon_height": height, "tip_height": tip_height,
                    "expect": { "ok": ok } }));
            }

            // The other half of §16.18.1, and the half a reader can get wrong
            // in the attacker's favour without noticing. The window says a
            // height is plausible; only the hash says it is real, and the
            // answer "cannot say yet" has to stay distinct from "yes".
            for (name, h, tip_height, known, want, why) in [
                ("beacon_verdict_confirmed", tip, tip, Some("5a"), "show",
                 "The height is in the window and carries the hash the notice claims. The only case that may be displayed."),
                ("beacon_verdict_hash_is_not_that_blocks", tip, tip, Some("11"), "refuse",
                 "In the window, and that block does not have that hash. This is the case the whole beacon rests on: the work is bound to the hash, so a notice whose hash is invented is a notice mined at leisure — the height beside it proves nothing, since two-minute blocks make a height months away predictable to within a few hundred."),
                ("beacon_verdict_not_yet_knowable", tip + 2, tip, None, "hold",
                 "Inside the window, two blocks above this reader's tip, so the hash cannot be checked yet. Held, not shown — the forward slack exists to keep an honest notice from being *refused*, and must not become a way to display one nobody has checked. It becomes knowable in minutes."),
                ("beacon_verdict_lookup_unavailable", tip - 100, tip, None, "hold",
                 "Inside the window and behind the tip, but this reader has no answer for that height — the lookup failed, or its per-board budget was spent. Same answer as above and for the same reason: collapsing \"cannot say\" into \"yes\" is exactly the reader an attacker mines against."),
                ("beacon_verdict_out_of_window", tip - 721, tip, Some("5a"), "refuse",
                 "Outside the window, so the hash is never consulted — a real block hash from last month is still last month's stamp, and refusing on the cheap test first is what keeps a doctored board from costing every reader a lookup per slot."),
                ("beacon_verdict_no_chain_view", tip, 0, None, "show",
                 "A reader with no chain view at all. The one case that skips both tests: reading a board has never needed a Monero node, and a marketplace that goes dark because a daemon is unreachable is a worse answer than the spam it was avoiding. Distinct from *hold* — this device never claimed to be able to check."),
            ] {
                v.push(json!({ "name": name, "why": why,
                    "verdict_height": h, "verdict_tip": tip_height,
                    "known_hash": known.map(|b: &str| b.repeat(32)),
                    "beacon_hash": "5a".repeat(32),
                    "expect": { "verdict": want } }));
            }

            for (name, drop, why) in [
                ("listing_sealed_no_beacon_height", ducat_core::wire::f::RN_BEACON_HEIGHT,
                 "A notice with no beacon height. There is no unstamped path: a reader that treated a missing beacon as legacy would be offering an attacker the whole of the precomputation back, and a defence with an opt-out is not one."),
                ("listing_sealed_no_beacon_hash", ducat_core::wire::f::RN_BEACON_HASH,
                 "A notice with no beacon hash. The same rule from the other side — the hash is what the work is actually bound to."),
            ] {
                let mut m = base.clone();
                m.remove(&drop);
                v.push(json!({ "name": name, "why": why,
                    "sealed_hex": hex(&ducat_core::cbor::Value::Map(m).encode()),
                    "board": board, "subkey": subkey,
                    "expect": { "ok": false, "reject": "MALFORMED",
                                "hint": "must name the block it was stamped against" } }));
            }
        }

    }

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
    v.push(slash("slash_unknown_reason",
        "Two reasons a bond may be slashed, and nothing else. A reader that treats an \
         unrecognised reason as the nearest one it knows is honouring a claim on somebody's \
         money for a cause nobody defined — and the nearest one here is the one that skips \
         the waiting period.",
        3, Some(&hex(&[0x5Au8; 32])), 30, 21_000_000_000,
        json!({"ok": false, "reject_code": RejectCode::Malformed as u8,
               "reject_name": "Malformed"})));
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
        "protocol_draft": protocol_draft(),
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
