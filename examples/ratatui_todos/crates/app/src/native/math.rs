//! Native math functions - hand-written Rust coexisting with .r.ts files.
//!
//! These functions can be imported in Rune via:
//! `import { fastSqrt, fibonacci } from "native:math";`

/// Fast square root using bit manip for common case.
pub fn fast_sqrt(value: f64) -> f64 {
    if value <= 0.0 {
        return 0.0;
    }
    // Newton-Raphson with initial guess from bit manipulation
    let bits = value.to_bits();
    let guess_bits = 0x5fe6eb50c7b537a9_u64 - (bits >> 1);
    let mut guess = f64::from_bits(guess_bits);
    
    for _ in 0..3 {
        guess = guess * (1.5 - value * 0.5 * guess * guess);
    }
    guess
}

/// Fibonacci number - iterative for speed.
pub fn fibonacci(n: u32) -> u64 {
    if n == 0 {
        return 0;
    }
    if n == 1 {
        return 1;
    }
    let mut a: u64 = 0;
    let mut b: u64 = 1;
    for _ in 2..=n {
        let temp = a + b;
        a = b;
        b = temp;
    }
    b
}

/// Clamp a value between min and max.
pub fn clamp(value: f64, min: f64, max: f64) -> f64 {
    if value < min {
        min
    } else if value > max {
        max
    } else {
        value
    }
}

/// Linear interpolation between two values.
pub fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * clamp(t, 0.0, 1.0)
}

/// Degrees to radians conversion.
pub fn to_radians(degrees: f64) -> f64 {
    degrees * std::f64::consts::PI / 180.0
}

/// Check if a number is even.
pub fn is_even(n: i32) -> bool {
    n % 2 == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fibonacci() {
        assert_eq!(fibonacci(0), 0);
        assert_eq!(fibonacci(1), 1);
        assert_eq!(fibonacci(10), 55);
        assert_eq!(fibonacci(20), 6765);
    }

    #[test]
    fn test_clamp() {
        assert_eq!(clamp(5.0, 0.0, 10.0), 5.0);
        assert_eq!(clamp(-5.0, 0.0, 10.0), 0.0);
        assert_eq!(clamp(15.0, 0.0, 10.0), 10.0);
    }

    #[test]
    fn test_is_even() {
        assert!(is_even(2));
        assert!(is_even(0));
        assert!(!is_even(3));
    }
}
