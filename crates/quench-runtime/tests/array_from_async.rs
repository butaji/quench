use quench_runtime::{Context, Value};

fn new_context() -> Context {
    let mut ctx = Context::new().unwrap();
    quench_runtime::builtins::bootstrap::bootstrap_js_builtins(&mut ctx).unwrap();
    ctx
}

#[test]
fn array_from_async_awaits_mapping_promises() {
    let mut ctx = new_context();
    ctx.eval("var result; Promise.all([Promise.resolve(2)]).then(v => { result = v[0]; });")
        .unwrap();
    assert_eq!(ctx.eval("result"), Ok(Value::Number(2.0)));
    ctx.eval("result = undefined;").unwrap();
    ctx.eval("Array.fromAsync([1], async x => x + 1).then(v => { result = v[0]; });")
        .unwrap();
    assert_eq!(ctx.eval("result"), Ok(Value::Number(2.0)));
}

#[test]
fn array_from_async_consumes_async_iterables() {
    let mut ctx = new_context();
    ctx.eval(
        "var result; var i=0; var input={[Symbol.asyncIterator](){return {next(){return Promise.resolve(i<2?{value:i++,done:false}:{done:true});}};}}; Array.fromAsync(input).then(v=>result=v.join(','));",
    )
    .unwrap();
    assert_eq!(ctx.eval("result"), Ok(Value::String("0,1".to_string())));
}

#[test]
fn array_from_async_accepts_async_generator_methods() {
    let mut ctx = new_context();
    ctx.eval("async function* gen(){yield 1; yield 2;} var result; Array.fromAsync({[Symbol.asyncIterator]:gen}, x=>x*2).then(v=>result=v.join(','));")
        .unwrap();
    assert_eq!(ctx.eval("result"), Ok(Value::String("2,4".to_string())));
}

#[test]
fn symbol_description_accepts_a_well_known_symbol_receiver() {
    let mut ctx = new_context();
    assert_eq!(
        ctx.eval("Symbol.asyncIterator.description"),
        Ok(Value::String("Symbol.asyncIterator".to_string()))
    );
}

#[test]
fn array_from_async_awaits_each_mapping_result_before_next_call() {
    let mut ctx = new_context();
    ctx.eval(
        "var result; var calls=[]; function map(v){calls.push('map'+v); return {then:function(resolve){calls.push('then'+v); resolve(v*2);}};} \
         Array.fromAsync([1,2,3], map).then(v=>{result=v.join(',')+'|'+calls.join(',');});",
    )
    .unwrap();
    assert_eq!(
        ctx.eval("result"),
        Ok(Value::String(
            "2,4,6|map1,then1,map2,then2,map3,then3".to_string()
        ))
    );
}

#[test]
fn array_from_async_rejects_non_callable_mapping_function() {
    let mut ctx = new_context();
    ctx.eval("var result; Array.fromAsync([], null).then(()=>result='ok', e=>result=e.name);")
        .unwrap();
    assert_eq!(
        ctx.eval("result"),
        Ok(Value::String("TypeError".to_string()))
    );
}

#[test]
fn array_from_async_rejects_non_callable_mapping_for_array_like_input() {
    let mut ctx = new_context();
    ctx.eval(
        "var result; Array.fromAsync({length:0}, null).then(()=>result='ok', e=>result=e.name);",
    )
    .unwrap();
    assert_eq!(
        ctx.eval("result"),
        Ok(Value::String("TypeError".to_string()))
    );
}

#[test]
fn array_from_async_rejects_array_like_lengths_above_array_maximum() {
    let mut ctx = new_context();
    ctx.eval(
        "var result; Array.fromAsync({length:4294967296}).then(()=>result='ok', e=>result=e.name);",
    )
    .unwrap();
    assert_eq!(
        ctx.eval("result"),
        Ok(Value::String("RangeError".to_string()))
    );
}

#[test]
fn array_from_async_uses_intrinsic_array_for_non_constructor_this() {
    let mut ctx = new_context();
    ctx.eval("var result; Array.fromAsync.call({length:4000}, [1,2]).then(v=>result=Array.isArray(v)&&v.join(',')==='1,2');")
        .unwrap();
    assert_eq!(ctx.eval("result"), Ok(Value::Boolean(true)));
}

#[test]
fn array_from_async_uses_intrinsic_array_for_arrow_this() {
    let mut ctx = new_context();
    ctx.eval("var result; Array.fromAsync.call(()=>{}, [1,2]).then(v=>result=Array.isArray(v)&&v.join(',')==='1,2');")
        .unwrap();
    assert_eq!(ctx.eval("result"), Ok(Value::Boolean(true)));
}

#[test]
fn array_from_async_awaits_non_promise_thenables() {
    let mut ctx = new_context();
    ctx.eval("var result; var v={}; var input={length:1,0:{then:function(resolve){resolve(v);}}}; Array.fromAsync(input).then(a=>{result=a[0]===v;});").unwrap();
    assert_eq!(ctx.eval("result"), Ok(Value::Boolean(true)));
}

#[test]
fn array_from_async_awaits_arraylike_values_and_mapping_promises() {
    let mut ctx = new_context();
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

#[test]
fn array_from_async_passes_this_arg_to_mapping_callback() {
    let mut ctx = new_context();
    ctx.eval(
        &("var result; Array.fromAsync([1], function(){return this.value;}, {value:7})".to_owned()
            + ".then(v=>{result=v[0];});"),
    )
    .unwrap();
    assert_eq!(ctx.eval("result"), Ok(Value::Number(7.0)));
}

#[test]
fn array_from_async_rejects_a_thenable_element() {
    let mut ctx = new_context();
    ctx.eval(
        "var result; var item={then:function(resolve,reject){reject('bad');}}; \
         Array.fromAsync({length:1,0:item}).then(function(){result='ok';}, function(e){result=e;});",
    )
    .unwrap();
    assert_eq!(ctx.eval("result"), Ok(Value::String("bad".to_string())));
}

#[test]
fn promise_resolve_rejects_a_thenable() {
    let mut ctx = new_context();
    ctx.eval(
        "var result; var item={then:function(resolve,reject){reject('bad');}}; \
         Promise.resolve(item).then(function(){result='ok';}, function(e){result=e;});",
    )
    .unwrap();
    assert_eq!(ctx.eval("result"), Ok(Value::String("bad".to_string())));
}

#[test]
fn promise_all_rejects_a_pending_thenable_promise() {
    let mut ctx = new_context();
    ctx.eval(
        "var result; var item={then:function(resolve,reject){reject('bad');}}; \
         Promise.all([Promise.resolve(item)]).then(function(){result='ok';}, function(e){result=e;});",
    )
    .unwrap();
    assert_eq!(ctx.eval("result"), Ok(Value::String("bad".to_string())));
}
