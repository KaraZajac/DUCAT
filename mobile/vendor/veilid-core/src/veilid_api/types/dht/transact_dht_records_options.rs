use super::*;

/// Options for DHT record transactions
#[apply(api_data_struct!)]
#[api(default)]
pub struct TransactDHTRecordsOptions {
    /// The signing keypair to use when opening the transaction.
    /// Setting this does not override any writer keys used by transaction operations.
    /// If a record in the transaction is already opened for writing then the writer key will be used.
    /// This is only useful if you have records in a transaction that are only open for reading.
    #[cfg_attr(feature = "schemars", schemars(with = "Option<String>"))]
    pub default_signing_keypair: Option<KeyPair>,
}
