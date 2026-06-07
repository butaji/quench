use super::helpers::*;
    mod codegen_verification {
        use super::*;

        #[test]
        fn test_all_primitive_types_codegen() {
            let types = vec![
                Type::String,
                Type::Number,
                Type::Boolean,
                Type::Null,
                Type::Undefined,
                Type::Void,
                Type::Never,
                Type::Any,
                Type::Unknown,
                Type::BigInt,
            ];
            for ty in types {
                let tokens = QuoteCodegen::default().gen_type(&ty);
                assert!(!tokens.is_empty(), "Type {:?} should produce output", ty);
            }
        }

        #[test]
        fn test_all_complex_types_codegen() {
            let types = vec![
                Type::Array { elem: Box::new(Type::String) },
                Type::Ref { name: "MyType".into(), generics: vec![] },
                Type::Ref { name: "Result".into(), generics: vec![Type::String] },
                Type::Union { types: vec![Type::String, Type::Number] },
                Type::Intersection { types: vec![Type::Object { members: vec![] }, Type::Object { members: vec![] }] },
                Type::Function { params: vec![Type::Number], ret: Box::new(Type::String) },
                Type::Object { members: vec![TypeMember { key: "x".into(), type_: Type::Number, optional: false, readonly: false }] },
            ];
            for ty in types {
                let tokens = QuoteCodegen::default().gen_type(&ty);
                assert!(!tokens.is_empty(), "Type {:?} should produce output", ty);
            }
        }

        #[test]
        fn test_literal_types_codegen() {
            let types = vec![
                Type::Literal { kind: LiteralKind::String, value: "ok".into() },
                Type::Literal { kind: LiteralKind::Number, value: "42".into() },
                Type::Literal { kind: LiteralKind::Boolean, value: "true".into() },
            ];
            for ty in types {
                let tokens = QuoteCodegen::default().gen_type(&ty);
                assert!(!tokens.is_empty(), "Literal type {:?} should produce output", ty);
            }
        }

        #[test]
        fn test_type_assertion_expr_codegen() {
            // TypeAssertion { expr, type_ } - should emit inner expr
            let expr = Expr::TypeAssertion {
                expr: Box::new(Expr::Ident { name: "x".into() }),
                type_: Box::new(Type::String),
            };
            let tokens = QuoteCodegen::default().gen_expr(&expr);
            let output = tokens.to_string();
            // Should contain "x" (the inner ident), not type annotation
            assert!(output.contains("x"), "TypeAssertion should emit inner expr");
        }

        #[test]
        fn test_satisfies_expr_codegen() {
            // Satisfies { expr, type_ } - should emit inner expr
            let expr = Expr::Satisfies {
                expr: Box::new(Expr::Number(42.0)),
                type_: Box::new(Type::Number),
            };
            let tokens = QuoteCodegen::default().gen_expr(&expr);
            let output = tokens.to_string();
            assert!(output.contains("42") || output.contains("f64"), "Satisfies should emit inner expr");
        }

        #[test]
        fn test_non_null_expr_codegen() {
            // NonNull { expr } - should emit inner expr
            let expr = Expr::NonNull {
                expr: Box::new(Expr::Ident { name: "value".into() }),
            };
            let tokens = QuoteCodegen::default().gen_expr(&expr);
            let output = tokens.to_string();
            assert!(output.contains("value"), "NonNull should emit inner expr");
        }

        #[test]
        fn test_enum_codegen() {
            // Test enum codegen
            let e = EnumDecl {
                name: "Color".into(),
                members: vec![
                    EnumMember { key: "Red".into(), value: Some(EnumValue::Number(1.0)) },
                    EnumMember { key: "Green".into(), value: Some(EnumValue::Number(2.0)) },
                    EnumMember { key: "Blue".into(), value: None },
                ],
                is_const: false,
            };
            let tokens = QuoteCodegen::default().gen_enum(&e);
            let output = tokens.to_string();
            assert!(output.contains("Color"), "Enum should contain name");
            assert!(output.contains("Red"), "Enum should contain Red");
            assert!(output.contains("Green"), "Enum should contain Green");
            assert!(output.contains("Blue"), "Enum should contain Blue");
        }

        #[test]
        fn test_const_enum_codegen() {
            // Test const enum codegen
            let e = EnumDecl {
                name: "ConstEnum".into(),
                members: vec![
                    EnumMember { key: "A".into(), value: Some(EnumValue::Number(1.0)) },
                ],
                is_const: true,
            };
            let tokens = QuoteCodegen::default().gen_enum(&e);
            let output = tokens.to_string();
            assert!(output.contains("const enum"), "Const enum should contain 'const enum'");
        }
    

}
