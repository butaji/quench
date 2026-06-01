//! Bitwise operator tests

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
    fn test_bit_or() {
        let e = parse_expr("const x = a | b;");
        assert_not_invalid("const x = a | b;", "bit or");
        assert_codegen_not_null(&e, "bit or");
    }

    #[test]
    fn test_bit_and() {
        let e = parse_expr("const x = a & b;");
        assert_not_invalid("const x = a & b;", "bit and");
        assert_codegen_not_null(&e, "bit and");
    }

    #[test]
    fn test_bit_xor() {
        let e = parse_expr("const x = a ^ b;");
        assert_not_invalid("const x = a ^ b;", "bit xor");
        assert_codegen_not_null(&e, "bit xor");
    }

    #[test]
    fn test_shift_left() {
        let e = parse_expr("const x = a << b;");
        assert_not_invalid("const x = a << b;", "shift left");
        assert_codegen_not_null(&e, "shift left");
    }

    #[test]
    fn test_shift_right() {
        let e = parse_expr("const x = a >> b;");
        assert_not_invalid("const x = a >> b;", "shift right");
        assert_codegen_not_null(&e, "shift right");
    }

    #[test]
    fn test_unsigned_shift_right() {
        let e = parse_expr("const x = a >>> b;");
        assert_not_invalid("const x = a >>> b;", "unsigned shift");
        assert_codegen_not_null(&e, "unsigned shift");
    }

    #[test]
    fn test_bitwise_combined() {
        let e = parse_expr("const x = (a | b) & (c ^ d);");
        assert_not_invalid("const x = (a | b) & (c ^ d);", "combined");
        assert_codegen_not_null(&e, "combined");
    }
}
