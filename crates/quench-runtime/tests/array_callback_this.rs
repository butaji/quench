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

#[test]
fn array_filter_reads_constructor_before_callback() {
    let mut ctx = Context::new().unwrap();
    assert_eq!(
        ctx.eval("var a=[]; Object.defineProperty(a,'constructor',{get(){throw new Error('sentinel');}}); try { a.filter(()=>true); false; } catch (e) { e.message==='sentinel'; }")
            .unwrap(),
        Value::Boolean(true)
    );
}

#[test]
fn array_filter_constructs_species_result() {
    let mut ctx = Context::new().unwrap();
    assert_eq!(
        ctx.eval("var C=function(){this.marker=7;}; var a=[]; a.constructor={}; a.constructor[Symbol.species]=C; var r=a.filter(()=>true); r.marker===7")
            .unwrap(),
        Value::Boolean(true)
    );
}

#[test]
fn array_map_constructs_species_result() {
    let mut ctx = Context::new().unwrap();
    assert_eq!(
        ctx.eval("var C=function(){this.marker=7;}; var a=[]; a.constructor={}; a.constructor[Symbol.species]=C; var r=a.map(()=>1); r.marker===7")
            .unwrap(),
        Value::Boolean(true)
    );
}

#[test]
fn array_from_calls_custom_constructor_for_iterables() {
    let mut ctx = Context::new().unwrap();
    assert_eq!(
        ctx.eval("var calls=0,args; function C(){calls++;args=arguments;} Array.from.call(C,{[Symbol.iterator](){return {next(){return {done:true};}};}}); calls===1&&args.length===0")
            .unwrap(),
        Value::Boolean(true)
    );
}

#[test]
fn array_from_constructs_before_iterating() {
    let mut ctx = Context::new().unwrap();
    assert_eq!(
        ctx.eval("var calls=0; function C(){calls++;throw new Error('sentinel');} try { Array.from.call(C,{[Symbol.iterator](){return undefined;}}); false; } catch (e) { calls===1; }")
            .unwrap(),
        Value::Boolean(true)
    );
}

#[test]
fn array_from_closes_iterator_when_result_property_creation_fails() {
    let mut ctx = Context::new().unwrap();
    assert_eq!(
        ctx.eval("var closed=0; function C(){Object.defineProperty(this,'0',{writable:true,configurable:false});} var x={[Symbol.iterator](){return {next(){return {done:false,value:1};},return(){closed++;}};}}; try { Array.from.call(C,x); } catch (e) {} closed===1")
            .unwrap(),
        Value::Boolean(true)
    );
}

#[test]
fn array_from_skips_elements_deleted_by_mapping_callback() {
    let mut ctx = Context::new().unwrap();
    assert_eq!(
        ctx.eval("var a=[0,1,2,3,4,5]; var r=Array.from(a,function(v){a.pop();return v;}); r.length===3&&r.join(',')==='0,1,2'")
            .unwrap(),
        Value::Boolean(true)
    );
}

#[test]
fn array_flat_map_constructs_species_result() {
    let mut ctx = Context::new().unwrap();
    assert_eq!(
        ctx.eval("var C=function(){this.marker=7;}; var a=[1]; a.constructor={}; a.constructor[Symbol.species]=C; var r=a.flatMap(x=>[x]); r.marker===7")
            .unwrap(),
        Value::Boolean(true)
    );
}

#[test]
fn array_flat_does_not_call_nested_flat_methods() {
    let mut ctx = Context::new().unwrap();
    assert_eq!(
        ctx.eval("var calls=[]; var nested=new Proxy([1],{get(t,p){calls.push(p);return Reflect.get(t,p);}}); [nested].flat(); calls.indexOf('flat')===-1")
            .unwrap(),
        Value::Boolean(true)
    );
}

#[test]
fn array_flat_writes_species_result_at_numeric_indices() {
    let mut ctx = Context::new().unwrap();
    assert_eq!(
        ctx.eval("function C(){Object.defineProperty(this,'0',{value:1,writable:false,configurable:true});} var a=[[2]]; a.constructor={}; a.constructor[Symbol.species]=C; var r=a.flat(); r[0]===2&&Object.getOwnPropertyDescriptor(r,'0').writable")
            .unwrap(),
        Value::Boolean(true)
    );
}
