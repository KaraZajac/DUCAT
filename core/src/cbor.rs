//! Deterministic CBOR, per protocol §18.1.
//!
//! This is hand-rolled rather than delegated to a serde CBOR crate for one
//! reason: §18.1 constrains *decoding* as tightly as encoding. A signature is
//! verified over received bytes (§18.3), and those bytes must independently be
//! proven canonical — otherwise a sender who encodes non-canonically produces an
//! object that verifies but hashes differently for the two parties, and every
//! commitment in the protocol (`offer_commit`, the §6 message chain,
//! `H(RECEIPT)`) silently diverges. Serde CBOR crates accept non-canonical input
//! by design. We must refuse it.
//!
//! Restrictions beyond RFC 8949 §4.2.1, all from §18.1:
//!   * map keys are unsigned integers only (COSE convention, and a size
//!     decision — string keys are unaffordable at a 190-byte token budget)
//!   * no floats, ever, in any position (§18.2 — money is integers)
//!   * no tags at all (the allowlist is currently empty)
//!   * no indefinite-length items
//!   * text is UTF-8 and appears only in advisory display fields

use std::collections::BTreeMap;

/// Everything that can appear on the wire. Deliberately missing: floats, tags,
/// indefinite-length anything, and non-integer map keys.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    /// Major type 0.
    Uint(u64),
    /// Major type 1. Holds the *encoded* value n, representing -1 - n.
    Nint(u64),
    /// Major type 2.
    Bytes(Vec<u8>),
    /// Major type 3, UTF-8 checked.
    Text(String),
    /// Major type 4.
    Array(Vec<Value>),
    /// Major type 5. BTreeMap gives canonical key order for free, and makes
    /// duplicate keys unrepresentable rather than merely detectable.
    Map(BTreeMap<u64, Value>),
    /// Major type 7, simple values only.
    Bool(bool),
    Null,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodecError {
    /// Input ended mid-item.
    Truncated,
    /// Trailing bytes after a complete top-level item. Always an error: a
    /// signed object is exactly its bytes, never a prefix of them.
    TrailingBytes(usize),
    /// An integer used a longer encoding than necessary (§18.1 smallest form).
    NonCanonicalInt,
    /// Map keys were not in ascending order, or repeated.
    NonCanonicalMapOrder,
    /// Indefinite-length item.
    IndefiniteLength,
    /// A float appeared. Never legal (§18.2).
    FloatForbidden,
    /// A tag appeared. The allowlist is empty (§18.1).
    TagForbidden,
    /// A map key was not an unsigned integer.
    NonIntegerMapKey,
    /// Reserved/unassigned simple value or additional-information pattern.
    Malformed,
    /// Text was not valid UTF-8.
    InvalidUtf8,
    /// Nesting deeper than the configured limit.
    TooDeep,
}

/// Bound on recursion. A hostile 1 KB payload can otherwise nest thousands of
/// arrays deep and blow the stack during decode — cheap for a sender, fatal for
/// a phone. The protocol's real objects nest ~4 deep.
pub const MAX_DEPTH: usize = 16;

// ---------------------------------------------------------------- encoding --

/// Encode a head: major type in the top 3 bits, plus the shortest possible
/// representation of `n`. This function is the single place canonical integer
/// form is decided, so encoding cannot disagree with the decoder's check.
fn put_head(out: &mut Vec<u8>, major: u8, n: u64) {
    let mt = major << 5;
    match n {
        0..=23 => out.push(mt | n as u8),
        24..=0xFF => {
            out.push(mt | 24);
            out.push(n as u8);
        }
        0x100..=0xFFFF => {
            out.push(mt | 25);
            out.extend_from_slice(&(n as u16).to_be_bytes());
        }
        0x1_0000..=0xFFFF_FFFF => {
            out.push(mt | 26);
            out.extend_from_slice(&(n as u32).to_be_bytes());
        }
        _ => {
            out.push(mt | 27);
            out.extend_from_slice(&n.to_be_bytes());
        }
    }
}

impl Value {
    /// Encode to canonical form. Infallible by construction: `Value` cannot
    /// represent anything non-canonical.
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        self.encode_into(&mut out);
        out
    }

    fn encode_into(&self, out: &mut Vec<u8>) {
        match self {
            Value::Uint(n) => put_head(out, 0, *n),
            Value::Nint(n) => put_head(out, 1, *n),
            Value::Bytes(b) => {
                put_head(out, 2, b.len() as u64);
                out.extend_from_slice(b);
            }
            Value::Text(s) => {
                put_head(out, 3, s.len() as u64);
                out.extend_from_slice(s.as_bytes());
            }
            Value::Array(items) => {
                put_head(out, 4, items.len() as u64);
                for v in items {
                    v.encode_into(out);
                }
            }
            Value::Map(m) => {
                put_head(out, 5, m.len() as u64);
                // BTreeMap iterates in ascending key order. For unsigned integer
                // keys, numeric order and canonical-encoding bytewise order
                // agree, because put_head is length-then-magnitude monotonic.
                for (k, v) in m {
                    put_head(out, 0, *k);
                    v.encode_into(out);
                }
            }
            Value::Bool(b) => out.push(0xE0 | if *b { 21 } else { 20 }),
            Value::Null => out.push(0xE0 | 22),
        }
    }
}

// ---------------------------------------------------------------- decoding --

struct Decoder<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Decoder<'a> {
    fn byte(&mut self) -> Result<u8, CodecError> {
        let b = *self.buf.get(self.pos).ok_or(CodecError::Truncated)?;
        self.pos += 1;
        Ok(b)
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], CodecError> {
        let end = self.pos.checked_add(n).ok_or(CodecError::Truncated)?;
        let s = self.buf.get(self.pos..end).ok_or(CodecError::Truncated)?;
        self.pos = end;
        Ok(s)
    }

    /// Read an argument, enforcing shortest-form encoding.
    fn head(&mut self) -> Result<(u8, u64), CodecError> {
        let ib = self.byte()?;
        let major = ib >> 5;
        let ai = ib & 0x1F;
        let n = match ai {
            0..=23 => ai as u64,
            24 => {
                let v = self.byte()? as u64;
                // 0..=23 must have used the immediate form.
                if v < 24 {
                    return Err(CodecError::NonCanonicalInt);
                }
                v
            }
            25 => {
                let b = self.take(2)?;
                let v = u16::from_be_bytes([b[0], b[1]]) as u64;
                if v <= 0xFF {
                    return Err(CodecError::NonCanonicalInt);
                }
                v
            }
            26 => {
                let b = self.take(4)?;
                let v = u32::from_be_bytes([b[0], b[1], b[2], b[3]]) as u64;
                if v <= 0xFFFF {
                    return Err(CodecError::NonCanonicalInt);
                }
                v
            }
            27 => {
                let b = self.take(8)?;
                let v = u64::from_be_bytes([
                    b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
                ]);
                if v <= 0xFFFF_FFFF {
                    return Err(CodecError::NonCanonicalInt);
                }
                v
            }
            31 => return Err(CodecError::IndefiniteLength),
            _ => return Err(CodecError::Malformed), // 28..=30 unassigned
        };
        Ok((major, n))
    }

    fn value(&mut self, depth: usize) -> Result<Value, CodecError> {
        if depth > MAX_DEPTH {
            return Err(CodecError::TooDeep);
        }
        // Peek for the cases where the head rules differ from the general form.
        let ib = *self.buf.get(self.pos).ok_or(CodecError::Truncated)?;
        let major = ib >> 5;
        let ai = ib & 0x1F;

        if major == 6 {
            return Err(CodecError::TagForbidden);
        }
        if major == 7 {
            self.pos += 1;
            return match ai {
                20 => Ok(Value::Bool(false)),
                21 => Ok(Value::Bool(true)),
                22 => Ok(Value::Null),
                // 25/26/27 are half/single/double floats.
                25 | 26 | 27 => Err(CodecError::FloatForbidden),
                31 => Err(CodecError::IndefiniteLength),
                _ => Err(CodecError::Malformed),
            };
        }

        let (major, n) = self.head()?;
        match major {
            0 => Ok(Value::Uint(n)),
            1 => Ok(Value::Nint(n)),
            2 => {
                let b = self.take(usize_of(n)?)?;
                Ok(Value::Bytes(b.to_vec()))
            }
            3 => {
                let b = self.take(usize_of(n)?)?;
                let s = std::str::from_utf8(b).map_err(|_| CodecError::InvalidUtf8)?;
                Ok(Value::Text(s.to_string()))
            }
            4 => {
                let len = usize_of(n)?;
                // Do not pre-allocate from a length field: a 4-byte header can
                // claim 2^32 items and OOM the process before any data arrives.
                let mut items = Vec::new();
                for _ in 0..len {
                    items.push(self.value(depth + 1)?);
                }
                Ok(Value::Array(items))
            }
            5 => {
                let len = usize_of(n)?;
                let mut m = BTreeMap::new();
                let mut prev: Option<u64> = None;
                for _ in 0..len {
                    // Keys must be unsigned integers, in strictly ascending
                    // order. Ascending-and-distinct is checked here rather than
                    // inferred from BTreeMap, so that a duplicate or misordered
                    // key is a decode error instead of a silent overwrite.
                    let (kmajor, k) = self.head()?;
                    if kmajor != 0 {
                        return Err(CodecError::NonIntegerMapKey);
                    }
                    if let Some(p) = prev {
                        if k <= p {
                            return Err(CodecError::NonCanonicalMapOrder);
                        }
                    }
                    prev = Some(k);
                    let v = self.value(depth + 1)?;
                    m.insert(k, v);
                }
                Ok(Value::Map(m))
            }
            _ => Err(CodecError::Malformed),
        }
    }
}

fn usize_of(n: u64) -> Result<usize, CodecError> {
    usize::try_from(n).map_err(|_| CodecError::Truncated)
}

/// Decode one complete item, rejecting anything non-canonical and anything
/// trailing. Success means the input is *exactly* the canonical encoding of the
/// returned value — so `decode(b).encode() == b` always holds.
pub fn decode(buf: &[u8]) -> Result<Value, CodecError> {
    let mut d = Decoder { buf, pos: 0 };
    let v = d.value(0)?;
    if d.pos != buf.len() {
        return Err(CodecError::TrailingBytes(buf.len() - d.pos));
    }
    Ok(v)
}

// -------------------------------------------------------- ergonomic access --

impl Value {
    pub fn as_map(&self) -> Option<&BTreeMap<u64, Value>> {
        match self {
            Value::Map(m) => Some(m),
            _ => None,
        }
    }
    pub fn as_uint(&self) -> Option<u64> {
        match self {
            Value::Uint(n) => Some(*n),
            _ => None,
        }
    }
    pub fn as_bytes(&self) -> Option<&[u8]> {
        match self {
            Value::Bytes(b) => Some(b),
            _ => None,
        }
    }
    pub fn as_text(&self) -> Option<&str> {
        match self {
            Value::Text(s) => Some(s),
            _ => None,
        }
    }
}

/// Build a map without hand-rolling BTreeMap plumbing at every call site.
#[macro_export]
macro_rules! cbor_map {
    ($($k:expr => $v:expr),* $(,)?) => {{
        let mut m = ::std::collections::BTreeMap::new();
        $( m.insert($k as u64, $v); )*
        $crate::cbor::Value::Map(m)
    }};
}
