use super::*;
use crate::storage_manager::SignedValueDescriptor;

pub fn decode_signed_value_descriptor(
    reader: &veilid_capnp::signed_value_descriptor::Reader,
) -> Result<SignedValueDescriptor, RPCError> {
    rpc_ignore_missing_property!(reader, owner);
    let or = reader.get_owner()?;
    let owner = decode_public_key(&or)?;
    rpc_ignore_missing_property!(reader, schema_data);
    let schema_data = reader.get_schema_data()?.to_vec();
    rpc_ignore_missing_property!(reader, signature);
    let sr = reader.get_signature()?;
    let signature = decode_signature(&sr)?;
    Ok(SignedValueDescriptor::new(
        owner,
        schema_data.into(),
        signature,
    ))
}

pub fn encode_signed_value_descriptor(
    signed_value_descriptor: &SignedValueDescriptor,
    builder: &mut veilid_capnp::signed_value_descriptor::Builder,
) -> Result<(), RPCError> {
    let mut ob = builder.reborrow().init_owner();
    encode_public_key(signed_value_descriptor.ref_owner(), &mut ob);
    builder.set_schema_data(signed_value_descriptor.schema_data());
    let mut sb = builder.reborrow().init_signature();
    encode_signature(signed_value_descriptor.ref_signature(), &mut sb);
    Ok(())
}
