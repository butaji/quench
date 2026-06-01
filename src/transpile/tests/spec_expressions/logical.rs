//! Logical operator tests

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
    fn test_logical_and() {
        let e = parse_expr("const x = a && b;");
        assert_not_invalid("const x = a && b;", "logical and");
        assert_codegen_not_null(&e, "logical and");
        assert_codegen_contains(&e, "&&", "logical and");
    }

    #[test]
    fn test_logical_or() {
        let e = parse_expr("const x = a || b;");
        assert_not_invalid("const x = a || b;", "logical or");
        assert_codegen_not_null(&e, "logical or");
        assert_codegen_contains(&e, "||", "logical or");
    }

    #[test]
    fn test_nullish_coalescing() {
        let e = parse_expr("const x = a ?? b;");
        assert_not_invalid("const x = a ?? b;", "nullish");
        assert_codegen_not_null(&e, "nullish");
        assert_codegen_contains(&e, "??", "nullish");
    }

    #[test]
    fn test_logical_short_circuit_and() {
        let e = parse_expr("const x = a && b && c;");
        assert_not_invalid("const x = a && b && c;", "chained and");
        assert_codegen_not_null(&e, "chained and");
    }

    #[test]
    fn test_logical_short_circuit_or() {
        let e = parse_expr("const x = a || b || c;");
        assert_not_invalid("const x = a || b || c;", "chained or");
        assert_codegen_not_null(&e, "chained or");
    }

    #[test]
    fn test_logical_mixed() {
        let e = parse_expr("const x = a && b || c;");
        assert_not_invalid("const x = a && b || c;", "mixed");
        assert_codegen_not_null(&e, "mixed");
        assert_codegen_contains(&e, "&&", "mixed");
        assert_codegen_contains(&e, "||", "mixed");
    }
}
