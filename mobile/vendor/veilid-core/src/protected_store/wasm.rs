use super::*;
use data_encoding::BASE64URL_NOPAD;

use web_sys::*;

impl_veilid_log_facility!("pstore");

/// Secure key-value storage for user secrets, backed by the browser's `localStorage`.
#[derive(Debug)]
#[must_use]
pub struct ProtectedStore {
    registry: VeilidComponentRegistry,
}

impl_veilid_component!(ProtectedStore);

/// Wraps the string description of a failed JavaScript `JsValue` operation.
#[derive(ThisError, Debug, Clone, Eq, PartialEq)]
#[error("JsValue error")]
pub struct JsValueError(String);

/// Convert a JavaScript `JsValue` into a `JsValueError`, using its string form when present.
#[must_use]
pub fn map_jsvalue_error(x: JsValue) -> JsValueError {
    JsValueError(x.as_string().unwrap_or_default())
}

fn map_js_error_generic<M: ToString>(message: M) -> impl FnOnce(JsValue) -> VeilidAPIError {
    move |x| VeilidAPIError::generic(format!("{}: {}", message.to_string(), map_jsvalue_error(x)))
}

impl ProtectedStore {
    pub(crate) fn new(registry: VeilidComponentRegistry) -> Self {
        Self { registry }
    }

    #[cfg_attr(feature = "instrument", instrument(level = "trace", skip(self), fields(__VEILID_LOG_KEY = self.log_key())))]
    /// Remove every known Veilid-managed protected store key, logging any failures.
    ///
    /// Idempotent: keys already absent are skipped.
    pub fn delete_all(&self) {
        for kpsk in &KNOWN_PROTECTED_STORE_KEYS {
            if let Err(e) = self.remove_user_secret(kpsk) {
                veilid_log!(self error "failed to delete protected store key '{}': {}", kpsk, e);
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
        if self.config().protected_store.delete {
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
    async fn terminate_async(&self) {}

    fn browser_key_name(&self, key: &str) -> String {
        let config = self.config();
        if config.namespace.is_empty() {
            format!("__veilid_protected_store_{}", key)
        } else {
            format!("__veilid_protected_store_{}_{}", config.namespace, key)
        }
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", skip(self, key, value), fields(__VEILID_LOG_KEY = self.log_key()))
    )]
    /// Store a string secret under a key. Returns true if a value already existed for that key.
    ///
    /// Overwrites any prior value via `localStorage.setItem`, so re-saving the same key/value is idempotent.
    ///
    /// Errors with [VeilidAPIError::Generic] if the window or `localStorage` is unavailable, or if `setItem` throws (e.g. storage quota exceeded). Panics if called outside a browser.
    pub fn save_user_secret_string<K: AsRef<str> + fmt::Debug, V: AsRef<str> + fmt::Debug>(
        &self,
        key: K,
        value: V,
    ) -> VeilidAPIResult<bool> {
        if is_browser() {
            let win = match window() {
                Some(w) => w,
                None => {
                    apibail_generic!("failed to get window");
                }
            };

            let ls = match win
                .local_storage()
                .map_err(map_js_error_generic("exception getting local storage"))?
            {
                Some(l) => l,
                None => {
                    apibail_generic!("failed to get local storage");
                }
            };

            let vkey = self.browser_key_name(key.as_ref());

            let prev = ls
                .get_item(&vkey)
                .map_err(map_js_error_generic("exception thrown"))?
                .is_some();

            ls.set_item(&vkey, value.as_ref())
                .map_err(map_js_error_generic("exception thrown"))?;

            Ok(prev)
        } else {
            unimplemented!()
        }
    }

    #[cfg_attr(feature = "instrument", instrument(level = "trace", skip(self, key), fields(__VEILID_LOG_KEY = self.log_key())))]
    /// Load a string secret by key, or `None` if no value is stored for it.
    ///
    /// A missing key returns `Ok(None)`. Errors with [VeilidAPIError::Generic] if the window or `localStorage` is unavailable, or if `getItem` throws. Panics if called outside a browser.
    pub fn load_user_secret_string<K: AsRef<str> + fmt::Debug>(
        &self,
        key: K,
    ) -> VeilidAPIResult<Option<String>> {
        if is_browser() {
            let win = match window() {
                Some(w) => w,
                None => {
                    apibail_generic!("failed to get window");
                }
            };

            let ls = match win
                .local_storage()
                .map_err(map_js_error_generic("exception getting local storage"))?
            {
                Some(l) => l,
                None => {
                    apibail_generic!("failed to get local storage");
                }
            };

            let vkey = self.browser_key_name(key.as_ref());
            ls.get_item(&vkey)
                .map_err(map_js_error_generic("exception thrown"))
        } else {
            unimplemented!();
        }
    }

    #[cfg_attr(feature = "instrument", instrument(level = "trace", skip(self, value), fields(__VEILID_LOG_KEY = self.log_key())))]
    /// Serialize a value to JSON and store it as a secret. Returns true if a value already existed for that key.
    ///
    /// Overwrites any prior value.
    ///
    /// Errors with [VeilidAPIError::Generic] if `value` fails to serialize, if the window or `localStorage` is unavailable, or if `setItem` throws. Panics if called outside a browser.
    pub fn save_user_secret_json<K, T>(&self, key: K, value: &T) -> VeilidAPIResult<bool>
    where
        K: AsRef<str> + fmt::Debug,
        T: serde::Serialize,
    {
        let v = serde_json::to_vec(value).map_err(VeilidAPIError::generic)?;
        self.save_user_secret(key, &v)
    }

    #[cfg_attr(feature = "instrument", instrument(level = "trace", skip(self), fields(__VEILID_LOG_KEY = self.log_key())))]
    /// Load a secret by key and deserialize it from JSON, or `None` if no value is stored for it.
    ///
    /// A missing key returns `Ok(None)`. Errors with [VeilidAPIError::Generic] if the stored bytes fail to deserialize or are not a valid buffer, or if `localStorage` access throws. Panics if called outside a browser.
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
    /// Overwrites any prior value.
    ///
    /// Errors with [VeilidAPIError::Generic] if the window or `localStorage` is unavailable, or if `setItem` throws. Panics if called outside a browser.
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
    /// A missing key returns `Ok(None)`. Errors with [VeilidAPIError::Generic] if the stored value lacks the buffer marker, fails base64 decode, or if `localStorage` access throws. Panics if called outside a browser.
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
    /// No-op returning false when the key is absent, so repeated removes are idempotent.
    ///
    /// An absent key returns `Ok(false)`. Errors with [VeilidAPIError::Generic] if the window or `localStorage` is unavailable, or if `getItem`/`removeItem` throws. Panics if called outside a browser.
    pub fn remove_user_secret<K: AsRef<str> + fmt::Debug>(&self, key: K) -> VeilidAPIResult<bool> {
        if is_browser() {
            let win = match window() {
                Some(w) => w,
                None => {
                    apibail_generic!("failed to get window");
                }
            };

            let ls = match win
                .local_storage()
                .map_err(map_js_error_generic("exception getting local storage"))?
            {
                Some(l) => l,
                None => {
                    apibail_generic!("failed to get local storage");
                }
            };

            let vkey = self.browser_key_name(key.as_ref());

            match ls
                .get_item(&vkey)
                .map_err(map_js_error_generic("exception thrown"))?
            {
                Some(_) => {
                    ls.delete(&vkey)
                        .map_err(map_js_error_generic("exception thrown"))?;
                    Ok(true)
                }
                None => Ok(false),
            }
        } else {
            unimplemented!();
        }
    }
}
