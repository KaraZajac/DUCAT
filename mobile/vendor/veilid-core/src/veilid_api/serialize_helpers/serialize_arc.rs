use alloc::sync::Arc;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Serializes the `Arc<T>` as its inner `T`.
pub fn serialize<T: Serialize, S: Serializer>(v: &Arc<T>, s: S) -> Result<S::Ok, S::Error> {
    T::serialize(v.as_ref(), s)
}

/// Deserializes a `T` and wraps it in an `Arc`.
pub fn deserialize<'de, T: Deserialize<'de>, D: Deserializer<'de>>(
    d: D,
) -> Result<Arc<T>, D::Error> {
    Ok(Arc::new(T::deserialize(d)?))
}
