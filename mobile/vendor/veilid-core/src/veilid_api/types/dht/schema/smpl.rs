use super::*;
pub use crate::storage_manager::MEMBER_ID_LENGTH;

/// Simple DHT Schema (SMPL) Member
#[apply(api_data_struct!)]
#[api(eq, ord)]
pub struct DHTSchemaSMPLMember {
    /// Member key
    #[cfg_attr(feature = "schemars", schemars(with = "String"))]
    pub m_key: BareMemberId,
    /// Member subkey count
    pub m_cnt: u16,
}

/// Simple DHT Schema (SMPL)
#[apply(api_data_struct!)]
#[api(eq, ord)]
pub struct DHTSchemaSMPL {
    /// Owner subkey count
    o_cnt: u16,
    /// Members
    members: Vec<DHTSchemaSMPLMember>,
}

impl DHTSchemaSMPL {
    /// Four-character code identifying this schema in its compiled form.
    pub const FCC: [u8; 4] = *b"SMPL";
    /// Byte length of the compiled header (fourcc plus owner count), before member entries.
    pub const FIXED_SIZE: usize = 6;
    /// Maximum number of members a schema may have.
    pub const MAX_MEMBER_COUNT: usize = 256;

    /// Make a schema
    ///
    /// Errors with `VeilidAPIError::InvalidArgument` if validation fails (see [DHTSchemaSMPL::validate]).
    pub fn new(o_cnt: u16, members: Vec<DHTSchemaSMPLMember>) -> VeilidAPIResult<Self> {
        let out = Self { o_cnt, members };
        out.validate()?;
        Ok(out)
    }

    /// Validate the data representation
    ///
    /// Errors with `VeilidAPIError::InvalidArgument` if any member key is not [MEMBER_ID_LENGTH] bytes,
    /// the member count exceeds [DHTSchemaSMPL::MAX_MEMBER_COUNT], the writer count exceeds
    /// [DHTSchema::MAX_WRITER_COUNT], or the total subkey count is 0 or exceeds
    /// [DHTSchema::MAX_SUBKEY_COUNT].
    pub fn validate(&self) -> VeilidAPIResult<()> {
        let mut subkey_count = self.o_cnt as usize;
        let mut writer_count = 0;
        if self.o_cnt > 0 {
            writer_count += 1;
        }

        let mut writers = HashSet::<BareMemberId>::new();
        for m in &self.members {
            if m.m_key.len() != MEMBER_ID_LENGTH {
                apibail_invalid_argument!(
                    "member hash digest is wrong size",
                    "m_key.len()",
                    m.m_key.len()
                );
            }
            if m.m_cnt > 0 {
                writers.insert(m.m_key.clone());
            }
            subkey_count += m.m_cnt as usize;
        }

        let member_count = self.members.len();
        if member_count > Self::MAX_MEMBER_COUNT {
            apibail_invalid_argument!("too many members", "member_count", member_count);
        }

        writer_count += writers.len();
        if writer_count > DHTSchema::MAX_WRITER_COUNT {
            apibail_invalid_argument!("too many writers", "writer_count", writer_count);
        }

        if subkey_count == 0 {
            apibail_invalid_argument!(
                "must have at least one subkey",
                "subkey_count",
                subkey_count
            );
        }
        if subkey_count > DHTSchema::MAX_SUBKEY_COUNT {
            apibail_invalid_argument!("too many subkeys", "subkey_count", subkey_count);
        }
        Ok(())
    }

    /// Get the owner subkey count
    #[must_use]
    pub fn o_cnt(&self) -> u16 {
        self.o_cnt
    }

    /// Get the members of the schema
    pub fn members(&self) -> &[DHTSchemaSMPLMember] {
        &self.members
    }

    /// Build the data representation of the schema
    #[must_use]
    pub fn compile(&self) -> Vec<u8> {
        let mut out = Vec::<u8>::with_capacity(
            Self::FIXED_SIZE + (self.members.len() * (MEMBER_ID_LENGTH + 2)),
        );
        // kind
        out.extend_from_slice(&Self::FCC);
        // o_cnt
        out.extend_from_slice(&self.o_cnt.to_le_bytes());
        // members
        for m in &self.members {
            // m_key
            out.extend_from_slice(&m.m_key);
            // m_cnt
            out.extend_from_slice(&m.m_cnt.to_le_bytes());
        }
        out
    }

    /// Get the maximum subkey this schema allocates
    #[must_use]
    pub fn max_subkey(&self) -> ValueSubkey {
        let subkey_count = self
            .members
            .iter()
            .fold(self.o_cnt as usize, |acc, x| acc + (x.m_cnt as usize));
        (subkey_count - 1) as ValueSubkey
    }

    /// Get the subkey count for this schema
    #[must_use]
    pub fn subkey_count(&self) -> usize {
        self.max_subkey() as usize + 1
    }

    /// Get the data size of this schema beyond the size of the structure itself
    #[must_use]
    pub fn data_size(&self) -> usize {
        self.members.len() * mem::size_of::<DHTSchemaSMPLMember>()
    }

    /// Check if a hash is a schema member
    #[must_use]
    pub fn is_member(&self, member_id: &BareMemberId) -> bool {
        for m in &self.members {
            if &m.m_key == member_id {
                return true;
            }
        }
        false
    }
}

impl TryFrom<&[u8]> for DHTSchemaSMPL {
    type Error = VeilidAPIError;
    /// Errors with `VeilidAPIError::Generic` if `b` is shorter than [DHTSchemaSMPL::FIXED_SIZE], has
    /// the wrong fourcc, or has a member region that is not a whole number of member entries, or
    /// `VeilidAPIError::InvalidArgument` if the decoded schema fails validation.
    fn try_from(b: &[u8]) -> Result<Self, Self::Error> {
        if b.len() < Self::FIXED_SIZE {
            apibail_generic!("invalid size");
        }
        if b[0..4] != Self::FCC {
            apibail_generic!("wrong fourcc");
        }
        if !(b.len() - Self::FIXED_SIZE).is_multiple_of(MEMBER_ID_LENGTH + 2) {
            apibail_generic!("invalid member length");
        }

        let o_cnt = u16::from_le_bytes(b[4..6].try_into().map_err(VeilidAPIError::internal)?);

        let members_len = (b.len() - Self::FIXED_SIZE) / (MEMBER_ID_LENGTH + 2);
        let mut members: Vec<DHTSchemaSMPLMember> = Vec::with_capacity(members_len);
        for n in 0..members_len {
            let mstart = Self::FIXED_SIZE + n * (MEMBER_ID_LENGTH + 2);
            let m_key = BareMemberId::new(&b[mstart..mstart + MEMBER_ID_LENGTH]);
            let m_cnt = u16::from_le_bytes(
                b[mstart + MEMBER_ID_LENGTH..mstart + MEMBER_ID_LENGTH + 2]
                    .try_into()
                    .map_err(VeilidAPIError::internal)?,
            );
            members.push(DHTSchemaSMPLMember { m_key, m_cnt });
        }

        Self::new(o_cnt, members)
    }
}
