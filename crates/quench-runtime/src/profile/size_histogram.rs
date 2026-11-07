//! A power-of-two histogram of sizes, shared by every profile that buckets bytes.

/// Buckets of a size histogram: empty sizes, sizes below the first power of two, one bucket
/// per doubling from it, and the rest.
pub(crate) const SIZE_BUCKETS: usize = 8;
/// The first doubling bucket starts at `1 << SIZE_BUCKET_FIRST_POWER` bytes.
const SIZE_BUCKET_FIRST_POWER: u32 = 3;

/// The histogram bucket of a size, as [`SIZE_BUCKETS`] lays them out.
#[inline(always)]
pub(crate) fn size_bucket(bytes: usize) -> usize {
    if bytes == 0 {
        return 0;
    }
    let doublings = (bytes.ilog2() + 1).saturating_sub(SIZE_BUCKET_FIRST_POWER) as usize;
    (1 + doublings).min(SIZE_BUCKETS - 1)
}
