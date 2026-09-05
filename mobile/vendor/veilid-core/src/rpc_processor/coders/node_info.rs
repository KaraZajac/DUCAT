use super::*;

pub fn decode_node_info(reader: &veilid_capnp::node_info::Reader) -> Result<NodeInfo, RPCError> {
    let timestamp = Timestamp::new(reader.get_timestamp());

    rpc_ignore_missing_property!(reader, envelope_support);
    let es_reader = reader.reborrow().get_envelope_support()?;
    rpc_ignore_min_max_len!(es_reader, 1, MAX_ENVELOPE_VERSIONS);
    let mut envelope_support: Vec<EnvelopeVersion> =
        Vec::with_capacity(es_reader.len().try_into().map_err(RPCError::protocol)?);
    for es in es_reader {
        let eversion = EnvelopeVersion::from(es);
        if !VALID_ENVELOPE_VERSIONS.contains(&eversion) {
            continue;
        }
        envelope_support.push(eversion);
    }

    // Ensure envelope versions are not duplicated
    // Unsorted is okay, some nodes may have a different envelope order preference
    // But nothing should show up more than once
    let mut eversions = envelope_support.clone();
    eversions.sort_unstable();
    eversions.dedup();
    if eversions.len() != envelope_support.len() {
        return Err(RPCError::protocol("duplicate envelope versions"));
    }

    rpc_ignore_missing_property!(reader, crypto_info_list);
    let cs_reader = reader.get_crypto_info_list()?;
    rpc_ignore_min_max_len!(cs_reader, 1, MAX_CRYPTO_KINDS);
    let mut crypto_info_list = Vec::<CryptoInfo>::with_capacity(
        cs_reader
            .len()
            .try_into()
            .map_err(RPCError::map_protocol("too many crypto infos"))?,
    );
    let mut crypto_kinds = HashSet::new();
    for ci in cs_reader.iter() {
        let Some(crypto_info) = decode_crypto_info(&ci).ignore_ok()? else {
            continue;
        };
        // Ensure crypto info kinds are not duplicated
        if !crypto_kinds.insert(crypto_info.kind()) {
            return Err(RPCError::protocol("duplicate crypto kind"));
        }
        crypto_info_list.push(crypto_info);
    }

    rpc_ignore_missing_property!(reader, capabilities);
    let cap_reader = reader.get_capabilities()?;
    rpc_ignore_max_len!(cap_reader, MAX_CAPABILITIES);
    let capabilities = cap_reader
        .as_slice()
        .map(|s| {
            s.iter()
                .map(|x| VeilidCapability::from(x.to_be_bytes()))
                .collect()
        })
        .unwrap_or_default();

    rpc_ignore_missing_property!(reader, outbound_protocols);
    let outbound_protocols = decode_protocol_type_set(&reader.get_outbound_protocols()?);

    rpc_ignore_missing_property!(reader, address_types);
    let address_types = decode_address_type_set(&reader.get_address_types()?);

    rpc_ignore_missing_property!(reader, dial_info_detail_list);
    let didl_reader = reader.get_dial_info_detail_list()?;
    let mut dial_info_detail_list = Vec::<DialInfoDetail>::with_capacity(
        didl_reader
            .len()
            .try_into()
            .map_err(RPCError::map_protocol("too many dial info details"))?,
    );
    for did in didl_reader.iter() {
        let Some(dial_info_detail) = decode_dial_info_detail(&did).ignore_ok()? else {
            continue;
        };
        dial_info_detail_list.push(dial_info_detail);
    }

    rpc_ignore_missing_property!(reader, relay_info_list);
    let ril_reader = reader.get_relay_info_list()?;
    let mut relay_info_list = Vec::<RelayInfo>::with_capacity(
        ril_reader
            .len()
            .try_into()
            .map_err(RPCError::map_protocol("too many relay infos"))?,
    );
    for ri in ril_reader.iter() {
        let Some(relay_info) = decode_relay_info(&ri).ignore_ok()? else {
            continue;
        };
        relay_info_list.push(relay_info);
    }

    Ok(NodeInfo::new(
        timestamp,
        envelope_support,
        crypto_info_list,
        capabilities,
        outbound_protocols,
        address_types,
        dial_info_detail_list,
        relay_info_list,
    ))
}

pub fn encode_node_info(
    node_info: &NodeInfo,
    builder: &mut veilid_capnp::node_info::Builder,
) -> Result<(), RPCError> {
    builder.set_timestamp(node_info.timestamp().as_u64());

    let mut es_builder = builder
        .reborrow()
        .init_envelope_support(node_info.envelope_support().len() as u32);
    if let Some(s) = es_builder.as_slice() {
        let envelope_support: Vec<u32> = node_info
            .envelope_support()
            .iter()
            .copied()
            .map(u32::from)
            .collect();
        s.clone_from_slice(&envelope_support);
    }

    let mut cil_builder = builder.reborrow().init_crypto_info_list(
        node_info
            .crypto_info_list()
            .len()
            .try_into()
            .map_err(RPCError::map_protocol("too many crypto info in node info"))?,
    );

    for idx in 0..node_info.crypto_info_list().len() {
        let mut ci_builder = cil_builder.reborrow().get(idx as u32);
        encode_crypto_info(&node_info.crypto_info_list()[idx], &mut ci_builder);
    }

    let mut cap_builder = builder
        .reborrow()
        .init_capabilities(node_info.capabilities().len() as u32);
    if let Some(s) = cap_builder.as_slice() {
        let capvec: Vec<u32> = node_info
            .capabilities()
            .iter()
            .copied()
            .map(u32::from)
            .collect();

        s.clone_from_slice(&capvec);
    }

    let mut ps_builder = builder.reborrow().init_outbound_protocols(
        node_info
            .outbound_protocols()
            .len()
            .try_into()
            .map_err(RPCError::map_protocol(
                "too many outbound protocols in node info",
            ))?,
    );
    encode_protocol_type_set(&node_info.outbound_protocols(), &mut ps_builder);

    let mut ats_builder =
        builder
            .reborrow()
            .init_address_types(node_info.address_types().len().try_into().map_err(
                RPCError::map_protocol("too many address types in node info"),
            )?);
    encode_address_type_set(&node_info.address_types(), &mut ats_builder);

    let mut didl_builder = builder.reborrow().init_dial_info_detail_list(
        node_info
            .dial_info_detail_list()
            .len()
            .try_into()
            .map_err(RPCError::map_protocol(
                "too many dial info details in node info",
            ))?,
    );

    for idx in 0..node_info.dial_info_detail_list().len() {
        let mut did_builder = didl_builder.reborrow().get(idx as u32);
        encode_dial_info_detail(&node_info.dial_info_detail_list()[idx], &mut did_builder)?;
    }

    let mut ril_builder = builder.reborrow().init_relay_info_list(
        node_info
            .relay_info_list()
            .len()
            .try_into()
            .map_err(RPCError::map_protocol("too many relay info in node info"))?,
    );

    for idx in 0..node_info.relay_info_list().len() {
        let mut ril_builder = ril_builder.reborrow().get(idx as u32);
        encode_relay_info(&node_info.relay_info_list()[idx], &mut ril_builder)?;
    }

    Ok(())
}
