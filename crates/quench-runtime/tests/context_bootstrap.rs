use quench_runtime::{Context, Value};

#[test]
fn new_context_bootstraps_self_hosted_builtins() {
    let mut ctx = Context::new().unwrap();
    assert_eq!(
        ctx.eval("'  quench  '.trim()"),
        Ok(Value::String("quench".to_string()))
    );
}

#[test]
fn string_constructor_called_without_new_returns_a_primitive() {
    let mut ctx = Context::new().unwrap();
    assert_eq!(
        ctx.eval("String('quench')"),
        Ok(Value::String("quench".to_string()))
    );
}

#[test]
fn string_constructor_unboxes_string_objects() {
    let mut ctx = Context::new().unwrap();
    assert_eq!(
        ctx.eval("String(new String('quench'))"),
        Ok(Value::String("quench".to_string()))
    );
}

#[test]
fn self_hosted_string_trim_dependencies_preserve_primitive_strings() {
    let mut ctx = Context::new().unwrap();
    quench_runtime::builtins::bootstrap::bootstrap_js_builtins(&mut ctx).unwrap();
    assert_eq!(
        ctx.eval("String('  quench  ').replace(/^\\s+|\\s+$/g, '')"),
        Ok(Value::String("quench".to_string()))
    );
}

#[test]
fn self_hosted_array_slice_accepts_error_constructors() {
    let mut ctx = Context::new().unwrap();
    assert_eq!(
        ctx.eval("[Error, EvalError, RangeError].slice().length"),
        Ok(Value::Number(3.0))
    );
}

#[test]
fn self_hosted_array_push_accepts_optional_error_constructors() {
    let mut ctx = Context::new().unwrap();
    assert_eq!(
        ctx.eval("var errors = [Error].slice(); if (typeof AggregateError !== 'undefined') errors.push(AggregateError); if (typeof SuppressedError !== 'undefined') errors.push(SuppressedError); errors.length"),
        Ok(Value::Number(3.0))
    );
}

#[test]
fn native_errors_harness_loads_after_assert_harness() {
    let mut ctx = Context::new().unwrap();
    assert!(ctx
        .eval(include_str!("../../../tests/test262/harness/assert.js"))
        .is_ok());
    assert!(ctx
        .eval(include_str!("../../../tests/test262/harness/deepEqual.js"))
        .is_ok());
    assert!(ctx
        .eval(include_str!(
            "../../../tests/test262/harness/nativeErrors.js"
        ))
        .is_ok());
}

#[test]
fn native_errors_harness_loads_in_an_indirect_sloppy_eval() {
    let mut ctx = Context::new().unwrap();
    let assert_source =
        serde_json::to_string(include_str!("../../../tests/test262/harness/assert.js")).unwrap();
    let deep_equal_source =
        serde_json::to_string(include_str!("../../../tests/test262/harness/deepEqual.js")).unwrap();
    let native_errors_source = serde_json::to_string(include_str!(
        "../../../tests/test262/harness/nativeErrors.js"
    ))
    .unwrap();
    assert!(ctx.eval(&format!("(0, eval)({assert_source})")).is_ok());
    assert!(ctx.eval(&format!("(0, eval)({deep_equal_source})")).is_ok());
    assert!(ctx
        .eval(&format!("(0, eval)({native_errors_source})"))
        .is_ok());
}
