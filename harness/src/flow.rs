//! The wire objects and the framing that carries them.
//!
//! Each protocol step is one `app_call` round trip: the payer sends an object,
//! the payee replies with the next one. That maps the request/response shape
//! Veilid gives onto §18.4's alternating exchange without inventing a session
//! layer neither party needs.

use ducat_core::cbor::decode;
use ducat_core::wire::*;

/// One byte of framing so a reply can say "refused" without ambiguity. A bare
/// object would have to be distinguished by parsing, and a parser that guesses
/// what it is looking at is how two implementations diverge.
pub const MSG_REQUEST_OFFER: u8 = 1;
pub const MSG_FULL_OFFER: u8 = 2;
pub const MSG_ACCEPT: u8 = 3;
pub const MSG_TXID: u8 = 4;
pub const MSG_RECEIPT: u8 = 5;
pub const MSG_BOND: u8 = 6;
pub const MSG_REJECT: u8 = 0xFF;

pub fn frame(kind: u8, body: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(1 + body.len());
    out.push(kind);
    out.extend_from_slice(body);
    out
}

pub fn unframe(msg: &[u8]) -> Result<(u8, &[u8]), String> {
    if msg.is_empty() {
        return Err("empty message".into());
    }
    Ok((msg[0], &msg[1..]))
}

pub fn reject(detail: &str) -> Vec<u8> {
    frame(MSG_REJECT, detail.as_bytes())
}

/// Decode an object of a known type, refusing anything else.
///
/// §18.1 requires the received bytes to be canonical, and §18.3 requires
/// signatures to be checked over those exact bytes — so this returns the value
/// and the caller keeps the slice it came from.
pub fn decode_offer(b: &[u8]) -> Result<FullOffer, String> {
    FullOffer::from_value(decode(b).map_err(|e| format!("{e:?}"))?)
        .map_err(|e| format!("{e:?}"))
}

pub fn decode_accept(b: &[u8]) -> Result<Accept, String> {
    Accept::from_value(decode(b).map_err(|e| format!("{e:?}"))?).map_err(|e| format!("{e:?}"))
}

pub fn decode_receipt(b: &[u8]) -> Result<Receipt, String> {
    Receipt::from_value(decode(b).map_err(|e| format!("{e:?}"))?).map_err(|e| format!("{e:?}"))
}

pub fn decode_tap(b: &[u8]) -> Result<TapPresent, String> {
    TapPresent::from_value(decode(b).map_err(|e| format!("{e:?}"))?).map_err(|e| format!("{e:?}"))
}
