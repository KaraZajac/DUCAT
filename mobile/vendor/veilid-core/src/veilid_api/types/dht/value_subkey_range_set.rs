use super::*;
use core::ops::{Deref, DerefMut};
use range_set_blaze::*;

/// A set of DHT subkeys stored as inclusive ranges of [ValueSubkey]
#[derive(Clone, Default, Hash, PartialOrd, PartialEq, Eq, Ord, Serialize, Deserialize, GetSize)]
#[cfg_attr(feature = "schemars", derive(JsonSchema))]
#[cfg_attr(
    all(target_arch = "wasm32", target_os = "unknown"),
    derive(Tsify),
    tsify(from_wasm_abi, into_wasm_abi)
)]
#[serde(transparent)]
#[must_use]
pub struct ValueSubkeyRangeSet {
    #[serde(with = "serialize_range_set_blaze")]
    #[cfg_attr(feature = "schemars", schemars(with = "Vec<(u32,u32)>"))]
    #[cfg_attr(
        all(target_arch = "wasm32", target_os = "unknown"),
        tsify(type = "Array<[ValueSubkey, ValueSubkey]>")
    )]
    #[get_size(size_fn = range_set_blaze_size_helper)]
    data: RangeSetBlaze<ValueSubkey>,
}

impl ValueSubkeyRangeSet {
    /// An empty set
    pub fn new() -> Self {
        Self {
            data: Default::default(),
        }
    }
    /// A set covering every subkey from `u32::MIN` to `u32::MAX` inclusive
    pub fn full() -> Self {
        let mut data = RangeSetBlaze::new();
        data.ranges_insert(u32::MIN..=u32::MAX);
        Self { data }
    }
    /// A set wrapping an existing range collection
    pub fn new_with_data(data: RangeSetBlaze<ValueSubkey>) -> Self {
        Self { data }
    }
    /// A set containing one subkey
    pub fn single(value: ValueSubkey) -> Self {
        let mut data = RangeSetBlaze::new();
        data.insert(value);
        Self { data }
    }
    /// A set containing the inclusive range `low..=high`
    pub fn single_range(low: ValueSubkey, high: ValueSubkey) -> Self {
        let mut data = RangeSetBlaze::new();
        data.ranges_insert(low..=high);
        Self { data }
    }

    /// The subkeys present in both this set and `other`
    pub fn intersect(&self, other: &ValueSubkeyRangeSet) -> ValueSubkeyRangeSet {
        Self::new_with_data(&self.data & &other.data)
    }
    /// The subkeys in this set that are not in `other`
    pub fn difference(&self, other: &ValueSubkeyRangeSet) -> ValueSubkeyRangeSet {
        Self::new_with_data(&self.data - &other.data)
    }
    /// The subkeys present in either this set or `other`
    pub fn union(&self, other: &ValueSubkeyRangeSet) -> ValueSubkeyRangeSet {
        Self::new_with_data(&self.data | &other.data)
    }

    /// The count of subkeys in the set
    #[must_use]
    #[allow(clippy::unnecessary_cast)]
    pub fn len(&self) -> u64 {
        self.data.len() as u64
    }

    /// True if the set contains no subkeys
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    /// True if the set covers the entire `u32::MIN..=u32::MAX` range, as produced by [ValueSubkeyRangeSet::full]
    #[must_use]
    pub fn is_full(&self) -> bool {
        self.data.ranges_len() == 1
            && self.data.first().unwrap_or_log() == u32::MIN
            && self.data.last().unwrap_or_log() == u32::MAX
    }

    /// The underlying range collection, by reference
    #[must_use]
    pub fn data(&self) -> &RangeSetBlaze<ValueSubkey> {
        &self.data
    }
    /// Consume the set and return the underlying range collection
    #[must_use]
    pub fn into_data(self) -> RangeSetBlaze<ValueSubkey> {
        self.data
    }

    /// The subkey at position `idx` in ascending order, or `None` if `idx` is out of bounds
    #[must_use]
    pub fn nth_subkey(&self, idx: usize) -> Option<ValueSubkey> {
        let mut idxleft = idx;
        for range in self.data.ranges() {
            let range_len = (*range.end() - *range.start() + 1) as usize;
            if idxleft < range_len {
                return Some(*range.start() + idxleft as u32);
            }
            idxleft -= range_len;
        }
        None
    }

    /// The ascending-order position of `subkey` in the set, or `None` if it is not present
    #[must_use]
    pub fn idx_of_subkey(&self, subkey: ValueSubkey) -> Option<usize> {
        let mut idx = 0usize;
        for range in self.data.ranges() {
            if range.contains(&subkey) {
                idx += (subkey - *range.start()) as usize;
                return Some(idx);
            } else {
                idx += (*range.end() - *range.start() + 1) as usize;
            }
        }
        None
    }
}

impl FromStr for ValueSubkeyRangeSet {
    type Err = VeilidAPIError;

    /// Errors with `VeilidAPIError::ParseError` if any comma-separated element lacks a `..=`
    /// separator or either endpoint is not a valid [ValueSubkey].
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let mut data = RangeSetBlaze::<ValueSubkey>::new();

        for r in value.split(',') {
            let r = r.trim();
            let Some((ss, es)) = r.split_once("..=") else {
                return Err(VeilidAPIError::parse_error(
                    "can not parse ValueSubkeyRangeSet",
                    r,
                ));
            };
            let sn = ValueSubkey::from_str(ss).map_err(|e| {
                VeilidAPIError::parse_error(format!("could not parse ValueSubkey: {e}"), ss)
            })?;
            let en = ValueSubkey::from_str(es).map_err(|e| {
                VeilidAPIError::parse_error(format!("could not parse ValueSubkey: {e}"), es)
            })?;
            data.ranges_insert(sn..=en);
        }

        Ok(ValueSubkeyRangeSet { data })
    }
}

impl FromIterator<ValueSubkey> for ValueSubkeyRangeSet {
    fn from_iter<T: IntoIterator<Item = ValueSubkey>>(iter: T) -> Self {
        let data = RangeSetBlaze::<ValueSubkey>::from_iter(iter);
        Self::new_with_data(data)
    }
}

impl Deref for ValueSubkeyRangeSet {
    type Target = RangeSetBlaze<ValueSubkey>;

    fn deref(&self) -> &Self::Target {
        &self.data
    }
}

impl DerefMut for ValueSubkeyRangeSet {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.data
    }
}

impl fmt::Debug for ValueSubkeyRangeSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.data)
    }
}

impl fmt::Display for ValueSubkeyRangeSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.data)
    }
}

fn range_set_blaze_size_helper<T: range_set_blaze::Integer>(rsb: &RangeSetBlaze<T>) -> usize {
    size_of::<T>() * 2 * rsb.ranges_len()
}
