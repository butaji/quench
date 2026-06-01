//! Complex expression and operator exhaustiveness tests

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
    fn test_binary_ops_all_variants() {
        let sources = [
            "a + b", "a - b", "a * b", "a / b", "a % b",
            "a | b", "a & b", "a ^ b",
            "a << b", "a >> b", "a >>> b",
            "a == b", "a === b", "a != b", "a !== b",
            "a < b", "a <= b", "a > b", "a >= b",
            "a instanceof b", "a in b",
            "a && b", "a || b", "a ?? b",
        ];
        for src in sources {
            let e = parse_expr(&format!("const x = {};", src));
            assert!(!matches!(e, Expr::Invalid), "BinaryOp '{}' should parse", src);
            assert_codegen_not_null(&e, src);
        }
    }

    #[test]
    fn test_unary_ops_all_variants() {
        let sources = ["-a", "+a", "!a", "~a", "typeof a", "void 0", "delete a"];
        for src in sources {
            let e = parse_expr(&format!("const x = {};", src));
            assert!(!matches!(e, Expr::Invalid), "UnaryOp '{}' should parse", src);
            assert_codegen_not_null(&e, src);
        }
    }

    #[test]
    fn test_logical_ops_all_variants() {
        let sources = ["a && b", "a || b", "a ?? b"];
        for src in sources {
            let e = parse_expr(&format!("const x = {};", src));
            assert!(!matches!(e, Expr::Invalid), "LogicalOp '{}' should parse", src);
            assert_codegen_not_null(&e, src);
        }
    }
}
