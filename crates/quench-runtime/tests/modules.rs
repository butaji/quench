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

#[test]
fn test_eval_es_module_export_const() {
    // Test evaluating ES module code with export const
    let result = eval_es_module("export const x = 1;");
    assert!(result.is_ok(), "Failed to eval: {:?}", result);
}

#[test]
fn test_eval_es_module_export_named() {
    // Test evaluating ES module code with named exports
    let result = eval_es_module("const a = 1; const b = 2; export { a, b };");
    assert!(result.is_ok(), "Failed to eval: {:?}", result);
}

#[test]
fn test_eval_es_module_export_function() {
    // Test evaluating ES module code with export function
    let result = eval_es_module("export function add(x, y) { return x + y; }");
    assert!(result.is_ok(), "Failed to eval: {:?}", result);
}

#[test]
fn test_eval_es_module_export_default_function() {
    // Test evaluating ES module code with export default function
    let result = eval_es_module("export default function() { return 42; }");
    assert!(result.is_ok(), "Failed to eval: {:?}", result);
}

#[test]
fn test_eval_es_module_export_default_named_function() {
    // Test evaluating ES module code with export default function name()
    let result = eval_es_module("export default function myFunc() { return 42; }");
    assert!(result.is_ok(), "Failed to eval: {:?}", result);
}

#[test]
fn test_eval_es_module_export_class() {
    // Test evaluating ES module code with export class
    let result = eval_es_module("export class Foo { };");
    assert!(result.is_ok(), "Failed to eval: {:?}", result);
}

#[test]
fn test_eval_es_module_export_default_class() {
    // Test evaluating ES module code with export default class
    let result = eval_es_module("export default class { };");
    assert!(result.is_ok(), "Failed to eval: {:?}", result);
}

#[test]
fn test_eval_es_module_import_named() {
    // Test evaluating ES module code with named imports
    // This test requires the module to be registered first
    let mut ctx = Context::new().unwrap();
    
    // Register a mock module with exports
    let mut mod_exports = quench_runtime::value::Object::new(quench_runtime::value::ObjectKind::Ordinary);
    mod_exports.set("foo", quench_runtime::Value::Number(42.0));
    ctx.register_module("./mod.js", mod_exports);
    
    // Now try to evaluate an import
    let result = ctx.eval_es_module("import { foo } from './mod.js'; foo;");
    assert!(result.is_ok(), "Failed to eval import: {:?}", result);
}

#[test]
fn test_eval_es_module_import_default() {
    // Test evaluating ES module code with default import
    let mut ctx = Context::new().unwrap();
    
    // Register a mock module with default export
    let mut mod_exports = quench_runtime::value::Object::new(quench_runtime::value::ObjectKind::Ordinary);
    mod_exports.set("default", quench_runtime::Value::Number(42.0));
    ctx.register_module("./mod.js", mod_exports);
    
    // Now try to evaluate an import
    let result = ctx.eval_es_module("import myMod from './mod.js'; myMod;");
    assert!(result.is_ok(), "Failed to eval import: {:?}", result);
}

#[test]
fn test_eval_es_module_import_namespace() {
    // Test evaluating ES module code with namespace import
    let mut ctx = Context::new().unwrap();
    
    // Register a mock module with exports
    let mut mod_exports = quench_runtime::value::Object::new(quench_runtime::value::ObjectKind::Ordinary);
    mod_exports.set("foo", quench_runtime::Value::Number(42.0));
    mod_exports.set("bar", quench_runtime::Value::String("hello".to_string()));
    ctx.register_module("./mod.js", mod_exports);
    
    // Now try to evaluate a namespace import
    let result = ctx.eval_es_module("import * as mod from './mod.js'; mod.foo;");
    assert!(result.is_ok(), "Failed to eval import: {:?}", result);
}

#[test]
fn test_eval_es_module_export_from() {
    // Test evaluating ES module code with export { } from syntax
    // This requires a mock module to be registered
    let mut ctx = Context::new().unwrap();
    
    // Register a mock module
    let mut mod_exports = quench_runtime::value::Object::new(quench_runtime::value::ObjectKind::Ordinary);
    mod_exports.set("foo", quench_runtime::Value::Number(42.0));
    ctx.register_module("./source.js", mod_exports);
    
    // Now try to evaluate an export from
    let result = ctx.eval_es_module("export { foo } from './source.js';");
    assert!(result.is_ok(), "Failed to eval export from: {:?}", result);
}

#[test]
fn test_eval_es_module_export_star() {
    // Test evaluating ES module code with export * from syntax
    // This requires a mock module to be registered
    let mut ctx = Context::new().unwrap();
    
    // Register a mock module with exports
    let mut mod_exports = quench_runtime::value::Object::new(quench_runtime::value::ObjectKind::Ordinary);
    mod_exports.set("foo", quench_runtime::Value::Number(42.0));
    mod_exports.set("bar", quench_runtime::Value::String("hello".to_string()));
    ctx.register_module("./source.js", mod_exports);
    
    // Now try to evaluate an export star
    let result = ctx.eval_es_module("export * from './source.js';");
    assert!(result.is_ok(), "Failed to eval export star: {:?}", result);
}

#[test]
fn test_eval_es_module_import_renamed() {
    // Test evaluating ES module code with renamed imports
    let mut ctx = Context::new().unwrap();
    
    // Register a mock module with exports
    let mut mod_exports = quench_runtime::value::Object::new(quench_runtime::value::ObjectKind::Ordinary);
    mod_exports.set("foo", quench_runtime::Value::Number(42.0));
    ctx.register_module("./mod.js", mod_exports);
    
    // Now try to evaluate a renamed import
    let result = ctx.eval_es_module("import { foo as bar } from './mod.js'; bar;");
    assert!(result.is_ok(), "Failed to eval renamed import: {:?}", result);
}

#[test]
fn test_eval_es_module_export_renamed() {
    // Test evaluating ES module code with renamed exports
    let result = eval_es_module("const foo = 42; export { foo as bar };");
    assert!(result.is_ok(), "Failed to eval renamed export: {:?}", result);
}

#[test]
fn test_quench_modules_cache_exists() {
    // Test that __quench_modules__ cache is initialized
    let ctx = Context::new().unwrap();
    let cache = ctx.get_global("__quench_modules__");
    assert!(cache.is_some(), "__quench_modules__ should be defined");
}

#[test]
fn test_register_module() {
    // Test registering a module for import resolution
    let mut ctx = Context::new().unwrap();
    
    let mut mod_exports = quench_runtime::value::Object::new(quench_runtime::value::ObjectKind::Ordinary);
    mod_exports.set("test", quench_runtime::Value::Number(123.0));
    ctx.register_module("./test.js", mod_exports);
    
    // Verify the module was registered
    let module = ctx.get_module("./test.js");
    assert!(module.is_some(), "Module should be registered");
}

#[test]
fn test_module_integration_named_export_and_import() {
    // Integration test: Register a module, then import from it
    let mut ctx = Context::new().unwrap();
    
    // Create a module with multiple exports
    let mut mod_exports = quench_runtime::value::Object::new(quench_runtime::value::ObjectKind::Ordinary);
    mod_exports.set("add", quench_runtime::Value::Number(1.0));  // placeholder
    mod_exports.set("name", quench_runtime::Value::String("test-module".to_string()));
    ctx.register_module("./utils.js", mod_exports);
    
    // Evaluate a script that uses the module
    let result = ctx.eval_es_module(r#"
        import { add, name } from './utils.js';
        export { add, name };
    "#);
    
    assert!(result.is_ok(), "Integration test failed: {:?}", result);
}

#[test]
fn test_module_integration_default_and_named() {
    // Integration test: Module with both default and named exports
    let mut ctx = Context::new().unwrap();
    
    // Create a module with default and named exports
    let mut mod_exports = quench_runtime::value::Object::new(quench_runtime::value::ObjectKind::Ordinary);
    mod_exports.set("default", quench_runtime::Value::Number(42.0));
    mod_exports.set("value", quench_runtime::Value::String("hello".to_string()));
    ctx.register_module("./lib.js", mod_exports);
    
    // Evaluate a script that imports both
    let result = ctx.eval_es_module(r#"
        import defaultExport, { value } from './lib.js';
        export { defaultExport, value };
    "#);
    
    assert!(result.is_ok(), "Integration test failed: {:?}", result);
}

#[test]
fn test_parse_export_all() {
    // Test parsing export * from 'module'
    let result = parse_es_module("export * from './other.js';");
    assert!(result.is_ok(), "Failed to parse export *: {:?}", result);
}

#[test]
fn test_parse_import_combined() {
    // Test parsing import with both default and named specifiers
    let result = parse_es_module("import defaultExport, { named1, named2 } from './mod.js';");
    assert!(result.is_ok(), "Failed to parse combined import: {:?}", result);
}

#[test]
fn test_parse_import_with_alias() {
    // Test parsing import with alias: import { foo as bar }
    let result = parse_es_module("import { foo as bar } from './mod.js';");
    assert!(result.is_ok(), "Failed to parse aliased import: {:?}", result);
}

#[test]
fn test_parse_export_with_alias() {
    // Test parsing export with alias: export { foo as bar }
    let result = parse_es_module("export { foo as bar };");
    assert!(result.is_ok(), "Failed to parse aliased export: {:?}", result);
}
