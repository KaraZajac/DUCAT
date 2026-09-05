use super::*;

pub fn decode_relay_info(reader: &veilid_capnp::relay_info::Reader) -> Result<RelayInfo, RPCError> {
    let timestamp = Timestamp::new(reader.get_timestamp());

    rpc_ignore_missing_property!(reader, node_ids);
    let nids_reader = reader.get_node_ids()?;

    let mut node_ids = NodeIdGroup::with_capacity(nids_reader.len() as usize);
    for nid_reader in nids_reader.iter() {
        let Some(nid) = decode_node_id(&nid_reader).ignore_ok()? else {
            continue;
        };
        node_ids.add(nid);
    }
    if node_ids.is_empty() {
        return Err(RPCError::ignore("no node ids for relay"));
    }

    rpc_ignore_missing_property!(reader, outbound_protocols);
    let outbound_protocols = decode_protocol_type_set(&reader.get_outbound_protocols()?);

    rpc_ignore_missing_property!(reader, address_types);
    let address_types = decode_address_type_set(&reader.get_address_types()?);

    rpc_ignore_missing_property!(reader, dial_info_detail_list);
    let didl_reader = reader.get_dial_info_detail_list()?;
    let mut dial_info_detail_list =
        Vec::<DialInfoDetail>::with_capacity(didl_reader.len().try_into().map_err(
            RPCError::map_protocol("too many dial info details for relay"),
        )?);
    for did in didl_reader.iter() {
        let Some(dial_info_detail) = decode_dial_info_detail(&did).ignore_ok()? else {
            continue;
        };
        // Skip relay info dial info that requires a signal
        if dial_info_detail.class.requires_signal() {
            continue;
        }
        dial_info_detail_list.push(dial_info_detail)
    }

    let relay_kind = decode_relay_kind(reader.get_relay_kind())?;

    Ok(RelayInfo::new(
        timestamp,
        node_ids,
        outbound_protocols,
        address_types,
        dial_info_detail_list,
        relay_kind,
    ))
}

pub fn encode_relay_info(
    relay_info: &RelayInfo,
    builder: &mut veilid_capnp::relay_info::Builder,
) -> Result<(), RPCError> {
    builder.set_timestamp(relay_info.timestamp().as_u64());

    let nid_count: u32 = relay_info
        .node_ids()
        .len()
        .try_into()
        .map_err(RPCError::map_protocol("too many node ids in relay info"))?;
    let mut nids_builder = builder.reborrow().init_node_ids(nid_count);
    for idx in 0..nid_count {
        let mut nid_builder = nids_builder.reborrow().get(idx);
        encode_node_id(&relay_info.node_ids()[idx as usize], &mut nid_builder);
    }

    let mut ps_builder = builder.reborrow().init_outbound_protocols(
        relay_info
            .outbound_protocols()
            .len()
            .try_into()
            .map_err(RPCError::map_protocol(
                "too many outbound protocols in relay info",
            ))?,
    );
    encode_protocol_type_set(&relay_info.outbound_protocols(), &mut ps_builder);

    let mut ats_builder = builder.reborrow().init_address_types(
        relay_info
            .address_types()
            .len()
            .try_into()
            .map_err(RPCError::map_protocol(
                "too many address types in relay info",
            ))?,
    );
    encode_address_type_set(&relay_info.address_types(), &mut ats_builder);

    let did_count: u32 = relay_info
        .dial_info_detail_list()
        .len()
        .try_into()
        .map_err(RPCError::map_protocol(
            "too many dial info details in relay info",
        ))?;
    let mut didl_builder = builder.reborrow().init_dial_info_detail_list(did_count);

    for idx in 0..did_count {
        let mut did_builder = didl_builder.reborrow().get(idx);
        encode_dial_info_detail(
            &relay_info.dial_info_detail_list()[idx as usize],
            &mut did_builder,
        )?;
    }

    builder.set_relay_kind(encode_relay_kind(relay_info.relay_kind()));

    Ok(())
}
