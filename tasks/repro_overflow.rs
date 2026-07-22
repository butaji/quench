// Repro for prototype-wiring.js stack overflow
#[test]
fn repro_prototype_wiring_overflow() {
    let mut ctx = crate::Context::new().unwrap();
    let result = ctx.eval(
        r#"
        class Base {
          constructor(x) {
            this.foobar = x;
          }
        }
        class Subclass extends Base {
          constructor(x) {
            super(x);
          }
        }
        var s = new Subclass(1);
        s.foobar
        "#,
    );
    eprintln!("Result: {:?}", result);
    assert_eq!(result.unwrap(), crate::Value::Number(1.0));
}
