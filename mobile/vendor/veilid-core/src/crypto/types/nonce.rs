use super::*;

impl Nonce {
    /// Produces and incremented Nonce in a big-endian fashion to match the behavior of the HashCoordinate and HashDistance operators
    pub fn incremented(&self) -> Nonce {
        let mut bytes = self.bytes().to_vec();
        for b in bytes.iter_mut().rev() {
            let carry;
            (*b, carry) = b.overflowing_add(1);
            if !carry {
                return Nonce::from(bytes);
            }
        }
        Nonce::from(bytes)
    }
}
