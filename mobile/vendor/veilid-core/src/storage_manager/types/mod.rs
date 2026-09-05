mod encrypted_value_data;
mod signed_value_data;
mod signed_value_descriptor;

use super::*;

pub use encrypted_value_data::*;
pub use signed_value_data::*;
pub use signed_value_descriptor::*;

/// Fixed length of MemberId (DHT Schema member id) in bytes
pub const MEMBER_ID_LENGTH: usize = 32;
/// The maximum size of a single subkey
pub const MAX_SUBKEY_SIZE: usize = EncryptedValueData::MAX_LEN;
/// The maximum total size of all subkeys of a record
pub const MAX_RECORD_DATA_SIZE: usize = 1_048_576;
