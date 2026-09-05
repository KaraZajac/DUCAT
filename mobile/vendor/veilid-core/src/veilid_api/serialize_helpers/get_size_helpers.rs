use super::*;

pub(crate) fn bytes_size_helper(bytes: &Bytes) -> usize {
    std::mem::size_of::<Bytes>() + bytes.len()
}
