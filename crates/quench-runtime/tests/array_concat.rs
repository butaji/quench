use quench_runtime::{Context, Value};

#[test]
fn concat_preserves_spread_positions_for_missing_elements() {
    let mut ctx = Context::new().unwrap();
    assert_eq!(
        ctx.eval("var ta=new Uint8Array(1); Object.defineProperty(ta,'length',{value:4}); ta.length"),
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
