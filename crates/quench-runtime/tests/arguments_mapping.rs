use quench_runtime::{Context, Value};

#[test]
fn sloppy_duplicate_parameters_map_only_the_last_argument_binding() {
    let mut ctx = Context::new().unwrap();
    assert_eq!(
        ctx.eval("var x=(function(a, a, a){ return arguments; })(1,2,3); x[0] + ',' + x[1]"),
        Ok(Value::String("1,2".to_string()))
    );
}

#[test]
fn arguments_length_redefinition_is_observable() {
    let mut ctx = Context::new().unwrap();
    assert_eq!(
        ctx.eval(
            "var x=(function(a,a,a){return arguments;})(1,2,3); \
             Object.defineProperty(x,'length',{value:6}); x.length"
        ),
        Ok(Value::Number(6.0))
    );
}

#[test]
fn expanded_arguments_length_supplies_undefined_concat_elements() {
    let mut ctx = Context::new().unwrap();
    assert_eq!(
        ctx.eval(
            "var x=(function(a,a,a){return arguments;})(1,2,3); \
             x[Symbol.isConcatSpreadable]=true; Object.defineProperty(x,'length',{value:6}); \
             [].concat(x).length"
        ),
        Ok(Value::Number(6.0))
    );
}
