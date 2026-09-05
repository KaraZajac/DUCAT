use super::*;
use lz4_flex::block;

impl_veilid_log_facility!("compression");

#[cfg_attr(
    feature = "instrument",
    instrument(level = "trace", target = "compression", skip_all)
)]
#[must_use]
pub(crate) fn compress_prepend_size(input: &[u8]) -> Vec<u8> {
    block::compress_prepend_size(input)
}

#[cfg_attr(
    feature = "instrument",
    instrument(level = "trace", target = "compression", skip_all, err)
)]
pub(crate) fn decompress_size_prepended(input: &[u8], max_size: usize) -> VeilidAPIResult<Vec<u8>> {
    let (uncompressed_size, input) = match block::uncompressed_size(input) {
        Ok(v) => v,
        Err(e) => {
            apibail_generic!("failed to get uncompressed size: {}", e);
        }
    };
    if uncompressed_size > max_size {
        apibail_generic!(
            "decompression exceeded maximum size: {} > {}",
            uncompressed_size,
            max_size
        );
    }
    match block::decompress(input, uncompressed_size) {
        Ok(v) => Ok(v),
        Err(e) => {
            #[cfg(feature = "backtrace")]
            apibail_generic!(
                "failed to decompress: {}:data_len={}\ndata={}\nbacktrace:\n{:#?}",
                e,
                input.len(),
                human_byte_data(input, Some(256)),
                backtrace::Backtrace::new()
            );
            #[cfg(not(feature = "backtrace"))]
            apibail_generic!(
                "failed to decompress: {}:data_len={}\ndata={}",
                e,
                input.len(),
                human_byte_data(input, Some(256))
            );
        }
    }
}
