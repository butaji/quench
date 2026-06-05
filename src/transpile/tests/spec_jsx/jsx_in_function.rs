use super::helpers::*;
    mod jsx_in_function {
        use super::*;

        #[test]
        fn jsx_return_simple() {
            let source = r#"function Component() { return <div>hello</div>; }"#;
            let module = parse_jsx(source);
            // Find JSX in return statement
            let jsx_opt = find_jsx_expr_in_stmt(&module);
            assert!(jsx_opt.is_some(), "Should find JSX in return statement");
            let jsx = jsx_opt.unwrap();
            assert!(matches!(jsx.opening.name, JSXName::Ident(ref n) if n == "div"));
            assert_codegen_not_empty(&jsx);
        }

        #[test]
        fn jsx_return_with_props() {
            let source = r#"function Component() { return <div class="home">Welcome</div>; }"#;
            let module = parse_jsx(source);
            let jsx = find_jsx_expr_in_stmt(&module).unwrap();
            assert!(matches!(jsx.opening.name, JSXName::Ident(ref n) if n == "div"));
            assert!(!jsx.opening.attrs.is_empty());
            assert_codegen_not_empty(&jsx);
        }

        #[test]
        fn jsx_return_fragment() {
            let source = r#"function Component() { return <>fragment</>; }"#;
            let module = parse_jsx(source);
            let jsx = find_jsx_expr_in_stmt(&module).unwrap();
            assert!(matches!(jsx.opening.name, JSXName::Fragment));
            assert_codegen_not_empty(&jsx);
        }
    
