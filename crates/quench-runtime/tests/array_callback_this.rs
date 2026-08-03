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

#[test]
fn array_of_uses_called_constructor_and_sets_length() {
    let mut ctx = Context::new().unwrap();
    assert_eq!(
        ctx.eval("var hits=0; function Pack(){Object.defineProperty(this,'length',{set:function(v){hits++;}});} var r=Array.of.call(Pack,1,2); [hits,r instanceof Pack].join('|')"),
        Ok(Value::String("1|true".to_string()))
    );
}

#[test]
fn array_prototype_has_unscopables_object() {
    let mut ctx = Context::new().unwrap();
    assert_eq!(
        ctx.eval("var u=Array.prototype[Symbol.unscopables]; var d=Object.getOwnPropertyDescriptor(u,'toReversed'); [Object.getPrototypeOf(u)===Object.prototype,d.value,d.writable,d.enumerable,d.configurable,u.toSorted,u.toSpliced,Object.prototype.hasOwnProperty.call(u,'with')].join('|')"),
        Ok(Value::String("true|true|true|true|true|true|true|false".to_string()))
    );
}

#[test]
fn array_from_uses_called_constructor_prototype() {
    let mut ctx = Context::new().unwrap();
    assert_eq!(
        ctx.eval("var r=Array.from.call(Object,[1,2]); r.constructor===Object"),
        Ok(Value::Boolean(true))
    );
}

#[test]
fn array_from_reads_array_elements_after_each_mapping_callback() {
    let mut ctx = Context::new().unwrap();
    assert_eq!(
        ctx.eval("var a=[127,4,8]; Array.from(a,function(v,i){if(i+1<a.length)a[i+1]=127; return v;}).join(',')"),
        Ok(Value::String("127,127,127".to_string()))
    );
}
