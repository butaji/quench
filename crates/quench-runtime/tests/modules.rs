//! Tests for module system functionality
//!
//! Tests for: ES6 imports/exports, export default, export *, dynamic import

use quench_runtime::{Context, Value, swc_parse::parse_es_module};

/// Helper to evaluate ES module source code
fn eval_es_module(source: &str) -> Result<Value, quench_runtime::JsError> {
    let mut ctx = Context::new()?;
    ctx.eval_es_module(source)
}

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

// ===== ES Module Tests =====

#[test]
fn test_parse_es_module_export_const() {
    // Test that parse_es_module handles export const declarations
    let result = parse_es_module("export const foo = 42;");
    assert!(result.is_ok(), "Failed to parse: {:?}", result);
}

#[test]
fn test_parse_es_module_export_function() {
    // Test that parse_es_module handles export function declarations
    let result = parse_es_module("export function bar() { return 42; }");
    assert!(result.is_ok(), "Failed to parse: {:?}", result);
}

#[test]
fn test_parse_es_module_export_default_expr() {
    // Test that parse_es_module handles export default expression
    let result = parse_es_module("export default 42;");
    assert!(result.is_ok(), "Failed to parse: {:?}", result);
}

#[test]
fn test_parse_es_module_export_named() {
    // Test that parse_es_module handles export { foo, bar }
    let result = parse_es_module("const foo = 1; const bar = 2; export { foo, bar };");
    assert!(result.is_ok(), "Failed to parse: {:?}", result);
}

#[test]
fn test_parse_es_module_export_default_function() {
    // Test that parse_es_module handles export default function
    let result = parse_es_module("export default function() { return 42; }");
    assert!(result.is_ok(), "Failed to parse: {:?}", result);
}

#[test]
fn test_parse_es_module_import_stripped() {
    // Test that parse_es_module strips import statements
    let result = parse_es_module("import foo from 'bar'; const x = 42; export { x };");
    assert!(result.is_ok(), "Failed to parse: {:?}", result);
}

#[test]
fn test_parse_es_module_export_star_as() {
    // Test that parse_es_module handles export * as ns from 'module'
    // This should be skipped (not supported)
    let result = parse_es_module("export * as ns from 'module';");
    assert!(result.is_ok(), "Failed to parse: {:?}", result);
}

#[test]
fn test_eval_es_module_export() {
    // Test evaluating ES module code with exports
    let result = eval_es_module("const foo = 42; export { foo };");
    assert!(result.is_ok(), "Failed to eval: {:?}", result);
}

#[test]
fn test_eval_es_module_export_default() {
    // Test evaluating ES module code with export default
    let result = eval_es_module("export default 42;");
    assert!(result.is_ok(), "Failed to eval: {:?}", result);
}
