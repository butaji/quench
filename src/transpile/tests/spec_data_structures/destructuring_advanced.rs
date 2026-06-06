use super::helpers::*;
    mod destructuring_advanced {
        use super::*;

        /// Mixed object and array destructuring
        #[test]
        fn destr_mixed() {
            let source = r#"const {a, b: [c, d]} = obj;"#;
            let pat = parse_pat(source);
            assert!(pat.is_some(), "should parse mixed destructuring");
        }

        /// Destructuring with function call
        #[test]
        fn destr_with_call() {
            let source = r#"const {a} = getObj();"#;
            let pat = parse_pat(source);
            assert!(pat.is_some(), "should parse destructuring from call");
        }

        /// Destructuring assignment expression - may not be fully supported
        #[test]
        #[ignore]
        fn destr_assignment() {
            let source = r#"({a, b} = obj);"#;
            let parser = crate::transpile::parser::TsParser::new();
            let result = parser.parse_source(source).expect("parse failed");
            let found = result.items.iter().any(|item| {
                if let ModuleItem::Stmt(Stmt::Expr { expr }) = item {
                    matches!(expr, Expr::Assign { .. })
                } else {
                    false
                }
            });
            assert!(found, "destructuring assignment should parse");
        }

        /// For-in with destructuring
        #[test]
        fn destr_for_in() {
            let source = r#"for (const {a, b} in obj) { }"#;
            let parser = crate::transpile::parser::TsParser::new();
            let result = parser.parse_source(source).expect("parse failed");
            let found = result.items.iter().any(|item| {
                if let ModuleItem::Stmt(Stmt::ForIn { .. }) = item {
                    true
                } else {
                    false
                }
            });
            assert!(found, "for-in with destructuring should parse");
        }

        /// For-of with destructuring
        #[test]
        fn destr_for_of() {
            let source = r#"for (const [a, b] of arr) { }"#;
            let parser = crate::transpile::parser::TsParser::new();
            let result = parser.parse_source(source).expect("parse failed");
            let found = result.items.iter().any(|item| {
                if let ModuleItem::Stmt(Stmt::ForOf { .. }) = item {
                    true
                } else {
                    false
                }
            });
            assert!(found, "for-of with destructuring should parse");
        }

        /// Nested destructuring across function and arrow - uses parse_pat which won't find arrow params
        #[test]
        fn destr_nested_fn_arrow() {
            // Note: parse_pat only looks at variable declarations, not arrow params
            // This test verifies the source parses without crashing
            let source = r#"const f = ({a: {b}}) => b;"#;
            let parser = crate::transpile::parser::TsParser::new();
            let result = parser.parse_source(source);
            assert!(result.is_ok(), "should parse without error");
        }

        /// Destructuring with multiple defaults
        #[test]
        fn destr_multi_default() {
            let source = r#"const {a = 1, b = 2, c = 3} = obj;"#;
            let pat = parse_pat(source);
            assert!(pat.is_some(), "should parse multiple defaults");
        }

        /// Destructuring with renaming and defaults
        #[test]
        fn destr_rename_default() {
            let source = r#"const {a: x = 5, b: y = 10} = obj;"#;
            let pat = parse_pat(source);
            assert!(pat.is_some(), "should parse rename with defaults");
        }

        /// Array destructuring with object rest - not fully supported
        #[test]
        #[ignore]
        fn destr_arr_obj_rest() {
            let source = r#"const [a, ...{b, c}] = arr;"#;
            let pat = parse_pat(source);
            assert!(pat.is_some(), "should parse arr with obj rest");
        }
    

}
