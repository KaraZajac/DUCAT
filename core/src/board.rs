//! Who posted a board notice, and what it cost them.
//!
//! A stand's write key is the cell name hashed (§15.12) — everybody who can
//! find a board holds the key to every slot on it. That is not a defect, it is
//! what having no operator means, and no amount of cleverness here changes it:
//! anyone can overwrite anything.
//!
//! Two things can still be true on top of that, and this module is both.
//!
//! **A notice can say who wrote it.** Not who *owns the slot* — nobody owns a
//! slot — but who authored the bytes. That is what makes substitution visible:
//! copy somebody's listing, swap in your own card, and it is a different
//! author, which a reader who saw the original can be told about. The
//! signature covers the slot as well as the content, so a valid signature
//! cannot be lifted onto another slot; without that, an attacker could scatter
//! somebody else's signed listing across a whole cell and have it read as that
//! person flooding the board.
//!
//! **A notice can cost something.** Filling every slot in a cell is 128 writes
//! and, before this, 128 writes cost nothing but bandwidth. A proof of work
//! bound to the same slot and the same bytes turns that into 128 separate
//! searches. It does not stop a determined attacker — nothing does, the key is
//! public — it makes the cheap version of the attack stop being cheap.
//!
//! The two are one mechanism because they have to be. If an unsigned notice
//! were readable, an attacker would simply post unsigned ones and skip the
//! work; so a notice without both is not a notice.

use crate::cbor::Value;
use crate::sig::{ObjectType, PublicKey, SecretKey, Suite};
use crate::reject::{Reject, RejectCode};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

/// Leading zero bits a notice's proof of work must show.
///
/// Measured rather than guessed — `pow_cost` here, and `BOARD_POW` in
/// `:desktop:boardnotice`, which times the real bridge the phone calls. About
/// half a second per notice on one desktop core, so a cell's 128 slots cost an
/// attacker something over a minute, and a region of a hundred cells costs a
/// couple of hours — repeated as notices expire. A poster pays it once per
/// listing per refresh, on the poll thread, which is where it belongs.
///
/// Be honest about what this does and does not do. Proof of work is symmetric
/// — better hardware helps the attacker exactly as much — and 128 slots is not
/// many, so a determined flood of one cell is not prevented by any difficulty
/// an honest phone could also pay. What changes is that flooding stops being
/// *free*: scripted, blanket, region-wide spraying now has a bill, and the
/// board's write key being public no longer means writes cost nothing.
///
/// Deliberately not tunable per notice. A difficulty a poster could choose is
/// a difficulty an attacker chooses zero.
pub const POW_BITS: u32 = 20;

/// Where the three added fields live in a given notice's map.
///
/// A notice's field ids are its own namespace, so a rental and a hail number
/// these differently while sharing every line of the logic below.
#[derive(Clone, Copy)]
pub struct NoticeFields {
    pub poster: u64,
    pub sig: u64,
    pub pow: u64,
}

/// §16.18's rental listing.
pub const RENTAL: NoticeFields = NoticeFields {
    poster: crate::wire::f::RN_POSTER,
    sig: crate::wire::f::RN_SIG,
    pow: crate::wire::f::RN_POW,
};

/// §16.17's hail.
pub const HAIL: NoticeFields = NoticeFields {
    poster: crate::wire::f::HN_POSTER,
    sig: crate::wire::f::HN_SIG,
    pow: crate::wire::f::HN_POW,
};

/// The key a listing signs with — not the persona.
///
/// A board is read by everyone, so a notice signed by the poster's persona
/// would publish which persona posted which listing, linkable across every
/// board and against every contact who already knows that persona. §16.3 keeps
/// transactions anonymous; a signature that undoes it for anybody browsing a
/// marketplace would be a poor trade for the thing it buys.
///
/// So the key is per listing: derived from the persona secret and the
/// listing's own local id, which makes it stable across the refreshes of that
/// one listing — which is the whole property a reader needs — and unlinkable
/// to the persona or to the poster's other listings, which nobody needs.
pub fn listing_seed(persona_secret: &[u8], listing_id: &str) -> [u8; 32] {
    Sha256::new()
        .chain_update(b"DUCAT-LISTING-v0")
        .chain_update([0u8])
        .chain_update(persona_secret)
        .chain_update([0u8])
        .chain_update(listing_id.as_bytes())
        .finalize()
        .into()
}

/// What the signature is over: the notice, and the slot it is going into.
fn signed_bytes(body: &[u8], board: &str, subkey: u32) -> Vec<u8> {
    let mut v = Vec::with_capacity(body.len() + board.len() + 8);
    v.extend_from_slice(board.as_bytes());
    v.push(0x00);
    v.extend_from_slice(&subkey.to_le_bytes());
    v.push(0x00);
    v.extend_from_slice(body);
    v
}

/// What the work is over: everything above, plus the signature.
fn pow_bytes(signed: &[u8], sig: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(signed.len() + sig.len() + 16);
    v.extend_from_slice(b"DUCAT-POW-v0");
    v.push(0x00);
    v.extend_from_slice(signed);
    v.push(0x00);
    v.extend_from_slice(sig);
    v
}

fn leading_zero_bits(h: &[u8]) -> u32 {
    let mut n = 0;
    for b in h {
        n += b.leading_zeros();
        if *b != 0 {
            break;
        }
    }
    n
}

/// Does this nonce satisfy the work?
fn meets(pow_input: &[u8], nonce: u64, bits: u32) -> bool {
    let h = Sha256::new()
        .chain_update(pow_input)
        .chain_update(nonce.to_le_bytes())
        .finalize();
    leading_zero_bits(&h) >= bits
}

/// Search for a nonce that does.
///
/// The prefix is absorbed once and the state cloned per attempt. Re-hashing
/// two hundred bytes of notice for every candidate made an honest post cost
/// seconds on a phone, for no security at all — an attacker would simply have
/// written the loop this way. The ratio between honest and hostile is
/// untouched; what changes is that the honest side stops paying for a mistake.
fn mine(pow_input: &[u8], bits: u32) -> u64 {
    let base = Sha256::new().chain_update(pow_input);
    let mut nonce: u64 = 0;
    loop {
        let mut h = base.clone();
        h.update(nonce.to_le_bytes());
        if leading_zero_bits(&h.finalize()) >= bits {
            return nonce;
        }
        nonce = nonce.wrapping_add(1);
    }
}

/// Sign a notice for one slot and find the work for it.
///
/// The map goes in without the three fields and comes out with them. Order
/// matters and is fixed: the signature is over the content, and the work is
/// over the content *and* the signature, so neither can be recomputed without
/// redoing the other.
///
/// Blocking, and meant to be — this is the cost. On a phone it is a second or
/// two, once, when a listing is posted or refreshed.
pub fn seal(
    mut m: BTreeMap<u64, Value>,
    f: NoticeFields,
    seed: &[u8; 32],
    board: &str,
    subkey: u32,
) -> Value {
    let key = SecretKey::ed25519_from_bytes(seed);
    let body = Value::Map(m.clone()).encode();
    let signed = signed_bytes(&body, board, subkey);
    let sig = key.sign(ObjectType::BoardNotice, &signed);

    let nonce = mine(&pow_bytes(&signed, &sig), POW_BITS);

    m.insert(f.poster, Value::Bytes(key.public().to_bytes()));
    m.insert(f.sig, Value::Bytes(sig.to_vec()));
    m.insert(f.pow, Value::Uint(nonce));
    Value::Map(m)
}

/// Check a notice's author and its work, and hand back the notice without them.
///
/// Returns the poster's public key and the map with the three fields removed,
/// so the caller's existing strict reader sees exactly what it saw before —
/// including its refusal of any field it does not know, which is what stops a
/// notice smuggling something past by hiding it among these.
///
/// There is no unsigned path. A notice missing either field is refused, not
/// treated as legacy: an accepted unsigned notice is an attacker's way to skip
/// the work entirely, and a defence with an opt-out is not one.
pub fn open(
    v: Value,
    f: NoticeFields,
    board: &str,
    subkey: u32,
) -> Result<(Vec<u8>, Value), Reject> {
    let Value::Map(mut m) = v else {
        return Err(Reject::with_detail(
            RejectCode::Malformed,
            "a notice is a map",
        ));
    };

    let bad = |d: &str| Reject::with_detail(RejectCode::Malformed, d.to_string());

    let poster = match m.remove(&f.poster) {
        Some(Value::Bytes(b)) => b,
        _ => return Err(bad("a board notice must say who wrote it")),
    };
    let sig = match m.remove(&f.sig) {
        Some(Value::Bytes(b)) => b,
        _ => return Err(bad("a board notice must be signed")),
    };
    let nonce = match m.remove(&f.pow) {
        Some(Value::Uint(n)) => n,
        _ => return Err(bad("a board notice must carry its proof of work")),
    };

    let sig64: [u8; 64] = sig
        .as_slice()
        .try_into()
        .map_err(|_| bad("a signature is 64 bytes"))?;
    let pk = PublicKey::from_bytes(Suite::Ed25519X25519, &poster)
        .map_err(|_| bad("that is not a verifying key"))?;

    // Re-encoded from the decoded map rather than sliced out of the wire
    // bytes. Normally that is the wrong instinct — sig.rs says as much, and
    // means it — because a decoder that tolerates two encodings of one value
    // would let a signature verify against bytes nobody signed.
    //
    // It is safe here, and only here, because cbor.rs refuses non-canonical
    // input outright: decoding succeeding *means* the input was exactly the
    // canonical encoding, so this reproduces the signer's bytes byte for byte.
    // The map is a BTreeMap, so re-encoding cannot reorder anything either.
    let body = Value::Map(m.clone()).encode();
    let signed = signed_bytes(&body, board, subkey);

    pk.verify_raw(ObjectType::BoardNotice, &signed, &sig64)
        .map_err(|_| bad("this notice was not signed for this slot"))?;

    if !meets(&pow_bytes(&signed, &sig64), nonce, POW_BITS) {
        return Err(bad("this notice did not pay for its slot"));
    }

    Ok((poster, Value::Map(m)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body() -> BTreeMap<u64, Value> {
        let mut m = BTreeMap::new();
        m.insert(1u64, Value::Text("Sunny room near the park".into()));
        m.insert(2u64, Value::Uint(500_000));
        m
    }

    const F: NoticeFields = NoticeFields { poster: 90, sig: 91, pow: 92 };

    #[test]
    fn a_sealed_notice_opens() {
        let seed = listing_seed(b"persona-secret", "listing-1");
        let v = seal(body(), F, &seed, "board-a", 3);
        let (poster, inner) = open(v, F, "board-a", 3).expect("opens");
        assert_eq!(poster.len(), 32);
        // What comes back is what went in: the three fields are gone, so the
        // notice's own strict reader sees exactly what it always saw.
        assert_eq!(inner, Value::Map(body()));
    }

    #[test]
    fn a_signature_does_not_travel_to_another_slot() {
        let seed = listing_seed(b"persona-secret", "listing-1");
        let v = seal(body(), F, &seed, "board-a", 3);
        // The same bytes, offered as slot 4. Without the slot in the signature
        // an attacker could paper a whole cell with one signed listing.
        assert!(open(v.clone(), F, "board-a", 4).is_err());
        // And onto another shard of the same cell.
        assert!(open(v, F, "board-a-1", 3).is_err());
    }

    #[test]
    fn edited_content_does_not_verify() {
        let seed = listing_seed(b"persona-secret", "listing-1");
        let v = seal(body(), F, &seed, "board-a", 3);
        let Value::Map(mut m) = v else { unreachable!() };
        m.insert(2u64, Value::Uint(1));
        assert!(open(Value::Map(m), F, "board-a", 3).is_err());
    }

    #[test]
    fn a_notice_without_the_fields_is_refused() {
        // The downgrade that would make all of this decorative.
        assert!(open(Value::Map(body()), F, "board-a", 3).is_err());

        let seed = listing_seed(b"persona-secret", "listing-1");
        for drop in [F.poster, F.sig, F.pow] {
            let Value::Map(mut m) = seal(body(), F, &seed, "board-a", 3) else { unreachable!() };
            m.remove(&drop);
            assert!(
                open(Value::Map(m), F, "board-a", 3).is_err(),
                "a notice missing field {drop} was accepted"
            );
        }
    }

    #[test]
    fn work_is_actually_checked() {
        let seed = listing_seed(b"persona-secret", "listing-1");
        let Value::Map(mut m) = seal(body(), F, &seed, "board-a", 3) else { unreachable!() };
        // Same author, same content, same slot — a nonce that does no work.
        m.insert(F.pow, Value::Uint(1));
        let e = open(Value::Map(m), F, "board-a", 3).unwrap_err();
        assert!(format!("{e:?}").contains("pay"), "{e:?}");
    }

    #[test]
    fn a_listing_key_is_its_own() {
        let a = listing_seed(b"persona-secret", "listing-1");
        let b = listing_seed(b"persona-secret", "listing-2");
        let c = listing_seed(b"other-secret", "listing-1");
        // Stable for one listing, so a reader can recognise it across
        // refreshes...
        assert_eq!(a, listing_seed(b"persona-secret", "listing-1"));
        // ...and unlinkable to the poster's other listings, or to the persona.
        assert_ne!(a, b);
        assert_ne!(a, c);
    }

    /// Not a test of behaviour — a measurement, so POW_BITS is chosen against
    /// a number rather than a guess. `cargo test -r -p ducat-core -- --ignored
    /// --nocapture pow_cost`
    #[test]
    #[ignore]
    fn pow_cost() {
        let seed = listing_seed(b"persona-secret", "listing-1");
        let t0 = std::time::Instant::now();
        let n = 20;
        for i in 0..n {
            let _ = seal(body(), F, &seed, "board-a", i);
        }
        let each = t0.elapsed().as_secs_f64() / n as f64;
        println!("POW_BITS={POW_BITS}: {each:.3}s per notice on this machine");
        println!("  a full cell (128 slots) costs {:.1}s", each * 128.0);
    }

    #[test]
    fn zero_bits_are_counted_across_bytes() {
        assert_eq!(leading_zero_bits(&[0xFF]), 0);
        assert_eq!(leading_zero_bits(&[0x7F]), 1);
        assert_eq!(leading_zero_bits(&[0x00, 0xFF]), 8);
        assert_eq!(leading_zero_bits(&[0x00, 0x00, 0x80]), 16);
        assert_eq!(leading_zero_bits(&[0x00, 0x00, 0x00]), 24);
    }
}
