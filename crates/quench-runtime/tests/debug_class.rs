use quench_runtime::Context;

#[test]
fn test_super_basic() {
    let mut ctx = Context::new().unwrap();
    
    // Test - try to access super directly
    let result = ctx.eval(r#"
        class Base {
            constructor() {
                this.value = 42;
            }
        }
        class Derived extends Base {
            constructor() {
                super();
            }
        }
        new Derived().value;
    "#);
    println!("Result: {:?}", result);
}
