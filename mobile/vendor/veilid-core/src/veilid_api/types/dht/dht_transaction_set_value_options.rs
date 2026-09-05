use super::*;

/// Options that override defaults for DHTTransaction::set
#[apply(api_data_struct!)]
#[api(default)]
pub struct DHTTransactionSetValueOptions {
    /// Override writer key pair for this operation
    #[cfg_attr(feature = "schemars", schemars(with = "Option<String>"))]
    pub writer: Option<KeyPair>,
}
