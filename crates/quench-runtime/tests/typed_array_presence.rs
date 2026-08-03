use quench_runtime::{Context, Value};

#[test]
fn typed_array_indices_are_present_properties() {
    let mut ctx = Context::new().unwrap();
    assert_eq!(
        ctx.eval("var ta=new Uint8Array([0,1,2,3]); 0 in ta"),
        Ok(Value::Boolean(true))
    );
    assert_eq!(
        ctx.eval("var b=new ArrayBuffer(4,{maxByteLength:8}); var view=new Uint8Array(b,0,4); b.resize(0); Array.from(view).length"),
        Ok(Value::Number(0.0))
    );
    assert_eq!(
        ctx.eval("var b=new ArrayBuffer(16,{maxByteLength:32}); var view=new Uint32Array(b,8,2); view.length"),
        Ok(Value::Number(2.0))
    );
}
