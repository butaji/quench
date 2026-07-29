use quench_runtime::*;

#[test]
fn test_ta_element_set() {
    let mut ctx = Context::new().unwrap();
    crate::builtins::register_builtins(&mut ctx);

    let result = ctx
        .eval(
            "var ta = new Uint8Array(10); \
         ta[0] = 42; \
         typeof ta[0]",
        )
        .unwrap();
    eprintln!("typeof ta[0] = {:?}", result);

    let r2 = ctx
        .eval(
            "var ta = new Uint8Array(10); \
         var before = ta[0]; \
         ta[0] = 42; \
         var after = ta[0]; \
         JSON.stringify([before, after])",
        )
        .unwrap();
    eprintln!("JSON result = {:?}", r2);
}
