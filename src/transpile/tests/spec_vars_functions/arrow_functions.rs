use super::helpers::*;
    mod arrow_functions {
        use super::*;

        #[test]
        fn arrow_no_params() {
            let expr = find_expr_in_var("const f = () => 1;");
            assert!(matches!(expr, Expr::ArrowFunction { .. }));
            if let Expr::ArrowFunction { params, .. } = &expr {
                assert!(params.is_empty());
            }
            assert_codegen_not_null(&expr, "arrow no params");
        }

        #[test]
        fn arrow_single_param() {
            let expr = find_expr_in_var("const f = x => x + 1;");
            assert!(matches!(expr, Expr::ArrowFunction { .. }));
            if let Expr::ArrowFunction { params, .. } = &expr {
                assert_eq!(params.len(), 1);
                assert_eq!(params[0].name, "x");
            }
            assert_codegen_not_null(&expr, "arrow single param");
        }

        #[test]
        fn arrow_with_parens_single_param() {
            let expr = find_expr_in_var("const f = (x) => x + 1;");
            assert!(matches!(expr, Expr::ArrowFunction { .. }));
            assert_codegen_not_null(&expr, "arrow with parens");
        }

        #[test]
        fn arrow_multiple_params() {
            let expr = find_expr_in_var("const f = (a, b, c) => a + b + c;");
            assert!(matches!(expr, Expr::ArrowFunction { .. }));
            if let Expr::ArrowFunction { params, .. } = &expr {
                assert_eq!(params.len(), 3);
            }
            assert_codegen_not_null(&expr, "arrow multiple params");
        }

        #[test]
        fn arrow_block_body() {
            let expr = find_expr_in_var("const f = () => { return 1; };");
            assert!(matches!(expr, Expr::ArrowFunction { .. }));
            assert_codegen_not_null(&expr, "arrow block body");
        }

        #[test]
        fn arrow_expr_body() {
            let expr = find_expr_in_var("const f = () => 42;");
            assert!(matches!(expr, Expr::ArrowFunction { .. }));
            assert_codegen_not_null(&expr, "arrow expr body");
        }

        #[test]
        fn arrow_async() {
            let expr = find_expr_in_var("const f = async () => {};");
            assert!(matches!(expr, Expr::ArrowFunction { .. }));
            if let Expr::ArrowFunction { is_async, .. } = &expr {
                assert!(*is_async);
            }
            assert_codegen_not_null(&expr, "arrow async");
        }

        #[test]
        fn arrow_async_with_param() {
            let expr = find_expr_in_var("const f = async x => await x;");
            assert!(matches!(expr, Expr::ArrowFunction { .. }));
            if let Expr::ArrowFunction { is_async, params, .. } = &expr {
                assert!(*is_async);
                assert_eq!(params.len(), 1);
            }
            assert_codegen_not_null(&expr, "arrow async with param");
        }
    
