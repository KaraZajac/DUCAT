use super::*;
use crate::storage_manager::*;

fn decode_value_data(
    reader: &veilid_capnp::value_data::Reader,
) -> Result<EncryptedValueData, RPCError> {
    let seq = ValueSeqNum::from(reader.get_seq());

    rpc_ignore_missing_property!(reader, data);
    let data = reader.get_data()?.to_vec();

    rpc_ignore_missing_property!(reader, writer);
    let wr = reader.get_writer()?;
    let writer = decode_public_key(&wr)?;

    let n = reader.get_nonce()?;
    let nonce = if n.has_value() {
        let nonce = decode_nonce(&n)?;
        if nonce.len() != 24 {
            return Err(RPCError::protocol("value data nonce has invalid size"));
        }
        Some(nonce)
    } else {
        None
    };

    EncryptedValueData::new(seq, data.into(), writer, nonce).map_err(RPCError::protocol)
}

pub fn decode_signed_value_data(
    reader: &veilid_capnp::signed_value_data::Reader,
) -> Result<SignedValueData, RPCError> {
    rpc_ignore_missing_property!(reader, value_data);
    let value_data_buf = reader.get_value_data()?;
    let mut value_data_cursor = &mut &value_data_buf[..];
    let tmp_reader = capnp::serialize::read_message(
        &mut value_data_cursor,
        capnp::message::ReaderOptions::new(),
    )?;
    let value_data_reader = tmp_reader.get_root::<veilid_capnp::value_data::Reader>()?;

    let encrypted_value_data = decode_value_data(&value_data_reader)?;

    rpc_ignore_missing_property!(reader, signature);
    let sr = reader.get_signature()?;
    let signature = decode_signature(&sr)?;

    Ok(SignedValueData::new(encrypted_value_data, signature))
}

pub fn encode_signed_value_data(
    signed_value_data: &SignedValueData,
    builder: &mut veilid_capnp::signed_value_data::Builder,
) -> Result<(), RPCError> {
    let encoded_value_data = signed_value_data.value_data().raw_blob();
    builder.set_value_data(encoded_value_data);

    let mut sb = builder.reborrow().init_signature();
    encode_signature(signed_value_data.signature(), &mut sb);

    Ok(())
}
