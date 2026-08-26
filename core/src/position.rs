//! The live-position stream after a ride's accept (§15.12).
//!
//! §5.2.3's disclosure ladder ends at *"during service: live position, over
//! E2EE"* and refuses every rung before it. This is that rung's payload: not
//! messages, but a single DHT record the sender overwrites on a fixed cadence,
//! its key handed over once inside the sealed thread (`contact::PositionRef`).
//! The record has a *now* and no past by construction — a chat history that
//! doubled as a movement log would be the surveillance database §5.2.3 refused,
//! rebuilt inside the E2EE.
//!
//! What the network sees, stated honestly: a value of one constant length
//! rewritten on a fixed cadence. Liveness and cadence leak; content and
//! parties do not, because the key is random and the reference was sealed.
//!
//! Three properties this module enforces, and one it cannot:
//!
//! - **Constant length.** Every frame pads to [`FRAME_LEN`] before sealing, so
//!   the ciphertext sequence carries nothing but its own heartbeat. A frame
//!   with a heading and one without are the same size on the wire.
//! - **Bound to its record.** The record key is the AEAD's associated data, so
//!   a value lifted from one ride's record cannot authenticate in another's —
//!   which is what stops a fresh key from silently linking two rides.
//! - **A monotonic counter**, carried in the plaintext, so a receiver can
//!   refuse an old frame replayed inside the ride.
//! - The counter check itself is *stateful* and therefore the caller's: this
//!   module parses the counter, the reader remembers the last one and drops a
//!   non-increasing frame (§15.12). Decoding a frame in isolation cannot know
//!   what came before it.

use crate::reject::{Reject, RejectCode};

/// The plaintext length every frame pads to before it is sealed.
///
/// 34 bytes are used; the rest is zero padding whose only job is to make every
/// ciphertext identical in length. Rounded to 64 so the number is legible and
/// leaves room for a field to grow without moving the wire size.
pub const FRAME_LEN: usize = 64;

/// The 24-byte XChaCha nonce rides in front of the ciphertext, so the reader
/// has it without a second field. `nonce (24) || ciphertext (FRAME_LEN + 16
/// tag)` — one constant length, [`SEALED_LEN`].
pub const NONCE_LEN: usize = 24;
const TAG_LEN: usize = 16;
/// The whole DHT value: nonce, then the sealed fixed-size frame. Constant.
pub const SEALED_LEN: usize = NONCE_LEN + FRAME_LEN + TAG_LEN;

/// Absent heading, in the two heading bytes. A real heading is 0..=359.
const HEADING_NONE: u16 = 0xFFFF;

/// One position update (§15.12).
///
/// Position is §15.12's 1e-7-degree integers, the same units the geocells use,
/// so nothing is re-projected. Heading is optional degrees. The capture time is
/// the sender's clock — a receiver renders staleness *as staleness* ("last seen
/// 40 s ago"), never a guessed position, so a stale clock costs honesty, not a
/// wrong dot on a map.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PositionFrame {
    /// Monotonic within one ride's stream; a receiver drops a non-increasing
    /// one as a replay.
    pub counter: u64,
    /// Latitude in 1e-7 degrees, [-900_000_000, 900_000_000].
    pub lat_e7: i64,
    /// Longitude in 1e-7 degrees, [-1_800_000_000, 1_800_000_000].
    pub lon_e7: i64,
    /// Heading in whole degrees, 0..=359, or `None`.
    pub heading: Option<u16>,
    /// The sender's capture time, unix seconds.
    pub captured: u64,
}

/// The furthest a latitude/longitude integer may sit from zero (±90°, ±180°).
const LAT_MAX: i64 = 900_000_000;
const LON_MAX: i64 = 1_800_000_000;

impl PositionFrame {
    /// The fixed-size plaintext, padded to [`FRAME_LEN`]. Big-endian so a
    /// hexdump reads in order; the encoding is by hand rather than CBOR
    /// precisely because CBOR's length varies with the values.
    fn to_plaintext(&self) -> [u8; FRAME_LEN] {
        let mut b = [0u8; FRAME_LEN];
        b[0..8].copy_from_slice(&self.counter.to_be_bytes());
        b[8..16].copy_from_slice(&self.lat_e7.to_be_bytes());
        b[16..24].copy_from_slice(&self.lon_e7.to_be_bytes());
        b[24..26].copy_from_slice(&self.heading.unwrap_or(HEADING_NONE).to_be_bytes());
        b[26..34].copy_from_slice(&self.captured.to_be_bytes());
        b
    }

    fn from_plaintext(b: &[u8]) -> Result<Self, Reject> {
        if b.len() != FRAME_LEN {
            return Err(Reject::with_detail(
                RejectCode::Malformed,
                "a position frame is a fixed length",
            ));
        }
        let counter = u64::from_be_bytes(b[0..8].try_into().unwrap());
        let lat_e7 = i64::from_be_bytes(b[8..16].try_into().unwrap());
        let lon_e7 = i64::from_be_bytes(b[16..24].try_into().unwrap());
        let heading_raw = u16::from_be_bytes(b[24..26].try_into().unwrap());
        let captured = u64::from_be_bytes(b[26..34].try_into().unwrap());
        // The padding is not "don't care": a frame is a constant length so it
        // leaks only its cadence, and a sender stuffing the pad with data would
        // be a covert channel riding the same key. Every padding byte MUST be
        // zero, and a reader refuses the frame if it is not.
        if b[34..].iter().any(|&x| x != 0) {
            return Err(Reject::with_detail(
                RejectCode::Malformed,
                "a position frame's padding must be zero",
            ));
        }
        if !(-LAT_MAX..=LAT_MAX).contains(&lat_e7) {
            return Err(Reject::with_detail(
                RejectCode::Malformed,
                "latitude out of range",
            ));
        }
        if !(-LON_MAX..=LON_MAX).contains(&lon_e7) {
            return Err(Reject::with_detail(
                RejectCode::Malformed,
                "longitude out of range",
            ));
        }
        let heading = if heading_raw == HEADING_NONE {
            None
        } else if heading_raw <= 359 {
            Some(heading_raw)
        } else {
            return Err(Reject::with_detail(
                RejectCode::Malformed,
                "heading is 0..=359 or absent",
            ));
        };
        Ok(PositionFrame {
            counter,
            lat_e7,
            lon_e7,
            heading,
            captured,
        })
    }
}

/// Seal one frame into the value written to the stream's record subkey.
///
/// `nonce` is fresh per write (the caller draws it); `record_key` is the DHT
/// record's own key, bound in as associated data so the sealed value cannot be
/// lifted into another record. Returns exactly [`SEALED_LEN`] bytes.
pub fn seal(stream_key: &[u8; 32], record_key: &str, nonce: &[u8; NONCE_LEN], frame: &PositionFrame) -> Vec<u8> {
    use chacha20poly1305::aead::{Aead, KeyInit, Payload};
    use chacha20poly1305::{XChaCha20Poly1305, XNonce};
    let cipher = XChaCha20Poly1305::new(stream_key.into());
    let ct = cipher
        .encrypt(
            XNonce::from_slice(nonce),
            Payload {
                msg: &frame.to_plaintext(),
                aad: record_key.as_bytes(),
            },
        )
        .expect("XChaCha encrypt is infallible for in-memory buffers");
    let mut out = Vec::with_capacity(SEALED_LEN);
    out.extend_from_slice(nonce);
    out.extend_from_slice(&ct);
    out
}

/// Open a value read from the stream's record.
///
/// The record key MUST be the one the value was written under (it is the AAD),
/// so a caller passes the record they fetched from — a mismatch fails to
/// authenticate rather than returning someone else's position.
pub fn open(stream_key: &[u8; 32], record_key: &str, value: &[u8]) -> Result<PositionFrame, Reject> {
    use chacha20poly1305::aead::{Aead, KeyInit, Payload};
    use chacha20poly1305::{XChaCha20Poly1305, XNonce};
    if value.len() != SEALED_LEN {
        return Err(Reject::with_detail(
            RejectCode::Malformed,
            "a sealed position frame is a fixed length",
        ));
    }
    let (nonce, ct) = value.split_at(NONCE_LEN);
    let cipher = XChaCha20Poly1305::new(stream_key.into());
    let plain = cipher
        .decrypt(
            XNonce::from_slice(nonce),
            Payload {
                msg: ct,
                aad: record_key.as_bytes(),
            },
        )
        .map_err(|_| Reject::with_detail(RejectCode::BadSig, "position frame did not authenticate"))?;
    PositionFrame::from_plaintext(&plain)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame() -> PositionFrame {
        PositionFrame {
            counter: 7,
            lat_e7: 525_000_000,
            lon_e7: 133_760_000,
            heading: Some(270),
            captured: 1_800_000_000,
        }
    }

    #[test]
    fn a_frame_round_trips() {
        let key = [0x5au8; 32];
        let rec = "VLD0:somerecordkey";
        let nonce = [0x11u8; NONCE_LEN];
        let sealed = seal(&key, rec, &nonce, &frame());
        assert_eq!(sealed.len(), SEALED_LEN);
        assert_eq!(open(&key, rec, &sealed).unwrap(), frame());
    }

    #[test]
    fn a_heading_of_none_is_the_same_length() {
        let key = [0x5au8; 32];
        let rec = "VLD0:r";
        let mut f = frame();
        f.heading = None;
        let a = seal(&key, rec, &[1u8; NONCE_LEN], &f);
        let b = seal(&key, rec, &[1u8; NONCE_LEN], &frame());
        assert_eq!(a.len(), b.len(), "a heading must not change the wire size");
        assert_eq!(open(&key, rec, &a).unwrap().heading, None);
    }

    #[test]
    fn the_record_key_is_bound_in() {
        let key = [0x5au8; 32];
        let sealed = seal(&key, "VLD0:one", &[2u8; NONCE_LEN], &frame());
        // The same key, a different record: lifting the value fails to open.
        assert!(open(&key, "VLD0:two", &sealed).is_err());
    }

    #[test]
    fn a_wrong_stream_key_does_not_open() {
        let sealed = seal(&[1u8; 32], "VLD0:r", &[3u8; NONCE_LEN], &frame());
        assert!(open(&[2u8; 32], "VLD0:r", &sealed).is_err());
    }

    #[test]
    fn out_of_range_coordinates_are_refused() {
        let key = [9u8; 32];
        let rec = "VLD0:r";
        for bad in [
            PositionFrame { lat_e7: LAT_MAX + 1, ..frame() },
            PositionFrame { lat_e7: -LAT_MAX - 1, ..frame() },
            PositionFrame { lon_e7: LON_MAX + 1, ..frame() },
        ] {
            let sealed = seal(&key, rec, &[4u8; NONCE_LEN], &bad);
            assert!(open(&key, rec, &sealed).is_err(), "accepted {bad:?}");
        }
    }

    #[test]
    fn a_heading_over_359_is_refused() {
        // Built by hand, because the constructor path would never produce it.
        let key = [9u8; 32];
        let rec = "VLD0:r";
        let mut b = frame().to_plaintext();
        b[24..26].copy_from_slice(&400u16.to_be_bytes());
        use chacha20poly1305::aead::{Aead, KeyInit, Payload};
        use chacha20poly1305::{XChaCha20Poly1305, XNonce};
        let cipher = XChaCha20Poly1305::new((&key).into());
        let ct = cipher.encrypt(XNonce::from_slice(&[5u8; NONCE_LEN]), Payload { msg: &b, aad: rec.as_bytes() }).unwrap();
        let mut v = vec![5u8; NONCE_LEN];
        v.extend_from_slice(&ct);
        assert!(open(&key, rec, &v).is_err());
    }

    #[test]
    fn non_zero_padding_is_refused() {
        // A covert channel in the pad is refused: the frame authenticates, so
        // this can only come from the legitimate sender, and a sender using the
        // padding is smuggling a second stream under the same key.
        let key = [9u8; 32];
        let rec = "VLD0:r";
        let mut b = frame().to_plaintext();
        b[FRAME_LEN - 1] = 1;
        use chacha20poly1305::aead::{Aead, KeyInit, Payload};
        use chacha20poly1305::{XChaCha20Poly1305, XNonce};
        let cipher = XChaCha20Poly1305::new((&key).into());
        let ct = cipher.encrypt(XNonce::from_slice(&[6u8; NONCE_LEN]), Payload { msg: &b, aad: rec.as_bytes() }).unwrap();
        let mut v = vec![6u8; NONCE_LEN];
        v.extend_from_slice(&ct);
        assert!(open(&key, rec, &v).is_err());
    }
}
