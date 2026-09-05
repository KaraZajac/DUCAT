use super::*;

// Don't trace these functions with events as they are used in the transfer of API logs, which will recurse!

#[cfg_attr(
    feature = "instrument",
    instrument(level = "trace", target = "json", skip_all)
)]
/// Deserializes `T` from a JSON string, mapping parse failures to `VeilidAPIError::ParseError`.
///
/// Errors with `VeilidAPIError::ParseError` if `arg` is not valid JSON or does not match `T`'s shape.
pub fn deserialize_json<'a, T: de::Deserialize<'a> + Debug>(arg: &'a str) -> VeilidAPIResult<T> {
    serde_json::from_str(arg).map_err(|e| VeilidAPIError::ParseError {
        message: e.to_string(),
        value: format!(
            "deserialize_json:\n---\n{}\n---\n to type {}",
            arg,
            std::any::type_name::<T>()
        ),
    })
}

/// Lenient JSON deserialization for command-line / `set_config` ergonomics.
///
/// Tries, in order, until one parses into `T`:
///   1. the value as strict JSON (canonical forms: `42`, `true`, `"s"`, `["a"]`)
///   2. the bare value as a string scalar (so `ipv4` -> `"ipv4"`)
///   3. the value parsed as JSON, wrapped in a one-element array (so `"ipv4"` -> `["ipv4"]`)
///   4. the bare value as a string scalar, wrapped in a one-element array (so `ipv4` -> `["ipv4"]`)
///
/// Strict JSON is always attempted first, so any value that parses today keeps its
/// current meaning; the fallbacks only accept inputs that strict parsing rejects.
/// Returns the strict-parse error if every interpretation fails.
///
/// Errors with `VeilidAPIError::ParseError` (the strict-path error) if none of the four
/// interpretations parse into `T`.
#[cfg_attr(
    feature = "instrument",
    instrument(level = "trace", target = "json", skip_all)
)]
pub fn deserialize_json_lenient<T: de::DeserializeOwned + Debug>(arg: &str) -> VeilidAPIResult<T> {
    // 1. Strict JSON
    let strict_err = match deserialize_json::<T>(arg) {
        Ok(v) => return Ok(v),
        Err(e) => e,
    };
    // 2. Bare value as a string scalar
    let as_string = serde_json::Value::String(arg.to_owned());
    if let Ok(v) = serde_json::from_value::<T>(as_string.clone()) {
        return Ok(v);
    }
    // 3. Parsed JSON scalar wrapped in a one-element array
    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(arg) {
        if let Ok(v) = serde_json::from_value::<T>(serde_json::Value::Array(vec![parsed])) {
            return Ok(v);
        }
    }
    // 4. Bare value as a string scalar wrapped in a one-element array
    if let Ok(v) = serde_json::from_value::<T>(serde_json::Value::Array(vec![as_string])) {
        return Ok(v);
    }
    Err(strict_err)
}

#[cfg_attr(
    feature = "instrument",
    instrument(level = "trace", target = "json", skip_all)
)]
/// Deserializes `T` from JSON bytes, mapping parse failures to `VeilidAPIError::ParseError`.
///
/// Errors with `VeilidAPIError::ParseError` if `arg` is not valid JSON or does not match `T`'s shape.
pub fn deserialize_json_bytes<'a, T: de::Deserialize<'a> + Debug>(
    arg: &'a [u8],
) -> VeilidAPIResult<T> {
    serde_json::from_slice(arg).map_err(|e| VeilidAPIError::ParseError {
        message: e.to_string(),
        value: format!(
            "deserialize_json_bytes:\n---\n{:?}\n---\n to type {}",
            arg,
            std::any::type_name::<T>()
        ),
    })
}

#[cfg_attr(
    feature = "instrument",
    instrument(level = "trace", target = "json", skip_all)
)]
/// Deserializes `T` from an optional JSON string, returning a `ParseError` when the argument is `None`.
///
/// Errors with `VeilidAPIError::ParseError` if `arg` is `None`, or if the string is not valid JSON for `T`.
pub fn deserialize_opt_json<T: de::DeserializeOwned + Debug>(
    arg: Option<String>,
) -> VeilidAPIResult<T> {
    let arg = arg.as_ref().ok_or_else(|| VeilidAPIError::ParseError {
        message: "invalid null string".to_owned(),
        value: format!(
            "deserialize_json_opt: null to type {}",
            std::any::type_name::<T>()
        ),
    })?;
    deserialize_json(arg)
}

#[cfg_attr(
    feature = "instrument",
    instrument(level = "trace", target = "json", skip_all)
)]
/// Deserializes `T` from optional JSON bytes, returning a `ParseError` when the argument is `None`.
///
/// Errors with `VeilidAPIError::ParseError` if `arg` is `None`, or if the bytes are not valid JSON for `T`.
pub fn deserialize_opt_json_bytes<T: de::DeserializeOwned + Debug>(
    arg: Option<Vec<u8>>,
) -> VeilidAPIResult<T> {
    let arg = arg.as_ref().ok_or_else(|| VeilidAPIError::ParseError {
        message: "invalid null string".to_owned(),
        value: format!(
            "deserialize_json_opt: null to type {}",
            std::any::type_name::<T>()
        ),
    })?;
    deserialize_json_bytes(arg.as_slice())
}

#[cfg_attr(
    feature = "instrument",
    instrument(level = "trace", target = "json", skip_all)
)]
/// Serializes `val` to a JSON string, panicking if serialization fails.
///
/// Does not return a `Result`: panics (rather than erroring) if `val`'s `Serialize` impl fails.
pub fn serialize_json<T: Serialize + Debug>(val: T) -> String {
    match serde_json::to_string(&val) {
        Ok(v) => v,
        Err(e) => {
            panic!("failed to serialize json value: {}\nval={:?}", e, val);
        }
    }
}

#[cfg_attr(
    feature = "instrument",
    instrument(level = "trace", target = "json", skip_all)
)]
/// Serializes `val` to a pretty-printed JSON string, panicking if serialization fails.
///
/// Does not return a `Result`: panics (rather than erroring) if `val`'s `Serialize` impl fails.
pub fn serialize_json_pretty<T: Serialize + Debug>(val: T) -> String {
    match serde_json::to_string_pretty(&val) {
        Ok(v) => v,
        Err(e) => {
            panic!(
                "failed to serialize pretty json value: {}\nval={:?}",
                e, val
            );
        }
    }
}

#[cfg_attr(
    feature = "instrument",
    instrument(level = "trace", target = "json", skip_all)
)]
/// Serializes `val` to JSON bytes, panicking if serialization fails.
///
/// Does not return a `Result`: panics (rather than erroring) if `val`'s `Serialize` impl fails.
pub fn serialize_json_bytes<T: Serialize + Debug>(val: T) -> Vec<u8> {
    match serde_json::to_vec(&val) {
        Ok(v) => v,
        Err(e) => {
            panic!(
                "failed to serialize json value to bytes: {}\nval={:?}",
                e, val
            );
        }
    }
}

/// serde `with`-module for byte buffers: base64url-nopad string for human-readable formats, raw bytes otherwise.
pub mod as_human_base64 {
    use data_encoding::BASE64URL_NOPAD;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    #[cfg(feature = "instrument")]
    use tracing::instrument;

    /// Serializes the bytes as a base64url-nopad string for human-readable formats, raw otherwise.
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "json", skip_all)
    )]
    pub fn serialize<S: Serializer, B: AsRef<[u8]>>(v: &B, s: S) -> Result<S::Ok, S::Error> {
        if s.is_human_readable() {
            let base64 = BASE64URL_NOPAD.encode(v.as_ref());
            String::serialize(&base64, s)
        } else {
            <[u8]>::serialize(v.as_ref(), s)
        }
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "json", skip_all)
    )]
    /// Decodes a base64url-nopad string for human-readable formats, raw bytes otherwise.
    ///
    /// In human-readable formats, yields a serde decode error (`D::Error`) if the string is not valid base64url-nopad.
    pub fn deserialize<'de, D: Deserializer<'de>, B: From<Vec<u8>>>(d: D) -> Result<B, D::Error> {
        if d.is_human_readable() {
            let base64 = String::deserialize(d)?;
            BASE64URL_NOPAD
                .decode(base64.as_bytes())
                .map_err(serde::de::Error::custom)
                .map(B::from)
        } else {
            Vec::<u8>::deserialize(d).map(B::from)
        }
    }
}

/// serde `with`-module for `Option<Vec<u8>>`: base64url-nopad string for human-readable formats, raw bytes otherwise.
pub mod as_human_opt_base64 {
    use data_encoding::BASE64URL_NOPAD;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    #[cfg(feature = "instrument")]
    use tracing::instrument;

    /// Serializes the optional bytes as a base64url-nopad string for human-readable formats, raw otherwise.
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "json", skip_all)
    )]
    pub fn serialize<S: Serializer>(v: &Option<Vec<u8>>, s: S) -> Result<S::Ok, S::Error> {
        if s.is_human_readable() {
            let base64 = v.as_ref().map(|x| BASE64URL_NOPAD.encode(x));
            Option::<String>::serialize(&base64, s)
        } else {
            Option::<Vec<u8>>::serialize(v, s)
        }
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "json", skip_all)
    )]
    /// Decodes a base64url-nopad string for human-readable formats, raw bytes otherwise.
    ///
    /// In human-readable formats, yields a serde decode error (`D::Error`) if a present string is not valid base64url-nopad.
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Vec<u8>>, D::Error> {
        if d.is_human_readable() {
            let base64 = Option::<String>::deserialize(d)?;
            base64
                .map(|x| {
                    BASE64URL_NOPAD
                        .decode(x.as_bytes())
                        .map_err(serde::de::Error::custom)
                })
                .transpose()
        } else {
            Option::<Vec<u8>>::deserialize(d)
        }
    }
}

/// serde `with`-module that uses a value's `Display`/`FromStr` for human-readable formats and its native serde impl otherwise.
pub mod as_human_string {
    use std::fmt::Display;
    use std::str::FromStr;

    use serde::{de, Deserialize, Deserializer, Serialize, Serializer};
    #[cfg(feature = "instrument")]
    use tracing::instrument;

    /// Serializes via `Display` for human-readable formats, native serde otherwise.
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "json", skip_all)
    )]
    pub fn serialize<T, S>(value: &T, s: S) -> Result<S::Ok, S::Error>
    where
        T: Display + Serialize,
        S: Serializer,
    {
        if s.is_human_readable() {
            s.collect_str(value)
        } else {
            T::serialize(value, s)
        }
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "json", skip_all)
    )]
    /// Parses via `FromStr` for human-readable formats, native serde otherwise.
    ///
    /// In human-readable formats, yields a serde decode error (`D::Error`) if `T::from_str` rejects the string.
    pub fn deserialize<'de, T, D>(d: D) -> Result<T, D::Error>
    where
        T: FromStr + Deserialize<'de>,
        T::Err: Display,
        D: Deserializer<'de>,
    {
        if d.is_human_readable() {
            String::deserialize(d)?.parse().map_err(de::Error::custom)
        } else {
            T::deserialize(d)
        }
    }
}

/// serde `with`-module for `Option<T>` that uses `Display`/`FromStr` for human-readable formats and native serde otherwise.
pub mod as_human_opt_string {
    use std::fmt::Display;
    use std::str::FromStr;

    use serde::{de, Deserialize, Deserializer, Serialize, Serializer};
    #[cfg(feature = "instrument")]
    use tracing::instrument;

    /// Serializes via `Display` for human-readable formats, native serde otherwise.
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "json", skip_all)
    )]
    pub fn serialize<T, S>(value: &Option<T>, s: S) -> Result<S::Ok, S::Error>
    where
        T: Display + Serialize,
        S: Serializer,
    {
        if s.is_human_readable() {
            match value {
                Some(v) => s.collect_str(v),
                None => s.serialize_none(),
            }
        } else {
            Option::<T>::serialize(value, s)
        }
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "json", skip_all)
    )]
    /// Parses via `FromStr` for human-readable formats, native serde otherwise.
    ///
    /// In human-readable formats, yields a serde decode error (`D::Error`) if `T::from_str` rejects a present string.
    pub fn deserialize<'de, T, D>(d: D) -> Result<Option<T>, D::Error>
    where
        T: FromStr + Deserialize<'de>,
        T::Err: Display,
        D: Deserializer<'de>,
    {
        if d.is_human_readable() {
            match Option::<String>::deserialize(d)? {
                None => Ok(None),
                Some(v) => Ok(Some(v.parse::<T>().map_err(de::Error::custom)?)),
            }
        } else {
            Option::<T>::deserialize(d)
        }
    }
}
