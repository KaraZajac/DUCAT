//! Address types for DHT-based communication.

use std::{fmt::Display, str::FromStr};

use anyhow::Error;
use veilid_core::RecordKey;

/// An address for DHT-based communication in the Veilid network.
///
/// A DHT address consists of a typed record key that identifies a DHT record
/// and a subkey that specifies a particular entry within that record.
/// This is commonly used for addressing services or endpoints.
///
/// # Examples
///
/// ```no_run
/// use veilnet::DHTAddr;
///
/// // Parse from string representation
/// let addr: DHTAddr = "VLD0:abcd1234...efgh5678:8080".parse()?;
/// println!("Connecting to {}", addr);
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DHTAddr {
    /// The DHT record key identifying the target record.
    pub key: RecordKey,
    /// The subkey within the DHT record, used as a port number.
    pub subkey: u16,
}

impl DHTAddr {
    pub fn new(key: RecordKey, subkey: u16) -> DHTAddr {
        Self { key, subkey }
    }
}

impl Display for DHTAddr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.key, self.subkey)
    }
}

impl FromStr for DHTAddr {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.rsplit_once(":") {
            Some((key_str, subkey_str)) => Ok(DHTAddr {
                key: key_str.parse()?,
                subkey: subkey_str.parse()?,
            }),
            None => Err(anyhow::anyhow!("invalid address: {s}")),
        }
    }
}
