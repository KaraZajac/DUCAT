mod crypto_system;
mod dh_cache;
mod envelope;
mod guard;
mod receipt;
mod types;

#[cfg(any(test, feature = "test-util"))]
#[doc(hidden)]
pub mod tests_crypto;

pub use crypto_system::*;
use dh_cache::*;
pub(crate) use envelope::*;
pub use guard::*;
pub(crate) use receipt::*;
pub use types::*;

use super::*;
use core::convert::TryInto;
use hashlink::linked_hash_map::Entry;
use hashlink::LruCache;

impl_veilid_log_facility!("crypto");

cfg_if! {
    if #[cfg(all(feature = "enable-crypto-none", feature = "enable-crypto-vld0"))] {
        /// Crypto kinds in order of preference, best cryptosystem is the first one, worst is the last one
        pub const VALID_CRYPTO_KINDS: [CryptoKind; 2] = [CRYPTO_KIND_VLD0, CRYPTO_KIND_NONE];
    }
    else if #[cfg(feature = "enable-crypto-none")] {
        /// Crypto kinds in order of preference, best cryptosystem is the first one, worst is the last one
        pub const VALID_CRYPTO_KINDS: [CryptoKind; 1] = [CRYPTO_KIND_NONE];
    }
    else if #[cfg(feature = "enable-crypto-vld0")] {
        /// Crypto kinds in order of preference, best cryptosystem is the first one, worst is the last one
        pub const VALID_CRYPTO_KINDS: [CryptoKind; 1] = [CRYPTO_KIND_VLD0];
    }
    // else if #[cfg(feature = "enable-crypto-vld1")] {
    //     /// Crypto kinds in order of preference, best cryptosystem is the first one, worst is the last one
    //     pub const VALID_CRYPTO_KINDS: [CryptoKind; 2] = [CRYPTO_KIND_VLD1, CRYPTO_KIND_VLD0];
    // }
    else {
        compile_error!("No crypto kinds enabled, specify an enable-crypto- feature");
    }
}
/// Number of cryptosystem signatures to keep on structures if many are present beyond the ones we consider valid
pub const MAX_CRYPTO_KINDS: usize = 3;

/// Return the best cryptosystem kind we support
pub(crate) fn best_crypto_kind() -> CryptoKind {
    VALID_CRYPTO_KINDS[0]
}

struct CryptoInner {
    dh_cache: DHCache,
    dh_cache_misses: usize,
    dh_cache_hits: usize,
    dh_cache_lru: usize,
}

impl fmt::Debug for CryptoInner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CryptoInner")
            //.field("dh_cache", &self.dh_cache)
            .field("dh_cache_misses", &self.dh_cache_misses)
            .field("dh_cache_hits", &self.dh_cache_hits)
            .field("dh_cache_lru", &self.dh_cache_lru)
            // .field("crypto_vld0", &self.crypto_vld0)
            // .field("crypto_none", &self.crypto_none)
            .finish()
    }
}

/// Crypto factory implementation
#[must_use]
pub struct Crypto {
    registry: VeilidComponentRegistry,
    inner: Mutex<CryptoInner>,
    #[cfg(feature = "enable-crypto-vld0")]
    crypto_vld0: Arc<dyn CryptoSystem + Send + Sync>,
    #[cfg(feature = "enable-crypto-none")]
    crypto_none: Arc<dyn CryptoSystem + Send + Sync>,
}

impl_veilid_component!(Crypto);

impl fmt::Debug for Crypto {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Crypto")
            //.field("registry", &self.registry)
            .field("inner", &self.inner)
            // .field("crypto_vld0", &self.crypto_vld0)
            // .field("crypto_none", &self.crypto_none)
            .finish()
    }
}

impl Crypto {
    fn new_inner() -> CryptoInner {
        CryptoInner {
            dh_cache: DHCache::new(DH_CACHE_SIZE),
            dh_cache_misses: 0,
            dh_cache_hits: 0,
            dh_cache_lru: 0,
        }
    }

    pub(crate) fn new(registry: VeilidComponentRegistry) -> Self {
        Self {
            registry: registry.clone(),
            inner: Mutex::new(Self::new_inner()),
            #[cfg(feature = "enable-crypto-vld0")]
            crypto_vld0: Arc::new(vld0::CryptoSystemVLD0::new(registry.clone())),
            #[cfg(feature = "enable-crypto-none")]
            crypto_none: Arc::new(none::CryptoSystemNONE::new(registry.clone())),
        }
    }

    fn log_facilities_impl(&self) -> VeilidComponentLogFacilities {
        VeilidComponentLogFacilities::new().with_facility(
            VeilidComponentLogFacility::try_new_with_tags("crypto", ["#common"]).unwrap(),
        )
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "crypto", skip_all, err, fields(__VEILID_LOG_KEY = self.log_key()))
    )]
    #[allow(clippy::unused_async)]
    async fn init_async(&self) -> EyreResult<()> {
        // Nothing to initialize at this time
        Ok(())
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "crypto", skip_all, err, fields(__VEILID_LOG_KEY = self.log_key()))
    )]
    #[allow(clippy::unused_async)]
    async fn post_init_async(&self) -> EyreResult<()> {
        Ok(())
    }

    #[allow(clippy::unused_async)]
    async fn pre_terminate_async(&self) {}

    #[expect(clippy::unused_async)]
    async fn terminate_async(&self) {
        // Nothing to terminate at this time
    }

    /// Factory method to get a specific crypto version
    ///
    /// Returns a guard borrowing this `Crypto`; the cryptosystem stays reachable for the guard's
    /// lifetime. Non-blocking (clones an `Arc`).
    pub fn get(&self, kind: CryptoKind) -> Option<CryptoSystemGuard<'_>> {
        match kind {
            #[cfg(feature = "enable-crypto-vld0")]
            CRYPTO_KIND_VLD0 => Some(CryptoSystemGuard::new(self.crypto_vld0.clone())),
            #[cfg(feature = "enable-crypto-none")]
            CRYPTO_KIND_NONE => Some(CryptoSystemGuard::new(self.crypto_none.clone())),
            _ => None,
        }
    }

    /// Factory method to get a specific crypto version for async use
    ///
    /// Returns a guard borrowing this `Crypto`; the cryptosystem stays reachable for the guard's
    /// lifetime. Non-blocking (clones an `Arc`).
    pub fn get_async(&self, kind: CryptoKind) -> Option<AsyncCryptoSystemGuard<'_>> {
        self.get(kind).map(|x| x.as_async())
    }

    // Factory method to get the best crypto version
    pub(crate) fn best(&self) -> CryptoSystemGuard<'_> {
        self.get(best_crypto_kind()).unwrap_or_log()
    }

    // Factory method to get the best crypto version for async use
    pub(crate) fn best_async(&self) -> AsyncCryptoSystemGuard<'_> {
        self.get_async(best_crypto_kind()).unwrap_or_log()
    }

    // Convenience validators

    /// Validate a shared secret against the cryptosystem named by its kind. Fails if the kind is unsupported.
    ///
    /// Errors `VeilidAPIError::Generic` if `secret`'s kind is unsupported or its length is wrong.
    pub fn check_shared_secret(&self, secret: &SharedSecret) -> VeilidAPIResult<()> {
        let Some(vcrypto) = self.get(secret.kind()) else {
            apibail_generic!("unsupported crypto kind");
        };
        vcrypto.check_shared_secret(secret)
    }

    /// Validate a hash digest against the cryptosystem named by its kind. Fails if the kind is unsupported.
    ///
    /// Errors `VeilidAPIError::Generic` if `hash`'s kind is unsupported or its length is wrong.
    pub fn check_hash_digest(&self, hash: &HashDigest) -> VeilidAPIResult<()> {
        let Some(vcrypto) = self.get(hash.kind()) else {
            apibail_generic!("unsupported crypto kind");
        };
        vcrypto.check_hash_digest(hash)
    }
    /// Validate a public key against the cryptosystem named by its kind. Fails if the kind is unsupported.
    ///
    /// Errors `VeilidAPIError::Generic` if `key`'s kind is unsupported or its length is wrong.
    pub fn check_public_key(&self, key: &PublicKey) -> VeilidAPIResult<()> {
        let Some(vcrypto) = self.get(key.kind()) else {
            apibail_generic!("unsupported crypto kind");
        };
        vcrypto.check_public_key(key)
    }
    /// Validate a secret key against the cryptosystem named by its kind. Fails if the kind is unsupported.
    ///
    /// Errors `VeilidAPIError::Generic` if `key`'s kind is unsupported or its length is wrong.
    pub fn check_secret_key(&self, key: &SecretKey) -> VeilidAPIResult<()> {
        let Some(vcrypto) = self.get(key.kind()) else {
            apibail_generic!("unsupported crypto kind");
        };
        vcrypto.check_secret_key(key)
    }
    /// Validate a signature against the cryptosystem named by its kind. Fails if the kind is unsupported.
    ///
    /// Errors `VeilidAPIError::Generic` if `signature`'s kind is unsupported or its length is wrong.
    pub fn check_signature(&self, signature: &Signature) -> VeilidAPIResult<()> {
        let Some(vcrypto) = self.get(signature.kind()) else {
            apibail_generic!("unsupported crypto kind");
        };
        vcrypto.check_signature(signature)
    }
    /// Validate a keypair against the cryptosystem named by its kind. Fails if the kind is unsupported.
    ///
    /// Errors `VeilidAPIError::Generic` if `key_pair`'s kind is unsupported, or if the pair or either
    /// key has the wrong length.
    pub fn check_keypair(&self, key_pair: &KeyPair) -> VeilidAPIResult<()> {
        let Some(vcrypto) = self.get(key_pair.kind()) else {
            apibail_generic!("unsupported crypto kind");
        };
        vcrypto.check_keypair(key_pair)
    }

    /// BareSignature set verification
    /// Returns Some() the set of signature cryptokinds that validate and are supported
    /// Returns None if any cryptokinds are supported and do not validate
    ///
    /// Local CPU only; verifies each signature inline (no offload).
    ///
    /// A supported signature that does not match returns `Ok(None)`, not an error. Errors
    /// `VeilidAPIError::Generic` or `VeilidAPIError::ParseError` if a matching public key or
    /// signature is malformed (propagated from the underlying verify).
    pub fn verify_signatures(
        &self,
        public_keys: &[PublicKey],
        data: &[u8],
        signatures: &[Signature],
    ) -> VeilidAPIResult<Option<PublicKeyGroup>> {
        let mut out = PublicKeyGroup::with_capacity(public_keys.len());
        for signature in signatures {
            for public_key in public_keys {
                if public_key.kind() == signature.kind() {
                    if let Some(vcrypto) = self.get(signature.kind()) {
                        if !vcrypto.verify(public_key, data, signature)? {
                            return Ok(None);
                        }
                        out.add(public_key.clone());
                    }
                }
            }
        }
        Ok(Some(out))
    }

    /// BareSignature set generation
    /// Generates the set of signatures that are supported
    /// Any cryptokinds that are not supported are silently dropped
    ///
    /// Local CPU only; signs inline for each keypair (no offload).
    ///
    /// Errors `VeilidAPIError::Generic`, `VeilidAPIError::ParseError`, or `VeilidAPIError::Internal`
    /// if a supported keypair is malformed (propagated from the underlying sign).
    pub fn generate_signatures<F, R>(
        &self,
        data: &[u8],
        key_pairs: &[KeyPair],
        transform: F,
    ) -> VeilidAPIResult<Vec<R>>
    where
        F: Fn(&KeyPair, Signature) -> R,
    {
        let mut out = Vec::<R>::with_capacity(key_pairs.len());
        for kp in key_pairs {
            if let Some(vcrypto) = self.get(kp.kind()) {
                let sig = vcrypto.sign(&kp.key(), &kp.secret(), data)?;
                out.push(transform(kp, sig))
            }
        }
        Ok(out)
    }

    /// Generate keypair
    /// Does not require startup/init
    ///
    /// Errors `VeilidAPIError::Generic` if `crypto_kind` is not a supported cryptosystem.
    pub fn generate_keypair(crypto_kind: CryptoKind) -> VeilidAPIResult<KeyPair> {
        #[cfg(feature = "enable-crypto-vld0")]
        if crypto_kind == CRYPTO_KIND_VLD0 {
            let kp = vld0_generate_keypair();
            return Ok(kp);
        }
        #[cfg(feature = "enable-crypto-none")]
        if crypto_kind == CRYPTO_KIND_NONE {
            let kp = none_generate_keypair();
            return Ok(kp);
        }
        Err(VeilidAPIError::generic("invalid crypto kind"))
    }

    // Internal utilities

    fn cached_dh_internal<T: CryptoSystem>(
        &self,
        vcrypto: &T,
        key: &PublicKey,
        secret: &SecretKey,
    ) -> VeilidAPIResult<SharedSecret> {
        vcrypto.check_public_key(key)?;
        vcrypto.check_secret_key(secret)?;

        let dh_cache_key = DHCacheKey {
            key: key.clone(),
            secret: secret.clone(),
        };

        {
            let inner = &mut *self.inner.lock();
            if let Some(value) = inner.dh_cache.get(&dh_cache_key) {
                inner.dh_cache_hits += 1;
                return Ok(value.shared_secret.clone());
            }
        }
        let shared_secret = vcrypto.compute_dh(key, secret)?;

        {
            let inner = &mut *self.inner.lock();
            let res = inner.dh_cache.entry_with_callback(dh_cache_key, |_, _| {
                inner.dh_cache_lru += 1;
            });
            match res {
                Entry::Occupied(_) => {
                    inner.dh_cache_hits += 1;
                }
                Entry::Vacant(e) => {
                    inner.dh_cache_misses += 1;
                    e.insert(DHCacheValue {
                        shared_secret: shared_secret.clone(),
                    });
                }
            }
        }

        Ok(shared_secret)
    }

    pub(crate) fn validate_crypto_kind(kind: CryptoKind) -> VeilidAPIResult<()> {
        if !VALID_CRYPTO_KINDS.contains(&kind) {
            apibail_generic!("invalid crypto kind");
        }
        Ok(())
    }

    #[cfg_attr(not(feature = "debug-api"), expect(dead_code))]
    pub(crate) fn debug_info_nodeinfo(&self) -> String {
        let inner = self.inner.lock();
        format!(
            "Crypto Stats:\n    DH Cache Hits/Misses/LRU: {} / {} / {}",
            inner.dh_cache_hits, inner.dh_cache_misses, inner.dh_cache_lru
        )
    }
}
