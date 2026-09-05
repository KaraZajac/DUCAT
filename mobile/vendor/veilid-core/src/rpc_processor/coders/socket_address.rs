use super::*;

pub fn decode_socket_address(
    reader: &veilid_capnp::socket_address::Reader,
) -> Result<SocketAddress, RPCError> {
    rpc_ignore_missing_property!(reader, address);
    let ar = reader.get_address()?;
    let address = decode_address(&ar)?;
    let port = reader.get_port();

    Ok(SocketAddress::new(address, port))
}

pub fn encode_socket_address(
    socket_address: &SocketAddress,
    builder: &mut veilid_capnp::socket_address::Builder,
) -> Result<(), RPCError> {
    let mut ab = builder.reborrow().init_address();
    encode_address(&socket_address.address(), &mut ab);
    builder.set_port(socket_address.port());
    Ok(())
}
