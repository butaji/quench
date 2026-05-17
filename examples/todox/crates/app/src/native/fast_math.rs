//! Fast math utilities written in Rust.

/// Fast square root approximation.
#[inline]
#[allow(dead_code)]
pub fn fast_sqrt(x: f64) -> f64 {
    x.sqrt()
}

/// Fast sine approximation.
#[inline]
#[allow(dead_code)]
pub fn fast_sin(x: f64) -> f64 {
    x.sin()
}

/// Fast cosine approximation.
#[inline]
#[allow(dead_code)]
pub fn fast_cos(x: f64) -> f64 {
    x.cos()
}

/// Batch add numbers.
#[inline]
#[allow(dead_code)]
pub fn batch_add(numbers: &[f64]) -> f64 {
    numbers.iter().sum()
}
