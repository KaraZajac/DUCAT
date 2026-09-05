use super::*;

/// Default DHT Schema (DFLT)
#[apply(api_data_struct!)]
#[api(eq, ord, ts(from_wasm_abi, into_wasm_abi))]
pub struct DHTSchemaDFLT {
    /// Owner subkey count
    o_cnt: u16,
}

impl DHTSchemaDFLT {
    /// Four-character code identifying this schema in its compiled form.
    pub const FCC: [u8; 4] = *b"DFLT";
    /// Byte length of the compiled schema (fourcc plus owner count).
    pub const FIXED_SIZE: usize = 6;

    /// Make a schema
    ///
    /// Errors with `VeilidAPIError::InvalidArgument` if `o_cnt` is 0 or exceeds [DHTSchema::MAX_SUBKEY_COUNT].
    pub fn new(o_cnt: u16) -> VeilidAPIResult<Self> {
        let out = Self { o_cnt };
        out.validate()?;
        Ok(out)
    }

    /// Validate the data representation
    ///
    /// Errors with `VeilidAPIError::InvalidArgument` if `o_cnt` is 0 or exceeds [DHTSchema::MAX_SUBKEY_COUNT].
    pub fn validate(&self) -> VeilidAPIResult<()> {
        if self.o_cnt == 0 {
            apibail_invalid_argument!("must have at least one subkey", "o_cnt", self.o_cnt);
        }
        if self.o_cnt > (DHTSchema::MAX_SUBKEY_COUNT as u16) {
            apibail_invalid_argument!("too many subkeys", "o_cnt", self.o_cnt);
        }

        Ok(())
    }

    /// Get the owner subkey count
    #[must_use]
    pub fn o_cnt(&self) -> u16 {
        self.o_cnt
    }

    /// Build the data representation of the schema
    #[must_use]
    pub fn compile(&self) -> Vec<u8> {
        let mut out = Vec::<u8>::with_capacity(Self::FIXED_SIZE);
        // kind
        out.extend_from_slice(&Self::FCC);
        // o_cnt
        out.extend_from_slice(&self.o_cnt.to_le_bytes());
        out
    }

    /// Get the maximum subkey this schema allocates
    #[must_use]
    pub fn max_subkey(&self) -> ValueSubkey {
        self.o_cnt as ValueSubkey - 1
    }

    /// Get the subkey count for this schema
    #[must_use]
    pub fn subkey_count(&self) -> usize {
        self.max_subkey() as usize + 1
    }

    /// Get the data size of this schema beyond the size of the structure itself
    #[must_use]
    pub fn data_size(&self) -> usize {
        0
    }

    /// Check if a hash is a schema member
    #[must_use]
    pub fn is_member(&self, _member_id: &BareMemberId) -> bool {
        false
    }
}

impl TryFrom<&[u8]> for DHTSchemaDFLT {
    type Error = VeilidAPIError;
    /// Errors with `VeilidAPIError::Generic` if `b` is not [DHTSchemaDFLT::FIXED_SIZE] bytes or has
    /// the wrong fourcc, or `VeilidAPIError::InvalidArgument` if the decoded schema fails validation.
    fn try_from(b: &[u8]) -> Result<Self, Self::Error> {
        if b.len() != Self::FIXED_SIZE {
            apibail_generic!("invalid size");
        }
        if b[0..4] != Self::FCC {
            apibail_generic!("wrong fourcc");
        }

        let o_cnt = u16::from_le_bytes(b[4..6].try_into().map_err(VeilidAPIError::internal)?);

        Self::new(o_cnt)
    }
}
