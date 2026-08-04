use quench_runtime::{Context, Value};

#[test]
fn array_sort_legacy_to_string_order() {
    let mut ctx = Context::new().unwrap();
    let source = r#"
      var obj = { valueOf: function() { return 1; }, toString: function() { return -2; } };
      var values = [undefined, 2, 1, "X", -1, "a", true, obj, NaN, Infinity];
      values.sort(function(x, y) { var xs=String(x), ys=String(y); return xs < ys ? 1 : xs > ys ? -1 : 0; });
      values.map(function(v) { return v === obj ? "obj" : String(v); }).join("|")
    "#;
    assert_eq!(
        ctx.eval(source),
        Ok(Value::String(
            "true|a|X|NaN|Infinity|2|1|obj|-1|undefined".to_string()
        ))
    );
}
