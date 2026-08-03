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
fn concat_does_not_spread_object_initialized_by_array_apply() {
    let mut ctx = Context::new().unwrap();
    assert_eq!(
        ctx.eval("class NonArray{constructor(){Array.apply(this,arguments);}} var o=new NonArray(1,2,3); Array.prototype.concat.call(o,4,5,6).length"),
        Ok(Value::Number(4.0))
    );
}
