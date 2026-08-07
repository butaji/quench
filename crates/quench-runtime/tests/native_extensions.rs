//! Integration tests: native TS/TSX/JSX support, module system, and TypeScript interface.
//!
//! Run with: cargo test -p quench-runtime

use quench_runtime::value::to_js_string;
use quench_runtime::{parser, parser::parse_es_module, Context, Value};

// =============================================================================
// Native JS / TS / TSX / JSX parsing
// =============================================================================

#[test]
fn native_js_eval() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval("'hello' + ' world';");
    assert!(result.is_ok(), "Failed: {:?}", result);
    assert_eq!(to_js_string(&result.unwrap()), "hello world");
}

#[test]
fn native_jsx_basic() {
    let result = parser::parse_jsx("const el = <div>hello</div>;");
    assert!(result.is_ok(), "Failed to parse JSX: {:?}", result);
}

#[test]
fn native_tsx_with_children() {
    let result = parser::parse_jsx("const el = <Box><Text>hi</Text></Box>;");
    assert!(
        result.is_ok(),
        "Failed to parse JSX with children: {:?}",
        result
    );
}

#[test]
fn native_tsx_with_props() {
    let result = parser::parse_jsx("const el = <Box color=\"red\"><Text>hi</Text></Box>;");
    assert!(
        result.is_ok(),
        "Failed to parse JSX with props: {:?}",
        result
    );
}

#[test]
fn native_tsx_fragment() {
    let result = parser::parse_jsx("const el = <ink.Fragment><Box /><Box /></ink.Fragment>;");
    assert!(result.is_ok(), "Failed to parse JSX Fragment: {:?}", result);
}

#[test]
fn native_jsx_eval_basic() {
    let mut ctx = Context::new().unwrap();
    ctx.eval(
        r#"
        const ink = { createElement: function(type, props) {
            return { type, props: props || {} };
        }};
    "#,
    )
    .unwrap();
    let result = ctx.eval("const el = <div>hello</div>; el;");
    assert!(result.is_ok(), "Failed to evaluate JSX: {:?}", result);
    if let Value::Object(obj) = result.unwrap() {
        assert_eq!(
            obj.borrow().get("type"),
            Some(Value::String("div".to_string()))
        );
    }
}

#[test]
fn native_jsx_eval_with_props() {
    let mut ctx = Context::new().unwrap();
    ctx.eval(
        r#"
        const ink = { createElement: function(type, props) {
            return { type, props: props || {} };
        }};
    "#,
    )
    .unwrap();
    let result = ctx.eval(r#"const el = <div className="container">Hello</div>; el;"#);
    assert!(
        result.is_ok(),
        "Failed to evaluate JSX with props: {:?}",
        result
    );
    if let Value::Object(obj) = result.unwrap() {
        let props = obj.borrow().get("props");
        assert!(props.is_some());
        if let Some(Value::Object(props_obj)) = props {
            assert_eq!(
                props_obj.borrow().get("className"),
                Some(Value::String("container".to_string()))
            );
        }
    }
}

// =============================================================================
// TypeScript parsing
// =============================================================================

#[test]
fn test_typescript_interface_stripped() {
    let code = r#"
interface Metrics { cpu: number; memory: number; }
interface ProgressBarProps { label: string; value: number; max?: number; }
function ProgressBar(props: ProgressBarProps): JSX.Element { return <Box>test</Box>; }
"#;
    let result = parser::parse_typescript(code);
    assert!(result.is_ok(), "Parse failed: {:?}", result.err());
}

#[test]
fn test_compiler_handles_typescript() {
    let code = r#"
interface Metrics { cpu: number; }
function App(): JSX.Element { return <Box>Hello</Box>; }
render(<App />);
"#;
    let result = parser::parse_typescript(code);
    assert!(result.is_ok(), "Parse failed: {:?}", result.err());
}

// =============================================================================
// CommonJS module system
// =============================================================================

#[test]
fn commonjs_exports_assignment() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval("exports.foo = 42");
    assert!(result.is_ok());
    if let Some(Value::Object(obj)) = ctx.get_global("exports") {
        assert_eq!(obj.borrow().get("foo"), Some(Value::Number(42.0)));
    }
}

#[test]
fn commonjs_module_exports_assignment() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval("module.exports = { bar: 'hello' }");
    assert!(result.is_ok());
    if let Some(Value::Object(module)) = ctx.get_global("module") {
        if let Some(Value::Object(exp)) = module.borrow().get("exports") {
            assert_eq!(
                exp.borrow().get("bar"),
                Some(Value::String("hello".to_string()))
            );
        }
    }
}

// =============================================================================
// ES module parsing
// =============================================================================

#[test]
fn es_module_parse_export_const() {
    let result = parse_es_module("export const foo = 42;");
    assert!(result.is_ok(), "Failed to parse: {:?}", result);
}

#[test]
fn es_module_parse_export_function() {
    let result = parse_es_module("export function bar() { return 42; }");
    assert!(result.is_ok(), "Failed to parse: {:?}", result);
}

#[test]
fn es_module_parse_export_default_expr() {
    let result = parse_es_module("export default 42;");
    assert!(result.is_ok(), "Failed to parse: {:?}", result);
}

#[test]
fn es_module_parse_export_named() {
    let result = parse_es_module("const foo = 1; const bar = 2; export { foo, bar };");
    assert!(result.is_ok(), "Failed to parse: {:?}", result);
}

#[test]
fn es_module_parse_import_stripped() {
    let result = parse_es_module("import foo from 'bar'; const x = 42; export { x };");
    assert!(result.is_ok(), "Failed to parse: {:?}", result);
}

#[test]
fn es_module_parse_export_star_as() {
    let result = parse_es_module("export * as ns from 'module';");
    assert!(result.is_ok(), "Failed to parse: {:?}", result);
}

#[test]
fn es_module_parse_export_all() {
    let result = parse_es_module("export * from './other.js';");
    assert!(result.is_ok(), "Failed to parse export *: {:?}", result);
}

#[test]
fn es_module_parse_import_combined() {
    let result = parse_es_module("import defaultExport, { named1, named2 } from './mod.js';");
    assert!(
        result.is_ok(),
        "Failed to parse combined import: {:?}",
        result
    );
}

#[test]
fn es_module_parse_import_with_alias() {
    let result = parse_es_module("import { foo as bar } from './mod.js';");
    assert!(
        result.is_ok(),
        "Failed to parse aliased import: {:?}",
        result
    );
}

#[test]
fn es_module_parse_export_with_alias() {
    let result = parse_es_module("export { foo }; export { foo as bar };");
    assert!(
        result.is_ok(),
        "Failed to parse aliased export: {:?}",
        result
    );
}

// =============================================================================
// ES module evaluation
// =============================================================================

fn eval_es_module(source: &str) -> Result<Value, quench_runtime::JsError> {
    let mut ctx = Context::new()?;
    ctx.eval_es_module(source)
}

#[test]
fn es_module_eval_export() {
    let result = eval_es_module("const foo = 42; export { foo };");
    assert!(result.is_ok(), "Failed to eval: {:?}", result);
}

#[test]
fn es_module_eval_export_default() {
    let result = eval_es_module("export default 42;");
    assert!(result.is_ok(), "Failed to eval: {:?}", result);
}

#[test]
fn es_module_eval_export_const() {
    let result = eval_es_module("export const x = 1;");
    assert!(result.is_ok(), "Failed to eval: {:?}", result);
}

#[test]
fn es_module_eval_export_named() {
    let result = eval_es_module("const a = 1; const b = 2; export { a, b };");
    assert!(result.is_ok(), "Failed to eval: {:?}", result);
}

#[test]
fn es_module_eval_export_function() {
    let result = eval_es_module("export function add(x, y) { return x + y; }");
    assert!(result.is_ok(), "Failed to eval: {:?}", result);
}

#[test]
fn es_module_eval_export_class() {
    let result = eval_es_module("export class Foo { };");
    assert!(result.is_ok(), "Failed to eval: {:?}", result);
}

#[test]
fn es_module_eval_export_default_class() {
    let result = eval_es_module("export default class { };");
    assert!(result.is_ok(), "Failed to eval: {:?}", result);
}

#[test]
fn es_module_eval_import_named() {
    let mut ctx = Context::new().unwrap();
    let mut mod_exports =
        quench_runtime::value::Object::new(quench_runtime::value::ObjectKind::Ordinary);
    mod_exports.set("foo", quench_runtime::Value::Number(42.0));
    ctx.register_module("./mod.js", mod_exports);
    let result = ctx.eval_es_module("import { foo } from './mod.js'; foo;");
    assert!(result.is_ok(), "Failed to eval import: {:?}", result);
}

#[test]
fn es_module_eval_import_default() {
    let mut ctx = Context::new().unwrap();
    let mut mod_exports =
        quench_runtime::value::Object::new(quench_runtime::value::ObjectKind::Ordinary);
    mod_exports.set("default", quench_runtime::Value::Number(42.0));
    ctx.register_module("./mod.js", mod_exports);
    let result = ctx.eval_es_module("import myMod from './mod.js'; myMod;");
    assert!(result.is_ok(), "Failed to eval import: {:?}", result);
}

#[test]
fn es_module_eval_import_namespace() {
    let mut ctx = Context::new().unwrap();
    let mut mod_exports =
        quench_runtime::value::Object::new(quench_runtime::value::ObjectKind::Ordinary);
    mod_exports.set("foo", quench_runtime::Value::Number(42.0));
    mod_exports.set("bar", quench_runtime::Value::String("hello".to_string()));
    ctx.register_module("./mod.js", mod_exports);
    let result = ctx.eval_es_module("import * as mod from './mod.js'; mod.foo;");
    assert!(
        result.is_ok(),
        "Failed to eval namespace import: {:?}",
        result
    );
}

#[test]
fn es_module_eval_import_renamed() {
    let mut ctx = Context::new().unwrap();
    let mut mod_exports =
        quench_runtime::value::Object::new(quench_runtime::value::ObjectKind::Ordinary);
    mod_exports.set("foo", quench_runtime::Value::Number(42.0));
    ctx.register_module("./mod.js", mod_exports);
    let result = ctx.eval_es_module("import { foo as bar } from './mod.js'; bar;");
    assert!(
        result.is_ok(),
        "Failed to eval renamed import: {:?}",
        result
    );
}

#[test]
fn es_module_eval_export_from() {
    let mut ctx = Context::new().unwrap();
    let mut mod_exports =
        quench_runtime::value::Object::new(quench_runtime::value::ObjectKind::Ordinary);
    mod_exports.set("foo", quench_runtime::Value::Number(42.0));
    ctx.register_module("./source.js", mod_exports);
    let result = ctx.eval_es_module("export { foo } from './source.js';");
    assert!(result.is_ok(), "Failed to eval export from: {:?}", result);
}

#[test]
fn es_module_eval_export_star() {
    let mut ctx = Context::new().unwrap();
    let mut mod_exports =
        quench_runtime::value::Object::new(quench_runtime::value::ObjectKind::Ordinary);
    mod_exports.set("foo", quench_runtime::Value::Number(42.0));
    mod_exports.set("bar", quench_runtime::Value::String("hello".to_string()));
    ctx.register_module("./source.js", mod_exports);
    let result = ctx.eval_es_module("export * from './source.js';");
    assert!(result.is_ok(), "Failed to eval export star: {:?}", result);
}

#[test]
fn quench_modules_cache_exists() {
    let ctx = Context::new().unwrap();
    assert!(ctx.get_global("__quench_modules__").is_some());
}

#[test]
fn register_module() {
    let mut ctx = Context::new().unwrap();
    let mut mod_exports =
        quench_runtime::value::Object::new(quench_runtime::value::ObjectKind::Ordinary);
    mod_exports.set("test", quench_runtime::Value::Number(123.0));
    ctx.register_module("./test.js", mod_exports);
    assert!(ctx.get_module("./test.js").is_some());
}

#[test]
fn module_integration_named_export_and_import() {
    let mut ctx = Context::new().unwrap();
    let mut mod_exports =
        quench_runtime::value::Object::new(quench_runtime::value::ObjectKind::Ordinary);
    mod_exports.set("add", quench_runtime::Value::Number(1.0));
    mod_exports.set(
        "name",
        quench_runtime::Value::String("test-module".to_string()),
    );
    ctx.register_module("./utils.js", mod_exports);
    let result = ctx.eval_es_module(
        r#"
        import { add, name } from './utils.js';
        export { add, name };
    "#,
    );
    assert!(result.is_ok(), "Integration test failed: {:?}", result);
}

#[test]
fn module_integration_default_and_named() {
    let mut ctx = Context::new().unwrap();
    let mut mod_exports =
        quench_runtime::value::Object::new(quench_runtime::value::ObjectKind::Ordinary);
    mod_exports.set("default", quench_runtime::Value::Number(42.0));
    mod_exports.set("value", quench_runtime::Value::String("hello".to_string()));
    ctx.register_module("./lib.js", mod_exports);
    let result = ctx.eval_es_module(
        r#"
        import defaultExport, { value } from './lib.js';
        export { defaultExport, value };
    "#,
    );
    assert!(result.is_ok(), "Integration test failed: {:?}", result);
}
