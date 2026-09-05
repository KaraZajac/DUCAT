use super::*;

aligned_u64_type!(NodeCount);
aligned_u64_type_default_debug_impl!(NodeCount);
aligned_u64_type_default_math_impl!(NodeCount);

impl fmt::Display for NodeCount {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_u64())
    }
}

impl FromStr for NodeCount {
    type Err = <u64 as FromStr>::Err;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(NodeCount(u64::from_str(s)?))
    }
}
