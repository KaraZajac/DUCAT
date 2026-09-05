mod dflt;
mod smpl;

use super::*;

pub use dflt::*;
pub use smpl::*;

/// Enum over all the supported DHT Schemas
#[apply(api_data_enum!)]
#[api(eq, ord)]
#[serde(tag = "kind")]
pub enum DHTSchema {
    /// Default schema: a fixed number of owner-writable subkeys.
    DFLT(DHTSchemaDFLT),
    /// Simple schema: owner subkeys plus per-member writable subkeys.
    SMPL(DHTSchemaSMPL),
}

impl DHTSchema {
    /// Maximum number of distinct writers (owner plus members) a schema may have.
    pub const MAX_WRITER_COUNT: usize = 256;
    /// Maximum number of subkeys a schema may allocate.
    pub const MAX_SUBKEY_COUNT: usize = 1024;

    /// Make a default schema with the given owner subkey count.
    ///
    /// Errors with `VeilidAPIError::InvalidArgument` if `o_cnt` is 0 or exceeds [DHTSchema::MAX_SUBKEY_COUNT].
    pub fn dflt(o_cnt: u16) -> VeilidAPIResult<DHTSchema> {
        Ok(DHTSchema::DFLT(DHTSchemaDFLT::new(o_cnt)?))
    }
    /// Make a simple schema with the given owner subkey count and members.
    ///
    /// Errors with `VeilidAPIError::InvalidArgument` if a member key is the wrong size, the member
    /// count exceeds [DHTSchemaSMPL::MAX_MEMBER_COUNT], the writer count exceeds
    /// [DHTSchema::MAX_WRITER_COUNT], or the total subkey count is 0 or exceeds
    /// [DHTSchema::MAX_SUBKEY_COUNT].
    pub fn smpl(o_cnt: u16, members: Vec<DHTSchemaSMPLMember>) -> VeilidAPIResult<DHTSchema> {
        Ok(DHTSchema::SMPL(DHTSchemaSMPL::new(o_cnt, members)?))
    }

    /// Validate the data representation
    ///
    /// Errors with `VeilidAPIError::InvalidArgument` if the variant's constraints are violated
    /// (see [DHTSchema::dflt] and [DHTSchema::smpl]).
    pub fn validate(&self) -> VeilidAPIResult<()> {
        match self {
            DHTSchema::DFLT(d) => d.validate(),
            DHTSchema::SMPL(s) => s.validate(),
        }
    }

    /// Build the data representation of the schema
    #[must_use]
    pub fn compile(&self) -> Vec<u8> {
        match self {
            DHTSchema::DFLT(d) => d.compile(),
            DHTSchema::SMPL(s) => s.compile(),
        }
    }

    /// Get maximum subkey number for this schema
    #[must_use]
    pub fn max_subkey(&self) -> ValueSubkey {
        match self {
            DHTSchema::DFLT(d) => d.max_subkey(),
            DHTSchema::SMPL(s) => s.max_subkey(),
        }
    }

    /// Get the subkey count for this schema
    #[must_use]
    pub fn subkey_count(&self) -> usize {
        match self {
            DHTSchema::DFLT(d) => d.subkey_count(),
            DHTSchema::SMPL(s) => s.subkey_count(),
        }
    }

    /// Get the data size of this schema beyond the size of the structure itself
    #[must_use]
    pub fn data_size(&self) -> usize {
        match self {
            DHTSchema::DFLT(d) => d.data_size(),
            DHTSchema::SMPL(s) => s.data_size(),
        }
    }

    /// Check if a hash is a schema member
    #[must_use]
    pub fn is_member(&self, member_id: &BareMemberId) -> bool {
        match self {
            DHTSchema::DFLT(d) => d.is_member(member_id),
            DHTSchema::SMPL(s) => s.is_member(member_id),
        }
    }

    /// Truncate a subkey range set to the schema
    /// Optionally also trim to maximum number of subkeys in the range
    pub fn truncate_subkeys(
        &self,
        subkeys: &ValueSubkeyRangeSet,
        opt_max_subkey_len: Option<usize>,
    ) -> ValueSubkeyRangeSet {
        // Get number of subkeys from schema and trim to the bounds of the schema
        let in_schema_subkeys =
            subkeys.intersect(&ValueSubkeyRangeSet::single_range(0, self.max_subkey()));

        // Cap the number of total subkeys being inspected to the amount we can send across the wire
        if let Some(max_subkey_len) = opt_max_subkey_len {
            if let Some(nth_subkey) = in_schema_subkeys.nth_subkey(max_subkey_len) {
                in_schema_subkeys.difference(&ValueSubkeyRangeSet::single_range(
                    nth_subkey,
                    ValueSubkey::MAX,
                ))
            } else {
                in_schema_subkeys
            }
        } else {
            in_schema_subkeys
        }
    }
}

impl Default for DHTSchema {
    fn default() -> Self {
        Self::dflt(1).unwrap_or_log()
    }
}

impl TryFrom<&[u8]> for DHTSchema {
    type Error = VeilidAPIError;
    /// Errors with `VeilidAPIError::Generic` if `b` is shorter than 4 bytes or its fourcc is not a
    /// known schema, or `VeilidAPIError::InvalidArgument` if the decoded schema fails validation.
    fn try_from(b: &[u8]) -> Result<Self, Self::Error> {
        if b.len() < 4 {
            apibail_generic!("invalid size");
        }
        let fcc: [u8; 4] = b[0..4].try_into().unwrap_or_log();
        let schema = match fcc {
            DHTSchemaDFLT::FCC => DHTSchema::DFLT(DHTSchemaDFLT::try_from(b)?),
            DHTSchemaSMPL::FCC => DHTSchema::SMPL(DHTSchemaSMPL::try_from(b)?),
            _ => {
                apibail_generic!("unknown fourcc");
            }
        };

        // Just to make sure, although it should come out of the try_from already validated.
        schema.validate()?;

        Ok(schema)
    }
}
