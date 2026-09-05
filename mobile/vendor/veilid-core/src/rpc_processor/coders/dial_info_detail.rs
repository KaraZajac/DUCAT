use super::*;

pub fn decode_dial_info_detail(
    reader: &veilid_capnp::dial_info_detail::Reader,
) -> Result<DialInfoDetail, RPCError> {
    rpc_ignore_missing_property!(reader, dial_info);
    let dial_info = decode_dial_info(&reader.get_dial_info()?)?;

    let class = decode_dial_info_class(reader.get_class())?;

    Ok(DialInfoDetail { dial_info, class })
}

pub fn encode_dial_info_detail(
    dial_info_detail: &DialInfoDetail,
    builder: &mut veilid_capnp::dial_info_detail::Builder,
) -> Result<(), RPCError> {
    let mut di_builder = builder.reborrow().init_dial_info();
    encode_dial_info(&dial_info_detail.dial_info, &mut di_builder)?;

    builder.set_class(encode_dial_info_class(dial_info_detail.class));
    Ok(())
}
