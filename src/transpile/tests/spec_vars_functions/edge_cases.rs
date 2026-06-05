use super::helpers::*;
    mod edge_cases {
        use super::*;

        #[test]
        fn function_in_block() {
            let source = "if (true) { function f() {} }";
            let items = parse_source(source);
            assert!(!items.is_empty());
        }

        #[test]
        fn multiple_variables() {
            let source = "const a = 1, b = 2, c = 3;";
            let items = parse_source(source);
            assert!(!items.is_empty());
        }

        #[test]

        fn destructure_function_return() {
            let source = "const {a, b} = foo();";
            let decl = parse_first_decl(source);
            match decl {
                Decl::Variable(ref v) => {
                    assert!(v.pattern.is_some());
                }
                _ => panic!("expected variable decl"),
            }
            let tokens = codegen_decl(&decl);
            assert!(!tokens.is_empty());
        }

        #[test]
        fn nested_arrow_in_function() {
            let func = find_function("function outer() { const f = () => 1; }");
            assert!(func.body.is_some());
            let tokens = codegen_fn(&func);
            assert!(!tokens.is_empty());
        }

        #[test]
        fn function_calling_function() {
            let source = "function a() {} function b() { a(); }";
            let items = parse_source(source);
            assert!(items.len() >= 2, "should parse multiple functions");
            let func_b = find_function("function b() { a(); }");
            assert!(func_b.body.is_some());
        }

        #[test]
        fn arrow_with_complex_body() {
            let expr = find_expr_in_var("const f = (x: number): number => { return x * 2; };");
            assert!(matches!(expr, Expr::ArrowFunction { .. }));
            assert_codegen_not_null(&expr, "arrow complex body");
        }

        #[test]
        #[ignore = "Invalid TypeScript syntax - throws is not a valid TS feature"]
        fn function_with_throws() {
            let source = "function f() throws { }";
            let func = find_function(source);
            let tokens = codegen_fn(&func);
            assert!(!tokens.is_empty());
        }
    
