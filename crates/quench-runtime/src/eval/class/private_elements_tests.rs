use super::*;
use crate::ast::Program;
use crate::Context;

fn eval(src: &str) -> Result<crate::value::Value, JsError> {
    Context::new().unwrap().eval(src)
}

fn is_syntax_error(err: &JsError) -> bool {
    err.0.contains("SyntaxError")
}

#[test]
fn undeclared_private_in_field_eval_throws_syntax_error() {
    let err = eval("class C { y = eval(\"executed = true; this.#x;\"); } new C();").unwrap_err();
    assert!(is_syntax_error(&err), "got {}", err.0);
}

#[test]
fn undeclared_private_in_top_level_eval_throws_syntax_error() {
    let err = eval("class C { #x; } eval(\"new C().#x\");").unwrap_err();
    assert!(is_syntax_error(&err), "got {}", err.0);
}

#[test]
fn direct_eval_super_property_allowed_in_derived_field() {
    eval("class A {} class C extends A { x = eval('super.x'); } new C();").unwrap();
}

#[test]
fn direct_eval_super_call_in_class_field_throws_syntax_error() {
    let err = eval("class A {} class C extends A { x = eval('super();'); } new C();").unwrap_err();
    assert!(is_syntax_error(&err));
}

#[test]
fn indirect_eval_super_call_in_class_field_throws_syntax_error() {
    let err =
        eval("class A {} class C extends A { x = (0, eval)('super();'); } new C();").unwrap_err();
    assert!(is_syntax_error(&err));
}

#[test]
fn indirect_eval_super_property_in_class_field_throws_syntax_error() {
    let err =
        eval("class A {} class C extends A { x = (0, eval)('super.x'); } new C();").unwrap_err();
    assert!(is_syntax_error(&err));
}

#[test]
fn direct_eval_arguments_in_class_field_throws_syntax_error() {
    let err = eval("class C { x = eval('arguments'); } new C();").unwrap_err();
    assert!(is_syntax_error(&err));
}

#[test]
fn nested_direct_eval_arguments_in_class_field_throws_syntax_error() {
    let err = eval(
        "class C { x = () => { var t = () => { eval('arguments'); }; t(); } } \
         new C().x();",
    )
    .unwrap_err();
    assert!(is_syntax_error(&err), "got {}", err.0);
}

#[test]
fn nested_indirect_eval_arguments_in_class_field_sees_outer_var() {
    let r = eval(
        "var arguments = 1; \
         class C { \
           x = () => { var t = () => (0, eval)('arguments;'); return t(); }; \
         } \
         new C().x()",
    )
    .unwrap();
    assert_eq!(r, crate::value::Value::Number(1.0));
}

#[test]
fn indirect_eval_new_target_in_class_field_throws_syntax_error() {
    let err = eval("class C { x = (0, eval)('new.target'); } new C();").unwrap_err();
    assert!(is_syntax_error(&err));
}

#[test]
fn super_property_is_not_super_call() {
    let program = Context::new().unwrap().parse("super.x;").unwrap();
    let Program::Script(body) = program;
    assert!(!program_contains_super_call(&body));
    assert!(program_contains_super_property(&body));
}

#[test]
fn super_call_in_arrow_within_eval_is_detected() {
    let program = Context::new().unwrap().parse("() => super();").unwrap();
    let Program::Script(body) = program;
    assert!(program_contains_super_call(&body));
}

#[test]
fn private_getter_setter_pair_on_same_name() {
    let src = "var s; class C { get #x() { return 'get'; } set #x(v) { s = v; } \
               getRef() { return this.#x; } setRef(v) { this.#x = v; } } \
               var c = new C(); c.getRef() === 'get' && (c.setRef('set'), s === 'set')";
    let r = eval(src).unwrap();
    assert_eq!(r, crate::value::Value::Boolean(true));
}

#[test]
fn subclass_and_superclass_same_private_bare_name() {
    let src = "class S { #m() { return 'super'; } superAccess() { return this.#m(); } } \
               class C extends S { #m() { return 'sub'; } access() { return this.#m(); } } \
               var c = new C(); c.access() === 'sub' && c.superAccess() === 'super'";
    let r = eval(src).unwrap();
    assert_eq!(r, crate::value::Value::Boolean(true));
}

#[test]
fn private_accessor_double_install_on_same_object_throws() {
    let err = eval(
        "class Base { constructor(o) { return o; } } \
         class C extends Base { get #p() {} set #p(x) {} } \
         var obj = {}; new C(obj); \
         try { new C(obj); 'ok'; } catch (e) { e.constructor.name; }",
    )
    .unwrap();
    assert_eq!(err, crate::value::Value::String("TypeError".into()));
}
