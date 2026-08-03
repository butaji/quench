use quench_runtime::{Context, Value};

#[test]
fn sloppy_duplicate_parameters_map_only_the_last_argument_binding() {
    let mut ctx = Context::new().unwrap();
    assert_eq!(
        ctx.eval("var x=(function(a, a, a){ return arguments; })(1,2,3); x[0] + ',' + x[1]"),
        Ok(Value::String("1,2".to_string()))
    );
}
