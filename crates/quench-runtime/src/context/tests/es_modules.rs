//! ES Module tests

#![allow(clippy::too_many_lines, clippy::complexity)]

#[cfg(test)]
use crate::Context;

#[cfg(test)]
#[test]
fn test_es_module_basic() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval_es_module(
        r#"
        export const x = 42;
        export function getX() { return x; }
    "#,
    );
    assert!(result.is_ok(), "basic ES module failed: {:?}", result);
}

#[cfg(test)]
#[test]
fn test_es_module_default_export() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval_es_module(
        r#"
        export default function() { return 42; }
    "#,
    );
    assert!(result.is_ok(), "default export failed: {:?}", result);
}

#[cfg(test)]
#[test]
fn imported_binding_assignment_throws_type_error() {
    let mut ctx = Context::new().unwrap();
    let mut module = crate::value::Object::new(crate::value::ObjectKind::ModuleNamespace);
    module.define(
        "x",
        crate::value::Value::Number(1.0),
        crate::value::PropertyFlags {
            value: None,
            writable: false,
            enumerable: true,
            configurable: false,
        },
    );
    ctx.register_module("module-name", module);
    let result = ctx.eval_es_module(
        "import { x } from 'module-name'; try { x = 2; } catch (error) { globalThis.result = error.name; }",
    );
    assert!(
        result.is_ok(),
        "import assignment should be caught: {result:?}"
    );
    assert_eq!(
        ctx.eval("result").unwrap(),
        crate::value::Value::String("TypeError".into())
    );
}

#[test]
fn module_exported_let_is_in_tdz_before_initialization() {
    let mut ctx = Context::new().unwrap();
    let parsed =
        crate::parser::parse_es_module("typeof test262; export let test262 = 23;").unwrap();
    let crate::ast::Program::Script(statements) = parsed;
    assert!(
        crate::interpreter::collect_let_const_declarations(&statements)
            .iter()
            .any(|(name, _)| name == "test262")
    );
    let result = ctx.eval_es_module("typeof test262; export let test262 = 23;");
    assert!(result.is_err());
}

#[test]
fn module_import_meta_is_an_object() {
    let mut ctx = Context::new().unwrap();
    let result = ctx
        .eval_es_module("typeof import.meta === 'object' && import.meta === import.meta")
        .unwrap();
    assert_eq!(result, crate::Value::Boolean(true));
}

#[test]
fn top_level_for_await_accepts_awaited_array_expression() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval_es_module(
        "var binding;\
        for await (binding of [await []]) { await []; break; }\
        for await (var binding of [await []]) { await []; break; }\
        for await (let binding of [await []]) { await []; break; }",
    );
    assert!(result.is_ok(), "for-await module failed: {:?}", result);
}

#[test]
fn module_exported_function_is_initialized_before_module_body() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval_es_module("typeof test262; export function test262() {};");
    assert_eq!(result, Ok(crate::Value::String("function".to_string())));
}

#[test]
fn named_import_reads_live_export_accessor_after_initialization() {
    use std::rc::Rc;

    let mut ctx = Context::new().unwrap();
    let mut module = crate::value::Object::new(crate::value::ObjectKind::ModuleNamespace);
    let env = Rc::clone(ctx.environment_view());
    let getter =
        crate::Value::NativeFunction(Rc::new(crate::value::NativeFunction::new(move |_| {
            Ok(env.borrow().get("A").unwrap_or(crate::Value::Undefined))
        })));
    module.define_accessor(
        "A",
        Some(getter),
        None,
        crate::value::PropertyFlags {
            value: None,
            writable: false,
            enumerable: true,
            configurable: false,
        },
    );
    ctx.register_module("./dep.js", module);
    let result = ctx.eval_es_module(
        "import { A as B } from './dep.js'; B(); export function A() { return 77; }",
    );
    assert_eq!(result, Ok(crate::Value::Number(77.0)));
}

#[test]
fn async_function_instances_have_no_prototype_property() {
    let mut ctx = Context::new().unwrap();
    let result = ctx
        .eval("async function foo() {}; foo.prototype === undefined && !foo.hasOwnProperty('prototype')")
        .unwrap();
    assert_eq!(result, crate::Value::Boolean(true));
}

#[test]
fn async_function_constructor_inherits_from_function() {
    let mut ctx = Context::new().unwrap();
    let result = ctx
        .eval("async function foo() {}; var AsyncFunction = foo.constructor; Object.getPrototypeOf(AsyncFunction) === Function")
        .unwrap();
    assert_eq!(result, crate::Value::Boolean(true));
}

#[test]
fn top_level_await_using_initializes_module_binding() {
    let mut ctx = Context::new().unwrap();
    let result = ctx
        .eval_es_module("await using resource = null; resource")
        .unwrap();
    assert_eq!(result, crate::Value::Null);
}

#[test]
fn module_await_using_block_shadow_does_not_clear_outer_binding() {
    let mut ctx = Context::new().unwrap();
    let result = ctx
        .eval_es_module(
            "await using resource = null; { await using resource = undefined; } resource",
        )
        .unwrap();
    assert_eq!(result, crate::Value::Null);
}

#[test]
fn module_await_using_for_binding_shadows_outer_binding() {
    let mut ctx = Context::new().unwrap();
    let result = ctx
        .eval_es_module(
            "await using outer = null; var i = 0; for (await using inner = undefined; i < 1; i++) { outer } outer",
        )
        .unwrap();
    assert_eq!(result, crate::Value::Null);
}

#[test]
fn module_resolution_error_prevents_module_body_evaluation() {
    let mut ctx = Context::new().unwrap();
    let mut errors = crate::value::Object::new(crate::value::ObjectKind::Ordinary);
    errors.set(
        "./current.js",
        crate::Value::String("Ambiguous export".to_string()),
    );
    ctx.set_global(
        "__quench_module_errors__".to_string(),
        crate::Value::Object(std::rc::Rc::new(std::cell::RefCell::new(errors))),
    );
    ctx.set_global(
        "__quench_current_module__".to_string(),
        crate::Value::String("./current.js".to_string()),
    );
    let result = ctx.eval_es_module("throw new Error('body reached'); export const x = 1;");
    assert!(result.is_err());
    let message = format!("{:?}", result.unwrap_err());
    assert!(message.contains("Ambiguous export"), "{message}");
}

#[test]
fn module_import_resolution_error_is_thrown_before_body() {
    let mut ctx = Context::new().unwrap();
    let mut errors = crate::value::Object::new(crate::value::ObjectKind::Ordinary);
    errors.set(
        "./dep.js",
        crate::Value::String("Ambiguous export".to_string()),
    );
    ctx.set_global(
        "__quench_module_errors__".to_string(),
        crate::Value::Object(std::rc::Rc::new(std::cell::RefCell::new(errors))),
    );
    ctx.register_module(
        "./dep.js",
        crate::value::Object::new(crate::value::ObjectKind::ModuleNamespace),
    );
    let result = ctx.eval_es_module("import x from './dep.js'; throw new Error('body reached');");
    assert!(result.is_err());
    let message = format!("{:?}", result.unwrap_err());
    assert!(message.contains("Ambiguous export"), "{message}");
}

#[test]
fn module_import_rejects_unknown_import_attribute() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval_es_module("import './missing.js' with {custom: 'value'};");
    let error = result.expect_err("unknown import attributes must fail during linking");
    assert!(format!("{error:?}").contains("SyntaxError"));
}
