use super::*;

pub fn decode_peer_info(
    decode_context: &RPCDecodeContext,
    reader: &veilid_capnp::peer_info::Reader,
) -> Result<PeerInfo, RPCError> {
    rpc_ignore_missing_property!(reader, node_info_message);
    let node_info_message = reader
        .get_node_info_message()
        .map_err(RPCError::map_protocol("can't get node info message"))?;

    rpc_ignore_missing_property!(reader, signatures);
    let sigs_reader = reader.get_signatures()?;
    let _sigs_len = rpc_ignore_max_len!(sigs_reader, MAX_CRYPTO_KINDS);
    let mut signatures = SignatureGroup::new();
    for sig_reader in sigs_reader {
        let Some(typed_signature) = decode_signature(&sig_reader).ignore_ok()? else {
            continue;
        };
        signatures.add(typed_signature);
    }

    let routing_table = decode_context.registry.routing_table();
    let opt_peer_info = PeerInfo::new_from_wire(
        &routing_table,
        decode_context.origin_routing_domain,
        node_info_message,
        signatures,
    )
    .map_err(RPCError::map_protocol("can't create peerinfo from wire"))?;
    let Some(peer_info) = opt_peer_info else {
        return Err(RPCError::ignore(
            "no valid crypto kinds and routing domains for peer info",
        ));
    };

    Ok(peer_info)
}

pub fn encode_peer_info(
    peer_info: &PeerInfo,
    builder: &mut veilid_capnp::peer_info::Builder,
) -> Result<(), RPCError> {
    builder.set_node_info_message(peer_info.node_info_message());

    let signatures = peer_info.signatures();
    let mut sigs_builder = builder.reborrow().init_signatures(
        signatures
            .len()
            .try_into()
            .map_err(RPCError::map_invalid_format("out of bound error"))?,
    );
    for (i, typed_signature) in signatures.iter().enumerate() {
        encode_signature(
            typed_signature,
            &mut sigs_builder.reborrow().get(
                i.try_into()
                    .map_err(RPCError::map_invalid_format("out of bound error"))?,
            ),
        );
    }

    Ok(())
}
