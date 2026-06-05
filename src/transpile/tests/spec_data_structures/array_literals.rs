use super::helpers::*;
    mod array_literals {
        use super::*;

        /// Simple array: [1, 2, 3]
        #[test]
        fn arr_simple() {
            let source = r#"const x = [1, 2, 3];"#;
            let expr = parse_expr(source);
            assert!(!matches!(expr, Expr::Invalid), "should parse array literal");
            if let Expr::Array { elems } = &expr {
                assert_eq!(elems.len(), 3, "should have 3 elements");
            }
        }

        /// Array spread: [...a, 4]
        #[test]
        fn arr_spread() {
            let source = r#"const x = [...a, 4];"#;
            let expr = parse_expr(source);
            // Just verify it parses without crashing
            assert!(!matches!(expr, Expr::Invalid), "should parse array with spread");
            if let Expr::Array { elems } = &expr {
                // oxc might represent spread elements differently
                println!("array with spread has {} elements", elems.len());
            }
        }

        /// Mixed array: [1, "two", true]
        #[test]
        fn arr_mixed() {
            let source = r#"const x = [1, "two", true];"#;
            let expr = parse_expr(source);
            assert!(!matches!(expr, Expr::Invalid), "should parse mixed array");
            if let Expr::Array { elems } = &expr {
                assert_eq!(elems.len(), 3, "should have 3 elements");
            }
        }

        /// Empty array: []
        #[test]
        fn arr_empty() {
            let source = r#"const x = [];"#;
            let expr = parse_expr(source);
            assert!(!matches!(expr, Expr::Invalid), "should parse empty array");
            if let Expr::Array { elems } = &expr {
                assert!(elems.is_empty(), "should have 0 elements");
            }
        }

        /// Array with null/undefined elements: [1, null, undefined, 5]
        #[test]
        fn arr_with_null_undefined() {
            let source = r#"const x = [1, null, undefined, 5];"#;
            let expr = parse_expr(source);
            assert!(!matches!(expr, Expr::Invalid), "should parse array with null/undefined");
            if let Expr::Array { elems } = &expr {
                assert_eq!(elems.len(), 4, "should have 4 elements");
            }
        }

        /// Array with nested array: [[1, 2], [3, 4]]
        #[test]
        fn arr_nested() {
            let source = r#"const x = [[1, 2], [3, 4]];"#;
            let expr = parse_expr(source);
            assert!(!matches!(expr, Expr::Invalid), "should parse nested array");
            if let Expr::Array { elems } = &expr {
                assert_eq!(elems.len(), 2, "should have 2 elements");
            }
        }

        /// Array with holes: [1, , 3]
        #[test]
        fn arr_with_holes() {
            let source = r#"const x = [1, , 3];"#;
            let expr = parse_expr(source);
            assert!(!matches!(expr, Expr::Invalid), "should parse array with holes");
            if let Expr::Array { elems } = &expr {
                assert_eq!(elems.len(), 3, "should have 3 elements (holes included)");
                assert!(elems[0].is_some(), "first element should be Some");
                assert!(elems[1].is_none(), "second element should be None (hole)");
                assert!(elems[2].is_some(), "third element should be Some");
            }
        }

        /// Array spread at start: [...a, 1, 2]
        #[test]
        fn arr_spread_start() {
            let source = r#"const x = [...a, 1, 2];"#;
            let expr = parse_expr(source);
            assert!(!matches!(expr, Expr::Invalid), "should parse spread at start");
        }

        /// Array spread in middle: [1, ...a, 2]
        #[test]
        fn arr_spread_middle() {
            let source = r#"const x = [1, ...a, 2];"#;
            let expr = parse_expr(source);
            assert!(!matches!(expr, Expr::Invalid), "should parse spread in middle");
        }

        /// Array with objects: [{ a: 1 }, { b: 2 }]
        #[test]
        fn arr_with_objects() {
            let source = r#"const x = [{ a: 1 }, { b: 2 }];"#;
            let expr = parse_expr(source);
            assert!(!matches!(expr, Expr::Invalid), "should parse array with objects");
        }

        /// Array with trailing comma
        #[test]
        fn arr_trailing_comma() {
            let source = r#"const x = [1, 2, 3, ];"#;
            let expr = parse_expr(source);
            assert!(!matches!(expr, Expr::Invalid), "should parse array with trailing comma");
            if let Expr::Array { elems } = &expr {
                assert_eq!(elems.len(), 3, "should have 3 elements");
            }
        }

        /// Multiple spreads in array
        #[test]
        fn arr_multiple_spreads() {
            let source = r#"const x = [...a, ...b, 1];"#;
            let expr = parse_expr(source);
            assert!(!matches!(expr, Expr::Invalid), "should parse multiple spreads");
        }
    
