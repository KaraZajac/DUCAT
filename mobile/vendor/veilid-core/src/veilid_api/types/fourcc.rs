/// Define a named four-character-code newtype wrapping `[u8; 4]`, with
/// conversions to and from `String` and the raw byte array.
macro_rules! fourcc_type {
    ($name:ident) => {
        pastey::paste! {
            /// A four-character code
            #[derive(
                Copy,
                Default,
                Clone,
                Hash,
                PartialOrd,
                Ord,
                PartialEq,
                Eq,
                Serialize,
                Deserialize,
                GetSize,
            )]
            #[cfg_attr(feature = "schemars", derive(JsonSchema))]
            #[serde(try_from = "String", into = "String")]
            #[must_use]
            #[cfg_attr(
                all(target_arch = "wasm32", target_os = "unknown"),
                derive(Tsify),
                tsify(into_wasm_abi, from_wasm_abi, type_suffix = "Inner"),
            )]
            #[cfg_attr(feature = "json-camel-case", serde(rename_all = "camelCase"))]
            pub struct $name(#[cfg_attr(all(target_arch = "wasm32", target_os = "unknown"),tsify(type = "string"))] [u8; 4]);

            impl $name {
                /// Make a code from four bytes.
                pub const fn new(b: [u8; 4]) -> Self {
                    $name(b)
                }
                /// Get the four bytes of the code.
                #[must_use]
                pub fn bytes(&self) -> &[u8; 4] {
                    &self.0
                }
            }

            impl From<[u8; 4]> for $name {
                fn from(b: [u8; 4]) -> Self {
                    Self(b)
                }
            }

            impl From<u32> for $name {
                fn from(u: u32) -> Self {
                    Self(u.to_be_bytes())
                }
            }

            impl From<$name> for u32 {
                fn from(u: $name) -> Self {
                    u32::from_be_bytes(u.0)
                }
            }

            impl From<$name> for String {
                fn from(u: $name) -> Self {
                    String::from_utf8_lossy(&u.0).to_string()
                }
            }

            impl TryFrom<&[u8]> for $name {
                type Error = VeilidAPIError;
                /// Errors with `VeilidAPIError::ParseError` if `b` is not exactly 4 bytes.
                fn try_from(b: &[u8]) -> Result<Self, Self::Error> {
                    Ok(Self(b.try_into().map_err(|e: std::array::TryFromSliceError| VeilidAPIError::parse_error(e.to_string(), hex::encode(b)))?))
                }
            }

            impl TryFrom<String> for $name {
                type Error = VeilidAPIError;
                /// Errors with `VeilidAPIError::ParseError` if `s` is not exactly 4 bytes.
                fn try_from(s: String) -> Result<Self, Self::Error> {
                    Self::from_str(s.as_str())
                }
            }

            impl fmt::Display for $name {
                fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
                    write!(f, "{}", String::from_utf8_lossy(&self.0))
                }
            }
            impl fmt::Debug for $name {
                fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
                    write!(f, "{}", String::from_utf8_lossy(&self.0))
                }
            }

            impl FromStr for $name {
                type Err = VeilidAPIError;
                /// Errors with `VeilidAPIError::ParseError` if `s` is not exactly 4 bytes.
                fn from_str(s: &str) -> Result<Self, Self::Err> {
                    Ok(Self(
                        s.as_bytes().try_into().map_err(|e: std::array::TryFromSliceError| VeilidAPIError::parse_error(e.to_string(), s))?,
                    ))
                }
            }
        }
    };
}
pub(crate) use fourcc_type;
