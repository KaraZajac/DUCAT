use super::*;

// In-memory Diffie-Hellman key agreement cache
#[derive(Debug, PartialEq, Eq, Hash)]
pub struct DHCacheKey {
    pub key: PublicKey,
    pub secret: SecretKey,
}

#[derive(Debug)]
pub struct DHCacheValue {
    pub shared_secret: SharedSecret,
}

pub type DHCache = LruCache<DHCacheKey, DHCacheValue>;
pub const DH_CACHE_SIZE: usize = 4096;
