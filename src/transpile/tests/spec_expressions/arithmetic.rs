//! Arithmetic operator tests

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

    fn assert_codegen_contains(expr: &Expr, pattern: &str, label: &str) {
        let tokens = codegen(expr);
        let s = tokens.to_string();
        assert!(s.contains(pattern), "{}: missing '{}'", label, pattern);
    }

    #[test]
    fn test_add() {
        let e = parse_expr("const x = a + b;");
        assert_not_invalid("const x = a + b;", "add");
        assert_codegen_not_null(&e, "add");
        assert_codegen_contains(&e, "+", "add");
    }

    #[test]
    fn test_sub() {
        let e = parse_expr("const x = a - b;");
        assert_not_invalid("const x = a - b;", "sub");
        assert_codegen_not_null(&e, "sub");
        assert_codegen_contains(&e, "-", "sub");
    }

    #[test]
    fn test_mul() {
        let e = parse_expr("const x = a * b;");
        assert_not_invalid("const x = a * b;", "mul");
        assert_codegen_not_null(&e, "mul");
        assert_codegen_contains(&e, "*", "mul");
    }

    #[test]
    fn test_div() {
        let e = parse_expr("const x = a / b;");
        assert_not_invalid("const x = a / b;", "div");
        assert_codegen_not_null(&e, "div");
        assert_codegen_contains(&e, "/", "div");
    }

    #[test]
    fn test_mod() {
        let e = parse_expr("const x = a % b;");
        assert_not_invalid("const x = a % b;", "mod");
        assert_codegen_not_null(&e, "mod");
        assert_codegen_contains(&e, "%", "mod");
    }

    #[test]
    fn test_string_concat() {
        let e = parse_expr(r#"const x = "hello " + name;"#);
        assert_not_invalid(r#"const x = "hello " + name;"#, "string concat");
        assert_codegen_not_null(&e, "string concat");
        assert_codegen_contains(&e, "+", "string concat");
    }

    #[test]
    fn test_arithmetic_nested() {
        let e = parse_expr("const x = (a + b) * (c - d);");
        assert_not_invalid("const x = (a + b) * (c - d);", "nested");
        assert_codegen_not_null(&e, "nested");
        assert_codegen_contains(&e, "+", "nested");
        assert_codegen_contains(&e, "-", "nested");
        assert_codegen_contains(&e, "*", "nested");
    }

    #[test]
    fn test_unary_minus() {
        let e = parse_expr("const x = -a;");
        assert_not_invalid("const x = -a;", "unary minus");
        assert_codegen_not_null(&e, "unary minus");
        assert_codegen_contains(&e, "-", "unary minus");
    }

    #[test]
    fn test_unary_plus() {
        let e = parse_expr("const x = +a;");
        assert_not_invalid("const x = +a;", "unary plus");
        assert_codegen_not_null(&e, "unary plus");
    }
}
