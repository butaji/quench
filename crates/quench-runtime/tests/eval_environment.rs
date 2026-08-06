use quench_runtime::{Context, Value};

#[test]
fn indirect_eval_var_conflicting_with_global_lexical_binding_throws_syntax_error() {
    let mut ctx = Context::new().unwrap();
    assert_eq!(
        ctx.eval("let x; try { (0, eval)('var x;'); false } catch (error) { error instanceof SyntaxError }"),
        Ok(Value::Boolean(true))
    );
}

#[test]
fn indirect_eval_var_ignores_lexical_bindings_in_nested_blocks() {
    let mut ctx = Context::new().unwrap();
    assert_eq!(
        ctx.eval("{ let x; { (0, eval)('var x;'); } }"),
        Ok(Value::Undefined)
    );
}

#[test]
fn direct_eval_new_var_binding_is_unresolvable_after_deletion() {
    let mut ctx = Context::new().unwrap();
    assert_eq!(
        ctx.eval("var initial = null; var postDeletion; (function() { eval('initial = x; delete x; postDeletion = function() { x; }; var x;'); }()); initial === undefined && (function() { try { postDeletion(); return false; } catch (error) { return error instanceof ReferenceError; } }())"),
        Ok(Value::Boolean(true))
    );
}

#[test]
fn direct_eval_new_function_binding_is_unresolvable_after_deletion() {
    let mut ctx = Context::new().unwrap();
    assert_eq!(
        ctx.eval("var initial, postDeletion; (function() { eval('initial = f; delete f; postDeletion = function() { f; }; function f() { return 33; }'); }()); typeof initial === 'function' && initial() === 33 && (function() { try { postDeletion(); return false; } catch (error) { return error instanceof ReferenceError; } }())"),
        Ok(Value::Boolean(true))
    );
}

#[test]
fn direct_eval_deleted_local_function_does_not_create_a_global_property() {
    let mut ctx = Context::new().unwrap();
    assert_eq!(
        ctx.eval("var result; (function() { eval('delete f; result = (\"f\" in globalThis); function f() {}'); }()); result"),
        Ok(Value::Boolean(false))
    );
}

#[test]
fn direct_eval_deleted_var_is_absent_from_the_retained_closure_environment() {
    let mut ctx = Context::new().unwrap();
    assert_eq!(
        ctx.eval("var postDeletion; (function() { eval('delete x; postDeletion = function() { x; }; var x;'); }());"),
        Ok(Value::Undefined)
    );
    let Some(Value::Function(post_deletion)) = ctx.get_global("postDeletion") else {
        panic!();
    };
    assert_eq!(ctx.get_global("x"), None);
    assert_eq!(post_deletion.closure.borrow().get("x"), None);
}

#[test]
fn nested_direct_eval_uses_the_enclosing_eval_variable_environment() {
    let mut ctx = Context::new().unwrap();
    assert_eq!(
        ctx.eval("var __10_4_2_1_5 = 'str'; function testcase() { var __10_4_2_1_5 = 'str1'; var r = eval(\"var __10_4_2_1_5 = 'str2'; eval(\\\"'str2' === __10_4_2_1_5\\\")\"); return r; } testcase()"),
        Ok(Value::Boolean(true))
    );
}

#[test]
fn nested_direct_eval_preserves_the_value_observed_by_the_harness_assertion() {
    let mut ctx = Context::new().unwrap();
    assert_eq!(
        ctx.eval("function assert(value) { if (!value) throw new Error('assertion failed'); } var __10_4_2_1_5 = 'str'; function testcase() { var __10_4_2_1_5 = 'str1'; var r = eval(\"var __10_4_2_1_5 = 'str2'; eval(\\\"'str2' === __10_4_2_1_5\\\")\"); assert(r); } testcase();"),
        Ok(Value::Undefined)
    );
}

#[test]
fn nested_direct_eval_handles_line_continuations_in_the_outer_source() {
    let mut ctx = Context::new().unwrap();
    assert_eq!(
        ctx.eval("var __10_4_2_1_5 = 'str'; function testcase() { var __10_4_2_1_5 = 'str1'; return eval(\"\\\nvar __10_4_2_1_5 = 'str2'; \\\neval(\\\"'str2' === __10_4_2_1_5\\\")\\\n\"); } testcase();"),
        Ok(Value::Boolean(true))
    );
}
