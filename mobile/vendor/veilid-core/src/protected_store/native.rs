use super::*;
use data_encoding::BASE64URL_NOPAD;
use keyring_manager::*;
use std::path::Path;

impl_veilid_log_facility!("pstore");

/// Mutable interior of the `ProtectedStore`, holding the open keyring backend.
pub struct ProtectedStoreInner {
    keyring_manager: Option<KeyringManager>,
}
impl fmt::Debug for ProtectedStoreInner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProtectedStoreInner").finish()
    }
}

/// Secure key-value storage for user secrets, backed by the platform's secure keyring
/// when available and falling back to an on-disk insecure keyring when permitted by config.
#[derive(Debug)]
#[must_use]
pub struct ProtectedStore {
    registry: VeilidComponentRegistry,
    inner: Mutex<ProtectedStoreInner>,
}

impl_veilid_component!(ProtectedStore);

impl ProtectedStore {
    fn new_inner() -> ProtectedStoreInner {
        ProtectedStoreInner {
            keyring_manager: None,
        }
    }

    pub(crate) fn new(registry: VeilidComponentRegistry) -> Self {
        Self {
            registry,
            inner: Mutex::new(Self::new_inner()),
        }
    }

    #[cfg_attr(feature = "instrument", instrument(level = "trace", skip(self), fields(__VEILID_LOG_KEY = self.log_key())))]
    /// Remove every known Veilid-managed protected store key, logging any failures.
    ///
    /// Idempotent: keys already absent are skipped. Blocks on the keyring backend (OS keyring or disk) once per key.
    pub fn delete_all(&self) {
        for kpsk in &KNOWN_PROTECTED_STORE_KEYS {
            if let Err(e) = self.remove_user_secret(kpsk) {
                veilid_log!(self error "failed to delete '{}': {}", kpsk, e);
            } else {
                veilid_log!(self debug "deleted protected store key '{}'", kpsk);
            }
        }
    }

    fn log_facilities_impl(&self) -> VeilidComponentLogFacilities {
        VeilidComponentLogFacilities::new().with_facility(
            VeilidComponentLogFacility::try_new_with_tags("pstore", ["#common"]).unwrap(),
        )
    }

    #[cfg_attr(feature = "instrument", instrument(level = "debug", skip(self), err, fields(__VEILID_LOG_KEY = self.log_key())))]
    #[allow(clippy::unused_async)]
    async fn init_async(&self) -> EyreResult<()> {
        let delete = {
            let config = self.config();
            let mut inner = self.inner.lock();
            if !config.protected_store.always_use_insecure_storage {
                // Attempt to open the secure keyring
                cfg_if! {
                    if #[cfg(target_os = "android")] {
                        let maybe_km = KeyringManager::new_secure(&config.program_name, crate::veilid_api::android::get_android_globals());
                    } else {
                        let maybe_km = KeyringManager::new_secure(&config.program_name);
                    }
                }

                inner.keyring_manager = match maybe_km {
                    Ok(v) => Some(v),
                    Err(e) => {
                        veilid_log!(self info "Secure key storage service unavailable, falling back to direct disk-based storage: {}", e);
                        None
                    }
                };
            }
            if (config.protected_store.always_use_insecure_storage
                || config.protected_store.allow_insecure_fallback)
                && inner.keyring_manager.is_none()
            {
                let directory = Path::new(&config.protected_store.directory);
                let insecure_keyring_file = directory.to_owned().join(format!(
                    "insecure_keyring{}",
                    if config.namespace.is_empty() {
                        "".to_owned()
                    } else {
                        format!("_{}", config.namespace)
                    }
                ));

                // Ensure permissions are correct
                ensure_file_private_owner(&insecure_keyring_file).map_err(|e| eyre!("{}", e))?;

                // Open the insecure keyring
                inner.keyring_manager = Some(
                    KeyringManager::new_insecure(&config.program_name, &insecure_keyring_file)
                        .wrap_err("failed to create insecure keyring")?,
                );
            }
            if inner.keyring_manager.is_none() {
                bail!("Could not initialize the protected store.");
            }
            config.protected_store.delete
        };

        if delete {
            self.delete_all();
        }

        Ok(())
    }

    #[cfg_attr(feature = "instrument", instrument(level = "debug", skip(self), err, fields(__VEILID_LOG_KEY = self.log_key())))]
    #[allow(clippy::unused_async)]
    async fn post_init_async(&self) -> EyreResult<()> {
        Ok(())
    }

    #[cfg_attr(feature = "instrument", instrument(level = "debug", skip(self), fields(__VEILID_LOG_KEY = self.log_key())))]
    #[allow(clippy::unused_async)]
    async fn pre_terminate_async(&self) {}

    #[cfg_attr(feature = "instrument", instrument(level = "debug", skip(self), fields(__VEILID_LOG_KEY = self.log_key())))]
    #[allow(clippy::unused_async)]
    async fn terminate_async(&self) {
        *self.inner.lock() = Self::new_inner();
    }

    fn service_name(&self) -> String {
        let config = self.config();
        if config.namespace.is_empty() {
            "veilid_protected_store".to_owned()
        } else {
            format!("veilid_protected_store_{}", config.namespace)
        }
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", skip(self, value), ret, err, fields(__VEILID_LOG_KEY = self.log_key()))
    )]
    /// Store a string secret under a key. Returns true if a value already existed for that key.
    ///
    /// Overwrites any prior value, so re-saving the same key/value is idempotent. Holds the inner lock and blocks on the keyring backend (OS keyring or disk).
    ///
    /// Errors with [VeilidAPIError::NotInitialized] if the store has no open keyring, or [VeilidAPIError::Generic] if the keyring backend rejects the write.
    pub fn save_user_secret_string<K: AsRef<str> + fmt::Debug, V: AsRef<str> + fmt::Debug>(
        &self,
        key: K,
        value: V,
    ) -> VeilidAPIResult<bool> {
        let inner = self.inner.lock();
        inner
            .keyring_manager
            .as_ref()
            .ok_or_else(VeilidAPIError::not_initialized)?
            .with_keyring(&self.service_name(), key.as_ref(), |kr| {
                let existed = kr.get_value().is_ok();
                kr.set_value(value.as_ref())?;
                Ok(existed)
            })
            .map_err(|e| VeilidAPIError::generic(format!("failed to save user secret: {}", e)))
    }

    #[cfg_attr(feature = "instrument", instrument(level = "trace", skip(self), err, fields(__VEILID_LOG_KEY = self.log_key())))]
    /// Load a string secret by key, or `None` if no value is stored for it.
    ///
    /// Holds the inner lock and blocks on the keyring backend (OS keyring or disk).
    ///
    /// A missing key returns `Ok(None)`. Errors with [VeilidAPIError::NotInitialized] if the store has no open keyring, or [VeilidAPIError::Generic] if the keyring backend fails the read.
    pub fn load_user_secret_string<K: AsRef<str> + fmt::Debug>(
        &self,
        key: K,
    ) -> VeilidAPIResult<Option<String>> {
        let inner = self.inner.lock();
        match inner
            .keyring_manager
            .as_ref()
            .ok_or_else(VeilidAPIError::not_initialized)?
            .with_keyring(&self.service_name(), key.as_ref(), |kr| kr.get_value())
        {
            Ok(v) => Ok(Some(v)),
            Err(KeyringError::NoPasswordFound) => Ok(None),
            Err(e) => Err(VeilidAPIError::generic(format!(
                "Failed to load user secret: {}",
                e
            ))),
        }
    }

    #[cfg_attr(feature = "instrument", instrument(level = "trace", skip(self, value), fields(__VEILID_LOG_KEY = self.log_key())))]
    /// Serialize a value to JSON and store it as a secret. Returns true if a value already existed for that key.
    ///
    /// Overwrites any prior value. Blocks on the keyring backend (OS keyring or disk).
    ///
    /// Errors with [VeilidAPIError::Generic] if `value` fails to serialize, if the keyring backend rejects the write, or [VeilidAPIError::NotInitialized] if the store has no open keyring.
    pub fn save_user_secret_json<K, T>(&self, key: K, value: &T) -> VeilidAPIResult<bool>
    where
        K: AsRef<str> + fmt::Debug,
        T: serde::Serialize,
    {
        let v = serde_json::to_vec(value).map_err(VeilidAPIError::generic)?;
        self.save_user_secret(&key, &v)
    }

    #[cfg_attr(feature = "instrument", instrument(level = "trace", skip(self), fields(__VEILID_LOG_KEY = self.log_key())))]
    /// Load a secret by key and deserialize it from JSON, or `None` if no value is stored for it.
    ///
    /// Blocks on the keyring backend (OS keyring or disk).
    ///
    /// A missing key returns `Ok(None)`. Errors with [VeilidAPIError::Generic] if the stored bytes fail to deserialize or are not a valid buffer, or [VeilidAPIError::NotInitialized] if the store has no open keyring.
    pub fn load_user_secret_json<K, T>(&self, key: K) -> VeilidAPIResult<Option<T>>
    where
        K: AsRef<str> + fmt::Debug,
        T: for<'de> serde::de::Deserialize<'de>,
    {
        let out = self.load_user_secret(key)?;
        let b = match out {
            Some(v) => v,
            None => {
                return Ok(None);
            }
        };

        let obj = serde_json::from_slice(&b).map_err(VeilidAPIError::generic)?;
        Ok(Some(obj))
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", skip(self, value), ret, err, fields(__VEILID_LOG_KEY = self.log_key()))
    )]
    /// Store a byte buffer as a secret. Returns true if a value already existed for that key.
    ///
    /// Overwrites any prior value. Blocks on the keyring backend (OS keyring or disk).
    ///
    /// Errors with [VeilidAPIError::NotInitialized] if the store has no open keyring, or [VeilidAPIError::Generic] if the keyring backend rejects the write.
    pub fn save_user_secret<K: AsRef<str> + fmt::Debug>(
        &self,
        key: K,
        value: &[u8],
    ) -> VeilidAPIResult<bool> {
        let mut s = BASE64URL_NOPAD.encode(value);
        s.push('!');

        self.save_user_secret_string(key, s.as_str())
    }

    #[cfg_attr(feature = "instrument", instrument(level = "trace", skip(self), err, fields(__VEILID_LOG_KEY = self.log_key())))]
    /// Load a byte buffer secret by key, or `None` if no value is stored for it.
    ///
    /// Blocks on the keyring backend (OS keyring or disk).
    ///
    /// A missing key returns `Ok(None)`. Errors with [VeilidAPIError::Generic] if the stored value lacks the buffer marker or fails base64 decode, or [VeilidAPIError::NotInitialized] if the store has no open keyring.
    pub fn load_user_secret<K: AsRef<str> + fmt::Debug>(
        &self,
        key: K,
    ) -> VeilidAPIResult<Option<Vec<u8>>> {
        let mut s = match self.load_user_secret_string(key)? {
            Some(s) => s,
            None => {
                return Ok(None);
            }
        };

        if s.pop() != Some('!') {
            apibail_generic!("User secret is not a buffer");
        }

        let mut bytes = Vec::<u8>::new();
        let res = BASE64URL_NOPAD.decode_len(s.len());
        match res {
            Ok(l) => {
                bytes.resize(l, 0u8);
            }
            Err(_) => {
                apibail_generic!("Failed to decode");
            }
        }

        let res = BASE64URL_NOPAD.decode_mut(s.as_bytes(), &mut bytes);
        match res {
            Ok(_) => Ok(Some(bytes)),
            Err(_) => apibail_generic!("Failed to decode"),
        }
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", skip(self), ret, err, fields(__VEILID_LOG_KEY = self.log_key()))
    )]
    /// Remove a secret by key. Returns true if a value was present and deleted.
    ///
    /// No-op returning false when the key is absent, so repeated removes are idempotent. Holds the inner lock and blocks on the keyring backend (OS keyring or disk).
    ///
    /// An absent key returns `Ok(false)`. Errors with [VeilidAPIError::NotInitialized] if the store has no open keyring, or [VeilidAPIError::Generic] if the keyring backend fails the delete.
    pub fn remove_user_secret<K: AsRef<str> + fmt::Debug>(&self, key: K) -> VeilidAPIResult<bool> {
        let inner = self.inner.lock();
        match inner
            .keyring_manager
            .as_ref()
            .ok_or_else(VeilidAPIError::not_initialized)?
            .with_keyring(&self.service_name(), key.as_ref(), |kr| kr.delete_value())
        {
            Ok(_) => Ok(true),
            Err(KeyringError::NoPasswordFound) => Ok(false),
            Err(e) => Err(VeilidAPIError::generic(format!(
                "Failed to remove user secret: {}",
                e
            ))),
        }
    }
}
