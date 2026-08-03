use quench_runtime::{Context, Value};

#[test]
fn copy_within_clamps_end_to_length() {
    let mut ctx = Context::new().unwrap();
    assert_eq!(
        ctx.eval("[0, 1, 2, 3].copyWithin(0, 1, 6).join(',')"),
        Ok(Value::String("1,2,3,3".to_string()))
    );
}
