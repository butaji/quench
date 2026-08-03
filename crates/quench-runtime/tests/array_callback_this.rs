use quench_runtime::{Context, Value};

#[test]
fn array_map_boxes_primitive_this_arg_for_sloppy_callback() {
    let mut ctx = Context::new().unwrap();
    assert_eq!(
        ctx.eval("[11].map(function(){return typeof this==='object' && this._value===101;},101)[0]"),
        Ok(Value::Boolean(true))
    );
}

#[test]
fn reverse_getter_length_shrink_removes_index() {
    let mut ctx = Context::new().unwrap();
    assert_eq!(
        ctx.eval("var a=['first','second']; Object.defineProperty(a,0,{get:function(){a.length=0;return 'first';}}); a.reverse(); (0 in a)+'|'+(1 in a)+'|'+a[1]"),
        Ok(Value::String("false|true|first".to_string()))
    );
}
