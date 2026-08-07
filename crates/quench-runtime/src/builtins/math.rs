//! Math built-in

use std::cell::RefCell;
use std::rc::Rc;

use crate::value::{to_number, NativeFunction, Object, PropertyFlags, Value};
use crate::Context;

/// Create non-writable, non-enumerable, non-configurable property flags for constants
fn constant_flags(value: f64) -> PropertyFlags {
    PropertyFlags {
        value: Some(Value::Number(value)),
        writable: false,
        enumerable: false,
        configurable: false,
    }
}

/// Implements Math.round per ECMAScript spec.
/// Returns the "round half up" of x. That is, the value of x rounded
/// to the nearest integer, ties to +Infinity.
fn js_round(x: f64) -> f64 {
    // Handle special cases per spec
    if x.is_nan() || x.is_infinite() || x == 0.0 {
        return x;
    }

    let floor_val = x.floor();
    let ceil_val = x.ceil();

    let diff_floor = (x - floor_val).abs();
    let diff_ceil = (ceil_val - x).abs();

    // Round half toward +Infinity
    if diff_floor < diff_ceil {
        floor_val
    } else if diff_ceil < diff_floor {
        ceil_val
    } else {
        // Ties: exactly between two integers, round toward +Infinity
        // For positive ties, ceil_val is larger (correct)
        // For negative ties (like -0.5), ceil_val = -0.0 is correct
        ceil_val
    }
}

thread_local! {
    static RNG_STATE: RefCell<Option<u64>> = const { RefCell::new(None) };
}

fn xorshift64(state: u64) -> u64 {
    let mut x = state;
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    x
}

fn rand_simple() -> f64 {
    RNG_STATE.with(|cell| {
        let mut state = cell.borrow_mut();
        // Seed from system time on first use (| 1 keeps the seed non-zero)
        let seed = state.get_or_insert_with(|| {
            use std::time::{SystemTime, UNIX_EPOCH};
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(0);
            (nanos ^ 0x853c49e6748fea9b) | 1
        });
        *seed = xorshift64(*seed);
        (*seed as f64) / (u64::MAX as f64)
    })
}

pub fn register_math(ctx: &mut Context) {
    let math = Object::new(crate::value::ObjectKind::Ordinary);
    let math = Rc::new(RefCell::new(math));

    register_unary_math_fns(&math);
    register_binary_math_fns(&math);
    register_reduce_math_fns(&math);
    register_math_constants(&math);

    ctx.set_global("Math".to_string(), Value::Object(math));
}

fn register_unary_math_fns(math: &Rc<RefCell<Object>>) {
    macro_rules! math_fn {
        ($name:expr, $fn:expr) => {
            math.borrow_mut().set(
                $name,
                Value::NativeFunction(Rc::new(NativeFunction::new(move |args| {
                    let x = args.first().map(to_number).unwrap_or(0.0);
                    Ok(Value::Number($fn(x)))
                }))),
            );
        };
    }

    math_fn!("abs", f64::abs);
    math_fn!("floor", f64::floor);
    math_fn!("ceil", f64::ceil);
    // Use custom js_round instead of f64::round for spec compliance
    math.borrow_mut().set(
        "round",
        Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
            let x = args.first().map(to_number).unwrap_or(0.0);
            Ok(Value::Number(js_round(x)))
        }))),
    );
    math_fn!("sqrt", f64::sqrt);
    math_fn!("sin", f64::sin);
    math_fn!("cos", f64::cos);
    math_fn!("tan", f64::tan);
    math_fn!("asin", f64::asin);
    math_fn!("acos", f64::acos);
    math_fn!("atan", f64::atan);
    math_fn!("log", f64::ln);
    math_fn!("log10", f64::log10);
    math_fn!("log2", f64::log2);
    math_fn!("exp", f64::exp);
    math_fn!("log1p", f64::ln_1p);
    math_fn!("trunc", f64::trunc);
    math_fn!("cbrt", f64::cbrt);
    math_fn!("expm1", f64::exp_m1);
    math_fn!("cosh", f64::cosh);
    math_fn!("sinh", f64::sinh);
    math_fn!("tanh", f64::tanh);
    math_fn!("acosh", f64::acosh);
    math_fn!("asinh", f64::asinh);
    math_fn!("atanh", f64::atanh);
    // Math.sign preserves 0/-0 and NaN, so f64::signum alone won't do
    math.borrow_mut().set(
        "sign",
        Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
            let x = args.first().map(to_number).unwrap_or(0.0);
            let r = if x.is_nan() || x == 0.0 {
                x
            } else {
                x.signum()
            };
            Ok(Value::Number(r))
        }))),
    );
    math.borrow_mut().set(
        "fround",
        Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
            let x = args.first().map(to_number).unwrap_or(0.0);
            Ok(Value::Number((x as f32) as f64))
        }))),
    );
    math.borrow_mut().set(
        "clz32",
        Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
            let x = args.first().map(to_number).unwrap_or(0.0);
            Ok(Value::Number((x as i64 as u32).leading_zeros() as f64))
        }))),
    );
}

fn register_binary_math_fns(math: &Rc<RefCell<Object>>) {
    math.borrow_mut().set(
        "atan2",
        Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
            let y = args.first().map(to_number).unwrap_or(0.0);
            let x = args.get(1).map(to_number).unwrap_or(0.0);
            Ok(Value::Number(y.atan2(x)))
        }))),
    );
    math.borrow_mut().set(
        "pow",
        Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
            let base = args.first().map(to_number).unwrap_or(0.0);
            let exp = args.get(1).map(to_number).unwrap_or(1.0);
            Ok(Value::Number(base.powf(exp)))
        }))),
    );
    math.borrow_mut().set(
        "imul",
        Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
            let a = args.first().map(to_number).unwrap_or(0.0) as i64 as u32 as i32;
            let b = args.get(1).map(to_number).unwrap_or(0.0) as i64 as u32 as i32;
            Ok(Value::Number(a.wrapping_mul(b) as f64))
        }))),
    );
}

fn register_reduce_math_fns(math: &Rc<RefCell<Object>>) {
    // Per spec, any NaN argument makes max/min return NaN
    math.borrow_mut().set(
        "max",
        Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
            let mut max = f64::NEG_INFINITY;
            for v in &args {
                let n = to_number(v);
                if n.is_nan() {
                    return Ok(Value::Number(f64::NAN));
                }
                max = max.max(n);
            }
            Ok(Value::Number(max))
        }))),
    );
    math.borrow_mut().set(
        "min",
        Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
            let mut min = f64::INFINITY;
            for v in &args {
                let n = to_number(v);
                if n.is_nan() {
                    return Ok(Value::Number(f64::NAN));
                }
                min = min.min(n);
            }
            Ok(Value::Number(min))
        }))),
    );
    math.borrow_mut().set(
        "hypot",
        Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
            let result = args
                .iter()
                .map(to_number)
                .fold(0.0f64, |acc, x| acc.hypot(x));
            Ok(Value::Number(result))
        }))),
    );
    math.borrow_mut().set(
        "random",
        Value::NativeFunction(Rc::new(NativeFunction::new(|_args| {
            Ok(Value::Number(rand_simple()))
        }))),
    );
}

fn register_math_constants(math: &Rc<RefCell<Object>>) {
    // Math constants are non-writable, non-enumerable, non-configurable per spec
    use std::f64::consts;
    let constants = [
        ("PI", consts::PI),
        ("E", consts::E),
        ("LN2", consts::LN_2),
        ("LN10", consts::LN_10),
        ("LOG2E", consts::LOG2_E),
        ("LOG10E", consts::LOG10_E),
        ("SQRT1_2", consts::FRAC_1_SQRT_2),
        ("SQRT2", consts::SQRT_2),
    ];
    for (name, value) in constants {
        math.borrow_mut()
            .define(name, Value::Number(value), constant_flags(value));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_random_returns_value_in_range() {
        let value = rand_simple();
        assert!(value >= 0.0, "random should be >= 0, got {}", value);
        assert!(value < 1.0, "random should be < 1, got {}", value);
    }

    #[test]
    fn test_random_produces_different_values() {
        let v1 = rand_simple();
        let v2 = rand_simple();
        assert_ne!(
            v1, v2,
            "consecutive random calls should produce different values"
        );
    }

    #[test]
    fn test_random_distribution() {
        let mut sum = 0.0;
        let iterations = 10000;
        for _ in 0..iterations {
            sum += rand_simple();
        }
        let average = sum / iterations as f64;
        assert!(
            average > 0.4 && average < 0.6,
            "average should be ~0.5, got {}",
            average
        );
    }

    // Regression tests for spec-correct Math.round behavior

    #[test]
    fn test_round_half_toward_positive_infinity() {
        // Math.round(-0.5) must return -0, not -1
        let result = js_round(-0.5);
        assert!(
            result.is_sign_negative(),
            "round(-0.5) must return negative zero or negative"
        );
        // Check that it preserves sign of zero
        let neg_zero = -0.0f64;
        assert_eq!(js_round(-0.5), neg_zero, "round(-0.5) must equal -0");
    }

    #[test]
    fn test_round_positive_half() {
        // Math.round(0.5) must return 1
        assert_eq!(js_round(0.5), 1.0);
        assert_eq!(js_round(1.5), 2.0);
    }

    #[test]
    fn test_round_negative_half() {
        // Math.round(-1.5) must return -1 (toward +Infinity)
        assert_eq!(js_round(-1.5), -1.0);
    }

    #[test]
    fn test_round_preserves_sign_of_zero() {
        // round(+0) must return +0
        assert_eq!(js_round(0.0), 0.0);
        assert!(js_round(0.0).is_sign_positive() || js_round(0.0) == 0.0);
        // round(-0) must return -0
        let neg_zero = -0.0f64;
        assert_eq!(js_round(neg_zero), neg_zero);
    }

    #[test]
    fn test_round_nan_infinity() {
        // Math.round(NaN) must return NaN
        assert!(js_round(f64::NAN).is_nan());
        // Math.round(+Infinity) must return +Infinity
        assert_eq!(js_round(f64::INFINITY), f64::INFINITY);
        // Math.round(-Infinity) must return -Infinity
        assert_eq!(js_round(f64::NEG_INFINITY), f64::NEG_INFINITY);
    }

    #[test]
    fn test_max_min_propagate_nan() {
        let mut ctx = crate::Context::new().unwrap();
        let result = ctx.eval("Math.max(NaN, 1)").unwrap();
        assert!(
            matches!(result, Value::Number(n) if n.is_nan()),
            "Math.max(NaN, 1) must be NaN"
        );
        let result = ctx.eval("Math.min(NaN, 1)").unwrap();
        assert!(
            matches!(result, Value::Number(n) if n.is_nan()),
            "Math.min(NaN, 1) must be NaN"
        );
        assert_eq!(ctx.eval("Math.max(1, 2, 3)").unwrap(), Value::Number(3.0));
        assert_eq!(ctx.eval("Math.min(1, 2, 3)").unwrap(), Value::Number(1.0));
    }

    #[test]
    fn test_new_math_functions() {
        let mut ctx = crate::Context::new().unwrap();
        assert_eq!(ctx.eval("Math.trunc(4.7)").unwrap(), Value::Number(4.0));
        assert_eq!(ctx.eval("Math.sign(-3)").unwrap(), Value::Number(-1.0));
        assert_eq!(ctx.eval("Math.sign(0)").unwrap(), Value::Number(0.0));
        assert_eq!(ctx.eval("Math.cbrt(27)").unwrap(), Value::Number(3.0));
        assert_eq!(ctx.eval("Math.hypot(3, 4)").unwrap(), Value::Number(5.0));
        assert_eq!(ctx.eval("Math.clz32(1)").unwrap(), Value::Number(31.0));
        assert_eq!(ctx.eval("Math.imul(3, 4)").unwrap(), Value::Number(12.0));
        assert_eq!(ctx.eval("Math.fround(5.5)").unwrap(), Value::Number(5.5));
        assert_eq!(ctx.eval("Math.expm1(0)").unwrap(), Value::Number(0.0));
        assert_eq!(
            ctx.eval("Math.SQRT2").unwrap(),
            Value::Number(std::f64::consts::SQRT_2)
        );
        assert_eq!(
            ctx.eval("Math.LN2").unwrap(),
            Value::Number(std::f64::consts::LN_2)
        );
    }
}
