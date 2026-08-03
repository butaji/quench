use quench_runtime::{Context, Value};

#[test]
fn array_concat_spreads_typed_arrays_when_requested() {
    let mut ctx = Context::new().unwrap();
    assert_eq!(
        ctx.eval("var ta=new Uint8Array([1,2]); ta[Symbol.isConcatSpreadable]=true; [].concat(ta).join(',')"),
        Ok(Value::String("1,2".to_string()))
    );
}
