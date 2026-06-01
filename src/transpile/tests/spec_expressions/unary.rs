//! Unary operator tests

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
    fn test_not() {
        let e = parse_expr("const x = !a;");
        assert_not_invalid("const x = !a;", "not");
        assert_codegen_not_null(&e, "not");
        assert_codegen_contains(&e, "!", "not");
    }

    #[test]
    fn test_negate() {
        let e = parse_expr("const x = -a;");
        assert_not_invalid("const x = -a;", "negate");
        assert_codegen_not_null(&e, "negate");
        assert_codegen_contains(&e, "-", "negate");
    }

    #[test]
    fn test_plus() {
        let e = parse_expr("const x = +a;");
        assert_not_invalid("const x = +a;", "plus");
        assert_codegen_not_null(&e, "plus");
    }

    #[test]
    fn test_typeof() {
        let e = parse_expr("const x = typeof x;");
        assert_not_invalid("const x = typeof x;", "typeof");
        assert_codegen_not_null(&e, "typeof");
        assert_codegen_contains(&e, "type_name_of_val", "typeof");
    }

    #[test]
    fn test_void() {
        let e = parse_expr("const x = void 0;");
        assert_not_invalid("const x = void 0;", "void");
        assert_codegen_not_null(&e, "void");
        assert_codegen_contains(&e, "()", "void");
    }

    #[test]
    fn test_bitnot() {
        let e = parse_expr("const x = ~a;");
        assert_not_invalid("const x = ~a;", "bitnot");
        assert_codegen_not_null(&e, "bitnot");
        assert_codegen_contains(&e, "!", "bitnot");
    }

    #[test]
    fn test_delete() {
        let e = parse_expr("const x = delete obj.prop;");
        assert_not_invalid("const x = delete obj.prop;", "delete");
        assert_codegen_not_null(&e, "delete");
    }

    #[test]
    fn test_double_negation() {
        let e = parse_expr("const x = !!a;");
        assert_not_invalid("const x = !!a;", "double neg");
        assert_codegen_not_null(&e, "double neg");
    }
}
