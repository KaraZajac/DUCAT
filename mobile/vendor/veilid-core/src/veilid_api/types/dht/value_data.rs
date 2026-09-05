use super::*;
use veilid_api::VeilidAPIResult;

/// A DHT value and its metadata
#[derive(Clone, PartialOrd, PartialEq, Eq, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(JsonSchema))]
#[cfg_attr(
    all(target_arch = "wasm32", target_os = "unknown"),
    derive(Tsify),
    tsify(from_wasm_abi, into_wasm_abi)
)]
#[cfg_attr(feature = "json-camel-case", serde(rename_all = "camelCase"))]
#[must_use]
pub struct ValueData {
    /// An increasing sequence number to time-order the DHT record changes
    seq: ValueSeqNum,

    /// The contents of a DHT Record
    #[cfg_attr(
        not(all(target_arch = "wasm32", target_os = "unknown")),
        serde(with = "as_human_base64")
    )]
    #[cfg_attr(feature = "schemars", schemars(with = "String"))]
    #[cfg_attr(
        all(target_arch = "wasm32", target_os = "unknown"),
        tsify(type = "Uint8Array")
    )]
    data: Bytes,

    /// The public identity key of the writer of the data
    #[cfg_attr(feature = "schemars", schemars(with = "String"))]
    #[serde(with = "public_key_try_untyped_vld0")]
    writer: PublicKey,
}

impl ValueData {
    /// Maximum length in bytes of the data a single subkey may hold
    pub const MAX_LEN: usize = 32768;

    /// Make a new value with sequence number [ValueSeqNum::ZERO]. Fails if `data` exceeds [ValueData::MAX_LEN].
    ///
    /// Errors with `VeilidAPIError::Generic` if `data` exceeds [ValueData::MAX_LEN].
    pub fn new<B: Into<Bytes>>(data: B, writer: PublicKey) -> VeilidAPIResult<Self> {
        let data = data.into();
        if data.len() > Self::MAX_LEN {
            apibail_generic!("invalid size");
        }
        Ok(Self {
            seq: ValueSeqNum::ZERO,
            data,
            writer,
        })
    }
    /// Make a new value with an explicit sequence number. Fails if `seq` is [ValueSeqNum::NONE] or `data` exceeds [ValueData::MAX_LEN].
    ///
    /// Errors with `VeilidAPIError::Generic` if `seq` is [ValueSeqNum::NONE] or `data` exceeds [ValueData::MAX_LEN].
    pub fn new_with_seq<B: Into<Bytes>>(
        seq: ValueSeqNum,
        data: B,
        writer: PublicKey,
    ) -> VeilidAPIResult<Self> {
        if seq.is_none() {
            apibail_generic!("invalid sequence number");
        }
        let data = data.into();
        if data.len() > Self::MAX_LEN {
            apibail_generic!("invalid size");
        }
        Ok(Self { seq, data, writer })
    }

    /// The public identity key of the writer of the data, by reference
    pub fn ref_writer(&self) -> &PublicKey {
        &self.writer
    }

    /// The stored bytes
    #[must_use]
    pub fn data(&self) -> &[u8] {
        &self.data
    }

    /// The stored bytes as an owned [Bytes]
    #[must_use]
    pub fn data_bytes(&self) -> Bytes {
        self.data.clone()
    }

    /// The sequence number ordering this value among changes to the same subkey
    #[must_use]
    pub fn seq(&self) -> ValueSeqNum {
        self.seq
    }

    /// The public identity key of the writer of the data
    pub fn writer(&self) -> PublicKey {
        self.writer.clone()
    }

    /// The length in bytes of the stored data
    #[must_use]
    pub fn data_size(&self) -> usize {
        self.data.len()
    }

    /// The in-memory footprint of this value, including the struct itself and its data
    #[must_use]
    pub fn total_size(&self) -> usize {
        mem::size_of::<Self>() + self.data.len()
    }
}

impl fmt::Debug for ValueData {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        fmt.debug_struct("ValueData")
            .field("seq", &u32::from(self.seq))
            .field("data", &human_byte_data(&self.data, Some(64)))
            .field("writer", &self.writer)
            .finish()
    }
}

impl fmt::Display for ValueData {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        write!(
            fmt,
            "seq={},len(data)={},writer={}",
            self.seq,
            self.data.len(),
            self.writer
        )
    }
}
