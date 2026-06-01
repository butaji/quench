//! Comparison operator tests

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

    #[test]
    fn test_eq() {
        let e = parse_expr("const x = a == b;");
        assert_not_invalid("const x = a == b;", "eq");
        assert_codegen_not_null(&e, "eq");
    }

    #[test]
    fn test_strict_eq() {
        let e = parse_expr("const x = a === b;");
        assert_not_invalid("const x = a === b;", "strict eq");
        assert_codegen_not_null(&e, "strict eq");
    }

    #[test]
    fn test_neq() {
        let e = parse_expr("const x = a != b;");
        assert_not_invalid("const x = a != b;", "neq");
        assert_codegen_not_null(&e, "neq");
    }

    #[test]
    fn test_strict_neq() {
        let e = parse_expr("const x = a !== b;");
        assert_not_invalid("const x = a !== b;", "strict neq");
        assert_codegen_not_null(&e, "strict neq");
    }

    #[test]
    fn test_lt() {
        let e = parse_expr("const x = a < b;");
        assert_not_invalid("const x = a < b;", "lt");
        assert_codegen_not_null(&e, "lt");
    }

    #[test]
    fn test_lte() {
        let e = parse_expr("const x = a <= b;");
        assert_not_invalid("const x = a <= b;", "lte");
        assert_codegen_not_null(&e, "lte");
    }

    #[test]
    fn test_gt() {
        let e = parse_expr("const x = a > b;");
        assert_not_invalid("const x = a > b;", "gt");
        assert_codegen_not_null(&e, "gt");
    }

    #[test]
    fn test_gte() {
        let e = parse_expr("const x = a >= b;");
        assert_not_invalid("const x = a >= b;", "gte");
        assert_codegen_not_null(&e, "gte");
    }

    #[test]
    fn test_instanceof() {
        let e = parse_expr("const x = x instanceof Y;");
        assert_not_invalid("const x = x instanceof Y;", "instanceof");
        assert_codegen_not_null(&e, "instanceof");
    }
}
