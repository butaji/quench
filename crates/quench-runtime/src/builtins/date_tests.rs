use super::*;

#[test]
fn test_days_from_ymd_before_1970_is_negative() {
    assert_eq!(days_from_ymd(1969, 1, 1), -365);
    assert_eq!(days_from_ymd(1968, 1, 1), -(365 + 366));
    assert_eq!(days_from_ymd(1970, 1, 1), 0);
}

#[test]
fn test_days_from_ymd_normalizes_month_overflow() {
    assert_eq!(days_from_ymd(2024, 14, 1), days_from_ymd(2025, 2, 1));
    assert_eq!(days_from_ymd(2024, 0, 1), days_from_ymd(2023, 12, 1));
}

#[test]
fn test_date_before_1970_has_negative_timestamp() {
    let mut ctx = crate::Context::new().unwrap();
    let result = ctx.eval("new Date(1969, 0, 1).getTime()").unwrap();
    match result {
        Value::Number(n) => assert!(n < 0.0),
        other => panic!("expected Number, got {:?}", other),
    }
}

#[test]
fn test_date_month_overflow_normalizes() {
    let mut ctx = crate::Context::new().unwrap();
    let overflow = ctx.eval("new Date(2024, 13, 1).getTime()").unwrap();
    let expected = ctx.eval("new Date(2025, 1, 1).getTime()").unwrap();
    assert_eq!(overflow, expected);
}

fn eval_num(src: &str) -> f64 {
    let mut ctx = crate::Context::new().unwrap();
    match ctx.eval(src).unwrap() {
        Value::Number(n) => n,
        other => panic!("expected Number from {:?}, got {:?}", src, other),
    }
}

#[test]
fn test_date_get_full_year_month_date() {
    let mut ctx = crate::Context::new().unwrap();
    assert_eq!(
        ctx.eval("new Date(1859, 10, 24).getFullYear()").unwrap(),
        Value::Number(1859.0)
    );
    assert_eq!(
        ctx.eval("new Date(1859, 10, 24).getMonth()").unwrap(),
        Value::Number(10.0)
    );
    assert_eq!(
        ctx.eval("new Date(1859, 10, 24).getDate()").unwrap(),
        Value::Number(24.0)
    );
}

#[test]
fn test_date_subclass_regular_subclassing() {
    let mut ctx = crate::Context::new().unwrap();
    ctx.eval("class D extends Date {}").unwrap();
    assert_eq!(
        ctx.eval("new D(1859, 10, 24).getFullYear()").unwrap(),
        Value::Number(1859.0)
    );
    assert_eq!(
        ctx.eval("new D(1859, 10, 24).getMonth()").unwrap(),
        Value::Number(10.0)
    );
    assert_eq!(
        ctx.eval("new D(1859, 10, 24).getDate()").unwrap(),
        Value::Number(24.0)
    );
}

#[test]
fn test_parse_float_accepts_infinity_literal() {
    assert!(eval_num("parseFloat(Infinity)").is_infinite());
    assert!(eval_num("parseFloat(Infinity)") > 0.0);
    assert!(eval_num("parseFloat(-Infinity)") < 0.0);
    assert!(eval_num("parseFloat('Infinity')").is_infinite());
    assert!(eval_num("parseFloat('-Infinity')").is_infinite());
    assert!(eval_num("parseFloat('-Infinity')") < 0.0);
    assert!(eval_num("parseFloat('infinity')").is_nan());
}

#[test]
fn test_parse_float_decimal_then_exponent() {
    assert_eq!(eval_num("parseFloat('.01e+2')"), 1.0);
    assert_eq!(eval_num("parseFloat('.5e1')"), 5.0);
    let expected = eval_num("3.14");
    assert!((eval_num("parseFloat('3.14')") - expected).abs() < 1e-10);
    assert_eq!(eval_num("parseFloat('.01')"), 0.01);
}
