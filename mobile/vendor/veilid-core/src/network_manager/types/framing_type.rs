use super::*;

/// High-level protocol framing: a continuous byte stream (Connection) or
/// discrete messages (Message). Orthogonal to the underlying socket type
/// (e.g. WebTransport-stream is Connection framing over a Datagram socket).
#[allow(clippy::derived_hash_with_manual_eq)]
#[derive(Debug, PartialOrd, Ord, Hash, EnumSetType, Serialize, Deserialize)]
#[enumset(repr = "u8")]
pub(crate) enum FramingType {
    Connection,
    Message,
}

impl fmt::Display for FramingType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if f.alternate() {
            match self {
                FramingType::Connection => write!(f, "CONN"),
                FramingType::Message => write!(f, "MESG"),
            }
        } else {
            match self {
                FramingType::Connection => write!(f, "Connection"),
                FramingType::Message => write!(f, "Message"),
            }
        }
    }
}

pub(crate) type FramingTypeSet = EnumSet<FramingType>;
