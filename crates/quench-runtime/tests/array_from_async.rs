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

#[test]
fn array_from_async_observes_array_mutation_during_iteration() {
    let mut ctx = Context::new().unwrap();
    ctx.eval("var items=[1,2,3]; var result; var p=Array.fromAsync(items); items[0]=7; items[1]=8; p.then(v=>{result=v.join(',');});").unwrap();
    assert_eq!(ctx.eval("result"), Ok(Value::String("1,8,3".to_string())));
}

#[test]
fn array_from_async_awaits_non_promise_thenables() {
    let mut ctx = Context::new().unwrap();
    ctx.eval("var result; var v={}; var input={length:1,0:{then:function(resolve){resolve(v);}}}; Array.fromAsync(input).then(a=>{result=a[0]===v;});").unwrap();
    assert_eq!(ctx.eval("result"), Ok(Value::Boolean(true)));
}

#[test]
fn array_from_async_awaits_arraylike_values_and_mapping_promises() {
    let mut ctx = Context::new().unwrap();
    ctx.eval(
        "var result; var input={length:4,0:0,1:2,2:Promise.resolve(4),3:6}; \
         async function map(v,i){return Promise.resolve(v*i);} \
         Array.fromAsync(input,map).then(v=>{result=v.join(',');},e=>{result='ERR:'+e;});",
    )
    .unwrap();
    assert_eq!(
        ctx.eval("result"),
        Ok(Value::String("0,2,8,18".to_string()))
    );
}
