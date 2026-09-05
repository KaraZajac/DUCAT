use core::hash::Hash;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::HashMap;

/// Serializes the map as a sequence of `(key, value)` pairs.
pub fn serialize<K: Serialize, V: Serialize, S: Serializer>(
    v: &HashMap<K, V>,
    s: S,
) -> Result<S::Ok, S::Error> {
    s.collect_seq(v.iter())
}

/// Deserializes a sequence of `(key, value)` pairs back into the map.
pub fn deserialize<'de, K, V, D>(d: D) -> Result<HashMap<K, V>, D::Error>
where
    K: Deserialize<'de> + Eq + Hash,
    V: Deserialize<'de>,
    D: Deserializer<'de>,
{
    Ok(Vec::<(K, V)>::deserialize(d)?.into_iter().collect())
}
