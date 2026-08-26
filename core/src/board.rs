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
/// Small, because each attempt is expensive. The old construction wanted
/// twenty bits of a SHA-256 search; this one wants eight of an Argon2id one,
/// and the honest cost lands in the same place — see [`POW_MEM_KIB`] for the
/// arithmetic and for what the change is actually buying.
///
/// Be honest about what this does and does not do. 128 slots is not many, so
/// a determined flood of one cell is not prevented by any difficulty an
/// honest phone could also pay. What changes is that flooding stops being
/// *free*: scripted, blanket, region-wide spraying has a bill, and the board's
/// write key being public no longer means writes cost nothing.
///
/// Note what it never priced, so nobody reads more into it than is there.
/// Denying a *slot* costs nothing and cannot be made to cost anything: the
/// write key is the cell name hashed, junk with no stamp at all still occupies
/// the record, and a write at `u32::MAX - 1` leaves the slot unwritable for
/// good (see `geo::STAND_EPOCH_SECS`, which is the answer to that one). What
/// a stamp prices is *readable* spam — a hundred and twenty-eight plausible
/// listings a browser has to wade through.
///
/// Deliberately not tunable per notice. A difficulty a poster could choose is
/// a difficulty an attacker chooses zero.
pub const POW_BITS: u32 = 8;

/// The stamp's memory, in KiB — and the reason it is memory at all.
///
/// **SHA-256 made the module's own cost model false.** The numbers above used
/// to say a hundred cells cost an attacker a couple of hours, and they did,
/// on a CPU. A commodity GPU runs SHA-256 some three thousand times faster
/// than one core, which turned those two hours into about two seconds; rented
/// mining hardware is faster again by orders of magnitude, and a 2^20 search
/// is beneath its noise floor. A proof of work whose cost collapses by 10^3 in
/// the attacker's hands is not pricing anything.
///
/// Argon2id is bounded by memory bandwidth rather than hash throughput, so the
/// same GPU buys perhaps one or two orders less. That is the whole gain, and
/// it is worth stating plainly rather than overselling: this does not make the
/// attacker equal to a phone, it stops them being three thousand times better.
///
/// **Four mebibytes because the reader pays too, and the reader is a phone.**
/// A sweep opens up to eighteen boards of eight slots, and every notice costs
/// one evaluation whether it is honest or not. `pow_cost` measures the whole
/// of `open` at 5.15 ms on one desktop core here — 3.11 ms of it the Argon2,
/// the rest the signature and the re-encode — so a full sweep is 0.74 s of
/// desktop CPU, two or three seconds of a phone's, against a lap that already
/// takes the better part of a minute in DHT reads. More memory is better
/// against a GPU and worse against the person browsing; this is where those
/// two meet. The ordering in [`open`] earns some of it back — the signature is
/// checked first, so unsigned junk is refused for the price of an Ed25519
/// verify and never reaches the memory-hard step.
///
/// A poster pays `2^POW_BITS` evaluations once per listing per refresh:
/// measured at 0.715 s on a desktop core, a few seconds on a phone, which is
/// where the SHA-256 construction sat before it. A full cell still costs an
/// attacker something over a minute per core — 91.5 s measured, against the
/// minute the old numbers claimed. What changed is not the price, it is that
/// the core is now the unit they cannot buy their way out of.
///
/// Pinned rather than taken from `Params::default()`, and pinned separately
/// from `backup.rs`'s: that one guards an offline file against a patient
/// attacker at 64 MiB, which here would be a minute per sweep and a denial of
/// service against the browser rather than the spammer.
pub const POW_MEM_KIB: u32 = 4096;
/// One pass. The cost is in the memory, and a second pass buys less per
/// millisecond of the reader's time than the same millisecond of memory does.
pub const POW_PASSES: u32 = 1;
/// One lane: a phone mining on the poll thread has no core to spare, and a
/// parallelism an honest poster cannot use is one the attacker keeps.
pub const POW_LANES: u32 = 1;

/// Where the three added fields live in a given notice's map.
///
/// A notice's field ids are its own namespace, so a rental and a hail number
/// these differently while sharing every line of the logic below.
#[derive(Clone, Copy)]
pub struct NoticeFields {
    pub poster: u64,
    pub sig: u64,
    pub pow: u64,
    pub beacon_height: u64,
    pub beacon_hash: u64,
}

/// §16.18's rental listing.
pub const RENTAL: NoticeFields = NoticeFields {
    poster: crate::wire::f::RN_POSTER,
    sig: crate::wire::f::RN_SIG,
    pow: crate::wire::f::RN_POW,
    beacon_height: crate::wire::f::RN_BEACON_HEIGHT,
    beacon_hash: crate::wire::f::RN_BEACON_HASH,
};

/// §16.17's hail.
pub const HAIL: NoticeFields = NoticeFields {
    poster: crate::wire::f::HN_POSTER,
    sig: crate::wire::f::HN_SIG,
    pow: crate::wire::f::HN_POW,
    beacon_height: crate::wire::f::HN_BEACON_HEIGHT,
    beacon_hash: crate::wire::f::HN_BEACON_HASH,
};

/// The block a notice is stamped against (§16.18.1).
///
/// **Without one, every stamp in the protocol's future can be mined this
/// afternoon.** A board name carries a weekly epoch and nothing else in the
/// preimage is unpredictable — cell, slot, body, signature are all the
/// poster's own — so an attacker could sit down once, mine every slot of every
/// cell in a region for every epoch of the coming year, and spend the rest of
/// it posting at no marginal cost whatever. Generations rotate boards on the
/// assumption that re-poisoning is paid for again each week; that assumption
/// was not true, and this is what makes it true.
///
/// The height rides along with the hash so a reader can decide *cheaply*
/// whether a notice is fresh — one comparison against a chain tip it already
/// knows — and only look the hash up for the heights that survive it. Checking
/// the hash is what actually matters: a beacon nobody verifies is thirty-two
/// bytes an attacker chooses, and choosing it is precomputation again.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Beacon {
    pub height: u64,
    pub hash: [u8; 32],
}

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

/// What the signature is over: the notice, the slot it is going into, and the
/// block it is stamped against.
///
/// The beacon is here rather than only in the work because it costs nothing to
/// cover it twice and it removes a question. Bound to the work alone, swapping
/// it would merely force an attacker to re-mine — no gain, but it takes a
/// paragraph to say why. Bound to the signature as well, a notice names one
/// block and cannot be made to name another at all.
fn signed_bytes(body: &[u8], board: &str, subkey: u32, beacon: &Beacon) -> Vec<u8> {
    let mut v = Vec::with_capacity(body.len() + board.len() + 48);
    v.extend_from_slice(board.as_bytes());
    v.push(0x00);
    v.extend_from_slice(&subkey.to_le_bytes());
    v.push(0x00);
    v.extend_from_slice(&beacon.height.to_le_bytes());
    v.push(0x00);
    v.extend_from_slice(&beacon.hash);
    v.push(0x00);
    v.extend_from_slice(body);
    v
}

/// The salt one notice's search runs against: everything above, plus the
/// signature, folded to sixteen bytes.
///
/// Folded because the nonce is the password and the notice is the salt, which
/// puts the two hundred bytes of listing through SHA-256 once per notice
/// instead of once per attempt. Argon2 has no midstate to clone the way the
/// old construction did, so the saving has to come from the shape of the call.
fn pow_salt(signed: &[u8], sig: &[u8]) -> [u8; 16] {
    let h = Sha256::new()
        .chain_update(b"DUCAT-POW-v1")
        .chain_update([0u8])
        .chain_update(signed)
        .chain_update([0u8])
        .chain_update(sig)
        .finalize();
    let mut salt = [0u8; 16];
    salt.copy_from_slice(&h[..16]);
    salt
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

fn argon() -> argon2::Argon2<'static> {
    // The parameters are constants of the protocol, so a failure to build them
    // is a bug here rather than a condition a caller could hit.
    let params = argon2::Params::new(POW_MEM_KIB, POW_PASSES, POW_LANES, Some(32))
        .expect("board proof-of-work parameters are valid");
    argon2::Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params)
}

/// One evaluation, into a buffer the caller owns.
fn stamp(a: &argon2::Argon2, salt: &[u8; 16], nonce: u64, blocks: &mut [argon2::Block]) -> u32 {
    let mut out = [0u8; 32];
    match a.hash_password_into_with_memory(&nonce.to_le_bytes(), salt, &mut out, blocks) {
        Ok(()) => leading_zero_bits(&out),
        // Cannot happen with pinned parameters and a fixed-size salt; treated
        // as "did not meet" rather than a panic, because this runs inside a
        // verifier that must not be crashable by anything on a public board.
        Err(_) => 0,
    }
}

/// Does this nonce satisfy the work?
fn meets(salt: &[u8; 16], nonce: u64, bits: u32) -> bool {
    let a = argon();
    let mut blocks = vec![argon2::Block::default(); a.params().block_count()];
    stamp(&a, salt, nonce, &mut blocks) >= bits
}

/// Search for a nonce that does.
///
/// The memory is allocated once and reused across attempts. Four mebibytes per
/// candidate, taken and given back a few hundred times, is a measurable slice
/// of an honest post for no security at all — and an attacker would simply
/// have written the loop this way.
fn mine(salt: &[u8; 16], bits: u32) -> u64 {
    let a = argon();
    let mut blocks = vec![argon2::Block::default(); a.params().block_count()];
    let mut nonce: u64 = 0;
    loop {
        if stamp(&a, salt, nonce, &mut blocks) >= bits {
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
    beacon: &Beacon,
) -> Value {
    let key = SecretKey::ed25519_from_bytes(seed);
    let body = Value::Map(m.clone()).encode();
    let signed = signed_bytes(&body, board, subkey, beacon);
    let sig = key.sign(ObjectType::BoardNotice, &signed);

    let nonce = mine(&pow_salt(&signed, &sig), POW_BITS);

    m.insert(f.poster, Value::Bytes(key.public().to_bytes()));
    m.insert(f.sig, Value::Bytes(sig.to_vec()));
    m.insert(f.pow, Value::Uint(nonce));
    m.insert(f.beacon_height, Value::Uint(beacon.height));
    m.insert(f.beacon_hash, Value::Bytes(beacon.hash.to_vec()));
    Value::Map(m)
}

/// A notice that opened: who wrote it, what it was stamped against, and the
/// notice itself with the five added fields taken back out.
///
/// The beacon rides out rather than being judged in here, because judging it
/// needs a chain and this file is a pure function of its arguments — the same
/// rule `geo::stand_epoch` states about the clock, and for the same reason:
/// §18.9's claim is that a case decided today decides the same way in a year,
/// which it cannot be if a decoder consults a network. Whether *this* block is
/// recent enough is the caller's, with the caller's own view of the chain.
#[derive(Debug)]
pub struct Opened {
    pub poster: Vec<u8>,
    pub beacon: Beacon,
    pub notice: Value,
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
) -> Result<Opened, Reject> {
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
    let beacon_height = match m.remove(&f.beacon_height) {
        Some(Value::Uint(n)) => n,
        _ => return Err(bad("a board notice must name the block it was stamped against")),
    };
    let beacon_hash: [u8; 32] = match m.remove(&f.beacon_hash) {
        Some(Value::Bytes(b)) => b
            .as_slice()
            .try_into()
            .map_err(|_| bad("a block hash is 32 bytes"))?,
        _ => return Err(bad("a board notice must carry the block hash it was stamped against")),
    };
    let beacon = Beacon { height: beacon_height, hash: beacon_hash };

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
    let signed = signed_bytes(&body, board, subkey, &beacon);

    // Signature first, and the order is load-bearing now that the work is
    // memory-hard: a slot full of random bytes is refused for the price of an
    // Ed25519 verify, tens of microseconds, instead of costing every reader on
    // the board four mebibytes and three milliseconds to say no. A defence
    // whose *failure* path is the expensive one is a denial of service with
    // extra steps.
    pk.verify_raw(ObjectType::BoardNotice, &signed, &sig64)
        .map_err(|_| bad("this notice was not signed for this slot"))?;

    if !meets(&pow_salt(&signed, &sig64), nonce, POW_BITS) {
        return Err(bad("this notice did not pay for its slot"));
    }

    Ok(Opened { poster, beacon, notice: Value::Map(m) })
}

/// How far back a notice's beacon may sit before it is stale (§16.18.1).
///
/// A day, not the hour a precomputation argument alone would want. The limit
/// is not the attacker, it is the reader: a phone whose node is behind, or
/// which has just come back from a week in a drawer, would otherwise reject
/// every honest notice on the board and show an empty marketplace with no
/// explanation. A day collapses the precomputation window from fifty-two
/// weeks to one, which is the whole of the gain, and leaves the slack where
/// somebody who is merely offline lives.
pub const BEACON_BLOCKS: u64 = 720;

/// A little the other way, for a reader whose tip is behind the poster's.
///
/// Two blocks: long enough that the ordinary case of a node a minute or two
/// stale does not refuse a fresh notice, short enough that it is not a window
/// anybody can post into.
///
/// **Its limit, stated rather than discovered.** A reader whose node is more
/// than two blocks — four minutes — behind the chain refuses fresh notices
/// outright rather than holding them, because this bound is the *decoder's*
/// and a decoder has only accept and reject. That is the right shape for
/// occupancy, where a made-up height must not be allowed to squat a slot, and
/// it is a real edge for anybody pointed at a node that is itself still
/// syncing. The severe version of the same failure — a tip carried across a
/// restart and a week out of date — is handled where it belongs, in the
/// reader's own state: a height it could not refresh is not treated as a tip
/// at all, and no-chain-view shows notices rather than hiding them.
pub const BEACON_AHEAD: u64 = 2;

/// Is a notice's beacon recent enough to have been mined against, given what
/// this device believes the chain tip to be?
///
/// Height only — deliberately. It is the cheap half of the test and it runs on
/// a number every client already has, so the expensive half (does that height
/// really have that hash) is asked only of the few heights that get this far.
/// Passing this is *not* freshness on its own: a beacon nobody looks up is
/// thirty-two bytes the attacker chose.
#[must_use]
pub fn beacon_in_window(beacon: &Beacon, tip_height: u64) -> bool {
    beacon.height <= tip_height.saturating_add(BEACON_AHEAD)
        && beacon.height + BEACON_BLOCKS >= tip_height
}

/// What a reader may do with a notice, once it has looked at the beacon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BeaconVerdict {
    /// Confirmed, or this device has no chain view and never claimed to.
    Show,
    /// **Cannot say yet.** Not a synonym for [`BeaconVerdict::Show`].
    Hold,
    /// Outside the window, or that height does not carry that hash.
    Refuse,
}

/// The whole of §16.18.1's freshness rule, in one place both implementations
/// can be held to.
///
/// It lives here rather than in [`open`] because it needs a chain and `open`
/// must be a pure function of its arguments — but it is still a *decision the
/// protocol makes*, not a matter of local taste, so it is written down and
/// pinned by `board.beacon_verdict` rather than left to each reader's prose.
///
/// **Three answers, and the third is the point.** The cheap half — is the
/// height inside the window — is free and forgeable on its own: Monero aims
/// at a block every two minutes, so a height months out is predictable to
/// within a few hundred, and an attacker can mine a spread of future heights
/// against block hashes they simply invented. Every reader that stops at the
/// height comparison takes all of them, and precomputation is back in full.
/// So `known_hash` of `None` — the height is above this reader's tip, or the
/// lookup failed, or its budget was spent — is [`BeaconVerdict::Hold`], never
/// `Show`. It becomes knowable in minutes.
///
/// `tip_height` of zero is the one case that skips everything: a device with
/// no chain view at all judges a notice on its signature and its work, which
/// is what it did before beacons existed. Reading a board has never required
/// a Monero node, and a marketplace that goes dark because a daemon is
/// unreachable is a worse answer than the spam it was avoiding.
///
/// A reader deciding **occupancy** rather than display — a poster asking
/// which slots are spoken for — uses [`beacon_in_window`] directly and
/// deliberately: a notice it merely cannot confirm *yet* is most likely an
/// honest one from a node slightly ahead, and writing over it would do the
/// damage this exists to prevent.
#[must_use]
pub fn beacon_verdict(
    beacon: &Beacon,
    tip_height: u64,
    known_hash: Option<&[u8; 32]>,
) -> BeaconVerdict {
    if tip_height == 0 {
        return BeaconVerdict::Show;
    }
    if !beacon_in_window(beacon, tip_height) {
        return BeaconVerdict::Refuse;
    }
    match known_hash {
        None => BeaconVerdict::Hold,
        Some(h) if *h == beacon.hash => BeaconVerdict::Show,
        Some(_) => BeaconVerdict::Refuse,
    }
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

    const F: NoticeFields = NoticeFields {
        poster: 90,
        sig: 91,
        pow: 92,
        beacon_height: 93,
        beacon_hash: 94,
    };

    fn beacon() -> Beacon {
        Beacon { height: 3_210_000, hash: [0x5au8; 32] }
    }

    #[test]
    fn a_sealed_notice_opens() {
        let seed = listing_seed(b"persona-secret", "listing-1");
        let v = seal(body(), F, &seed, "board-a", 3, &beacon());
        let o = open(v, F, "board-a", 3).expect("opens");
        assert_eq!(o.poster.len(), 32);
        // The beacon comes back out for the caller to judge against a chain.
        assert_eq!(o.beacon, beacon());
        // What comes back is what went in: the five fields are gone, so the
        // notice's own strict reader sees exactly what it always saw.
        assert_eq!(o.notice, Value::Map(body()));
    }

    #[test]
    fn a_signature_does_not_travel_to_another_slot() {
        let seed = listing_seed(b"persona-secret", "listing-1");
        let v = seal(body(), F, &seed, "board-a", 3, &beacon());
        // The same bytes, offered as slot 4. Without the slot in the signature
        // an attacker could paper a whole cell with one signed listing.
        assert!(open(v.clone(), F, "board-a", 4).is_err());
        // And onto another shard of the same cell.
        assert!(open(v, F, "board-a-1", 3).is_err());
    }

    #[test]
    fn edited_content_does_not_verify() {
        let seed = listing_seed(b"persona-secret", "listing-1");
        let v = seal(body(), F, &seed, "board-a", 3, &beacon());
        let Value::Map(mut m) = v else { unreachable!() };
        m.insert(2u64, Value::Uint(1));
        assert!(open(Value::Map(m), F, "board-a", 3).is_err());
    }

    /// The whole point of the beacon: it cannot be restated after the fact.
    #[test]
    fn the_beacon_cannot_be_swapped() {
        let seed = listing_seed(b"persona-secret", "listing-1");
        let v = seal(body(), F, &seed, "board-a", 3, &beacon());
        let Value::Map(base) = v else { unreachable!() };

        // A different block, claiming the same work: the signature covers the
        // beacon, so this fails before the stamp is even considered.
        let mut m = base.clone();
        m.insert(F.beacon_hash, Value::Bytes(vec![0x11u8; 32]));
        assert!(open(Value::Map(m), F, "board-a", 3).is_err());

        // And the height alone, which is the field a reader tests cheaply —
        // an attacker who could move it would mine once and claim any tip.
        let mut m = base;
        m.insert(F.beacon_height, Value::Uint(9_999_999));
        assert!(open(Value::Map(m), F, "board-a", 3).is_err());
    }

    #[test]
    fn a_block_hash_is_thirty_two_bytes() {
        let seed = listing_seed(b"persona-secret", "listing-1");
        let Value::Map(mut m) = seal(body(), F, &seed, "board-a", 3, &beacon()) else {
            unreachable!()
        };
        m.insert(F.beacon_hash, Value::Bytes(vec![0x5au8; 31]));
        let e = open(Value::Map(m), F, "board-a", 3).unwrap_err();
        assert!(format!("{e:?}").contains("32 bytes"), "{e:?}");
    }

    #[test]
    fn a_notice_without_the_fields_is_refused() {
        // The downgrade that would make all of this decorative.
        assert!(open(Value::Map(body()), F, "board-a", 3).is_err());

        let seed = listing_seed(b"persona-secret", "listing-1");
        for drop in [F.poster, F.sig, F.pow, F.beacon_height, F.beacon_hash] {
            let Value::Map(mut m) = seal(body(), F, &seed, "board-a", 3, &beacon()) else {
                unreachable!()
            };
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
        let Value::Map(mut m) = seal(body(), F, &seed, "board-a", 3, &beacon()) else {
            unreachable!()
        };
        // Same author, same content, same slot — a nonce that does no work.
        // 0 rather than 1: a search this short can legitimately end at a small
        // nonce, and a test that asserts against the answer is a flake.
        m.insert(F.pow, Value::Uint(u64::MAX));
        let e = open(Value::Map(m), F, "board-a", 3).unwrap_err();
        assert!(format!("{e:?}").contains("pay"), "{e:?}");
    }

    /// The window is a *range*, and both of its edges are pinned — one past
    /// each is refused. A bound tested from one side is a bound the other
    /// implementation gets to choose (§18.9's own lesson about enumerations).
    #[test]
    fn the_beacon_window_has_two_edges() {
        let tip = 3_210_000u64;
        let at = |h: u64| Beacon { height: h, hash: [0u8; 32] };

        // The tip itself, and the oldest block still inside the window.
        assert!(beacon_in_window(&at(tip), tip));
        assert!(beacon_in_window(&at(tip - BEACON_BLOCKS), tip));
        // One block older than that is stale.
        assert!(!beacon_in_window(&at(tip - BEACON_BLOCKS - 1), tip));

        // A reader whose own tip is a block or two behind the poster's still
        // accepts them...
        assert!(beacon_in_window(&at(tip + BEACON_AHEAD), tip));
        // ...but a height nobody could have mined against does not.
        assert!(!beacon_in_window(&at(tip + BEACON_AHEAD + 1), tip));

        // A device with no chain at all has a tip of zero, which must not
        // silently mean "everything is stale" — the caller skips the test
        // rather than passing a zero, and this pins what a zero does mean.
        assert!(beacon_in_window(&at(0), 0));
        assert!(!beacon_in_window(&at(0), BEACON_BLOCKS + 1));
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
            let _ = seal(body(), F, &seed, "board-a", i, &beacon());
        }
        let each = t0.elapsed().as_secs_f64() / n as f64;
        println!(
            "POW_BITS={POW_BITS} mem={POW_MEM_KIB}KiB t={POW_PASSES}: \
             {each:.3}s per notice on this machine"
        );
        println!("  a full cell (128 slots) costs {:.1}s", each * 128.0);

        // The other half of the trade, and the one that lands on a phone
        // somebody is merely browsing with: a reader pays one evaluation per
        // notice, honest or not.
        let v = seal(body(), F, &seed, "board-a", 3, &beacon());
        let t1 = std::time::Instant::now();
        let reads = 20;
        for _ in 0..reads {
            let _ = open(v.clone(), F, "board-a", 3).expect("opens");
        }
        let per = t1.elapsed().as_secs_f64() / reads as f64;
        println!("  one verify costs {:.2}ms", per * 1000.0);
        println!("  a sweep of 18 boards x 8 slots costs {:.2}s", per * 144.0);
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
