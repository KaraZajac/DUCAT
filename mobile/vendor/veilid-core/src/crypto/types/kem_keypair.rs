use super::*;

/// Untyped KEM encapsulation/decapsulation key pair, carrying no cryptosystem kind.
#[cfg_attr(all(target_arch = "wasm32", target_os = "unknown"), wasm_bindgen)]
#[derive(Clone, Default, Hash, PartialOrd, Ord, PartialEq, Eq, GetSize)]
#[must_use]
pub struct BareKemKeyPair {
    key: BareEncapsulationKey,
    secret: BareDecapsulationKey,
}

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
impl_try_from_js_value!(BareKemKeyPair);

impl BareKemKeyPair {
    /// Builds a KEM key pair from an encapsulation key and its decapsulation key.
    pub fn new(key: BareEncapsulationKey, secret: BareDecapsulationKey) -> Self {
        Self { key, secret }
    }
    /// Returns a reference to the encapsulation key.
    pub fn ref_key(&self) -> &BareEncapsulationKey {
        &self.key
    }
    /// Returns a reference to the decapsulation key.
    pub fn ref_secret(&self) -> &BareDecapsulationKey {
        &self.secret
    }
    /// Returns references to the encapsulation and decapsulation keys.
    pub fn ref_split(&self) -> (&BareEncapsulationKey, &BareDecapsulationKey) {
        (&self.key, &self.secret)
    }
    /// Clones out the encapsulation and decapsulation keys.
    pub fn split(&self) -> (BareEncapsulationKey, BareDecapsulationKey) {
        (self.key.clone(), self.secret.clone())
    }
    /// Consumes the pair, returning the encapsulation and decapsulation keys.
    pub fn into_split(self) -> (BareEncapsulationKey, BareDecapsulationKey) {
        (self.key, self.secret)
    }
}

#[cfg_attr(all(target_arch = "wasm32", target_os = "unknown"), wasm_bindgen)]
#[allow(dead_code)]
impl BareKemKeyPair {
    #[cfg_attr(
        all(target_arch = "wasm32", target_os = "unknown"),
        wasm_bindgen(getter)
    )]
    /// Returns a clone of the encapsulation key.
    pub fn key(&self) -> BareEncapsulationKey {
        self.key.clone()
    }
    /// Returns a clone of the decapsulation key.
    #[cfg_attr(
        all(target_arch = "wasm32", target_os = "unknown"),
        wasm_bindgen(getter)
    )]
    pub fn secret(&self) -> BareDecapsulationKey {
        self.secret.clone()
    }
    /// Encodes the pair as `<encapsulation>:<decapsulation>`, each part base64url-nopad.
    pub fn encode(&self) -> String {
        format!("{}:{}", self.key.encode(), self.secret.encode())
    }
    /// Returns the length of the `encode` string.
    #[cfg_attr(
        all(target_arch = "wasm32", target_os = "unknown"),
        wasm_bindgen(getter, js_name = "encodedLength")
    )]
    pub fn encoded_len(&self) -> usize {
        self.key.encoded_len() + 1 + self.secret.encoded_len()
    }
    /// Decodes a pair from an `<encapsulation>:<decapsulation>` string.
    ///
    /// Errors with `VeilidAPIError::ParseError` if `input` is not exactly two colon-separated parts,
    /// or `VeilidAPIError::Generic` if either part is not valid base64url-nopad.
    #[cfg_attr(
        all(target_arch = "wasm32", target_os = "unknown"),
        wasm_bindgen(js_name = "tryDecode")
    )]
    pub fn try_decode(input: &str) -> VeilidAPIResult<Self> {
        let b = input.as_bytes();
        Self::try_decode_bytes(b)
    }

    /// Decodes a pair from `<encapsulation>:<decapsulation>` bytes, requiring exactly two
    /// colon-separated parts.
    ///
    /// Errors with `VeilidAPIError::ParseError` if `b` is not exactly two colon-separated parts,
    /// or `VeilidAPIError::Generic` if either part is not valid base64url-nopad.
    #[cfg_attr(
        all(target_arch = "wasm32", target_os = "unknown"),
        wasm_bindgen(js_name = "tryDecodeBytes")
    )]
    pub fn try_decode_bytes(b: &[u8]) -> VeilidAPIResult<Self> {
        let parts: Vec<_> = b.split(|x| *x == b':').collect();
        if parts.len() != 2 {
            apibail_parse_error!(
                "input has incorrect parts",
                format!("parts={}", parts.len())
            );
        }
        let key = BareEncapsulationKey::try_decode_bytes(parts[0])?;
        let secret = BareDecapsulationKey::try_decode_bytes(parts[1])?;
        Ok(BareKemKeyPair { key, secret })
    }
}

impl fmt::Display for BareKemKeyPair {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.encode())
    }
}

impl fmt::Debug for BareKemKeyPair {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "BareKemKeyPair({})", self.encode())
    }
}

impl From<&BareKemKeyPair> for String {
    fn from(value: &BareKemKeyPair) -> Self {
        value.encode()
    }
}

impl FromStr for BareKemKeyPair {
    type Err = VeilidAPIError;

    /// Errors with `VeilidAPIError::ParseError` if `s` is not an `<encapsulation>:<decapsulation>`
    /// pair, or `VeilidAPIError::Generic` if either part is not valid base64url-nopad.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        BareKemKeyPair::try_from(s)
    }
}

impl TryFrom<String> for BareKemKeyPair {
    type Error = VeilidAPIError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        BareKemKeyPair::try_from(value.as_str())
    }
}

impl TryFrom<&str> for BareKemKeyPair {
    type Error = VeilidAPIError;
    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::try_decode(value)
    }
}

impl serde::Serialize for BareKemKeyPair {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let s = self.encode();
        serde::Serialize::serialize(&s, serializer)
    }
}

impl<'de> serde::Deserialize<'de> for BareKemKeyPair {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = <String as serde::Deserialize>::deserialize(deserializer)?;
        if s.is_empty() {
            return Ok(BareKemKeyPair::default());
        }
        BareKemKeyPair::try_decode(s.as_str()).map_err(serde::de::Error::custom)
    }
}

////////////////////////////////////////////////////////////////////////////

impl KemKeyPair {
    /// Consumes the pair, returning the kinded encapsulation and decapsulation keys.
    pub fn into_split(self) -> (EncapsulationKey, DecapsulationKey) {
        let kind = self.kind;
        let (ek, dk) = self.into_value().into_split();
        (
            EncapsulationKey::new(kind, ek),
            DecapsulationKey::new(kind, dk),
        )
    }

    /// Returns a reference to the kindless decapsulation key.
    pub fn ref_bare_secret(&self) -> &BareDecapsulationKey {
        self.ref_value().ref_secret()
    }
}

#[cfg_attr(all(target_arch = "wasm32", target_os = "unknown"), wasm_bindgen)]
#[allow(dead_code)]
impl KemKeyPair {
    #[cfg_attr(
        all(target_arch = "wasm32", target_os = "unknown"),
        wasm_bindgen(js_name = "newFromParts")
    )]
    /// Builds a KEM key pair, taking the cryptosystem kind from the encapsulation key.
    pub fn new_from_parts(key: EncapsulationKey, bare_secret: BareDecapsulationKey) -> Self {
        Self {
            kind: key.kind(),
            value: BareKemKeyPair::new(key.value(), bare_secret),
        }
    }

    /// Returns the kinded encapsulation key.
    #[cfg_attr(
        all(target_arch = "wasm32", target_os = "unknown"),
        wasm_bindgen(getter)
    )]
    pub fn key(&self) -> EncapsulationKey {
        EncapsulationKey::new(self.kind, self.ref_value().key())
    }
    /// Returns the kinded decapsulation key.
    #[cfg_attr(
        all(target_arch = "wasm32", target_os = "unknown"),
        wasm_bindgen(getter)
    )]
    pub fn secret(&self) -> DecapsulationKey {
        DecapsulationKey::new(self.kind, self.ref_value().secret())
    }
    /// Returns the kindless decapsulation key.
    #[cfg_attr(
        all(target_arch = "wasm32", target_os = "unknown"),
        wasm_bindgen(getter, js_name = "bareSecret")
    )]
    pub fn bare_secret(&self) -> BareDecapsulationKey {
        self.ref_value().secret()
    }
}
