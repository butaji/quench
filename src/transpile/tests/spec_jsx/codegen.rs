use super::helpers::*;
    mod codegen {
        use super::*;

        #[test]
        fn codegen_jsx_simple_div() {
            let jsx = assert_jsx_parses(r#"const x = <div />;"#);
            let tokens = QuoteCodegen::default().gen_expr(&Expr::JSX(jsx));
            assert!(!tokens.is_empty(), "JSX codegen should produce tokens");
            // JSX should produce VNode, not Value::Null
            let s = tokens.to_string();
            eprintln!("DEBUG: codegen output: {}", s);
            assert!(s.contains("VNode"), "JSX codegen should produce VNode: {}", s);
        }

        #[test]
        fn codegen_jsx_with_attrs() {
            let jsx = assert_jsx_parses(r#"const x = <div class="home" id="main" />;"#);
            let tokens = QuoteCodegen::default().gen_expr(&Expr::JSX(jsx));
            assert!(!tokens.is_empty(), "JSX codegen should produce tokens");
        }

        #[test]
        fn codegen_jsx_with_children() {
            let jsx = assert_jsx_parses(r#"const x = <div><span>text</span></div>;"#);
            let tokens = QuoteCodegen::default().gen_expr(&Expr::JSX(jsx));
            assert!(!tokens.is_empty(), "JSX codegen should produce tokens");
        }

        #[test]
        fn codegen_jsx_fragment() {
            let jsx = assert_jsx_parses(r#"const x = <></>;"#);
            let tokens = QuoteCodegen::default().gen_expr(&Expr::JSX(jsx));
            assert!(!tokens.is_empty(), "JSX fragment codegen should produce tokens");
        }

        #[test]
        fn codegen_jsx_component() {
            let jsx = assert_jsx_parses(r#"const x = <Counter />;"#);
            let tokens = QuoteCodegen::default().gen_expr(&Expr::JSX(jsx));
            assert!(!tokens.is_empty(), "JSX component codegen should produce tokens");
        }

        #[test]
        fn codegen_jsx_expr_child() {
            let jsx = assert_jsx_parses(r#"const x = <div>{name}</div>;"#);
            let tokens = QuoteCodegen::default().gen_expr(&Expr::JSX(jsx));
            assert!(!tokens.is_empty(), "JSX with expr child codegen should produce tokens");
        }

        #[test]
        fn codegen_jsx_conditional() {
            let jsx = assert_jsx_parses(r#"const x = <div>{flag && <A />}</div>;"#);
            let tokens = QuoteCodegen::default().gen_expr(&Expr::JSX(jsx));
            assert!(!tokens.is_empty(), "JSX with conditional codegen should produce tokens");
        }

        #[test]
        fn codegen_jsx_ternary() {
            let jsx = assert_jsx_parses(r#"const x = <div>{flag ? <A /> : <B />}</div>;"#);
            let tokens = QuoteCodegen::default().gen_expr(&Expr::JSX(jsx));
            assert!(!tokens.is_empty(), "JSX with ternary codegen should produce tokens");
        }
    
