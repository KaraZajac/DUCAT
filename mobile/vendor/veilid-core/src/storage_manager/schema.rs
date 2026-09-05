use super::*;

impl StorageManager {
    /// Produce member id from writer public key
    pub fn generate_member_id(&self, writer_key: &PublicKey) -> VeilidAPIResult<MemberId> {
        if writer_key.ref_value().len() == MEMBER_ID_LENGTH {
            return Ok(MemberId::new(
                writer_key.kind(),
                BareMemberId::new(writer_key.ref_value()),
            ));
        }

        let crypto = self.crypto();
        let Some(vcrypto) = crypto.get(writer_key.kind()) else {
            apibail_generic!("unsupported cryptosystem");
        };

        let idhash = vcrypto.generate_hash(writer_key.ref_value());
        if idhash.ref_value().len() < MEMBER_ID_LENGTH {
            apibail_internal!(format!(
                "generate_hash needs to produce at least {} bytes",
                MEMBER_ID_LENGTH
            ));
        }
        Ok(MemberId::new(
            writer_key.kind(),
            BareMemberId::new(&idhash.ref_value()[0..MEMBER_ID_LENGTH]),
        ))
    }

    /// Check a subkey value data against the schema
    pub fn check_subkey_value_data(
        &self,
        schema: &DHTSchema,
        owner: &PublicKey,
        subkey: ValueSubkey,
        value_data: &EncryptedValueData,
    ) -> VeilidAPIResult<()> {
        // First verify the record key

        match schema {
            DHTSchema::DFLT(d) => self.check_subkey_value_data_dflt(d, owner, subkey, value_data),
            DHTSchema::SMPL(s) => self.check_subkey_value_data_smpl(s, owner, subkey, value_data),
        }
    }

    /// Check a subkey value data against the DFLT schema
    pub(crate) fn check_subkey_value_data_dflt(
        &self,
        schema_dflt: &DHTSchemaDFLT,
        owner: &PublicKey,
        subkey: ValueSubkey,
        value_data: &EncryptedValueData,
    ) -> VeilidAPIResult<()> {
        let subkey = subkey as usize;

        // Check if subkey is in owner range
        if subkey < (schema_dflt.o_cnt() as usize) {
            // Check value data has valid writer
            if &value_data.writer() == owner {
                let max_value_len = usize::min(
                    MAX_SUBKEY_SIZE,
                    MAX_RECORD_DATA_SIZE / schema_dflt.o_cnt() as usize,
                );

                // Ensure value size is within additional limit
                if value_data.data_size() <= max_value_len {
                    return Ok(());
                }

                // Value too big
                apibail_invalid_argument!(
                    "value too big",
                    "data",
                    human_byte_data(value_data.data(), Some(64))
                );
            }

            // Wrong writer
            apibail_invalid_argument!(
                "wrong writer",
                "writer",
                format!("{:?} != {:?}", value_data.writer(), owner)
            );
        }

        // Subkey out of range
        apibail_invalid_argument!("subkey out of range", "subkey", subkey);
    }

    /// Check a subkey value data against the SMPL schema
    pub(crate) fn check_subkey_value_data_smpl(
        &self,
        schema_smpl: &DHTSchemaSMPL,
        owner: &PublicKey,
        subkey: ValueSubkey,
        value_data: &EncryptedValueData,
    ) -> VeilidAPIResult<()> {
        let mut cur_subkey = subkey as usize;

        let max_value_len = usize::min(
            MAX_SUBKEY_SIZE,
            MAX_RECORD_DATA_SIZE / schema_smpl.subkey_count(),
        );

        // Check if subkey is in owner range
        if cur_subkey < (schema_smpl.o_cnt() as usize) {
            // Check value data has valid writer
            if &value_data.writer() == owner {
                // Ensure value size is within additional limit
                if value_data.data_size() <= max_value_len {
                    return Ok(());
                }

                // Value too big
                apibail_invalid_argument!(
                    "value too big",
                    "data",
                    human_byte_data(value_data.data(), Some(64))
                );
            }
            // Wrong writer
            apibail_invalid_argument!(
                "wrong writer",
                "writer",
                format!("{:?}", value_data.writer())
            );
        }
        cur_subkey -= schema_smpl.o_cnt() as usize;

        let writer_hash = self.generate_member_id(&value_data.writer())?;

        // Check all member ranges
        for m in schema_smpl.members() {
            // Check if subkey is in member range
            if cur_subkey < (m.m_cnt as usize) {
                // Check value data has valid writer
                if writer_hash.ref_value() == &m.m_key {
                    // Ensure value size is in allowed range
                    if value_data.data_size() <= max_value_len {
                        return Ok(());
                    }

                    // Value too big
                    apibail_invalid_argument!(
                        "value too big",
                        "data",
                        human_byte_data(value_data.data(), Some(64))
                    );
                }
                // Wrong writer
                apibail_invalid_argument!(
                    "wrong writer",
                    "writer",
                    format!("{:?}", value_data.writer())
                );
            }
            cur_subkey -= m.m_cnt as usize;
        }

        // Subkey out of range
        apibail_invalid_argument!("subkey out of range", "subkey", subkey);
    }
}
