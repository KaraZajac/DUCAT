use super::*;

/// Socket-style classification: stream (connection-oriented) vs datagram.
#[allow(clippy::derived_hash_with_manual_eq)]
#[derive(Debug, PartialOrd, Ord, Hash, EnumSetType, Serialize, Deserialize)]
#[enumset(repr = "u8")]
pub(crate) enum SocketType {
    Stream = 0,
    Datagram = 1,
}

#[expect(dead_code)]
pub(crate) type SocketTypeSet = EnumSet<SocketType>;

impl fmt::Display for SocketType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if f.alternate() {
            match self {
                SocketType::Stream => write!(f, "STRM"),
                SocketType::Datagram => write!(f, "DGRM"),
            }
        } else {
            match self {
                SocketType::Stream => write!(f, "Stream"),
                SocketType::Datagram => write!(f, "Datagram"),
            }
        }
    }
}
