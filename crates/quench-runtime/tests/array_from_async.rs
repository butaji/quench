use quench_runtime::{Context, Value};

#[test]
fn array_from_async_awaits_mapping_promises() {
    let mut ctx = Context::new().unwrap();
    ctx.eval("var result; Promise.all([Promise.resolve(2)]).then(v => { result = v[0]; });")
        .unwrap();
    assert_eq!(ctx.eval("result"), Ok(Value::Number(2.0)));
    ctx.eval("result = undefined;").unwrap();
    ctx.eval("Array.fromAsync([1], async x => x + 1).then(v => { result = v[0]; });")
        .unwrap();
    assert_eq!(ctx.eval("result"), Ok(Value::Number(2.0)));
}
