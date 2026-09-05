use super::*;

use chacha20poly1305 as ch;

/// A growable, mutable byte buffer that AEAD operations can encrypt or decrypt in place.
///
/// Implemented for `Vec<u8>` and `BytesMut`. Encryption appends the authentication tag
/// (growing the buffer); decryption truncates it back off.
pub trait CryptoSystemBuffer: AsRef<[u8]> + AsMut<[u8]> {
    /// Get the length of the buffer
    fn len(&self) -> usize;

    /// Is the buffer empty?
    fn is_empty(&self) -> bool;

    /// Extend this buffer from the given slice
    fn extend_from_slice(&mut self, other: &[u8]);

    /// Truncate this buffer to the given size
    fn truncate(&mut self, len: usize);
}

impl CryptoSystemBuffer for Vec<u8> {
    fn len(&self) -> usize {
        Vec::<u8>::len(self)
    }

    fn is_empty(&self) -> bool {
        Vec::<u8>::is_empty(self)
    }

    fn extend_from_slice(&mut self, other: &[u8]) {
        Vec::<u8>::extend_from_slice(self, other);
    }

    fn truncate(&mut self, len: usize) {
        Vec::<u8>::truncate(self, len);
    }
}

impl CryptoSystemBuffer for BytesMut {
    fn len(&self) -> usize {
        BytesMut::len(self)
    }

    fn is_empty(&self) -> bool {
        BytesMut::is_empty(self)
    }

    fn extend_from_slice(&mut self, other: &[u8]) {
        BytesMut::extend_from_slice(self, other);
    }

    fn truncate(&mut self, len: usize) {
        BytesMut::truncate(self, len);
    }
}

/// Adapts a [`CryptoSystemBuffer`] to the `chacha20poly1305` AEAD `Buffer` trait so in-place AEAD
/// operations can grow and truncate the underlying `Vec<u8>` or `BytesMut`.
pub(super) struct BufferWrapper<'a> {
    buffer: &'a mut dyn CryptoSystemBuffer,
}

impl<'a> BufferWrapper<'a> {
    /// Wrap a mutable [`CryptoSystemBuffer`] for use as an AEAD buffer.
    pub fn new(buffer: &'a mut dyn CryptoSystemBuffer) -> Self {
        Self { buffer }
    }
}
impl<'a> AsRef<[u8]> for BufferWrapper<'a> {
    fn as_ref(&self) -> &[u8] {
        self.buffer.as_ref()
    }
}
impl<'a> AsMut<[u8]> for BufferWrapper<'a> {
    fn as_mut(&mut self) -> &mut [u8] {
        self.buffer.as_mut()
    }
}
impl<'a> ch::aead::Buffer for BufferWrapper<'a> {
    fn len(&self) -> usize {
        self.buffer.len()
    }
    fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }
    fn extend_from_slice(&mut self, other: &[u8]) -> Result<(), ch::Error> {
        self.buffer.extend_from_slice(other);
        Ok(())
    }
    fn truncate(&mut self, len: usize) {
        self.buffer.truncate(len)
    }
}
