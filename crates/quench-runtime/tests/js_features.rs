//! Tests for JavaScript and TypeScript features:
//! - TypeScript interface/type parsing
//! - JSX fragment handling
//! - Arrow function with block body
//! - Optional chaining (?.)
//!
//! Each test is named after the specific behavior it protects.

#[cfg(test)]
mod tests {
    use quench_runtime::{Context, Value};
    use quench_runtime::swc_parse;

    // =========================================================================
    // TypeScript interface/type parsing
    // =========================================================================

    #[test]
    fn test_typescript_interface_simple() {
        let code = r#"
interface Config {
    name: string;
    value: number;
}
const x: Config = { name: "test", value: 42 };
"#;
        let result = swc_parse::parse_typescript(code);
        assert!(result.is_ok(), "Interface parsing failed: {:?}", result.err());
    }

    #[test]
    fn test_typescript_interface_with_optional() {
        let code = r#"
interface Options {
    timeout?: number;
    callback?: () => void;
}
"#;
        let result = swc_parse::parse_typescript(code);
        assert!(result.is_ok(), "Optional field interface failed: {:?}", result.err());
    }

    #[test]
    fn test_typescript_type_alias() {
        let code = r#"
type ID = string | number;
type Callback = (x: number) => void;
type Result = { success: boolean; data: string };
"#;
        let result = swc_parse::parse_typescript(code);
        assert!(result.is_ok(), "Type alias parsing failed: {:?}", result.err());
    }

    #[test]
    fn test_typescript_generic_interface() {
        let code = r#"
interface Box<T> {
    value: T;
    get(): T;
}
"#;
        let result = swc_parse::parse_typescript(code);
        assert!(result.is_ok(), "Generic interface failed: {:?}", result.err());
    }

    #[test]
    fn test_typescript_interface_extends() {
        let code = r#"
interface Base {
    name: string;
}
interface Child extends Base {
    age: number;
}
"#;
        let result = swc_parse::parse_typescript(code);
        assert!(result.is_ok(), "Interface extends failed: {:?}", result.err());
    }

    // =========================================================================
    // JSX fragment handling
    // =========================================================================

    #[test]
    fn test_jsx_fragment_syntax() {
        let result = swc_parse::parse_jsx("<><Text>Hello</Text></>");
        assert!(result.is_ok(), "JSX fragment parsing failed: {:?}", result.err());
    }

    #[test]
    fn test_jsx_fragment_with_multiple_children() {
        let result = swc_parse::parse_jsx("<><Box /><Text>Text</Text><Box /></>");
        assert!(result.is_ok(), "JSX fragment with children failed: {:?}", result.err());
    }

    #[test]
    fn test_jsx_fragment_empty() {
        let result = swc_parse::parse_jsx("<></>");
        assert!(result.is_ok(), "Empty JSX fragment failed: {:?}", result.err());
    }

    #[test]
    fn test_jsx_fragment_nested() {
        let result = swc_parse::parse_jsx("<><><Text>nested</Text></></>");
        assert!(result.is_ok(), "Nested JSX fragment failed: {:?}", result.err());
    }

    #[test]
    fn test_jsx_fragment_with_expression() {
        let result = swc_parse::parse_jsx("<><Text>{1 + 1}</Text></>");
        assert!(result.is_ok(), "JSX fragment with expression failed: {:?}", result.err());
    }

    // =========================================================================
    // Arrow function with block body
    // =========================================================================

    #[test]
    fn test_arrow_function_block_body_returns_undefined() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("const f = () => { }; f();").unwrap();
        assert_eq!(result, Value::Undefined);
    }

    #[test]
    fn test_arrow_function_block_body_with_return() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("const f = () => { return 42; }; f()").unwrap();
        assert_eq!(result, Value::Number(42.0));
    }

    #[test]
    fn test_arrow_function_block_body_with_multiple_statements() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("const f = () => { const x = 1; const y = 2; return x + y; }; f()").unwrap();
        assert_eq!(result, Value::Number(3.0));
    }

    #[test]
    fn test_arrow_function_block_body_no_return_value() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("const f = () => { console.log('hello'); }; f()").unwrap();
        assert_eq!(result, Value::Undefined);
    }

    #[test]
    fn test_arrow_function_expression_body() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("const f = () => 42; f()").unwrap();
        assert_eq!(result, Value::Number(42.0));
    }

    #[test]
    fn test_arrow_function_with_params() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("const f = (a, b) => { return a + b; }; f(1, 2)").unwrap();
        assert_eq!(result, Value::Number(3.0));
    }

    // =========================================================================
    // Optional chaining (?.)
    // =========================================================================

    #[test]
    fn test_optional_chaining_property_access() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("const obj = { a: { b: 42 } }; obj?.a?.b").unwrap();
        assert_eq!(result, Value::Number(42.0));
    }

    #[test]
    fn test_optional_chaining_null_shortcircuit() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("const obj = null; obj?.a?.b").unwrap();
        assert_eq!(result, Value::Undefined);
    }

    #[test]
    fn test_optional_chaining_undefined_shortcircuit() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("const obj = undefined; obj?.a?.b").unwrap();
        assert_eq!(result, Value::Undefined);
    }

    #[test]
    fn test_optional_chaining_deep_chain() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("const obj = { a: { b: { c: { d: 42 } } } }; obj?.a?.b?.c?.d").unwrap();
        assert_eq!(result, Value::Number(42.0));
    }

    #[test]
    fn test_optional_chaining_with_arrays() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("const arr = [1, 2, 3]; arr?.[0]").unwrap();
        assert_eq!(result, Value::Number(1.0));
    }

    #[test]
    fn test_optional_chaining_null_array() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("const arr = null; arr?.[0]").unwrap();
        assert_eq!(result, Value::Undefined);
    }

    #[test]
    fn test_optional_chaining_with_nullish_coalescing() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("const obj = null; obj?.a ?? 'default'").unwrap();
        assert_eq!(result, Value::String("default".to_string()));
    }

    #[test]
    fn test_optional_chaining_partial_chain() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("const obj = { a: null }; obj?.a?.b ?? 'fallback'").unwrap();
        assert_eq!(result, Value::String("fallback".to_string()));
    }
}
