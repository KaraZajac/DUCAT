/// Length of a crypto key in bytes
pub const VLD0_PUBLIC_KEY_LENGTH: usize = 32;
/// Length of a secret key in bytes
pub const VLD0_SECRET_KEY_LENGTH: usize = 32;
/// Length of a signature in bytes
pub const VLD0_SIGNATURE_LENGTH: usize = 64;
/// Length of a nonce in bytes
pub const VLD0_NONCE_LENGTH: usize = 24;
/// Length of a hash digest in bytes
pub const VLD0_HASH_DIGEST_LENGTH: usize = 32;
/// Length of a shared secret in bytes
pub const VLD0_SHARED_SECRET_LENGTH: usize = 32;
/// Length of a KEM encapsulation key in bytes (x25519 public key)
pub const VLD0_ENCAPSULATION_KEY_LENGTH: usize = 32;
/// Length of a KEM decapsulation key in bytes (x25519 secret key)
pub const VLD0_DECAPSULATION_KEY_LENGTH: usize = 32;
/// Length of the `enc` field of an HPKE sealed blob in bytes (DHKEM X25519 KEM ciphertext)
pub const VLD0_HPKE_ENC_LENGTH: usize = 32;
