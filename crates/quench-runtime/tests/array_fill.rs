use quench_runtime::{Context, Value};

#[test]
fn fill_treats_explicit_undefined_end_as_length() {
    let mut ctx = Context::new().unwrap();
    assert_eq!(
        ctx.eval("[0, 0].fill(1, 0, undefined).join(',')"),
        Ok(Value::String("1,1".to_string()))
    );
}
