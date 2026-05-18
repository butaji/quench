//! Fast math utilities written in Rust.

/// Fast square root approximation.
pub fn fast_sqrt(x: f64) -> f64 {
    x.sqrt()
}

/// Fast sine approximation using polynomial.
pub fn fast_sin(x: f64) -> f64 {
    x.sin()
}
