//! Literal and compound literal tests

#[cfg(test)]
mod tests {
    use crate::transpile::hir::*;
    use crate::transpile::parser::TsParser;

    fn parse_expr(source: &str) -> Expr {
        let parser = TsParser::new();
        let result = parser.parse_source(source).expect("parse failed");
        for item in &result.items {
            if let ModuleItem::Decl(Decl::Variable(ref v)) = item {
                if let Some(expr) = &v.init {
                    return (*expr).clone();
                }
            }
        }
        Expr::Invalid
    }

    fn codegen(expr: &Expr) -> proc_macro2::TokenStream {
        use quote::ToTokens;
        QuoteCodegen::default().gen_expr(expr)
    }

    fn assert_not_invalid(source: &str, label: &str) {
        let expr = parse_expr(source);
        assert!(!matches!(expr, Expr::Invalid), "{}: parse failed", label);
    }

    fn assert_codegen_not_null(expr: &Expr, label: &str) {
        let tokens = codegen(expr);
        let s = tokens.to_string();
        assert!(!s.contains("Value::Null"), "{}: codegen null", label);
    }

    fn contains_value_null(tokens: &proc_macro2::TokenStream) -> bool {
        let s = tokens.to_string();
        s.contains("Value::Null")
    }

    #[test]
    fn test_string() {
        let e = parse_expr(r#"const x = "hello";"#);
        assert_not_invalid(r#"const x = "hello";"#, "string");
        assert_codegen_not_null(&e, "string");
    }

    #[test]
    fn test_number_integer() {
        let e = parse_expr("const x = 42;");
        assert_not_invalid("const x = 42;", "integer");
        assert_codegen_not_null(&e, "integer");
    }

    #[test]
    fn test_number_float() {
        let e = parse_expr("const x = 3.14;");
        assert_not_invalid("const x = 3.14;", "float");
        assert_codegen_not_null(&e, "float");
    }

    #[test]
    fn test_boolean_true() {
        let e = parse_expr("const x = true;");
        assert_not_invalid("const x = true;", "true");
        assert_codegen_not_null(&e, "true");
    }

    #[test]
    fn test_boolean_false() {
        let e = parse_expr("const x = false;");
        assert_not_invalid("const x = false;", "false");
        assert_codegen_not_null(&e, "false");
    }

    #[test]
    fn test_null() {
        let e = parse_expr("const x = null;");
        assert_not_invalid("const x = null;", "null");
        // null SHOULD produce Value::Null
        let tokens = codegen(&e);
        let s = tokens.to_string();
        // Just verify it parses and codegen runs
        assert!(!s.is_empty(), "null codegen should not be empty");
    }

    #[test]
    fn test_undefined() {
        let e = parse_expr("const x = undefined;");
        assert_not_invalid("const x = undefined;", "undefined");
        // undefined should parse, codegen may produce Value::Null or similar
        let tokens = codegen(&e);
        let s = tokens.to_string();
        assert!(!s.is_empty(), "undefined codegen should not be empty");
    }

    #[test]
    fn test_bigint() {
        let e = parse_expr("const x = 123n;");
        assert_not_invalid("const x = 123n;", "bigint");
        assert_codegen_not_null(&e, "bigint");
    }

    #[test]
    fn test_regexp() {
        let e = parse_expr("const x = /abc/;");
        assert_not_invalid("const x = /abc/;", "regexp");
        assert_codegen_not_null(&e, "regexp");
    }

    #[test]
    fn test_array_empty() {
        let e = parse_expr("const x = [];");
        assert_not_invalid("const x = [];", "empty array");
        assert_codegen_not_null(&e, "empty array");
    }

    #[test]
    fn test_array_with_elements() {
        let e = parse_expr("const x = [1, 2, 3];");
        assert_not_invalid("const x = [1, 2, 3];", "array elements");
        assert_codegen_not_null(&e, "array elements");
    }

    #[test]
    fn test_array_spread() {
        let e = parse_expr("const x = [...arr];");
        assert_not_invalid("const x = [...arr];", "array spread");
        assert_codegen_not_null(&e, "array spread");
    }

    #[test]
    fn test_object_empty() {
        let e = parse_expr("const x = {};");
        assert_not_invalid("const x = {};", "empty object");
        assert_codegen_not_null(&e, "empty object");
    }

    #[test]
    fn test_object_with_props() {
        let e = parse_expr("const x = {a: 1, b: 2};");
        assert_not_invalid("const x = {a: 1, b: 2};", "object props");
        assert_codegen_not_null(&e, "object props");
    }

    #[test]
    fn test_object_spread() {
        let e = parse_expr("const x = {...obj};");
        assert_not_invalid("const x = {...obj};", "object spread");
        assert_codegen_not_null(&e, "object spread");
    }

    #[test]
    fn test_sequence_two() {
        let e = parse_expr("const x = (a, b);");
        assert_not_invalid("const x = (a, b);", "sequence");
        assert_codegen_not_null(&e, "sequence");
    }

    #[test]
    fn test_sequence_three() {
        let e = parse_expr("const x = (a, b, c);");
        assert_not_invalid("const x = (a, b, c);", "sequence 3");
        assert_codegen_not_null(&e, "sequence 3");
    }
}
