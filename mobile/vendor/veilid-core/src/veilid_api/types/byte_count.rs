use super::*;

aligned_u64_type!(ByteCount);
aligned_u64_type_default_debug_impl!(ByteCount);
aligned_u64_type_default_math_impl!(ByteCount);

impl fmt::Display for ByteCount {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if f.alternate() {
            write!(f, "{}", human_byte_count(self.as_u64()))
        } else {
            write!(f, "{}", self.as_u64())
        }
    }
}

impl FromStr for ByteCount {
    type Err = <u64 as FromStr>::Err;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(ByteCount(u64::from_str(s)?))
    }
}
