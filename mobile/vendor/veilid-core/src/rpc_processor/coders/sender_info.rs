use super::*;

pub fn decode_sender_info(
    reader: &veilid_capnp::sender_info::Reader,
) -> Result<SenderInfo, RPCError> {
    rpc_ignore_missing_property!(reader, socket_address);
    let sa_reader = reader.get_socket_address()?;
    let socket_address = decode_socket_address(&sa_reader)?;

    Ok(SenderInfo { socket_address })
}

pub fn encode_sender_info(
    sender_info: &SenderInfo,
    builder: &mut veilid_capnp::sender_info::Builder,
) -> Result<(), RPCError> {
    let mut sab = builder.reborrow().init_socket_address();
    encode_socket_address(&sender_info.socket_address, &mut sab)?;
    Ok(())
}
