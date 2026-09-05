use super::*;

/// Untyped DHT record key: an opaque record key with an optional record encryption secret.
#[cfg_attr(all(target_arch = "wasm32", target_os = "unknown"), wasm_bindgen)]
#[derive(Clone, Default, PartialOrd, Ord, PartialEq, Eq, Hash, GetSize)]
#[must_use]
pub struct BareRecordKey {
    key: BareOpaqueRecordKey,
    encryption_key: Option<BareSharedSecret>,
}

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
impl_try_from_js_value!(BareRecordKey);

impl BareRecordKey {
    /// Builds a record key from an opaque record key and an optional encryption secret.
    pub fn new(key: BareOpaqueRecordKey, encryption_key: Option<BareSharedSecret>) -> Self {
        Self {
            key,
            encryption_key,
        }
    }
    /// Returns a reference to the opaque record key.
    pub fn ref_key(&self) -> &BareOpaqueRecordKey {
        &self.key
    }
    /// Returns a reference to the encryption secret, if present.
    pub fn ref_encryption_key(&self) -> Option<&BareSharedSecret> {
        self.encryption_key.as_ref()
    }
    /// Clones out the opaque record key and optional encryption secret.
    pub fn split(&self) -> (BareOpaqueRecordKey, Option<BareSharedSecret>) {
        (self.key.clone(), self.encryption_key.clone())
    }
    /// Consumes the key, returning the opaque record key and optional encryption secret.
    pub fn into_split(self) -> (BareOpaqueRecordKey, Option<BareSharedSecret>) {
        (self.key, self.encryption_key)
    }
}

#[cfg_attr(all(target_arch = "wasm32", target_os = "unknown"), wasm_bindgen)]
#[allow(dead_code)]
impl BareRecordKey {
    #[cfg_attr(
        all(target_arch = "wasm32", target_os = "unknown"),
        wasm_bindgen(getter)
    )]
    /// Returns a clone of the opaque record key.
    pub fn key(&self) -> BareOpaqueRecordKey {
        self.key.clone()
    }
    /// Returns a clone of the encryption secret, if present.
    #[cfg_attr(
        all(target_arch = "wasm32", target_os = "unknown"),
        wasm_bindgen(getter, js_name = "encryptionKey")
    )]
    pub fn encryption_key(&self) -> Option<BareSharedSecret> {
        self.encryption_key.clone()
    }
    /// Encodes as `<key>` or `<key>:<encryption_key>`, each part base64url-nopad.
    pub fn encode(&self) -> String {
        if let Some(encryption_key) = &self.encryption_key {
            format!("{}:{}", self.key.encode(), encryption_key.encode())
        } else {
            self.key.encode()
        }
    }
    #[cfg_attr(
        all(target_arch = "wasm32", target_os = "unknown"),
        wasm_bindgen(getter, js_name = "encodedLen")
    )]
    /// Returns the length of the `encode` string.
    pub fn encoded_len(&self) -> usize {
        if let Some(encryption_key) = &self.encryption_key {
            self.key.encoded_len() + 1 + encryption_key.encoded_len()
        } else {
            self.key.encoded_len()
        }
    }
    /// Decodes from a `<key>` or `<key>:<encryption_key>` string.
    ///
    /// Errors with `VeilidAPIError::ParseError` if `input` has more than two colon-separated parts,
    /// or `VeilidAPIError::Generic` if any part is not valid base64url-nopad.
    #[cfg_attr(
        all(target_arch = "wasm32", target_os = "unknown"),
        wasm_bindgen(js_name = "tryDecode")
    )]
    pub fn try_decode(input: &str) -> VeilidAPIResult<Self> {
        let b = input.as_bytes();
        Self::try_decode_bytes(b)
    }
    /// Decodes from `<key>` or `<key>:<encryption_key>` bytes.
    ///
    /// Errors with `VeilidAPIError::ParseError` if `b` has more than two colon-separated parts,
    /// or `VeilidAPIError::Generic` if any part is not valid base64url-nopad.
    #[cfg_attr(
        all(target_arch = "wasm32", target_os = "unknown"),
        wasm_bindgen(js_name = "tryDecodeBytes")
    )]
    pub fn try_decode_bytes(b: &[u8]) -> VeilidAPIResult<Self> {
        let parts: Vec<_> = b.split(|x| *x == b':').collect();
        match parts[..] {
            [key] => {
                let key = BareOpaqueRecordKey::try_decode_bytes(key)?;
                Ok(BareRecordKey {
                    key,
                    encryption_key: None,
                })
            }
            [key, encryption_key] => {
                let key = BareOpaqueRecordKey::try_decode_bytes(key)?;
                let encryption_key = BareSharedSecret::try_decode_bytes(encryption_key)?;
                Ok(BareRecordKey {
                    key,
                    encryption_key: Some(encryption_key),
                })
            }
            _ => {
                apibail_parse_error!(
                    "input has incorrect parts",
                    format!("parts={}", parts.len())
                );
            }
        }
    }
}

impl fmt::Display for BareRecordKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.encode())
    }
}

impl fmt::Debug for BareRecordKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "BareRecordKey({})", self.encode())
    }
}

impl From<&BareRecordKey> for String {
    fn from(value: &BareRecordKey) -> Self {
        value.encode()
    }
}

impl FromStr for BareRecordKey {
    type Err = VeilidAPIError;

    /// Errors with `VeilidAPIError::ParseError` if `s` has more than two colon-separated parts,
    /// or `VeilidAPIError::Generic` if any part is not valid base64url-nopad.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        BareRecordKey::try_from(s)
    }
}

impl TryFrom<String> for BareRecordKey {
    type Error = VeilidAPIError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        BareRecordKey::try_from(value.as_str())
    }
}

impl TryFrom<&str> for BareRecordKey {
    type Error = VeilidAPIError;
    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::try_decode(value)
    }
}

impl serde::Serialize for BareRecordKey {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let s = self.encode();
        serde::Serialize::serialize(&s, serializer)
    }
}

impl<'de> serde::Deserialize<'de> for BareRecordKey {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = <String as serde::Deserialize>::deserialize(deserializer)?;
        if s.is_empty() {
            return Ok(BareRecordKey::default());
        }
        BareRecordKey::try_decode(s.as_str()).map_err(serde::de::Error::custom)
    }
}

////////////////////////////////////////////////////////////////////////////

impl RecordKey {
    /// Builds a record key from an opaque record key and an optional encryption secret, keeping its kind.
    pub fn from_opaque(
        opaque_record_key: OpaqueRecordKey,
        encryption_key: Option<BareSharedSecret>,
    ) -> Self {
        RecordKey::new(
            opaque_record_key.kind(),
            BareRecordKey::new(opaque_record_key.into_value(), encryption_key),
        )
    }
    /// Returns the kinded opaque record key, dropping any encryption secret.
    pub fn opaque(&self) -> OpaqueRecordKey {
        OpaqueRecordKey::new(self.kind, self.ref_value().key())
    }
    /// Consumes the key, returning the kinded opaque record key and optional encryption secret.
    pub fn into_split(self) -> (OpaqueRecordKey, Option<SharedSecret>) {
        let kind = self.kind;
        let (bork, bss) = self.into_value().into_split();
        (
            OpaqueRecordKey::new(kind, bork),
            bss.map(|x| SharedSecret::new(kind, x)),
        )
    }
}

#[cfg_attr(all(target_arch = "wasm32", target_os = "unknown"), wasm_bindgen)]
#[allow(dead_code)]
impl RecordKey {
    #[cfg_attr(
        all(target_arch = "wasm32", target_os = "unknown"),
        wasm_bindgen(getter, js_name = "encryptionKey")
    )]
    /// Returns the kinded encryption secret, if present.
    pub fn encryption_key(&self) -> Option<SharedSecret> {
        self.ref_value()
            .encryption_key()
            .map(|v| SharedSecret::new(self.kind, v.clone()))
    }
}
