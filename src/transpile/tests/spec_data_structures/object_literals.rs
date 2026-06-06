use super::helpers::*;
    mod object_literals {
        use super::*;

        /// Simple object: { a: 1, b: "two" }
        #[test]
        fn obj_simple() {
            let source = r#"const x = { a: 1, b: "two" };"#;
            let expr = parse_expr(source);
            assert!(!matches!(expr, Expr::Invalid), "should parse object literal");
            if let Expr::Object { members } = &expr {
                assert_eq!(members.len(), 2, "should have 2 members");
            }
        }

        /// Shorthand property: { a, b }
        #[test]
        fn obj_shorthand() {
            let source = r#"const x = { a, b };"#;
            let expr = parse_expr(source);
            assert!(!matches!(expr, Expr::Invalid), "should parse shorthand object");
            if let Expr::Object { members } = &expr {
                assert!(!members.is_empty(), "should have members");
            }
        }

        /// Object spread: { ...obj, c: 3 }
        #[test]
        fn obj_spread() {
            let source = r#"const x = { ...obj, c: 3 };"#;
            let expr = parse_expr(source);
            assert!(!matches!(expr, Expr::Invalid), "should parse object with spread");
            if let Expr::Object { members } = &expr {
                let has_spread = members.iter().any(|m| matches!(m.prop, ObjectProp::Spread { .. }));
                assert!(has_spread, "should contain spread property");
            }
        }

        /// Nested object: { a: { b: 1 } }
        #[test]
        fn obj_nested() {
            let source = r#"const x = { a: { b: 1 } };"#;
            let expr = parse_expr(source);
            assert!(!matches!(expr, Expr::Invalid), "should parse nested object");
            if let Expr::Object { members } = &expr {
                assert_eq!(members.len(), 1, "should have 1 member");
                if let ObjectProp::Init { value, .. } = &members[0].prop {
                    assert!(matches!(*value, Expr::Object { .. }), "value should be nested object");
                }
            }
        }

        /// Computed key object: { [key]: value } - may not be fully supported
        #[test]
        #[ignore]
        fn obj_computed_key() {
            let source = r#"const key = "myKey"; const x = { [key]: value };"#;
            let expr = parse_expr(source);
            assert!(!matches!(expr, Expr::Invalid), "should parse computed key object");
        }

        /// Method shorthand: { method() {} } - may not be fully supported
        #[test]
        #[ignore]
        fn obj_method_shorthand() {
            let source = r#"const x = { method() { return 1; } };"#;
            let expr = parse_expr(source);
            assert!(!matches!(expr, Expr::Invalid), "should parse method shorthand");
        }

        /// Getter in object: { get value() { return 1; } } - not supported
        #[test]
        #[ignore]
        fn obj_getter() {
            let source = r#"const x = { get value() { return 1; } };"#;
            let expr = parse_expr(source);
            assert!(!matches!(expr, Expr::Invalid), "should parse getter");
        }

        /// Setter in object: { set value(v) { } } - not supported
        #[test]
        #[ignore]
        fn obj_setter() {
            let source = r#"const x = { set value(v) { } };"#;
            let expr = parse_expr(source);
            assert!(!matches!(expr, Expr::Invalid), "should parse setter");
        }

        /// Empty object: {}
        #[test]
        fn obj_empty() {
            let source = r#"const x = {};"#;
            let expr = parse_expr(source);
            assert!(!matches!(expr, Expr::Invalid), "should parse empty object");
            if let Expr::Object { members } = &expr {
                assert!(members.is_empty(), "should have 0 members");
            }
        }

        /// Object with numeric key: { 1: "one" }
        #[test]
        fn obj_numeric_key() {
            let source = r#"const x = { 1: "one" };"#;
            let expr = parse_expr(source);
            assert!(!matches!(expr, Expr::Invalid), "should parse object with numeric key");
        }

        /// Object with string key: { "my-key": 123 }
        #[test]
        fn obj_string_key() {
            let source = r#"const x = { "my-key": 123 };"#;
            let expr = parse_expr(source);
            assert!(!matches!(expr, Expr::Invalid), "should parse object with string key");
        }

        /// Object with mixed value types
        #[test]
        fn obj_mixed_types() {
            let source = r#"const x = { a: 1, b: "hello", c: true, d: null };"#;
            let expr = parse_expr(source);
            assert!(!matches!(expr, Expr::Invalid), "should parse object with mixed types");
            if let Expr::Object { members } = &expr {
                assert_eq!(members.len(), 4, "should have 4 members");
            }
        }

        /// Multiple spreads in object
        #[test]
        fn obj_multiple_spreads() {
            let source = r#"const x = { ...a, ...b };"#;
            let expr = parse_expr(source);
            assert!(!matches!(expr, Expr::Invalid), "should parse multiple spreads");
            if let Expr::Object { members } = &expr {
                let spread_count = members.iter().filter(|m| matches!(m.prop, ObjectProp::Spread { .. })).count();
                assert_eq!(spread_count, 2, "should have 2 spread properties");
            }
        }

        /// Object with only spread
        #[test]
        fn obj_only_spread() {
            let source = r#"const x = { ...obj };"#;
            let expr = parse_expr(source);
            assert!(!matches!(expr, Expr::Invalid), "should parse object with only spread");
            if let Expr::Object { members } = &expr {
                assert_eq!(members.len(), 1, "should have 1 member (spread)");
                assert!(matches!(&members[0].prop, ObjectProp::Spread { .. }), "should be spread");
            }
        }
    

}
