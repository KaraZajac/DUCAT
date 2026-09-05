use super::*;
use pastey::paste;

// Utility Macros
macro_rules! define_typed_byte_data_coder {
    ($capnp_name: ident, $rust_name: ident) => {
        paste! {
            pub fn [< decode_ $capnp_name >](
                reader: &veilid_capnp::$capnp_name::Reader,
            ) -> Result<$rust_name, RPCError> {
                rpc_ignore_missing_property!(reader, value);
                let value = reader.get_value()?;
                let kind = reader.get_kind();

                Ok($rust_name::new(
                    CryptoKind::from(kind.to_be_bytes()),
                    [< Bare $rust_name >]::new(value),
                ))
            }

            pub fn [< encode_ $capnp_name >](
                $capnp_name: &$rust_name,
                builder: &mut veilid_capnp::$capnp_name::Builder,
            ) {
                builder.set_kind(u32::from($capnp_name.kind()));
                builder.set_value($capnp_name.ref_value());
            }
        }
    };
}

macro_rules! define_untyped_byte_data_coder {
    ($capnp_name: ident, $rust_name: ident) => {
        paste! {
            pub fn [< decode_ $capnp_name >](
                reader: &veilid_capnp::$capnp_name::Reader,
            ) -> Result<$rust_name, RPCError> {
                rpc_ignore_missing_property!(reader, value);
                let value = reader.get_value()?;

                Ok(
                    [< $rust_name >]::new(value),
                )
            }

            pub fn [< encode_ $capnp_name >](
                $capnp_name: &$rust_name,
                builder: &mut veilid_capnp::$capnp_name::Builder,
            ) {
                builder.set_value($capnp_name);
            }
        }
    };
}

// OpaqueRecordKey
define_typed_byte_data_coder!(opaque_record_key, OpaqueRecordKey);
// BlockId
#[cfg(feature = "unstable-blockstore")]
define_typed_byte_data_coder!(block_id, BlockId);
// NodeId
define_typed_byte_data_coder!(node_id, NodeId);
// PublicKey
define_typed_byte_data_coder!(public_key, PublicKey);
// RouteId
#[cfg(feature = "unstable-blockstore")]
define_typed_byte_data_coder!(route_id, RouteId);
// Signature
define_typed_byte_data_coder!(signature, Signature);

// Nonce
define_untyped_byte_data_coder!(nonce, Nonce);
