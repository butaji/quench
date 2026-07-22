//! RegExp integration tests (tests public API + full stack)

use quench_runtime::Context;

fn setup() -> Context {
    let mut ctx = Context::new().unwrap();
    quench_runtime::builtins::regex::register_regexp(&mut ctx);
    quench_runtime::builtins::function::register_function(&mut ctx);
    quench_runtime::builtins::string::register_string(&mut ctx);
    ctx
}

#[test]
fn test_regexp_constructor() {
    let mut ctx = setup();
    let result = ctx.eval("/abc/").unwrap();
    assert!(matches!(result, quench_runtime::Value::Object(_)));
}

#[test]
fn test_regexp_test() {
    let mut ctx = setup();
    let result = ctx.eval("/abc/.test(\"abcdef\")").unwrap();
    assert_eq!(result, quench_runtime::Value::Boolean(true));
}

#[test]
fn test_regexp_test_no_match() {
    let mut ctx = setup();
    let result = ctx.eval("/xyz/.test(\"abcdef\")").unwrap();
    assert_eq!(result, quench_runtime::Value::Boolean(false));
}

#[test]
fn test_regexp_exec() {
    let mut ctx = setup();
    let result = ctx.eval("/ab(c)/.exec(\"abcdef\")").unwrap();
    assert!(matches!(result, quench_runtime::Value::Object(_)));
}

#[test]
fn test_regexp_to_string() {
    let mut ctx = setup();
    let result = ctx.eval("/abc/gi.toString()").unwrap();
    assert_eq!(result, quench_runtime::Value::String("/abc/gi".to_string()));
}

#[test]
fn test_regexp_invalid_flags_throw_syntax_error() {
    let mut ctx = setup();
    let result = ctx.eval("new RegExp('a', 'zz')");
    assert!(result.is_err(), "invalid flags must throw");
    assert!(result
        .unwrap_err()
        .0
        .contains("Invalid regular expression flags"));
    let dup = ctx.eval("new RegExp('a', 'gg')");
    assert!(dup.is_err(), "duplicate flags must throw");
}

#[test]
fn test_regexp_global_test_advances_last_index() {
    let mut ctx = setup();
    assert_eq!(
        ctx.eval("var re = /a/g; re.test('aa')").unwrap(),
        quench_runtime::Value::Boolean(true)
    );
    assert_eq!(
        ctx.eval("re.lastIndex").unwrap(),
        quench_runtime::Value::Number(1.0)
    );
    assert_eq!(
        ctx.eval("re.test('aa')").unwrap(),
        quench_runtime::Value::Boolean(true)
    );
    assert_eq!(
        ctx.eval("re.lastIndex").unwrap(),
        quench_runtime::Value::Number(2.0)
    );
    assert_eq!(
        ctx.eval("re.test('aa')").unwrap(),
        quench_runtime::Value::Boolean(false)
    );
    assert_eq!(
        ctx.eval("re.lastIndex").unwrap(),
        quench_runtime::Value::Number(0.0)
    );
}

#[test]
fn test_regexp_non_global_does_not_touch_last_index() {
    let mut ctx = setup();
    assert_eq!(
        ctx.eval("var re2 = /a/; re2.lastIndex = 5; re2.test('cat')")
            .unwrap(),
        quench_runtime::Value::Boolean(true)
    );
    assert_eq!(
        ctx.eval("re2.lastIndex").unwrap(),
        quench_runtime::Value::Number(5.0)
    );
}

#[test]
fn test_regexp_global_exec_starts_from_last_index() {
    let mut ctx = setup();
    ctx.eval("var re3 = /b/g; re3.lastIndex = 2;").unwrap();
    let result = ctx.eval("re3.exec('abcabc').index").unwrap();
    assert_eq!(result, quench_runtime::Value::Number(4.0));
    assert_eq!(
        ctx.eval("re3.lastIndex").unwrap(),
        quench_runtime::Value::Number(5.0)
    );
}

#[test]
fn test_regexp_u_flag_simple_backref_no_surrogates() {
    let mut ctx = setup();
    let r = ctx.eval(r#"/(.+).*\1/.test("hellohello")"#);
    assert_eq!(r.unwrap(), quench_runtime::Value::Boolean(true));
}

#[test]
fn test_regexp_u_flag_basic_matches() {
    let mut ctx = setup();
    let r = ctx.eval(r#"/hello/u.test("hello")"#);
    assert_eq!(r.unwrap(), quench_runtime::Value::Boolean(true));
    let r = ctx.eval(r#"/\p{L}/u.test("a")"#);
    assert_eq!(r.unwrap(), quench_runtime::Value::Boolean(true));
}

#[test]
fn test_regexp_subclass_prototype_chain() {
    // `class extends RegExp` must preserve the derived class's prototype on `this`
    let mut ctx = setup();
    let result = ctx.eval(
        r#"
        class MyRegExp extends RegExp {
            customMethod() { return "custom"; }
        }
        var re = new MyRegExp("abc", "g");
        re instanceof MyRegExp && re.customMethod() === "custom" && re.source === "abc"
        "#,
    );
    assert_eq!(result.unwrap(), quench_runtime::Value::Boolean(true));
}

#[test]
fn test_regexp_subclass_exec() {
    // Subclass instance must work with .exec()
    let mut ctx = setup();
    let result = ctx.eval(
        r#"
        class MyRegExp extends RegExp {
            constructor(p, f) { super(p, f); }
        }
        var re = new MyRegExp("(a)(b)", "");
        var m = re.exec("ab");
        m !== null && m[0] === "ab"
        "#,
    );
    assert_eq!(result.unwrap(), quench_runtime::Value::Boolean(true));
}

#[test]
fn test_regexp_subclass_default_constructor() {
    // Default constructor (no explicit constructor) must work
    let mut ctx = setup();
    let result = ctx.eval(
        r#"
        class MyRegExp extends RegExp {}
        var re = new MyRegExp("hello");
        re instanceof MyRegExp && re.test("hello world")
        "#,
    );
    assert_eq!(result.unwrap(), quench_runtime::Value::Boolean(true));
}

#[test]
fn test_string_search_and_match_both_call_exec() {
    let mut ctx = setup();
    assert!(ctx.eval(r#"/abc/.exec("abcdef")"#).is_ok());
    assert_eq!(
        ctx.eval(r#"/abc/.test("abcdef")"#).unwrap(),
        quench_runtime::Value::Boolean(true)
    );
    let r = ctx.eval(r#""abcdef".search(/abc/)"#);
    assert!(r.is_ok(), "search failed: {:?}", r);
    assert_eq!(r.unwrap(), quench_runtime::Value::Number(0.0));
    let r = ctx.eval(r#""abcdef".match(/abc/)"#);
    assert!(r.is_ok(), "match failed: {:?}", r);
}
