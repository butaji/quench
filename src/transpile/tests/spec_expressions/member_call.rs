//! Member, call, arrow function, update, assignment tests

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

    fn parse_expr_in_fn(source: &str) -> Expr {
        let parser = TsParser::new();
        let result = parser.parse_source(source).expect("parse failed");
        for item in &result.items {
            if let ModuleItem::Decl(Decl::Function(ref f)) = item {
                if let Some(ref body) = f.body {
                    for stmt in &body.0 {
                        if let Stmt::Block { stmts } = stmt {
                            for inner in stmts {
                                if let Stmt::Expr { expr } = inner {
                                    return (*expr).clone();
                                }
                            }
                        }
                    }
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
    #[ignore = "optional chaining ?.) not yet fully implemented in codegen"]
    fn test_optional_property() {
        let e = parse_expr("const x = obj?.prop;");
        assert_not_invalid("const x = obj?.prop;", "opt prop");
        assert_codegen_not_null(&e, "opt prop");
    }

    #[test]
    #[ignore = "optional chaining ?.) not yet fully implemented in codegen"]
    fn test_optional_method() {
        let e = parse_expr("const x = obj?.method();");
        assert_not_invalid("const x = obj?.method();", "opt method");
        assert_codegen_not_null(&e, "opt method");
    }

    #[test]
    #[ignore = "optional chaining ?.) not yet fully implemented in codegen"]
    fn test_optional_chained() {
        let e = parse_expr("const x = obj?.prop?.method();");
        assert_not_invalid("const x = obj?.prop?.method();", "opt chained");
        assert_codegen_not_null(&e, "opt chained");
    }

    #[test]
    fn test_member_property() {
        let e = parse_expr("const x = obj.prop;");
        assert_not_invalid("const x = obj.prop;", "member prop");
        assert_codegen_not_null(&e, "member prop");
    }

    #[test]
    fn test_member_computed() {
        let e = parse_expr("const x = arr[0];");
        assert_not_invalid("const x = arr[0];", "computed");
        assert_codegen_not_null(&e, "computed");
    }

    #[test]
    fn test_call_simple() {
        let e = parse_expr("const x = foo();");
        assert_not_invalid("const x = foo();", "call");
        assert_codegen_not_null(&e, "call");
    }

    #[test]
    fn test_call_method() {
        let e = parse_expr("const x = obj.method();");
        assert_not_invalid("const x = obj.method();", "method call");
        assert_codegen_not_null(&e, "method call");
    }

    #[test]
    fn test_new_expr() {
        let e = parse_expr("const x = new Foo();");
        assert_not_invalid("const x = new Foo();", "new");
        assert_codegen_not_null(&e, "new");
    }

    #[test]
    fn test_arrow_no_params() {
        let e = parse_expr("const x = () => 1;");
        assert_not_invalid("const x = () => 1;", "arrow no params");
        assert_codegen_not_null(&e, "arrow no params");
    }

    #[test]
    fn test_arrow_with_param() {
        let e = parse_expr("const x = (y) => y + 1;");
        assert_not_invalid("const x = (y) => y + 1;", "arrow param");
        assert_codegen_not_null(&e, "arrow param");
    }

    #[test]
    fn test_arrow_block_body() {
        let e = parse_expr("const x = () => { return 1; };");
        assert_not_invalid("const x = () => { return 1; };", "arrow block");
        assert_codegen_not_null(&e, "arrow block");
    }

    #[test]
    #[ignore = "async/await codegen needs proper Future/async block handling"]
    fn test_arrow_async() {
        let e = parse_expr("const x = async () => await foo();");
        assert_not_invalid("const x = async () => await foo();", "async arrow");
        assert_codegen_not_null(&e, "async arrow");
    }

    #[test]
    fn test_pre_increment() {
        let e = parse_expr("const x = ++i;");
        assert_not_invalid("const x = ++i;", "pre inc");
        assert_codegen_not_null(&e, "pre inc");
    }

    #[test]
    fn test_post_increment() {
        let e = parse_expr("const x = i++;");
        assert_not_invalid("const x = i++;", "post inc");
        assert_codegen_not_null(&e, "post inc");
    }

    #[test]
    fn test_pre_decrement() {
        let e = parse_expr("const x = --i;");
        assert_not_invalid("const x = --i;", "pre dec");
        assert_codegen_not_null(&e, "pre dec");
    }

    #[test]
    fn test_post_decrement() {
        let e = parse_expr("const x = i--;");
        assert_not_invalid("const x = i--;", "post dec");
        assert_codegen_not_null(&e, "post dec");
    }

    #[test]
    fn test_assign() {
        let e = parse_expr("const x = (a = 1);");
        assert_not_invalid("const x = (a = 1);", "assign");
        assert_codegen_not_null(&e, "assign");
    }

    #[test]
    fn test_add_assign() {
        let e = parse_expr("const x = (a += 2);");
        assert_not_invalid("const x = (a += 2);", "add assign");
        assert_codegen_not_null(&e, "add assign");
    }

    #[test]
    fn test_sub_assign() {
        let e = parse_expr("const x = (a -= 2);");
        assert_not_invalid("const x = (a -= 2);", "sub assign");
        assert_codegen_not_null(&e, "sub assign");
    }

    #[test]
    fn test_mul_assign() {
        let e = parse_expr("const x = (a *= 2);");
        assert_not_invalid("const x = (a *= 2);", "mul assign");
        assert_codegen_not_null(&e, "mul assign");
    }

    #[test]
    fn test_div_assign() {
        let e = parse_expr("const x = (a /= 2);");
        assert_not_invalid("const x = (a /= 2);", "div assign");
        assert_codegen_not_null(&e, "div assign");
    }

    #[test]
    fn test_mod_assign() {
        let e = parse_expr("const x = (a %= 2);");
        assert_not_invalid("const x = (a %= 2);", "mod assign");
        assert_codegen_not_null(&e, "mod assign");
    }

    #[test]
    #[ignore = "await codegen needs proper Future/async block handling"]
    fn test_await() {
        let e = parse_expr_in_fn("async function f() { const x = await p; }");
        assert!(!matches!(e, Expr::Invalid), "await should parse");
        assert_codegen_not_null(&e, "await");
    }

    #[test]
    fn test_yield() {
        let source = "function* g() { yield 1; }";
        let parser = TsParser::new();
        let result = parser.parse_source(source).expect("parse failed");
        assert!(!result.items.is_empty(), "yield should parse");
    }
}
