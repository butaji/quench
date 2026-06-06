use super::helpers::*;
    mod destructuring_array {
        use super::*;

        /// Basic array destructuring: const [a, b] = arr
        #[test]
        fn destr_arr_basic() {
            let source = r#"const [a, b] = arr;"#;
            let pat = parse_pat(source);
            assert!(pat.is_some(), "should parse array destructuring");
            if let Some(Pat::Array { elems, rest }) = pat {
                assert_eq!(elems.len(), 2, "should have 2 elements");
                assert!(rest.is_none(), "should not have rest");
            }
        }

        /// Nested array destructuring: const [a, [b]] = arr
        #[test]
        fn destr_arr_nested() {
            let source = r#"const [a, [b]] = arr;"#;
            let pat = parse_pat(source);
            assert!(pat.is_some(), "should parse nested array destructuring");
            if let Some(Pat::Array { elems, .. }) = pat {
                assert_eq!(elems.len(), 2, "should have 2 elements");
                if let Some(inner) = &elems[1] {
                    assert!(matches!(inner, Pat::Array { .. }), "second should be nested array");
                }
            }
        }

        /// Array destructuring with default: const [a = 1] = arr
        #[test]
        fn destr_arr_default() {
            let source = r#"const [a = 1] = arr;"#;
            let pat = parse_pat(source);
            assert!(pat.is_some(), "should parse array destructuring with default");
        }

        /// Array destructuring with rest: const [a, ...rest] = arr
        #[test]
        fn destr_arr_rest() {
            let source = r#"const [a, ...rest] = arr;"#;
            let pat = parse_pat(source);
            assert!(pat.is_some(), "should parse array destructuring with rest");
            if let Some(Pat::Array { elems, rest }) = pat {
                let has_rest_elem = elems.iter().any(|e| e.as_ref().is_some_and(|p| matches!(p, Pat::Rest { .. })));
                let has_rest_field = rest.is_some();
                assert!(has_rest_elem || has_rest_field, "should have rest");
            }
        }

        /// Array destructuring with type annotation
        #[test]
        fn destr_arr_with_type() {
            let source = r#"const [a]: number[] = arr;"#;
            let pat = parse_pat(source);
            if pat.is_some() {
                println!("parsed array destructuring with type: {:?}", pat);
            }
        }

        /// Array destructuring with holes: const [a, , b] = arr
        #[test]
        fn destr_arr_with_holes() {
            let source = r#"const [a, , b] = arr;"#;
            let pat = parse_pat(source);
            assert!(pat.is_some(), "should parse array destructuring with holes");
            if let Some(Pat::Array { elems, .. }) = pat {
                assert_eq!(elems.len(), 3, "should have 3 elements (hole included)");
                assert!(elems[0].is_some(), "first should be Some");
                assert!(elems[1].is_none(), "second should be None (hole)");
                assert!(elems[2].is_some(), "third should be Some");
            }
        }

        /// Array destructuring with defaults and rest
        #[test]
        fn destr_arr_default_rest() {
            let source = r#"const [a = 1, ...rest] = arr;"#;
            let pat = parse_pat(source);
            assert!(pat.is_some(), "should parse array destructuring with default and rest");
        }

        /// Deeply nested array destructuring
        #[test]
        fn destr_arr_deep_nested() {
            let source = r#"const [[a], [[b]], c] = arr;"#;
            let pat = parse_pat(source);
            assert!(pat.is_some(), "should parse deeply nested array destructuring");
        }

        /// Array destructuring in function param
        #[test]
        fn destr_arr_fn_param() {
            let source = r#"function f([a, b]) { }"#;
            let parser = crate::transpile::parser::TsParser::new();
            let result = parser.parse_source(source).expect("parse failed");
            let mut found = false;
            for item in &result.items {
                if let ModuleItem::Decl(Decl::Function(ref f)) = item {
                    for param in &f.params {
                        if let Some(Pat::Array { .. }) = &param.pattern {
                            found = true;
                        }
                    }
                }
            }
            assert!(found, "function param should have array pattern");
        }

        /// Array destructuring in arrow function param
        #[test]
        fn destr_arr_arrow_param() {
            let source = r#"const f = ([a]) => a;"#;
            let parser = crate::transpile::parser::TsParser::new();
            let result = parser.parse_source(source).expect("parse failed");
            let mut found = false;
            for item in &result.items {
                if let ModuleItem::Decl(Decl::Variable(ref v)) = item {
                    if let Some(Expr::ArrowFunction { params, .. }) = &v.init {
                        for param in params {
                            if param.pattern.as_ref().is_some_and(|p| matches!(p, Pat::Array { .. })) {
                                found = true;
                            }
                        }
                    }
                }
            }
            assert!(found, "arrow param should have array pattern");
        }

        /// Empty array destructuring
        #[test]
        fn destr_arr_empty() {
            let source = r#"const [] = arr;"#;
            let pat = parse_pat(source);
            assert!(pat.is_some(), "should parse empty array destructuring");
            if let Some(Pat::Array { elems, .. }) = pat {
                assert!(elems.is_empty(), "should have 0 elements");
            }
        }

        /// Array destructuring with object pattern inside
        #[test]
        fn destr_arr_with_obj_inside() {
            let source = r#"const [{a, b}, c] = arr;"#;
            let pat = parse_pat(source);
            assert!(pat.is_some(), "should parse array with object pattern inside");
        }
    

}
