//! The HdrHistogram Node records into (`deps/histogram`, HdrHistogram_c), held in place in a
//! byte buffer the collector owns: `Header` words, then one count word per value bucket.
//!
//! A value is kept to `figures` significant decimal digits: each bucket of the counts
//! stands for a range of equivalent values, which every statistic reports by its lowest,
//! median or highest member exactly as HdrHistogram_c does.

/// A 64-bit word of the buffer.
const WORD: usize = std::mem::size_of::<i64>();

/// The header words, in buffer order.
#[derive(Clone, Copy)]
#[repr(usize)]
enum Header {
    Lowest,
    Highest,
    Figures,
    /// The least nonzero value recorded, `i64::MAX` while there is none.
    MinValue,
    MaxValue,
    TotalCount,
    /// Node's own count of recorded values (`Histogram::count_`).
    Count,
    /// Node's count of values the counts could not hold (`Histogram::exceeds_`).
    Exceeds,
    /// The clock reading of the last `recordDelta`, zero before the first.
    Previous,
}
const HEADER_WORDS: usize = Header::Previous as usize + 1;

/// HdrHistogram_c's largest bucket shift: a unit and sub-bucket magnitude beyond it would
/// shift a value out of 64 bits.
const MAX_MAGNITUDE: u32 = 61;
/// The significant figures Node accepts.
pub(crate) const FIGURES: std::ops::RangeInclusive<i64> = 1..=5;
const DECIMAL: i64 = 10;
/// The percentile each percentile iteration ends with.
const FULL_PERCENTILE: f64 = 100.0;

/// The bucket layout `hdr_calculate_bucket_config` derives from the bounds and precision.
#[derive(Clone, Copy)]
struct Layout {
    unit_magnitude: u32,
    sub_bucket_half_count_magnitude: u32,
    sub_bucket_count: i64,
    sub_bucket_half_count: i64,
    sub_bucket_mask: i64,
    counts_len: usize,
}

impl Layout {
    fn new(lowest: i64, highest: i64, figures: i64) -> Option<Self> {
        if lowest < 1 || !FIGURES.contains(&figures) || lowest.checked_mul(2)? > highest {
            return None;
        }
        let single_unit_resolution = 2 * DECIMAL.pow(figures as u32);
        let sub_bucket_count_magnitude = ((single_unit_resolution as f64).ln() / 2f64.ln()).ceil() as u32;
        let sub_bucket_half_count_magnitude = sub_bucket_count_magnitude.max(1) - 1;
        let unit_magnitude = ((lowest as f64).ln() / 2f64.ln()) as u32;
        if unit_magnitude + sub_bucket_half_count_magnitude > MAX_MAGNITUDE {
            return None;
        }
        let sub_bucket_count = 1i64 << (sub_bucket_half_count_magnitude + 1);
        let sub_bucket_half_count = sub_bucket_count / 2;
        let sub_bucket_mask = (sub_bucket_count - 1) << unit_magnitude;
        let mut smallest_untrackable = sub_bucket_count << unit_magnitude;
        let mut bucket_count = 1usize;
        while smallest_untrackable <= highest {
            if smallest_untrackable > i64::MAX / 2 {
                bucket_count += 1;
                break;
            }
            smallest_untrackable <<= 1;
            bucket_count += 1;
        }
        let counts_len = (bucket_count + 1) * sub_bucket_half_count as usize;
        Some(Self { unit_magnitude, sub_bucket_half_count_magnitude, sub_bucket_count, sub_bucket_half_count, sub_bucket_mask, counts_len })
    }

    fn bucket_index(&self, value: i64) -> u32 {
        let pow2_ceiling = u64::BITS - ((value | self.sub_bucket_mask) as u64).leading_zeros();
        pow2_ceiling - self.unit_magnitude - (self.sub_bucket_half_count_magnitude + 1)
    }

    fn sub_bucket_index(&self, value: i64, bucket: u32) -> i64 {
        value >> (bucket + self.unit_magnitude)
    }

    fn counts_index(&self, value: i64) -> i64 {
        let bucket = self.bucket_index(value);
        let sub_bucket = self.sub_bucket_index(value, bucket);
        ((i64::from(bucket) + 1) << self.sub_bucket_half_count_magnitude) + sub_bucket - self.sub_bucket_half_count
    }

    fn value_at_index(&self, index: usize) -> i64 {
        let index = index as i64;
        let mut bucket = (index >> self.sub_bucket_half_count_magnitude) - 1;
        let mut sub_bucket = (index & (self.sub_bucket_half_count - 1)) + self.sub_bucket_half_count;
        if bucket < 0 {
            sub_bucket -= self.sub_bucket_half_count;
            bucket = 0;
        }
        sub_bucket << (bucket as u32 + self.unit_magnitude)
    }

    fn equivalent_range(&self, value: i64) -> i64 {
        let bucket = self.bucket_index(value);
        let adjusted = if self.sub_bucket_index(value, bucket) >= self.sub_bucket_count { bucket + 1 } else { bucket };
        1i64 << (self.unit_magnitude + adjusted)
    }

    fn lowest_equivalent(&self, value: i64) -> i64 {
        let bucket = self.bucket_index(value);
        self.sub_bucket_index(value, bucket) << (bucket + self.unit_magnitude)
    }

    fn highest_equivalent(&self, value: i64) -> i64 {
        self.lowest_equivalent(value) + self.equivalent_range(value) - 1
    }

    fn median_equivalent(&self, value: i64) -> i64 {
        self.lowest_equivalent(value) + (self.equivalent_range(value) >> 1)
    }
}

/// The buffer size a histogram of these bounds needs; none for bounds HdrHistogram rejects.
pub(crate) fn buffer_len(lowest: i64, highest: i64, figures: i64) -> Option<usize> {
    Layout::new(lowest, highest, figures).map(|layout| (HEADER_WORDS + layout.counts_len) * WORD)
}

/// A histogram's buffer, read and written in place.
pub(crate) struct Histogram<'a> {
    words: &'a mut [u8],
    layout: Layout,
}

impl<'a> Histogram<'a> {
    /// A zeroed buffer of `buffer_len` bytes, initialized with its bounds.
    pub(crate) fn initialize(words: &'a mut [u8], lowest: i64, highest: i64, figures: i64) -> Self {
        for (field, value) in [(Header::Lowest, lowest), (Header::Highest, highest), (Header::Figures, figures)] {
            write_word(words, field as usize, value);
        }
        let mut histogram = Self::open(words);
        histogram.reset();
        histogram
    }

    pub(crate) fn open(words: &'a mut [u8]) -> Self {
        let [lowest, highest, figures] = [Header::Lowest, Header::Highest, Header::Figures].map(|field| read_word(words, field as usize));
        let layout = Layout::new(lowest, highest, figures).expect("an initialized histogram's bounds");
        Self { words, layout }
    }

    fn get(&self, field: Header) -> i64 {
        read_word(self.words, field as usize)
    }

    fn set(&mut self, field: Header, value: i64) {
        write_word(self.words, field as usize, value);
    }

    fn count_at(&self, index: usize) -> i64 {
        read_word(self.words, HEADER_WORDS + index)
    }

    /// The buckets holding a count, with the value each starts at.
    fn recorded(&self) -> impl Iterator<Item = (i64, i64)> + '_ {
        (0..self.layout.counts_len)
            .map(|index| (self.layout.value_at_index(index), self.count_at(index)))
            .filter(|&(_, count)| count != 0)
    }

    /// `hdr_record_values`: whether the counts hold `value`.
    fn record_values(&mut self, value: i64, count: i64) -> bool {
        if value < 0 {
            return false;
        }
        let index = self.layout.counts_index(value);
        if index < 0 || index as usize >= self.layout.counts_len {
            return false;
        }
        let slot = HEADER_WORDS + index as usize;
        write_word(self.words, slot, read_word(self.words, slot) + count);
        self.set(Header::TotalCount, self.get(Header::TotalCount) + count);
        if value != 0 && value < self.get(Header::MinValue) {
            self.set(Header::MinValue, value);
        }
        if value > self.get(Header::MaxValue) {
            self.set(Header::MaxValue, value);
        }
        true
    }

    /// `Histogram::Record`: `value` counted, or an exceeding value counted as such.
    pub(crate) fn record(&mut self, value: i64) {
        let field = if self.record_values(value, 1) { Header::Count } else { Header::Exceeds };
        self.set(field, self.get(field) + 1);
    }

    /// `Histogram::RecordDelta`: the time since the previous call, from the second call on.
    pub(crate) fn record_delta(&mut self, now_ns: i64) {
        let previous = self.get(Header::Previous);
        if previous > 0 {
            self.record(now_ns - previous);
        }
        self.set(Header::Previous, now_ns);
    }

    /// `Histogram::Add`: `other`'s counts, recorded and exceeding, added to this one's.
    pub(crate) fn add(&mut self, other: &Histogram<'_>) {
        for field in [Header::Count, Header::Exceeds] {
            self.set(field, self.get(field) + other.get(field));
        }
        self.set(Header::Previous, self.get(Header::Previous).max(other.get(Header::Previous)));
        for (value, count) in other.recorded() {
            self.record_values(value, count);
        }
    }

    pub(crate) fn reset(&mut self) {
        self.words[HEADER_WORDS * WORD..].fill(0);
        for (field, value) in [(Header::MinValue, i64::MAX), (Header::MaxValue, 0), (Header::TotalCount, 0), (Header::Count, 0), (Header::Exceeds, 0), (Header::Previous, 0)] {
            self.set(field, value);
        }
    }

    pub(crate) fn count(&self) -> i64 {
        self.get(Header::Count)
    }

    pub(crate) fn exceeds(&self) -> i64 {
        self.get(Header::Exceeds)
    }

    /// `hdr_min`: zero when zero was recorded, `i64::MAX` when nothing was.
    pub(crate) fn min(&self) -> i64 {
        if self.count_at(0) > 0 {
            return 0;
        }
        match self.get(Header::MinValue) {
            i64::MAX => i64::MAX,
            min => self.layout.lowest_equivalent(min),
        }
    }

    /// `hdr_max`.
    pub(crate) fn max(&self) -> i64 {
        match self.get(Header::MaxValue) {
            0 => 0,
            max => self.layout.highest_equivalent(max),
        }
    }

    /// `hdr_mean`: NaN when nothing was recorded.
    pub(crate) fn mean(&self) -> f64 {
        let total = self
            .recorded()
            .fold(0i64, |total, (value, count)| total.wrapping_add(count.wrapping_mul(self.layout.median_equivalent(value))));
        total as f64 / self.get(Header::TotalCount) as f64
    }

    /// `hdr_stddev`.
    pub(crate) fn stddev(&self) -> f64 {
        let mean = self.mean();
        let deviation = self.recorded().fold(0.0, |total, (value, count)| {
            let deviation = self.layout.median_equivalent(value) as f64 - mean;
            total + deviation * deviation * count as f64
        });
        (deviation / self.get(Header::TotalCount) as f64).sqrt()
    }

    /// `hdr_value_at_percentile`, for `percentile` in (0, 100].
    pub(crate) fn percentile(&self, percentile: f64) -> i64 {
        let requested = percentile.min(FULL_PERCENTILE);
        let target = (requested / FULL_PERCENTILE * self.get(Header::TotalCount) as f64).round() as i64;
        let target = target.max(1);
        let mut seen = 0;
        for index in 0..self.layout.counts_len {
            seen += self.count_at(index);
            if seen >= target {
                return self.layout.highest_equivalent(self.layout.value_at_index(index));
            }
        }
        0
    }

    /// The (percentile, value) pairs HdrHistogram_c's percentile iteration reports at one
    /// tick per half distance, as Node's `percentiles` map holds them.
    pub(crate) fn percentiles(&self) -> Vec<(f64, i64)> {
        let total = self.get(Header::TotalCount);
        let mut reported = Vec::new();
        let mut cumulative = 0;
        let mut target = 0.0;
        let mut value = 0;
        for index in 0..self.layout.counts_len {
            if cumulative >= total {
                break;
            }
            let count = self.count_at(index);
            value = self.layout.value_at_index(index);
            cumulative += count;
            // The bucket completing the count reports once; the final 100 follows it.
            while count != 0 && target <= FULL_PERCENTILE * cumulative as f64 / total as f64 {
                reported.push((target, value));
                let half_distance = 2f64.powi(((FULL_PERCENTILE / (FULL_PERCENTILE - target)).ln() / 2f64.ln()) as i32 + 1);
                target += FULL_PERCENTILE / half_distance;
                if cumulative >= total {
                    break;
                }
            }
        }
        reported.push((FULL_PERCENTILE, value));
        reported
    }
}

fn read_word(words: &[u8], index: usize) -> i64 {
    let start = index * WORD;
    i64::from_ne_bytes(words[start..start + WORD].try_into().expect("a word"))
}

fn write_word(words: &mut [u8], index: usize, value: i64) {
    let start = index * WORD;
    words[start..start + WORD].copy_from_slice(&value.to_ne_bytes());
}

#[cfg(test)]
mod tests {
    use super::{buffer_len, Histogram};

    #[test]
    fn statistics_follow_hdr_histogram() {
        let (lowest, highest, figures) = (1, (1 << 53) - 1, 3);
        let mut bytes = vec![0; buffer_len(lowest, highest, figures).expect("valid bounds")];
        let mut histogram = Histogram::initialize(&mut bytes, lowest, highest, figures);
        for value in [1, 2, 3, 100, 1000, 2047, 2048, 2049, 4095, 4096, 12345, 123456, 1234567, 98765432, (1 << 40) + 12345, 5] {
            histogram.record(value);
        }
        assert_eq!(histogram.max(), 1100585369599);
        assert_eq!(histogram.percentile(50.0), 2049);
        assert_eq!(histogram.mean(), 68759292495.3125);
    }
}
