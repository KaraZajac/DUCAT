use super::*;

pub const HASH_COORDINATE_LENGTH: usize = 32;

// Internal types

pub(crate) trait ToHashCoordinate {
    fn to_hash_coordinate(&self) -> HashCoordinate;
}

pub(crate) trait ToBareHashCoordinate {
    fn to_bare_hash_coordinate(&self) -> BareHashCoordinate;
}

impl ToHashCoordinate for NodeId {
    fn to_hash_coordinate(&self) -> HashCoordinate {
        HashCoordinate::new(self.kind(), self.ref_value().to_bare_hash_coordinate())
    }
}
impl ToBareHashCoordinate for BareNodeId {
    fn to_bare_hash_coordinate(&self) -> BareHashCoordinate {
        BareHashCoordinate::new(self)
    }
}

impl ToHashCoordinate for OpaqueRecordKey {
    fn to_hash_coordinate(&self) -> HashCoordinate {
        HashCoordinate::new(self.kind(), self.ref_value().to_bare_hash_coordinate())
    }
}
impl ToBareHashCoordinate for BareOpaqueRecordKey {
    fn to_bare_hash_coordinate(&self) -> BareHashCoordinate {
        BareHashCoordinate::new(self)
    }
}

impl ToHashCoordinate for RecordKey {
    fn to_hash_coordinate(&self) -> HashCoordinate {
        HashCoordinate::new(self.kind(), self.ref_value().to_bare_hash_coordinate())
    }
}
impl ToBareHashCoordinate for BareRecordKey {
    fn to_bare_hash_coordinate(&self) -> BareHashCoordinate {
        BareHashCoordinate::new(self.ref_key())
    }
}

impl ToHashCoordinate for HashDigest {
    #[allow(dead_code)]
    fn to_hash_coordinate(&self) -> HashCoordinate {
        HashCoordinate::new(self.kind(), self.ref_value().to_bare_hash_coordinate())
    }
}

impl ToBareHashCoordinate for BareHashDigest {
    #[allow(dead_code)]
    fn to_bare_hash_coordinate(&self) -> BareHashCoordinate {
        BareHashCoordinate::new(self)
    }
}

impl HashCoordinate {
    pub fn distance(&self, other: &HashCoordinate) -> HashDistance {
        debug_assert_eq!(self.kind(), other.kind());
        self.ref_value().distance(other.ref_value())
    }

    pub fn offset(&self, distance: &HashDistance) -> Self {
        HashCoordinate::new(self.kind(), self.ref_value().offset(distance))
    }
}

impl core::ops::Add for HashDistance {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        let mut out = BytesMut::zeroed(HASH_COORDINATE_LENGTH);

        let mut carry = false;
        for n in (0..HASH_COORDINATE_LENGTH).rev() {
            let mut sum = self.bytes()[n];
            if carry {
                (sum, carry) = sum.overflowing_add(1);
            }
            let next_carry;
            (out[n], next_carry) = sum.overflowing_add(other.bytes()[n]);
            carry = carry || next_carry;
        }

        // Allow final carry to wrap

        HashDistance::new_from_bytes(out.freeze())
    }
}

impl core::ops::AddAssign for HashDistance {
    fn add_assign(&mut self, other: Self) {
        *self = self.clone() + other;
    }
}

impl core::ops::Sub for HashDistance {
    type Output = Self;

    fn sub(self, other: Self) -> Self {
        let mut out = BytesMut::zeroed(HASH_COORDINATE_LENGTH);

        let mut borrow = false;
        for n in (0..HASH_COORDINATE_LENGTH).rev() {
            let mut diff = self.bytes()[n];
            if borrow {
                (diff, borrow) = diff.overflowing_sub(1);
            }
            let next_borrow;
            (out[n], next_borrow) = diff.overflowing_sub(other.bytes()[n]);
            borrow = borrow || next_borrow;
        }

        // Allow final borrow to wrap

        HashDistance::new_from_bytes(out.freeze())
    }
}

impl core::ops::SubAssign for HashDistance {
    fn sub_assign(&mut self, other: Self) {
        *self = self.clone() - other;
    }
}

impl BareHashCoordinate {
    pub fn distance(&self, other: &BareHashCoordinate) -> HashDistance {
        debug_assert_eq!(self.len(), HASH_COORDINATE_LENGTH);
        debug_assert_eq!(other.len(), HASH_COORDINATE_LENGTH);

        let mut bytes = BytesMut::zeroed(HASH_COORDINATE_LENGTH);

        (0..HASH_COORDINATE_LENGTH).for_each(|n| {
            bytes[n] = self[n] ^ other[n];
        });

        HashDistance::new_from_bytes(bytes.freeze())
    }

    pub fn offset(&self, distance: &HashDistance) -> Self {
        debug_assert_eq!(self.len(), HASH_COORDINATE_LENGTH);
        debug_assert_eq!(distance.len(), HASH_COORDINATE_LENGTH);

        let mut bytes = BytesMut::zeroed(HASH_COORDINATE_LENGTH);

        (0..HASH_COORDINATE_LENGTH).for_each(|n| {
            bytes[n] = self[n] ^ distance[n];
        });

        Self::new_from_bytes(bytes.freeze())
    }
}
