//! Tests for the runtime context.

#[cfg(test)]
use super::*;
#[cfg(test)]
use crate::interpreter;
#[cfg(test)]
use crate::stack_machine;

#[cfg(test)]
#[test]
fn test_context_creation() {
    let ctx = Context::new();
    assert!(ctx.is_ok());
}

#[cfg(test)]
#[test]
fn test_globals() {
    let mut ctx = Context::new().unwrap();
    ctx.set_global("test".to_string(), Value::Number(42.0));
    assert_eq!(ctx.get_global("test"), Some(Value::Number(42.0)));
}

#[cfg(test)]
#[test]
fn test_eval_simple() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval("1 + 2");
    assert!(result.is_ok());
    if let Ok(v) = result {
        assert_eq!(v, Value::Number(3.0));
    }
}

#[cfg(test)]
#[test]
fn test_console_exists() {
    let ctx = Context::new().unwrap();
    let console = ctx.get_global("console");
    assert!(console.is_some());
}

#[cfg(test)]
#[test]
fn test_global_this_assignment() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval("typeof globalThis");
    assert!(result.is_ok(), "typeof globalThis failed: {:?}", result);
    assert_eq!(result.unwrap(), Value::String("object".to_string()));

    let result = ctx.eval("globalThis.testProp = 42");
    assert!(result.is_ok(), "globalThis assignment failed: {:?}", result);

    let result = ctx.eval("globalThis.testProp");
    assert!(result.is_ok(), "globalThis read failed: {:?}", result);
    assert_eq!(result.unwrap(), Value::Number(42.0));
}

#[cfg(test)]
#[test]
fn test_date_prototype_access() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval("Date.prototype");
    assert!(result.is_ok(), "Date.prototype failed: {:?}", result);

    let result = ctx.eval("Date.prototype.toLocaleTimeString");
    assert!(result.is_ok(), "Date.prototype.toLocaleTimeString failed: {:?}", result);
}

#[cfg(test)]
#[test]
fn test_deep_recursion_does_not_stack_overflow() {
    interpreter::reset_depth();
    let ctx = Context::new().unwrap();
    let source = r#"
            function recurse(n) {
                if (n <= 0) return 0;
                return 1 + recurse(n - 1);
            }
            recurse(100);
        "#;
    let program = ctx.parse(source).unwrap();
    let mut env = std::rc::Rc::clone(ctx.env());
    let result = stack_machine::eval_program(&program, &mut env);
    assert!(result.is_ok(), "deep recursion should not stack overflow: {:?}", result);
    assert_eq!(result.unwrap(), Value::Number(100.0));
}

#[cfg(test)]
#[test]
fn test_function_declaration_overrides_existing_global() {
    let mut ctx = Context::new().unwrap();
    ctx.eval("function mountTree() { return 'runtime'; }").unwrap();
    let result = ctx.eval(r#"
            function mountTree() { return 'user'; }
            mountTree();
        "#).unwrap();
    assert_eq!(result, Value::String("user".to_string()));
}

#[cfg(test)]
#[test]
fn test_duplicate_function_declaration_last_wins() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval(r#"
            function f() { return 1; }
            function f() { return 2; }
            f();
        "#).unwrap();
    assert_eq!(result, Value::Number(2.0));
}

#[cfg(test)]
#[test]
fn test_eval_shadow_simple_add() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval_shadow("1 + 2", crate::shadow::ModuleMode::Static).unwrap();
    assert_eq!(result, Value::Number(3.0));
}

#[cfg(test)]
#[test]
fn test_eval_shadow_var() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval_shadow("var x = 5; x + x", crate::shadow::ModuleMode::Static).unwrap();
    assert_eq!(result, Value::Number(10.0));
}

#[cfg(test)]
#[test]
fn test_eval_shadow_object_prop() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval_shadow("var o = {a: 3}; o.a", crate::shadow::ModuleMode::Static).unwrap();
    assert_eq!(result, Value::Number(3.0));
}

#[cfg(test)]
#[test]
fn test_runtime_ink_object() {
    let mut ctx = Context::new().unwrap();
    let runtime_path = std::path::Path::new(
        "/Users/admin/Code/GitHub/quench/src/runtime.js"
    );
    ctx.load_runtime_from(runtime_path).unwrap();

    let result = ctx.eval("typeof ink");
    assert!(result.is_ok(), "typeof ink failed: {:?}", result);
    assert_eq!(result.unwrap(), Value::String("object".to_string()));

    let result = ctx.eval("typeof ink.createElement");
    assert!(result.is_ok(), "typeof ink.createElement failed: {:?}", result);
    assert_eq!(result.unwrap(), Value::String("function".to_string()));

    let result = ctx.eval("typeof ink.render");
    assert!(result.is_ok(), "typeof ink.render failed: {:?}", result);
    assert_eq!(result.unwrap(), Value::String("function".to_string()));

    let result = ctx.eval("ink.Box");
    assert!(result.is_ok(), "ink.Box failed: {:?}", result);
    assert_eq!(result.unwrap(), Value::String("ink-box".to_string()));

    let result = ctx.eval("ink.Text");
    assert!(result.is_ok(), "ink.Text failed: {:?}", result);
    assert_eq!(result.unwrap(), Value::String("ink-text".to_string()));
}

#[cfg(test)]
#[test]
fn test_function_call_and_apply() {
    let mut ctx = Context::new().unwrap();
    let runtime_path = std::path::Path::new(
        "/Users/admin/Code/GitHub/quench/src/runtime.js"
    );
    ctx.load_runtime_from(runtime_path).unwrap();

    let result = ctx.eval(r#"
            const obj = { x: 42 };
            const test = function() { return this.x; };
            test.call(obj);
        "#);
    assert_eq!(result.unwrap(), Value::Number(42.0));

    let result = ctx.eval(r#"
            const test = function() { return 42; };
            test.call();
        "#);
    assert_eq!(result.unwrap(), Value::Number(42.0));

    let result = ctx.eval(r#"
            const obj = { x: 100 };
            const test = function() { return this.x; };
            test.apply(obj);
        "#);
    assert_eq!(result.unwrap(), Value::Number(100.0));
}

#[cfg(test)]
#[test]
fn test_component_instance_render() {
    let mut ctx = Context::new().unwrap();
    let runtime_path = std::path::Path::new(
        "/Users/admin/Code/GitHub/quench/src/runtime.js"
    );

    ctx.load_runtime_from(runtime_path).unwrap();

    let result = ctx.eval("typeof ComponentInstance.prototype");
    println!("prototype type: {:?}", result);

    let result = ctx.eval_stack_machine("ComponentInstance.prototype.testProp = 42");
    println!("stack machine set result: {:?}", result);

    let result = ctx.eval("ComponentInstance.prototype.testProp");
    println!("testProp value: {:?}", result);
}
