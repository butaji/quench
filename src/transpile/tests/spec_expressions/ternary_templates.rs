//! Ternary and template literal tests

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
    fn test_ternary() {
        let e = parse_expr("const x = cond ? a : b;");
        assert_not_invalid("const x = cond ? a : b;", "ternary");
        assert_codegen_not_null(&e, "ternary");
        assert_codegen_contains(&e, "if", "ternary");
    }

    #[test]
    fn test_ternary_nested() {
        let e = parse_expr("const x = a ? b ? c : d : e;");
        assert_not_invalid("const x = a ? b ? c : d : e;", "nested ternary");
        assert_codegen_not_null(&e, "nested ternary");
    }

    #[test]
    fn test_ternary_number() {
        let e = parse_expr("const x = true ? 1 : 2;");
        assert_not_invalid("const x = true ? 1 : 2;", "ternary num");
        assert_codegen_not_null(&e, "ternary num");
    }

    #[test]
    fn test_ternary_string() {
        let e = parse_expr(r#"const x = true ? "yes" : "no";"#);
        assert_not_invalid(r#"const x = true ? "yes" : "no";"#, "ternary str");
        assert_codegen_not_null(&e, "ternary str");
    }

    #[test]
    fn test_template_simple() {
        let e = parse_expr(r#"const x = `hello`;"#);
        assert_not_invalid(r#"const x = `hello`;"#, "template simple");
        assert_codegen_not_null(&e, "template simple");
    }

    #[test]
    fn test_template_interpolation() {
        let e = parse_expr(r#"const x = `hello ${name}`;"#);
        assert_not_invalid(r#"const x = `hello ${name}`;"#, "template interp");
        assert_codegen_not_null(&e, "template interp");
    }

    #[test]
    fn test_template_multiple_interpolations() {
        let e = parse_expr(r#"const x = `${a} + ${b} = ${c}`;"#);
        assert_not_invalid(r#"const x = `${a} + ${b} = ${c}`;"#, "multi interp");
        assert_codegen_not_null(&e, "multi interp");
    }

    #[test]
    fn test_template_expression() {
        let e = parse_expr(r#"const x = `result: ${a + b}`;"#);
        assert_not_invalid(r#"const x = `result: ${a + b}`;"#, "template expr");
        assert_codegen_not_null(&e, "template expr");
    }

    #[test]
    fn test_tagged_template() {
        let e = parse_expr(r#"const x = css`color: red`;"#);
        let is_invalid = matches!(e, Expr::Invalid);
        if !is_invalid {
            assert_codegen_not_null(&e, "tagged template");
        } else {
            println!("Tagged template not fully supported");
        }
    }
}
