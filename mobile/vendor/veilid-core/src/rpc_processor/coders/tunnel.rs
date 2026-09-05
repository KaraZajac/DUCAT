use super::*;

pub fn decode_tunnel_mode(tunnel_endpoint_mode: veilid_capnp::TunnelEndpointMode) -> TunnelMode {
    match tunnel_endpoint_mode {
        veilid_capnp::TunnelEndpointMode::Raw => TunnelMode::Raw,
        veilid_capnp::TunnelEndpointMode::Turn => TunnelMode::Turn,
    }
}

pub fn encode_tunnel_mode(tunnel_mode: TunnelMode) -> veilid_capnp::TunnelEndpointMode {
    match tunnel_mode {
        TunnelMode::Raw => veilid_capnp::TunnelEndpointMode::Raw,
        TunnelMode::Turn => veilid_capnp::TunnelEndpointMode::Turn,
    }
}

pub fn decode_tunnel_error(tunnel_error: veilid_capnp::TunnelError) -> TunnelError {
    match tunnel_error {
        veilid_capnp::TunnelError::BadId => TunnelError::BadId,
        veilid_capnp::TunnelError::NoEndpoint => TunnelError::NoEndpoint,
        veilid_capnp::TunnelError::RejectedMode => TunnelError::RejectedMode,
        veilid_capnp::TunnelError::NoCapacity => TunnelError::NoCapacity,
    }
}

pub fn encode_tunnel_error(tunnel_error: TunnelError) -> veilid_capnp::TunnelError {
    match tunnel_error {
        TunnelError::BadId => veilid_capnp::TunnelError::BadId,
        TunnelError::NoEndpoint => veilid_capnp::TunnelError::NoEndpoint,
        TunnelError::RejectedMode => veilid_capnp::TunnelError::RejectedMode,
        TunnelError::NoCapacity => veilid_capnp::TunnelError::NoCapacity,
    }
}

pub fn decode_tunnel_endpoint(
    reader: &veilid_capnp::tunnel_endpoint::Reader,
) -> Result<TunnelEndpoint, RPCError> {
    let mode = decode_tunnel_mode(reader.get_mode()?);
    rpc_ignore_missing_property!(reader, description);
    let description = reader.get_description()?.to_owned();

    Ok(TunnelEndpoint { mode, description })
}

pub fn encode_tunnel_endpoint(
    tunnel_endpoint: &TunnelEndpoint,
    builder: &mut veilid_capnp::tunnel_endpoint::Builder,
) -> Result<(), RPCError> {
    builder.set_mode(encode_tunnel_mode(tunnel_endpoint.mode));
    builder.set_description(&tunnel_endpoint.description);

    Ok(())
}

pub fn decode_full_tunnel(
    reader: &veilid_capnp::full_tunnel::Reader,
) -> Result<FullTunnel, RPCError> {
    let id = TunnelId::new(reader.get_id());
    let timeout = TimestampDuration::new(reader.get_timeout());
    rpc_ignore_missing_property!(reader, local);
    let l_reader = reader.get_local()?;
    let local = decode_tunnel_endpoint(&l_reader)?;
    rpc_ignore_missing_property!(reader, remote);
    let r_reader = reader.get_remote()?;
    let remote = decode_tunnel_endpoint(&r_reader)?;

    Ok(FullTunnel {
        id,
        timeout,
        local,
        remote,
    })
}

pub fn encode_full_tunnel(
    full_tunnel: &FullTunnel,
    builder: &mut veilid_capnp::full_tunnel::Builder,
) -> Result<(), RPCError> {
    builder.set_id(full_tunnel.id.as_u64());
    builder.set_timeout(full_tunnel.timeout.as_u64());
    let mut l_builder = builder.reborrow().init_local();
    encode_tunnel_endpoint(&full_tunnel.local, &mut l_builder)?;
    let mut r_builder = builder.reborrow().init_remote();
    encode_tunnel_endpoint(&full_tunnel.remote, &mut r_builder)?;
    Ok(())
}

pub fn decode_partial_tunnel(
    reader: &veilid_capnp::partial_tunnel::Reader,
) -> Result<PartialTunnel, RPCError> {
    let id = TunnelId::new(reader.get_id());
    let timeout = TimestampDuration::new(reader.get_timeout());
    rpc_ignore_missing_property!(reader, local);
    let l_reader = reader.get_local()?;
    let local = decode_tunnel_endpoint(&l_reader)?;

    Ok(PartialTunnel { id, timeout, local })
}

pub fn encode_partial_tunnel(
    partial_tunnel: &PartialTunnel,
    builder: &mut veilid_capnp::partial_tunnel::Builder,
) -> Result<(), RPCError> {
    builder.set_id(partial_tunnel.id.as_u64());
    builder.set_timeout(partial_tunnel.timeout.as_u64());
    let mut l_builder = builder.reborrow().init_local();
    encode_tunnel_endpoint(&partial_tunnel.local, &mut l_builder)?;
    Ok(())
}
