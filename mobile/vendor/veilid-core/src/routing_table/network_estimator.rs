//! Network size estimator: circular histogram of per-bucket high-water counts.

use super::*;

const HISTOGRAM_SLOT_MINUTES: u32 = 60;
const HISTOGRAM_WINDOW_HOURS: u32 = 24;
const HISTOGRAM_SLOT_COUNT: usize =
    ((HISTOGRAM_WINDOW_HOURS * 60) / HISTOGRAM_SLOT_MINUTES) as usize;
const HISTOGRAM_SLOT_DURATION: TimestampDuration =
    TimestampDuration::new_secs(HISTOGRAM_SLOT_MINUTES * 60);

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(in crate::routing_table) struct HistogramSlot {
    pub bucket_counts: BTreeMap<CryptoKind, Vec<u16>>,
    pub pairwise_intersection_counts: BTreeMap<(CryptoKind, CryptoKind), u32>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(in crate::routing_table) struct NetworkEstimator {
    histogram: Vec<HistogramSlot>,
    cur_slot: Option<u64>,
}

impl Default for NetworkEstimator {
    fn default() -> Self {
        Self::new()
    }
}

impl NetworkEstimator {
    pub fn new() -> Self {
        Self {
            histogram: (0..HISTOGRAM_SLOT_COUNT)
                .map(|_| HistogramSlot::default())
                .collect(),
            cur_slot: None,
        }
    }

    /// Fold an observation at `ts` into the histogram. Slots are only zeroed
    /// when we ENTER them with fresh data; gap-period slots keep their old
    /// values until we naturally cycle back to them.
    pub fn record_observation(
        &mut self,
        ts: Timestamp,
        bucket_counts: &BTreeMap<CryptoKind, Vec<usize>>,
        pairwise_intersection_counts: &BTreeMap<(CryptoKind, CryptoKind), usize>,
    ) {
        let slot = ts.as_u64() / HISTOGRAM_SLOT_DURATION.as_u64();
        let idx = (slot as usize) % HISTOGRAM_SLOT_COUNT;

        let zero_current = match self.cur_slot {
            None => true,
            Some(cur) if slot > cur => true,
            Some(cur) if slot < cur => return,
            _ => false,
        };

        if zero_current {
            self.histogram[idx] = HistogramSlot::default();
            self.cur_slot = Some(slot);
        }

        let target = &mut self.histogram[idx];

        for (ck, counts) in bucket_counts {
            let entry = target
                .bucket_counts
                .entry(*ck)
                .or_insert_with(|| vec![0u16; BUCKET_COUNT]);
            for (k, &c) in counts.iter().enumerate().take(BUCKET_COUNT) {
                let c_u16 = u16::try_from(c).unwrap_or(u16::MAX);
                if c_u16 > entry[k] {
                    entry[k] = c_u16;
                }
            }
        }

        for (pair, &c) in pairwise_intersection_counts {
            let c_u32 = u32::try_from(c).unwrap_or(u32::MAX);
            let slot_entry = target
                .pairwise_intersection_counts
                .entry(*pair)
                .or_insert(0);
            if c_u32 > *slot_entry {
                *slot_entry = c_u32;
            }
        }
    }

    fn per_bucket_high_water(&self, ck: CryptoKind) -> Vec<u16> {
        let mut hi = vec![0u16; BUCKET_COUNT];
        for slot in &self.histogram {
            if let Some(counts) = slot.bucket_counts.get(&ck) {
                for (k, &c) in counts.iter().enumerate().take(BUCKET_COUNT) {
                    if c > hi[k] {
                        hi[k] = c;
                    }
                }
            }
        }
        hi
    }

    fn pair_high_water(&self, pair: (CryptoKind, CryptoKind)) -> u32 {
        let mut hi: u32 = 0;
        for slot in &self.histogram {
            if let Some(&c) = slot.pairwise_intersection_counts.get(&pair) {
                if c > hi {
                    hi = c;
                }
            }
        }
        hi
    }

    /// Per-kind estimate from the lowest unsaturated bucket: `N̂ = count * 2^(k+1)`.
    pub fn estimate(&self, ck: CryptoKind) -> u64 {
        let hi = self.per_bucket_high_water(ck);
        for (k, &count) in hi.iter().enumerate() {
            let depth = RoutingTableInner::bucket_depth(k) as u16;
            if count < depth {
                if k + 1 >= 64 {
                    return u64::MAX;
                }
                return u64::from(count).saturating_mul(1u64 << (k + 1));
            }
        }
        u64::MAX
    }

    /// Combined estimate across crypto kinds via inclusion-exclusion.
    pub fn estimate_combined(&self) -> u64 {
        let kinds = VALID_CRYPTO_KINDS;
        if kinds.is_empty() {
            return 0;
        }
        if kinds.len() == 1 {
            return self.estimate(kinds[0]);
        }

        let mut sum: u64 = 0;
        for ck in kinds {
            sum = sum.saturating_add(self.estimate(ck));
        }

        for i in 0..kinds.len() {
            for j in (i + 1)..kinds.len() {
                let k1 = kinds[i];
                let k2 = kinds[j];
                let (lo, hi) = if k1 < k2 { (k1, k2) } else { (k2, k1) };
                let est_lo = self.estimate(lo);
                let est_hi = self.estimate(hi);

                let lo_count_total: u32 = self
                    .per_bucket_high_water(lo)
                    .iter()
                    .map(|&c| u32::from(c))
                    .sum();
                if lo_count_total == 0 {
                    continue;
                }
                let pair_hi = self.pair_high_water((lo, hi));
                let ratio = f64::from(pair_hi) / f64::from(lo_count_total);
                let intersection_est = ((ratio * est_lo as f64) as u64).min(est_lo.min(est_hi));
                sum = sum.saturating_sub(intersection_est);
            }
        }
        sum
    }
}
