//! The block scans of an ASCII subject for the units in a few byte ranges (`StartRanges`).
use super::{SCAN_BLOCK, START_SCAN_RANGES, StartRanges};

/// The longest subject `class_mask` answers for: one bit of a `u64` each.
const CLASS_MASK_UNITS: usize = u64::BITS as usize;

/// The broadcast bounds of a `StartRanges`, made once per scan rather than per block.
#[cfg(target_arch = "x86_64")]
struct BlockBounds {
    pairs: [(std::arch::x86_64::__m128i, std::arch::x86_64::__m128i); START_SCAN_RANGES],
    count: usize,
}

impl StartRanges {
    #[cfg(target_arch = "x86_64")]
    #[inline(always)]
    fn bounds(self) -> BlockBounds {
        use std::arch::x86_64::_mm_set1_epi8;
        // SAFETY: SSE2 is part of the x86-64 baseline.
        let pairs = std::array::from_fn(|range| unsafe {
            (_mm_set1_epi8(self.low[range] as i8), _mm_set1_epi8(self.high[range] as i8))
        });
        BlockBounds { pairs, count: self.count }
    }

    /// The units of the block of `haystack` at `offset` that are in a range, one bit each.
    #[cfg(target_arch = "x86_64")]
    #[inline(always)]
    fn block_hits(bounds: &BlockBounds, haystack: &[u8], offset: usize) -> u32 {
        use std::arch::x86_64::{
            _mm_cmpeq_epi8, _mm_loadu_si128, _mm_max_epu8, _mm_min_epu8, _mm_movemask_epi8, _mm_or_si128,
            _mm_setzero_si128,
        };
        assert!(offset + SCAN_BLOCK <= haystack.len());
        // SAFETY: SSE2 is part of the x86-64 baseline, and the load reads a whole block
        // inside `haystack`.
        unsafe {
            let block = _mm_loadu_si128(haystack.as_ptr().add(offset).cast());
            let mut hits = _mm_setzero_si128();
            for (low, high) in &bounds.pairs[..bounds.count] {
                // A unit is in `low..=high` exactly when clamping it there leaves it.
                let clamped = _mm_min_epu8(_mm_max_epu8(block, *low), *high);
                hits = _mm_or_si128(hits, _mm_cmpeq_epi8(clamped, block));
            }
            _mm_movemask_epi8(hits) as u32
        }
    }

    /// The offset of the first unit in a range of a `haystack` of at least one block, found
    /// a block at a time; the last block overlaps the units already scanned, none of which hit.
    #[cfg(target_arch = "x86_64")]
    pub(super) fn find_blocks(self, haystack: &[u8]) -> Option<usize> {
        let bounds = self.bounds();
        let mut offset = 0;
        while offset + SCAN_BLOCK <= haystack.len() {
            let mask = Self::block_hits(&bounds, haystack, offset);
            if mask != 0 {
                return Some(offset + mask.trailing_zeros() as usize);
            }
            offset += SCAN_BLOCK;
        }
        let last = haystack.len() - SCAN_BLOCK;
        let mask = Self::block_hits(&bounds, haystack, last);
        (mask != 0).then(|| last + mask.trailing_zeros() as usize)
    }

    /// Which units of a `haystack` of one to four blocks are in a range, one bit each; `None`
    /// where the scan cannot answer (another size or architecture).
    pub(super) fn class_mask(self, haystack: &[u8]) -> Option<u64> {
        #[cfg(target_arch = "x86_64")]
        {
            let length = haystack.len();
            if !(SCAN_BLOCK..=CLASS_MASK_UNITS).contains(&length) {
                return None;
            }
            let bounds = self.bounds();
            let mut mask = 0u64;
            let mut offset = 0;
            while offset + SCAN_BLOCK <= length {
                mask |= u64::from(Self::block_hits(&bounds, haystack, offset)) << offset;
                offset += SCAN_BLOCK;
            }
            // The last block overlaps units already tested; their bits agree.
            let last = length - SCAN_BLOCK;
            return Some(mask | u64::from(Self::block_hits(&bounds, haystack, last)) << last);
        }
        #[cfg(not(target_arch = "x86_64"))]
        {
            let _ = haystack;
            None
        }
    }
}
