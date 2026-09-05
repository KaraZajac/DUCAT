use super::*;

pub fn decode_signal_info(
    decode_context: &RPCDecodeContext,
    reader: &veilid_capnp::operation_signal::Reader,
) -> Result<SignalInfo, RPCError> {
    Ok(match reader.which()? {
        veilid_capnp::operation_signal::HolePunch(r) => {
            // Extract hole punch reader
            let r = r?;
            rpc_ignore_missing_property!(r, receipt);
            let receipt = Bytes::copy_from_slice(r.get_receipt()?);
            rpc_ignore_missing_property!(r, peer_info);
            let pi_reader = r.get_peer_info()?;
            let peer_info = Arc::new(decode_peer_info(decode_context, &pi_reader)?);
            let opt_dial_info = if r.has_dial_info() {
                let di_reader = r.get_dial_info()?;
                Some(decode_dial_info(&di_reader)?)
            } else {
                None
            };

            SignalInfo::HolePunch {
                receipt,
                peer_info,
                opt_dial_info,
            }
        }
        veilid_capnp::operation_signal::ReverseConnect(r) => {
            // Extract reverse connect reader
            let r = r?;
            rpc_ignore_missing_property!(r, receipt);
            let receipt = Bytes::copy_from_slice(r.get_receipt()?);
            rpc_ignore_missing_property!(r, peer_info);
            let pi_reader = r.get_peer_info()?;
            let peer_info = Arc::new(decode_peer_info(decode_context, &pi_reader)?);

            SignalInfo::ReverseConnect { receipt, peer_info }
        }
    })
}

pub fn encode_signal_info(
    signal_info: &SignalInfo,
    builder: &mut veilid_capnp::operation_signal::Builder,
) -> Result<(), RPCError> {
    match signal_info {
        SignalInfo::HolePunch {
            receipt,
            peer_info,
            opt_dial_info,
        } => {
            let mut hp_builder = builder.reborrow().init_hole_punch();
            let r_builder = hp_builder
                .reborrow()
                .init_receipt(receipt.len().try_into().map_err(RPCError::map_protocol(
                    "invalid receipt length in encode_signal_info",
                ))?);
            r_builder.copy_from_slice(receipt);
            let mut pi_builder = hp_builder.reborrow().init_peer_info();
            encode_peer_info(peer_info, &mut pi_builder)?;
            if let Some(dial_info) = opt_dial_info {
                let mut di_builder = hp_builder.init_dial_info();
                encode_dial_info(dial_info, &mut di_builder)?;
            }
        }
        SignalInfo::ReverseConnect { receipt, peer_info } => {
            let mut rc_builder = builder.reborrow().init_reverse_connect();
            let r_builder = rc_builder
                .reborrow()
                .init_receipt(receipt.len().try_into().map_err(RPCError::map_protocol(
                    "invalid receipt length in encode_signal_info",
                ))?);
            r_builder.copy_from_slice(receipt);
            let mut pi_builder = rc_builder.init_peer_info();
            encode_peer_info(peer_info, &mut pi_builder)?;
        }
    }

    Ok(())
}
