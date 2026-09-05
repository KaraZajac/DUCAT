use super::*;

/// Options that override defaults for set_dht_value
#[apply(api_data_struct!)]
pub struct SetDHTValueOptions {
    /// Override writer key pair for this operation
    #[cfg_attr(feature = "schemars", schemars(with = "Option<String>"))]
    pub writer: Option<KeyPair>,
    /// Defaults to true. If false, the value will not be written if the node is offline,
    /// and a TryAgain error will be returned.
    pub allow_offline: Option<AllowOffline>,
}

impl Default for SetDHTValueOptions {
    fn default() -> Self {
        Self {
            writer: None,
            allow_offline: Some(AllowOffline(true)),
        }
    }
}
