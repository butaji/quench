use quench_runtime::{Context, Value};

#[test]
fn concat_preserves_spread_positions_for_missing_elements() {
    let mut ctx = Context::new().unwrap();
    assert_eq!(
        ctx.eval(
            "var ta=new Uint8Array(1); Object.defineProperty(ta,'length',{value:4}); ta.length"
        ),
        Ok(Value::Number(4.0))
    );
    assert_eq!(
        ctx.eval(
            "var ta=new Uint8Array(1); Object.defineProperty(ta,'length',{value:4}); \
             ta[Symbol.isConcatSpreadable]=true; var r=[].concat(ta); [r.length,r[0],1 in r,3 in r].join('|')"
        ),
        Ok(Value::String("4|0|false|false".to_string()))
    );
}

#[test]
fn concat_reads_array_constructor_before_spreadability() {
    let mut ctx = Context::new().unwrap();
    assert_eq!(
        ctx.eval(
            "var calls=[]; var a=[]; Object.defineProperty(a,'constructor',{get(){calls.push('constructor');return Array;}}); \
             Object.defineProperty(a,Symbol.isConcatSpreadable,{get(){calls.push('spread');}}); a.concat(1); calls.join(',')"
        ),
        Ok(Value::String("constructor,spread".to_string()))
    );
}

#[test]
fn concat_constructs_array_species() {
    let mut ctx = Context::new().unwrap();
    assert_eq!(
        ctx.eval("var calls=0; function C(n){calls++;this.length=n;} var a=[]; a.constructor={}; a.constructor[Symbol.species]=C; a.concat(); calls===1")
            .unwrap(),
        Value::Boolean(true)
    );
}
