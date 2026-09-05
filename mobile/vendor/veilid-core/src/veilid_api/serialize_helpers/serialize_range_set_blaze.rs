use core::fmt;
use core::marker::PhantomData;
use core::ops::RangeInclusive;
use range_set_blaze::*;
use serde::{
    de::SeqAccess, de::Visitor, ser::SerializeSeq, Deserialize, Deserializer, Serialize, Serializer,
};

/// Serializes the set as a sequence of inclusive `(start, end)` range pairs.
pub fn serialize<T: Integer + Serialize, S: Serializer>(
    v: &RangeSetBlaze<T>,
    s: S,
) -> Result<S::Ok, S::Error> {
    let cnt = v.ranges_len();
    let mut seq = s.serialize_seq(Some(cnt))?;
    for range in v.ranges() {
        seq.serialize_element(&(range.start(), range.end()))?;
    }
    seq.end()
}

/// Deserializes a sequence of inclusive `(start, end)` range pairs back into the set.
pub fn deserialize<'de, T: Integer + Deserialize<'de>, D: Deserializer<'de>>(
    d: D,
) -> Result<RangeSetBlaze<T>, D::Error> {
    struct RangeSetBlazeVisitor<T> {
        marker: PhantomData<T>,
    }

    impl<'de, T> Visitor<'de> for RangeSetBlazeVisitor<T>
    where
        T: Deserialize<'de> + Integer,
    {
        type Value = RangeSetBlaze<T>;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a RangeSetBlaze")
        }

        fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
        where
            A: SeqAccess<'de>,
        {
            let mut values = RangeSetBlaze::<T>::new();

            while let Some((start, end)) = seq.next_element::<(T, T)>()? {
                values.ranges_insert(RangeInclusive::new(start, end));
            }

            Ok(values)
        }
    }

    let visitor = RangeSetBlazeVisitor {
        marker: PhantomData,
    };

    d.deserialize_seq(visitor)
}
