use super::*;
use core::convert::TryInto;

fn reject_unspecified_socket_address(socket_address: &SocketAddress) -> Result<(), RPCError> {
    if socket_address.address().is_unspecified() {
        return Err(RPCError::protocol(
            "dial info with unspecified (0.0.0.0 or ::) address not allowed",
        ));
    }
    Ok(())
}

pub fn decode_dial_info(reader: &veilid_capnp::dial_info::Reader) -> Result<DialInfo, RPCError> {
    let pt = reader.get_protocol_type();

    match pt {
        FOURCC_PROTOCOL_TYPE_UDP => {
            let udp = reader
                .get_detail()
                .get_as::<veilid_capnp::dial_info_u_d_p::Reader>()?;

            rpc_ignore_missing_property!(udp, socket_address);
            let socket_address_reader = udp.get_socket_address()?;
            let socket_address = decode_socket_address(&socket_address_reader)?;
            reject_unspecified_socket_address(&socket_address)?;
            Ok(DialInfo::udp(socket_address))
        }
        FOURCC_PROTOCOL_TYPE_TCP => {
            let tcp = reader
                .get_detail()
                .get_as::<veilid_capnp::dial_info_t_c_p::Reader>()?;

            rpc_ignore_missing_property!(tcp, socket_address);
            let socket_address_reader = tcp.get_socket_address()?;
            let socket_address = decode_socket_address(&socket_address_reader)?;
            reject_unspecified_socket_address(&socket_address)?;
            Ok(DialInfo::tcp(socket_address))
        }
        FOURCC_PROTOCOL_TYPE_WS => {
            let ws = reader
                .get_detail()
                .get_as::<veilid_capnp::dial_info_w_s::Reader>()?;

            rpc_ignore_missing_property!(ws, socket_address);
            let socket_address_reader = ws.get_socket_address()?;
            let socket_address = decode_socket_address(&socket_address_reader)?;
            reject_unspecified_socket_address(&socket_address)?;
            rpc_ignore_missing_property!(ws, request);
            let request = ws.get_request()?;
            DialInfo::try_ws(
                socket_address,
                request
                    .to_string()
                    .map_err(RPCError::map_protocol("invalid WS request string"))?,
            )
            .map_err(RPCError::map_protocol("invalid WS dial info"))
        }
        #[cfg(feature = "enable-protocol-wss")]
        FOURCC_PROTOCOL_TYPE_WSS => {
            let wss = reader
                .get_detail()
                .get_as::<veilid_capnp::dial_info_w_s_s::Reader>()?;

            rpc_ignore_missing_property!(wss, socket_address);
            let socket_address_reader = wss
                .get_socket_address()
                .map_err(RPCError::map_protocol("missing WSS socketAddress"))?;
            let socket_address = decode_socket_address(&socket_address_reader)?;
            reject_unspecified_socket_address(&socket_address)?;
            rpc_ignore_missing_property!(wss, request);
            let request = wss.get_request()?;
            DialInfo::try_wss(
                socket_address,
                request
                    .to_string()
                    .map_err(RPCError::map_protocol("invalid WSS request string"))?,
            )
            .map_err(RPCError::map_protocol("invalid WSS dial info"))
        }
        _ => Err(RPCError::ignore("unknown protocol type")),
    }
}

pub fn encode_dial_info(
    dial_info: &DialInfo,
    builder: &mut veilid_capnp::dial_info::Builder,
) -> Result<(), RPCError> {
    match dial_info {
        DialInfo::UDP(udp) => {
            builder.set_protocol_type(FOURCC_PROTOCOL_TYPE_UDP);
            let mut di_udp_builder = builder
                .reborrow()
                .init_detail()
                .init_as::<veilid_capnp::dial_info_u_d_p::Builder>();

            encode_socket_address(
                &udp.socket_address,
                &mut di_udp_builder.reborrow().init_socket_address(),
            )?;
        }
        DialInfo::TCP(tcp) => {
            builder.set_protocol_type(FOURCC_PROTOCOL_TYPE_TCP);
            let mut di_tcp_builder = builder
                .reborrow()
                .init_detail()
                .init_as::<veilid_capnp::dial_info_t_c_p::Builder>();

            encode_socket_address(
                &tcp.socket_address,
                &mut di_tcp_builder.reborrow().init_socket_address(),
            )?;
        }
        DialInfo::WS(ws) => {
            builder.set_protocol_type(FOURCC_PROTOCOL_TYPE_WS);
            let mut di_ws_builder = builder
                .reborrow()
                .init_detail()
                .init_as::<veilid_capnp::dial_info_w_s::Builder>();

            encode_socket_address(
                &ws.socket_address,
                &mut di_ws_builder.reborrow().init_socket_address(),
            )?;
            let request = dial_info
                .request()
                .ok_or_else(RPCError::else_internal("no request for WS dialinfo"))?;

            let mut requestb = di_ws_builder.init_request(
                request
                    .len()
                    .try_into()
                    .map_err(RPCError::map_protocol("request too long"))?,
            );
            requestb.push_str(request.as_str());
        }
        #[cfg(feature = "enable-protocol-wss")]
        DialInfo::WSS(wss) => {
            builder.set_protocol_type(FOURCC_PROTOCOL_TYPE_WSS);
            let mut di_wss_builder = builder
                .reborrow()
                .init_detail()
                .init_as::<veilid_capnp::dial_info_w_s_s::Builder>();

            encode_socket_address(
                &wss.socket_address,
                &mut di_wss_builder.reborrow().init_socket_address(),
            )?;
            let request = dial_info
                .request()
                .ok_or_else(RPCError::else_internal("no request for WSS dialinfo"))?;

            let mut requestb = di_wss_builder.init_request(
                request
                    .len()
                    .try_into()
                    .map_err(RPCError::map_protocol("request too long"))?,
            );
            requestb.push_str(request.as_str());
        }
    };
    Ok(())
}
