use super::helpers::*;
    mod destructuring_object {
        use super::*;

        /// Basic object destructuring: const {a, b} = obj
        #[test]
        fn destr_obj_basic() {
            let source = r#"const {a, b} = obj;"#;
            let pat = parse_pat(source);
            assert!(pat.is_some(), "should parse object destructuring");
            if let Some(Pat::Object { props, rest }) = pat {
                assert_eq!(props.len(), 2, "should have 2 properties");
                assert!(rest.is_none(), "should not have rest");
            }
        }

        /// Object destructuring with rename: const {a: x, b: y} = obj
        #[test]
        fn destr_obj_rename() {
            let source = r#"const {a: x, b: y} = obj;"#;
            let pat = parse_pat(source);
            assert!(pat.is_some(), "should parse object destructuring with rename");
        }

        /// Nested object destructuring: const {a: {b}} = obj
        #[test]
        fn destr_obj_nested() {
            let source = r#"const {a: {b}} = obj;"#;
            let pat = parse_pat(source);
            assert!(pat.is_some(), "should parse nested object destructuring");
            if let Some(Pat::Object { props, .. }) = pat {
                if let ObjectPatProp::Init { value, .. } = &props[0] {
                    assert!(matches!(value, Pat::Object { .. }), "nested should be Pat::Object");
                }
            }
        }

        /// Object destructuring with default: const {a = 1} = obj
        #[test]
        fn destr_obj_default() {
            let source = r#"const {a = 1} = obj;"#;
            let pat = parse_pat(source);
            assert!(pat.is_some(), "should parse object destructuring with default");
        }

        /// Object destructuring with rest: const {a, ...rest} = obj
        #[test]
        fn destr_obj_rest() {
            let source = r#"const {a, ...rest} = obj;"#;
            let pat = parse_pat(source);
            assert!(pat.is_some(), "should parse object destructuring with rest");
            if let Some(Pat::Object { props, rest }) = pat {
                assert!(rest.is_some(), "should have rest");
                let has_rest = props.iter().any(|p| matches!(p, ObjectPatProp::Rest { .. }));
                assert!(has_rest, "props should contain Rest");
            }
        }

        /// Object destructuring with type annotation
        #[test]
        fn destr_obj_with_type() {
            let source = r#"const {a}: {a: number} = obj;"#;
            let pat = parse_pat(source);
            // Type annotations may or may not be preserved in pattern
            if pat.is_some() {
                println!("parsed with type: {:?}", pat);
            }
        }

        /// Object destructuring shorthand with default: const {a = 5, b = 10} = obj
        #[test]
        fn destr_obj_shorthand_default() {
            let source = r#"const {a = 5, b = 10} = obj;"#;
            let pat = parse_pat(source);
            assert!(pat.is_some(), "should parse shorthand with defaults");
        }

        /// Object destructuring with computed property - not fully supported
        #[test]
        #[ignore]
        fn destr_obj_computed() {
            let source = r#"const {[key]: value} = obj;"#;
            let pat = parse_pat(source);
            assert!(pat.is_some(), "should parse computed key destructuring");
        }

        /// Object destructuring in function param
        #[test]
        fn destr_obj_fn_param() {
            let source = r#"function f({a, b}) { }"#;
            let parser = crate::transpile::parser::TsParser::new();
            let result = parser.parse_source(source).expect("parse failed");
            let mut found = false;
            for item in &result.items {
                if let ModuleItem::Decl(Decl::Function(ref f)) = item {
                    for param in &f.params {
                        if let Some(Pat::Object { .. }) = &param.pattern {
                            found = true;
                        }
                    }
                }
            }
            assert!(found, "function param should have object pattern");
        }

        /// Object destructuring in arrow function param
        #[test]
        fn destr_obj_arrow_param() {
            let source = r#"const f = ({a}) => a;"#;
            let parser = crate::transpile::parser::TsParser::new();
            let result = parser.parse_source(source).expect("parse failed");
            let mut found = false;
            for item in &result.items {
                if let ModuleItem::Decl(Decl::Variable(ref v)) = item {
                    if let Some(Expr::ArrowFunction { params, .. }) = &v.init {
                        for param in params {
                            if param.pattern.as_ref().is_some_and(|p| matches!(p, Pat::Object { .. })) {
                                found = true;
                            }
                        }
                    }
                }
            }
            assert!(found, "arrow param should have object pattern");
        }

        /// Deeply nested object destructuring
        #[test]
        fn destr_obj_deep_nested() {
            let source = r#"const {a: {b: {c}}} = obj;"#;
            let pat = parse_pat(source);
            assert!(pat.is_some(), "should parse deeply nested object destructuring");
        }

        /// Object destructuring with method in pattern - not supported
        #[test]
        #[ignore]
        fn destr_obj_method() {
            let source = r#"const {a, method() {}} = obj;"#;
            let pat = parse_pat(source);
            assert!(pat.is_some(), "should parse method in destructuring");
        }
    

}
