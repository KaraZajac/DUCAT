use super::*;

/// An allocated route's id paired with its encoded blob for import by another node.
#[apply(api_data_struct!)]
pub struct RouteBlob {
    /// The id of the allocated route.
    #[serde(with = "as_human_string")]
    #[cfg_attr(feature = "schemars", schemars(with = "String"))]
    pub route_id: RouteId,
    /// The encoded route blob to import as a remote private route.
    #[cfg_attr(
        not(all(target_arch = "wasm32", target_os = "unknown")),
        serde(with = "as_human_base64")
    )]
    #[cfg_attr(feature = "schemars", schemars(with = "String"))]
    #[cfg_attr(
        all(target_arch = "wasm32", target_os = "unknown"),
        serde(with = "serde_bytes")
    )]
    pub blob: Vec<u8>,
}
