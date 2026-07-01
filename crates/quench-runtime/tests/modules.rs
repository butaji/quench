//! Tests for module system functionality
//!
//! Tests for: ES6 imports/exports, export default, export *, dynamic import

use quench_runtime::{Context, Value};

#[test]
fn test_exports_object_exists() {
    let ctx = Context::new().unwrap();
    let exports = ctx.get_global("exports");
    assert!(exports.is_some());
}

#[test]
fn test_module_object_exists() {
    let ctx = Context::new().unwrap();
    let module = ctx.get_global("module");
    assert!(module.is_some());
}

#[test]
fn test_module_exports_property() {
    let ctx = Context::new().unwrap();
    let module = ctx.get_global("module").unwrap();
    if let Value::Object(obj) = module {
        let exports = obj.borrow().get("exports");
        assert!(exports.is_some());
    } else {
        panic!("module should be an object");
    }
}

#[test]
fn test_commonjs_exports_assignment() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval("exports.foo = 42");
    assert!(result.is_ok());
    
    // Check that the exports object has the property
    let exports = ctx.get_global("exports").unwrap();
    if let Value::Object(obj) = exports {
        let foo = obj.borrow().get("foo");
        assert!(foo.is_some());
        assert_eq!(foo.unwrap(), Value::Number(42.0));
    } else {
        panic!("exports should be an object");
    }
}

#[test]
fn test_module_exports_assignment() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval("module.exports = { bar: 'hello' }");
    assert!(result.is_ok());
    
    // Check that the module.exports property is updated
    let module = ctx.get_global("module").unwrap();
    if let Value::Object(obj) = module {
        let exports = obj.borrow().get("exports");
        assert!(exports.is_some());
        if let Value::Object(exp) = exports.unwrap() {
            let bar = exp.borrow().get("bar");
            assert!(bar.is_some());
            assert_eq!(bar.unwrap(), Value::String("hello".to_string()));
        } else {
            panic!("exports should be an object");
        }
    } else {
        panic!("module should be an object");
    }
}
