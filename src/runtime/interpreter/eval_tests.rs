//! TypeScript Language Spec Coverage for Runtime Interpreter
//!
//! Tests cover ECMAScript expressions: https://tc39.es/ecma262/#sec-ecmascript-language-expressions

#[cfg(test)]
mod eval_tests {
    use crate::runtime::interpreter::Interpreter;

    fn run_eval(src: &str) -> String {
        let parser = crate::transpile::TsParser::new();
        let source = format!("const __result = {};", src);
        let module = parser.parse_tsx(&source).unwrap();
        let interpreter = Interpreter::new();
        interpreter.eval_module(&module)
    }

    // ========================================================================
    // 12.2 Primary Expressions
    // ========================================================================

    #[test]
    fn test_undefined() {
        assert_eq!(run_eval("undefined"), "undefined");
    }

    #[test]
    fn test_null() {
        assert_eq!(run_eval("null"), "null");
    }

    #[test]
    fn test_boolean_literals() {
        assert_eq!(run_eval("true"), "true");
        assert_eq!(run_eval("false"), "false");
    }

    #[test]
    fn test_number_literals() {
        assert_eq!(run_eval("0"), "0");
        assert_eq!(run_eval("42"), "42");
        assert_eq!(run_eval("3.14"), "3.14");
    }

    #[test]
    fn test_string_literals() {
        assert_eq!(run_eval("\"hello\""), "hello");
        assert_eq!(run_eval("'world'"), "world");
    }

    // ========================================================================
    // 12.6 Multiplicative Operators
    // ========================================================================

    #[test]
    fn test_multiplication() {
        assert_eq!(run_eval("2 * 3"), "6");
        assert_eq!(run_eval("0 * 5"), "0");
    }

    #[test]
    fn test_division() {
        assert_eq!(run_eval("10 / 2"), "5");
    }

    #[test]
    fn test_remainder() {
        assert_eq!(run_eval("10 % 3"), "1");
    }

    // ========================================================================
    // 12.7 Additive Operators
    // ========================================================================

    #[test]
    fn test_addition() {
        assert_eq!(run_eval("1 + 2"), "3");
    }

    #[test]
    fn test_subtraction() {
        assert_eq!(run_eval("5 - 3"), "2");
    }

    // ========================================================================
    // 12.12 Binary Logical Operators
    // ========================================================================

    #[test]
    fn test_logical_and() {
        assert_eq!(run_eval("true && true"), "true");
        assert_eq!(run_eval("true && false"), "false");
        assert_eq!(run_eval("false && true"), "false");
    }

    #[test]
    fn test_logical_or() {
        assert_eq!(run_eval("false || true"), "true");
        assert_eq!(run_eval("false || false"), "false");
    }

    // ========================================================================
    // 12.13 Conditional (Ternary) Operator
    // ========================================================================

    #[test]
    fn test_conditional() {
        assert_eq!(run_eval("true ? 1 : 2"), "1");
        assert_eq!(run_eval("false ? 1 : 2"), "2");
    }

    // ========================================================================
    // 12.16 Function Expressions
    // ========================================================================

    #[test]
    fn test_arrow_function() {
        assert_eq!(run_eval("(x => x)(5)"), "5");
        assert_eq!(run_eval("(x => x * 2)(3)"), "6");
    }

    // ========================================================================
    // Nested expressions
    // ========================================================================

    #[test]
    fn test_nested_expressions() {
        assert_eq!(run_eval("(1 + 2) * 3"), "9");
        assert_eq!(run_eval("2 * (3 + 4)"), "14");
    }

    // TODO: Implement remaining features
    // - Template literals
    // - Array/object literals
    // - Comparison operators
    // - Bitwise operators
    // - Unary operators
    // - Assignment operators
    // - Update operators
    // - Sequence expressions
    // - Member expressions
    // - Call expressions
}
