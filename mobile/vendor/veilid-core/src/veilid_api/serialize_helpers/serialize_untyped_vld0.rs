/// Generates a `<name>_try_untyped_vld0` serde `with`-module for a typed crypto value.
///
/// The module serializes the typed value normally, but on deserialize falls back to
/// decoding a bare (untyped) `VLD0` value and tagging it with `CRYPTO_KIND_VLD0`.
macro_rules! untyped_vld0_serializers {
    ($rust_name:ident, $typed:ty, $bare:ty) => {
        pastey::paste! {
            /// serde `with`-module accepting either a typed value or a bare `VLD0` value.
            pub mod [< $rust_name _try_untyped_vld0 >] {
                use crate::CRYPTO_KIND_VLD0;
                use core::str::FromStr;
                use serde::{Deserialize, Deserializer, Serialize, Serializer};

                /// Serializes the typed value in its standard form.
                pub fn serialize<S: Serializer>(v: &$typed, s: S) -> Result<S::Ok, S::Error> {
                    v.serialize(s)
                }
                /// Parses the typed value, falling back to a bare `VLD0` value tagged with `CRYPTO_KIND_VLD0`.
                pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<$typed, D::Error> {
                    let s = <String as Deserialize>::deserialize(d)?;
                    match $typed::from_str(&s) {
                        Ok(v) => Ok(v),
                        Err(e) => match $bare::try_decode(&s) {
                            Ok(v) => Ok($typed::new(CRYPTO_KIND_VLD0, v)),
                            Err(_) => Err(serde::de::Error::custom(e)),
                        },
                    }
                }
            }
        }
    };
}

// public_key_try_untyped_vld0
untyped_vld0_serializers!(public_key, crate::PublicKey, crate::BarePublicKey);
// signature_try_untyped_vld0
untyped_vld0_serializers!(signature, crate::Signature, crate::BareSignature);
