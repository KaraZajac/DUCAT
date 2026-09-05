use super::*;

pub const MAX_TRANSACT_COMMAND_Q_SEQS_LEN: usize = DHTSchema::MAX_SUBKEY_COUNT;
pub const MAX_TRANSACT_COMMAND_A_SEQS_LEN: usize = DHTSchema::MAX_SUBKEY_COUNT;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransactCommand {
    End,
    Commit,
    Rollback,
    Get,
    Set,
    // Sync,
}

impl fmt::Display for TransactCommand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            match self {
                TransactCommand::End => "end",
                TransactCommand::Commit => "commit",
                TransactCommand::Rollback => "rollback",
                TransactCommand::Get => "get",
                TransactCommand::Set => "set",
                // TransactCommand::Sync => "sync",
            }
        )
    }
}

#[derive(Debug, Clone)]
pub(in crate::rpc_processor) struct ValidateTransactCommandContext {
    pub opaque_record_key: OpaqueRecordKey,
    pub command: TransactCommand,
    pub descriptor: Arc<SignedValueDescriptor>,
    pub opt_subkey: Option<ValueSubkey>,
    pub opt_value: Option<Arc<SignedValueData>>,
}

#[derive(Debug, Clone)]
pub(in crate::rpc_processor) struct RPCOperationTransactCommandQ {
    key: OpaqueRecordKey,
    transaction_id: u64,
    command: TransactCommand,
    seqs: Option<Vec<ValueSeqNum>>,
    subkey: Option<ValueSubkey>,
    value: Option<Arc<SignedValueData>>,
}

impl RPCOperationTransactCommandQ {
    pub fn new(
        key: OpaqueRecordKey,
        transaction_id: u64,
        command: TransactCommand,
        seqs: Option<Vec<ValueSeqNum>>,
        subkey: Option<ValueSubkey>,
        value: Option<Arc<SignedValueData>>,
    ) -> Result<Self, RPCError> {
        // Transaction id should never be zero here as that is the sentinel for None
        if transaction_id == 0u64 {
            return Err(RPCError::protocol("invalid transaction id"));
        }

        if let Some(seqs) = &seqs {
            if seqs.len() > MAX_TRANSACT_COMMAND_Q_SEQS_LEN {
                return Err(RPCError::protocol(
                    "encoded TransactCommandQ seqs length too long",
                ));
            }
        }

        Ok(Self {
            key,
            transaction_id,
            command,
            seqs,
            subkey,
            value,
        })
    }

    pub fn validate(&self, _validate_context: &RPCValidateContext<'_>) -> Result<(), RPCError> {
        // Descriptor not available at this time, so don't check those things, let StorageManager do it

        // Validate the presence of command arguments
        match self.command {
            TransactCommand::End | TransactCommand::Commit | TransactCommand::Rollback => {
                if self.seqs.is_some() {
                    // Seqs must not be specified here
                    return Err(RPCError::protocol(
                        "seqs must not be specified for this command",
                    ));
                }
                if self.subkey.is_some() {
                    // Subkey must not be specified here
                    return Err(RPCError::protocol(
                        "subkey must not be specified for this command",
                    ));
                }
                if self.value.is_some() {
                    // Value must not be specified here
                    return Err(RPCError::protocol(
                        "value must not be specified for this command",
                    ));
                }
            }
            TransactCommand::Get => {
                if self.seqs.is_some() {
                    // Seqs must not be specified here
                    return Err(RPCError::protocol(
                        "seqs must not be specified for this command",
                    ));
                }
                // Get with no subkey is just a 'keepalive'
                if self.value.is_some() {
                    // Value must not be specified here
                    return Err(RPCError::protocol(
                        "value must not be specified for this command",
                    ));
                }
            }
            TransactCommand::Set => {
                if self.seqs.is_some() {
                    // Seqs must not be specified here
                    return Err(RPCError::protocol(
                        "seqs must not be specified for this command",
                    ));
                }
                // Subkey must be specified
                if self.subkey.is_none() {
                    return Err(RPCError::protocol("subkey was not specified for get/set"));
                }
                if self.value.is_none() {
                    // Value must be specified here
                    return Err(RPCError::protocol(
                        "value must be specified for this command",
                    ));
                }
            } // TransactCommand::Sync => {
              //     if self.seqs.is_none() {
              //         // Seqs must be specified here
              //         return Err(RPCError::protocol(
              //             "seqs must be specified for this command",
              //         ));
              //     }

              //     if self.subkey.is_some() != self.value.is_some() {
              //         // Subkey and Value must be specified together if at all
              //         return Err(RPCError::protocol(
              //             "subkey and value must both be specified or neither",
              //         ));
              //     }
              // }
        }
        Ok(())
    }

    #[expect(clippy::type_complexity)]
    pub fn destructure(
        self,
    ) -> (
        OpaqueRecordKey,
        u64,
        TransactCommand,
        Option<Vec<ValueSeqNum>>,
        Option<ValueSubkey>,
        Option<Arc<SignedValueData>>,
    ) {
        (
            self.key,
            self.transaction_id,
            self.command,
            self.seqs,
            self.subkey,
            self.value,
        )
    }

    pub fn decode(
        _decode_context: &RPCDecodeContext,
        reader: &veilid_capnp::operation_transact_command_q::Reader,
    ) -> Result<Self, RPCError> {
        rpc_ignore_missing_property!(reader, key);
        let k_reader = reader.get_key()?;
        let key = decode_opaque_record_key(&k_reader)?;

        let cmd_reader = reader.get_command()?;
        let command = match cmd_reader {
            veilid_capnp::TransactCommand::End => TransactCommand::End,
            veilid_capnp::TransactCommand::Commit => TransactCommand::Commit,
            veilid_capnp::TransactCommand::Rollback => TransactCommand::Rollback,
            veilid_capnp::TransactCommand::Get => TransactCommand::Get,
            veilid_capnp::TransactCommand::Set => TransactCommand::Set,
            // veilid_capnp::TransactCommand::Sync => TransactCommand::Sync,
        };

        let transaction_id = reader.get_transaction_id();
        if transaction_id == 0 {
            return Err(RPCError::protocol("transaction id must not be zero"));
        }

        let seqs = if reader.has_seqs() {
            let seqs_reader = reader.get_seqs()?;
            rpc_ignore_max_len!(seqs_reader, MAX_TRANSACT_COMMAND_Q_SEQS_LEN);
            let seqs = seqs_reader.iter().map(ValueSeqNum::from).collect();
            Some(seqs)
        } else {
            None
        };

        // Subkey is required for set, optional for get and sync, and not used by others
        let subkey = reader.get_subkey();
        let subkey = if subkey == ValueSubkey::MAX {
            None
        } else {
            Some(subkey)
        };

        // Value is required for set, optional for sync and not used by others
        let value = if matches!(command, TransactCommand::Set) {
            if !reader.has_value() {
                return Err(RPCError::protocol("set requires value"));
            }
            let svd_reader = reader.get_value()?;
            Some(Arc::new(decode_signed_value_data(&svd_reader)?))
        }
        // else if matches!(command, TransactCommand::Sync) {
        //     if reader.has_value() {
        //         let svd_reader = reader.get_value()?;
        //         Some(decode_signed_value_data(&svd_reader)?)
        //     } else {
        //         None
        //     }
        // }
        else {
            None
        };

        Self::new(key, transaction_id, command, seqs, subkey, value)
    }
    pub fn encode(
        &self,
        builder: &mut veilid_capnp::operation_transact_command_q::Builder,
    ) -> Result<(), RPCError> {
        let mut k_builder = builder.reborrow().init_key();
        encode_opaque_record_key(&self.key, &mut k_builder);

        builder.set_transaction_id(self.transaction_id);

        builder.set_command(match self.command {
            TransactCommand::End => veilid_capnp::TransactCommand::End,
            TransactCommand::Commit => veilid_capnp::TransactCommand::Commit,
            TransactCommand::Rollback => veilid_capnp::TransactCommand::Rollback,
            TransactCommand::Get => veilid_capnp::TransactCommand::Get,
            TransactCommand::Set => veilid_capnp::TransactCommand::Set,
            // TransactCommand::Sync => veilid_capnp::TransactCommand::Sync,
        });

        if let Some(seqs) = &self.seqs {
            let mut seqs_builder = builder.reborrow().init_seqs(
                seqs.len()
                    .try_into()
                    .map_err(RPCError::map_internal("invalid seqs list length"))?,
            );
            for (i, seq) in seqs.iter().enumerate() {
                seqs_builder.set(i as u32, u32::from(*seq));
            }
        }

        builder.set_subkey(self.subkey.unwrap_or(ValueSubkey::MAX));

        if let Some(value) = &self.value {
            let mut v_builder = builder.reborrow().init_value();
            encode_signed_value_data(value.as_ref(), &mut v_builder)?;
        }

        Ok(())
    }
}

////////////////////////////////////////////////////////////////////////////////////////////////////

#[derive(Debug, Clone)]
pub(in crate::rpc_processor) struct RPCOperationTransactCommandA {
    transaction_valid: bool,
    duration: TimestampDuration,
    seqs: Option<Vec<ValueSeqNum>>,
    subkey: Option<ValueSubkey>,
    value: Option<Arc<SignedValueData>>,
}

impl RPCOperationTransactCommandA {
    pub fn new(
        transaction_valid: bool,
        duration: TimestampDuration,
        seqs: Option<Vec<ValueSeqNum>>,
        subkey: Option<ValueSubkey>,
        value: Option<Arc<SignedValueData>>,
    ) -> Result<Self, RPCError> {
        // Should not be invalid but also provide other fields
        if !transaction_valid
            && (!duration.is_zero() || seqs.is_some() || subkey.is_some() || value.is_some())
        {
            return Err(RPCError::internal("not valid but fields provided"));
        }

        if let Some(seqs) = &seqs {
            if seqs.len() > MAX_TRANSACT_COMMAND_A_SEQS_LEN {
                return Err(RPCError::protocol(
                    "encoded TransactCommandA seqs length too long",
                ));
            }
        }

        Ok(Self {
            transaction_valid,
            duration,
            seqs,
            subkey,
            value,
        })
    }

    pub async fn validate(
        &self,
        validate_context: &RPCValidateContext<'_>,
    ) -> Result<(), RPCError> {
        let question_context = validate_context
            .question_context
            .as_ref()
            .expect_or_log("TransactCommandA requires question context");
        let QuestionContext::TransactCommand(transact_command_context) = question_context else {
            return Err(RPCError::internal(
                "Wrong context type for TransactCommandA",
            ));
        };

        // No validation necessary if the transaction was not valid
        if !self.transaction_valid {
            return Ok(());
        }

        // Validate the presence of command arguments
        match transact_command_context.command {
            TransactCommand::End | TransactCommand::Commit | TransactCommand::Rollback => {
                if self.seqs.is_some() {
                    // Seqs must not be specified here
                    return Err(RPCError::protocol(
                        "seqs must not be specified for this command",
                    ));
                }
                if self.subkey.is_some() {
                    // Subkey must not be specified here
                    return Err(RPCError::protocol(
                        "subkey must not be specified for this command",
                    ));
                }
                if self.value.is_some() {
                    // Value must not be specified here
                    return Err(RPCError::protocol(
                        "value must not be specified for this command",
                    ));
                }
            }
            TransactCommand::Get => {
                // Subkey not specified is just a 'keepalive'

                // Subkey must match
                if self.subkey != transact_command_context.opt_subkey {
                    return Err(RPCError::protocol(
                        "subkey returned did not match requested subkey for get",
                    ));
                }
            }
            TransactCommand::Set => {
                if self.subkey.is_none() {
                    // Subkey must be specified
                    return Err(RPCError::protocol("subkey must be specified"));
                }

                // Subkey must match if specified
                if self.subkey != transact_command_context.opt_subkey {
                    return Err(RPCError::protocol(
                        "subkey returned did not match requested subkey for set",
                    ));
                }

                // Value returned must have equal or greater sequence number
                if let Some(value) = &self.value {
                    if value.value_data().seq()
                        < transact_command_context
                            .opt_value
                            .as_ref()
                            .map(|x| x.value_data().seq())
                            .unwrap_or_default()
                    {
                        return Err(RPCError::protocol(
                            "value returned did not have equal or greater sequence number",
                        ));
                    }
                }
            } // TransactCommand::Sync => {
              //     if self.seqs.is_none() {
              //         // Seqs must be specified here
              //         return Err(RPCError::protocol(
              //             "seqs must be specified for this command",
              //         ));
              //     }

              //     if self.subkey.is_some() != self.value.is_some() {
              //         // Subkey and Value must be specified together if at all
              //         return Err(RPCError::protocol(
              //             "subkey and value must both be specified or neither",
              //         ));
              //     }
              // }
        }

        let crypto = validate_context.crypto();
        let Some(vcrypto) = crypto.get_async(transact_command_context.opaque_record_key.kind())
        else {
            return Err(RPCError::protocol("unsupported cryptosystem"));
        };

        let schema = transact_command_context
            .descriptor
            .schema()
            .map_err(RPCError::protocol)?;

        // Check the subkey and seqs against the schema
        if let Some(seqs) = &self.seqs {
            if seqs.len() != schema.subkey_count() {
                return Err(RPCError::protocol("seqs list mismatch"));
            }
        }
        if let Some(subkey) = self.subkey {
            if subkey > schema.max_subkey() {
                return Err(RPCError::protocol("subkey out of range"));
            }
        }

        // Ensure the value validates
        if let Some(value) = &self.value {
            // If value is specified, so must be subkey
            let Some(subkey) = self.subkey else {
                return Err(RPCError::protocol("value specified without subkey"));
            };

            if !value
                .validate(
                    transact_command_context.descriptor.ref_owner(),
                    subkey,
                    &vcrypto,
                )
                .await
                .map_err(RPCError::protocol)?
            {
                return Err(RPCError::protocol("signed value data did not validate"));
            }
        }

        Ok(())
    }

    #[expect(clippy::type_complexity)]
    pub fn destructure(
        self,
    ) -> (
        bool,
        TimestampDuration,
        Option<Vec<ValueSeqNum>>,
        Option<ValueSubkey>,
        Option<Arc<SignedValueData>>,
    ) {
        (
            self.transaction_valid,
            self.duration,
            self.seqs,
            self.subkey,
            self.value,
        )
    }

    pub fn decode(
        _decode_context: &RPCDecodeContext,
        reader: &veilid_capnp::operation_transact_command_a::Reader,
    ) -> Result<Self, RPCError> {
        let transaction_valid = reader.get_transaction_valid();

        let duration = TimestampDuration::new(reader.get_duration());

        let seqs = if reader.has_seqs() {
            let seqs_reader = reader.get_seqs()?;
            rpc_ignore_max_len!(seqs_reader, MAX_TRANSACT_COMMAND_A_SEQS_LEN);
            let seqs = seqs_reader.iter().map(ValueSeqNum::from).collect();
            Some(seqs)
        } else {
            None
        };

        let subkey = reader.get_subkey();
        let subkey = if subkey == ValueSubkey::MAX {
            None
        } else {
            Some(subkey)
        };
        let value = if reader.has_value() {
            let v_reader = reader.get_value()?;
            let value = decode_signed_value_data(&v_reader)?;
            Some(Arc::new(value))
        } else {
            None
        };

        // Should not be invalid but also provide other fields
        if !transaction_valid
            && (!duration.is_zero() || seqs.is_some() || subkey.is_some() || value.is_some())
        {
            return Err(RPCError::protocol("not valid but fields provided"));
        }

        Self::new(transaction_valid, duration, seqs, subkey, value)
    }

    pub fn encode(
        &self,
        builder: &mut veilid_capnp::operation_transact_command_a::Builder,
    ) -> Result<(), RPCError> {
        builder.set_transaction_valid(self.transaction_valid);
        builder.set_duration(self.duration.as_u64());

        if let Some(seqs) = &self.seqs {
            let mut seqs_builder = builder.reborrow().init_seqs(
                seqs.len()
                    .try_into()
                    .map_err(RPCError::map_internal("invalid seqs list length"))?,
            );
            for (i, seq) in seqs.iter().enumerate() {
                seqs_builder.set(i as u32, u32::from(*seq));
            }
        }

        builder.set_subkey(self.subkey.unwrap_or(ValueSubkey::MAX));

        if let Some(value) = &self.value {
            let mut v_builder = builder.reborrow().init_value();
            encode_signed_value_data(value, &mut v_builder)?;
        }

        Ok(())
    }
}
