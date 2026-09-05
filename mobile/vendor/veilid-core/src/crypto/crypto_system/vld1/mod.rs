use super::*;

/// [`CryptoKind`] fourcc identifying the VLD1 cryptosystem.
pub const CRYPTO_KIND_VLD1: CryptoKind = CryptoKind::new(*b"VLD1");
/// The VLD1 fourcc as a big-endian `u32`.
pub const CRYPTO_KIND_VLD1_FOURCC: u32 = u32::from_be_bytes(*b"VLD1");
