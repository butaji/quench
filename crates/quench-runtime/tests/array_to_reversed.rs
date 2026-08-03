use quench_runtime::{Context, Value};

#[test]
fn array_to_reversed_materializes_holes_and_inherited_values() {
    let mut ctx = Context::new().unwrap();
    assert_eq!(
        ctx.eval("var a=[0,,2,,4]; Array.prototype[3]=3; var r=a.toReversed(); delete Array.prototype[3]; [r.join(','),r.hasOwnProperty(3)].join('|')"),
        Ok(Value::String("4,3,2,,0|true".to_string()))
    );
}

#[test]
fn array_to_reversed_reads_source_in_descending_order() {
    let mut ctx = Context::new().unwrap();
    assert_eq!(
        ctx.eval("var order=[]; var a={length:3,get 0(){order.push(0);},get 1(){order.push(1);},get 2(){order.push(2);}}; Object.setPrototypeOf(a,Array.prototype); a.toReversed(); order.join(',')"),
        Ok(Value::String("2,1,0".to_string()))
    );
}
