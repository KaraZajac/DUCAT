use super::*;

pub fn decode_crypto_info(
    reader: &veilid_capnp::crypto_info::Reader,
) -> Result<CryptoInfo, RPCError> {
    let ck = reader.get_crypto_kind();
    match ck {
        #[cfg(feature = "enable-crypto-none")]
        CRYPTO_KIND_NONE_FOURCC => {
            let none = reader
                .get_detail()
                .get_as::<veilid_capnp::crypto_info_n_o_n_e::Reader>()?;
            let public_key = BarePublicKey::new(none.get_public_key()?);
            Ok(CryptoInfo::NONE { public_key })
        }
        #[cfg(feature = "enable-crypto-vld0")]
        CRYPTO_KIND_VLD0_FOURCC => {
            let vld0 = reader
                .get_detail()
                .get_as::<veilid_capnp::crypto_info_v_l_d0::Reader>()?;
            let public_key = BarePublicKey::new(vld0.get_public_key()?);
            Ok(CryptoInfo::VLD0 { public_key })
        }
        // #[cfg(feature = "enable-crypto-vld1")]
        // CRYPTO_KIND_VLD1_FOURCC => {
        //     let vld1 = reader
        //         .get_detail()
        //         .get_as::<veilid_capnp::crypto_info_v_l_d1::Reader>()?;
        //     let encapsulation_key = BareEncapsulationKey::new(vld1.get_encapsulation_key()?);
        //     let signing_key = BarePublicKey::new(vld1.get_signing_key()?);
        //     Ok(CryptoInfo::VLD1 {
        //         encapsulation_key,
        //         signing_key,
        //     })
        // }
        _ => Err(RPCError::ignore("unknown crypto kind")),
    }
}

pub fn encode_crypto_info(
    crypto_info: &CryptoInfo,
    builder: &mut veilid_capnp::crypto_info::Builder,
) {
    match crypto_info {
        #[cfg(feature = "enable-crypto-none")]
        CryptoInfo::NONE { public_key } => {
            builder.set_crypto_kind(CRYPTO_KIND_NONE_FOURCC);
            let mut noneb = builder
                .reborrow()
                .init_detail()
                .init_as::<veilid_capnp::crypto_info_n_o_n_e::Builder>();
            noneb.set_public_key(public_key);
        }
        #[cfg(feature = "enable-crypto-vld0")]
        CryptoInfo::VLD0 { public_key } => {
            builder.set_crypto_kind(CRYPTO_KIND_VLD0_FOURCC);
            let mut vld0b = builder
                .reborrow()
                .init_detail()
                .init_as::<veilid_capnp::crypto_info_v_l_d0::Builder>();
            vld0b.set_public_key(public_key);
        } // #[cfg(feature = "enable-crypto-vld1")]
          // CryptoInfo::VLD1 {
          //     encapsulation_key,
          //     signing_key,
          // } => {
          //     builder.set_crypto_kind(CRYPTO_KIND_VLD1_FOURCC);
          //     let mut vld1b = builder.reborrow()
          //         .init_detail()
          //         .init_as::<veilid_capnp::crypto_info_v_l_d1::Builder>();
          //     vld1b.set_encapsulation_key(encapsulation_key);
          //     vld1b.set_signing_key(signing_key);
          // }
    };
}
