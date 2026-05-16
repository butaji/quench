//! Fast Math
//!
//! Hand-written Rust math functions for performance-critical operations.

/// Fast square root using native Rust.
pub fn fast_sqrt(x: f64) -> f64 {
    x.sqrt()
}

/// Batch add operation.
pub fn batch_add(values: &[f64], n: f64) -> Vec<f64> {
    values.iter().map(|v| v + n).collect()
}

/// Calculate mean of values.
pub fn mean(values: &[f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    values.iter().sum::<f64>() / values.len() as f64
}

/// Calculate variance of values.
pub fn variance(values: &[f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let m = mean(values);
    values.iter().map(|v| (v - m).powi(2)).sum::<f64>() / values.len() as f64
}

/// Standard deviation.
pub fn std_dev(values: &[f64]) -> f64 {
    variance(values).sqrt()
}
