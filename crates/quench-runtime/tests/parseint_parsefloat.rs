//! parseInt and parseFloat spec compliance tests

use quench_runtime::Context;
use quench_runtime::Value;

fn eval_js(ctx: &mut Context, js: &str) -> Value {
    ctx.eval(js).unwrap()
}

fn assert_number_eq(ctx: &mut Context, js: &str, expected: f64) {
    let result = eval_js(ctx, js);
    match result {
        Value::Number(n) => {
            if expected.is_nan() {
                assert!(n.is_nan(), "Expected NaN, got {}", n);
            } else if expected.is_infinite() {
                assert_eq!(n.is_infinite(), expected.is_infinite());
                assert_eq!(n.is_sign_positive(), expected.is_sign_positive());
            } else {
                assert!((n - expected).abs() < 1e-10, "Expected {}, got {}", expected, n);
            }
        }
        _ => panic!("Expected Number, got {:?}", result),
    }
}

// ============================================================================
// parseInt tests
// ============================================================================

#[test]
fn test_parseint_basic() {
    let mut ctx = Context::new().unwrap();
    assert_number_eq(&mut ctx, "parseInt('42')", 42.0);
}

#[test]
fn test_parseint_leading_whitespace() {
    let mut ctx = Context::new().unwrap();
    assert_number_eq(&mut ctx, "parseInt('  42')", 42.0);
    assert_number_eq(&mut ctx, "parseInt('\\n  42')", 42.0);
    assert_number_eq(&mut ctx, "parseInt('\\t  42')", 42.0);
}

#[test]
fn test_parseint_with_radix() {
    let mut ctx = Context::new().unwrap();
    assert_number_eq(&mut ctx, "parseInt('123', 16)", 291.0);
    assert_number_eq(&mut ctx, "parseInt('10', 8)", 8.0);
    assert_number_eq(&mut ctx, "parseInt('10', 2)", 2.0);
}

#[test]
fn test_parseint_hex_prefix() {
    let mut ctx = Context::new().unwrap();
    assert_number_eq(&mut ctx, "parseInt('0xFF')", 255.0);
    assert_number_eq(&mut ctx, "parseInt('0xFF', 16)", 255.0);
}

#[test]
fn test_parseint_stops_at_invalid_char() {
    let mut ctx = Context::new().unwrap();
    assert_number_eq(&mut ctx, "parseInt('123abc')", 123.0);
    assert_number_eq(&mut ctx, "parseInt('42xyz')", 42.0);
}

#[test]
fn test_parseint_negative() {
    let mut ctx = Context::new().unwrap();
    assert_number_eq(&mut ctx, "parseInt('-42')", -42.0);
    assert_number_eq(&mut ctx, "parseInt('+42')", 42.0);
}

#[test]
fn test_parseint_nan() {
    let mut ctx = Context::new().unwrap();
    assert_number_eq(&mut ctx, "parseInt('abc')", f64::NAN);
    assert_number_eq(&mut ctx, "parseInt('')", f64::NAN);
}

#[test]
fn test_parseint_radix_bounds() {
    let mut ctx = Context::new().unwrap();
    // Radix must be 2-36 or 0
    // With radix 0, defaults to 10
    assert_number_eq(&mut ctx, "parseInt('10', 0)", 10.0);
    assert_number_eq(&mut ctx, "parseInt('10', 1)", f64::NAN);
    assert_number_eq(&mut ctx, "parseInt('10', 37)", f64::NAN);
}

#[test]
fn test_parseint_decimal_only_integers() {
    let mut ctx = Context::new().unwrap();
    // parseInt should only parse integers, so decimal points stop parsing
    assert_number_eq(&mut ctx, "parseInt('3.14')", 3.0);
}

// ============================================================================
// parseFloat tests
// ============================================================================

#[test]
fn test_parsefloat_basic() {
    let mut ctx = Context::new().unwrap();
    assert_number_eq(&mut ctx, "parseFloat('3.14')", 3.14);
}

#[test]
fn test_parsefloat_leading_whitespace() {
    let mut ctx = Context::new().unwrap();
    assert_number_eq(&mut ctx, "parseFloat('  3.14')", 3.14);
    assert_number_eq(&mut ctx, "parseFloat('\\n  3.14')", 3.14);
}

#[test]
fn test_parsefloat_stops_at_invalid_char() {
    let mut ctx = Context::new().unwrap();
    assert_number_eq(&mut ctx, "parseFloat('3.14abc')", 3.14);
    assert_number_eq(&mut ctx, "parseFloat('123.45xyz')", 123.45);
}

#[test]
fn test_parsefloat_negative() {
    let mut ctx = Context::new().unwrap();
    assert_number_eq(&mut ctx, "parseFloat('-3.14')", -3.14);
    assert_number_eq(&mut ctx, "parseFloat('+3.14')", 3.14);
}

#[test]
fn test_parsefloat_nan() {
    let mut ctx = Context::new().unwrap();
    assert_number_eq(&mut ctx, "parseFloat('abc')", f64::NAN);
    assert_number_eq(&mut ctx, "parseFloat('')", f64::NAN);
}

#[test]
fn test_parsefloat_integer() {
    let mut ctx = Context::new().unwrap();
    assert_number_eq(&mut ctx, "parseFloat('42')", 42.0);
}

#[test]
fn test_parsefloat_exponent() {
    let mut ctx = Context::new().unwrap();
    assert_number_eq(&mut ctx, "parseFloat('3.14e2')", 314.0);
    assert_number_eq(&mut ctx, "parseFloat('3.14E+2')", 314.0);
    assert_number_eq(&mut ctx, "parseFloat('3.14e-2')", 0.0314);
}

#[test]
fn test_parsefloat_only_decimal_point() {
    let mut ctx = Context::new().unwrap();
    assert_number_eq(&mut ctx, "parseFloat('.5')", 0.5);
}
