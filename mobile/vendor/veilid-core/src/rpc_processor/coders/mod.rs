mod address;
mod address_type_set;
mod byte_array_types;
mod crypto_info;
mod dial_info;
mod dial_info_class;
mod dial_info_detail;
mod node_info;
mod node_status;
mod operations;
mod peer_info;
mod private_safety_route;
mod protocol_type_set;
mod relay_info;
mod relay_kind;
mod sender_info;
mod sequencing;
mod signal_info;
mod signed_value_data;
mod signed_value_descriptor;
mod socket_address;
#[cfg(feature = "unstable-tunnels")]
mod tunnel;

pub use address::*;
pub use address_type_set::*;
pub use byte_array_types::*;
pub use crypto_info::*;
pub use dial_info::*;
pub use dial_info_class::*;
pub use dial_info_detail::*;
pub use node_info::*;
pub use node_status::*;
pub use operations::*;
pub use peer_info::*;
pub use private_safety_route::*;
pub use protocol_type_set::*;
pub use relay_info::*;
pub use relay_kind::*;
pub use sender_info::*;
pub use sequencing::*;
pub use signal_info::*;
pub use signed_value_data::*;
pub use signed_value_descriptor::*;
pub use socket_address::*;
#[cfg(feature = "unstable-tunnels")]
pub use tunnel::*;

use super::*;
use capnp::message::ReaderSegments;

impl_veilid_log_facility!("rpc");

#[derive(Debug)]
#[allow(clippy::enum_variant_names)]
pub(in crate::rpc_processor) enum QuestionContext {
    GetValue(ValidateGetValueContext),
    SetValue(ValidateSetValueContext),
    InspectValue(ValidateInspectValueContext),
    TransactBegin(ValidateTransactBeginContext),
    TransactCommand(ValidateTransactCommandContext),
}

pub(in crate::rpc_processor) struct RPCValidateContext<'a> {
    pub registry: VeilidComponentRegistry,
    pub question_context: Option<&'a QuestionContext>,
}
impl_veilid_component_accessors!(RPCValidateContext<'_>);

#[derive(Clone)]
pub struct RPCDecodeContext {
    pub registry: VeilidComponentRegistry,
    pub origin_routing_domain: RoutingDomain,
}

pub fn canonical_message_builder_to_bytes_writer_packed<'a, T>(
    builder: capnp::message::Builder<T>,
    bytes_writer_callback: impl FnOnce(usize) -> BytesWriter,
) -> Result<BytesWriter, RPCError>
where
    T: capnp::message::Allocator + 'a,
{
    // Canonicalize builder
    let out = if builder.len() != 1 {
        let root = builder
            .get_root_as_reader::<capnp::any_pointer::Reader>()
            .map_err(RPCError::protocol)?;

        let size = root.target_size()?.word_count + 1;
        let mut builder = capnp::message::Builder::new(
            capnp::message::HeapAllocator::new().first_segment_words(size as u32),
        );
        builder.set_root_canonical(root)?;

        // Size of -unpacked- output. Should allocate at least this much capacity,
        // because we can't tell what the size of the packed output will be.
        let size = capnp::serialize::compute_serialized_size_in_words(&builder)
            * capnp::private::units::BYTES_PER_WORD;

        let mut out = bytes_writer_callback(size);

        capnp::serialize_packed::write_message(&mut out, &builder).map_err(RPCError::protocol)?;

        out
    } else {
        // Size of -unpacked- output. Should allocate at least this much capacity,
        // because we can't tell what the size of the packed output will be.
        let size = capnp::serialize::compute_serialized_size_in_words(&builder)
            * capnp::private::units::BYTES_PER_WORD;

        let mut out = bytes_writer_callback(size);

        capnp::serialize_packed::write_message(&mut out, &builder).map_err(RPCError::protocol)?;

        out
    };

    Ok(out)
}

pub fn canonical_message_builder_to_bytes_writer_unpacked<'a, T>(
    builder: capnp::message::Builder<T>,
    bytes_writer_callback: impl FnOnce(usize) -> BytesWriter,
) -> Result<BytesWriter, RPCError>
where
    T: capnp::message::Allocator + 'a,
{
    // Canonicalize builder
    let out = if builder.len() != 1 {
        let root = builder
            .get_root_as_reader::<capnp::any_pointer::Reader>()
            .map_err(RPCError::protocol)?;

        let size = root.target_size()?.word_count + 1;
        let mut builder = capnp::message::Builder::new(
            capnp::message::HeapAllocator::new().first_segment_words(size as u32),
        );
        builder.set_root_canonical(root)?;

        let size = capnp::serialize::compute_serialized_size_in_words(&builder)
            * capnp::private::units::BYTES_PER_WORD;

        let mut out = bytes_writer_callback(size);

        capnp::serialize::write_message(&mut out, &builder).map_err(RPCError::protocol)?;

        out
    } else {
        let size = capnp::serialize::compute_serialized_size_in_words(&builder)
            * capnp::private::units::BYTES_PER_WORD;

        let mut out = bytes_writer_callback(size);

        capnp::serialize::write_message(&mut out, &builder).map_err(RPCError::protocol)?;

        out
    };

    Ok(out)
}
