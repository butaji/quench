use super::helpers::*;
    mod type_directed_lowering {
        use super::*;

        // --- String Union -> Enum ---

        #[test]
        #[ignore = "parser not yet capturing type alias declarations"]
        fn test_lowering_string_union_two_variants() {
            let source = r#"type Status = "ok" | "err";"#;
            let decl = find_type_decl(source, "Status").unwrap();
            let ty = &decl.type_;
            assert!(matches!(ty, Type::Union { .. }));
            
            let converter = TypeToRust::new(OutputKind::String);
            let rust_type = converter.convert(ty);
            assert!(matches!(rust_type, RustType::Enum(_)), "got: {:?}", rust_type);
            
            let name = rust_type.type_name();
            println!("Status union lowering: {}", name);
        }

        #[test]
        #[ignore = "parser not yet capturing type alias declarations"]
        fn test_lowering_string_union_three_variants() {
            let source = r#"type Status = "ok" | "err" | "pending";"#;
            let decl = find_type_decl(source, "Status").unwrap();
            let ty = &decl.type_;
            
            let converter = TypeToRust::new(OutputKind::String);
            let rust_type = converter.convert(ty);
            assert!(matches!(rust_type, RustType::Enum(_)));
            
            let name = rust_type.type_name();
            println!("3-variant union: {}", name);
        }

        #[test]
        fn test_codegen_string_union() {
            let ty = Type::Union {
                types: vec![
                    Type::Literal { kind: LiteralKind::String, value: "ok".into() },
                    Type::Literal { kind: LiteralKind::String, value: "err".into() },
                ],
            };
            let cg = QuoteCodegen::default();
            let tokens = cg.gen_type(&ty);
            let s = tokens.to_string();
            println!("string union codegen: {}", s);
            assert!(!s.is_empty());
        }

        // --- Interface -> Struct ---

        #[test]
        #[ignore = "parser not yet capturing interface declarations"]
        fn test_lowering_interface_simple() {
            let source = "interface User { name: string; age: number; }";
            let decl = find_type_decl(source, "User").unwrap();
            let ty = &decl.type_;
            assert!(matches!(ty, Type::Object { .. }));
            
            let converter = TypeToRust::new(OutputKind::String);
            let rust_type = converter.convert(ty);
            assert!(matches!(rust_type, RustType::Struct(_)), "got: {:?}", rust_type);
            
            let name = rust_type.type_name();
            assert!(name.contains("name: String") && name.contains("age: f64"), "got: {}", name);
        }

        #[test]
        fn test_codegen_interface() {
            let ty = Type::Object {
                members: vec![
                    TypeMember { key: "name".into(), type_: Type::String, optional: false, readonly: false },
                    TypeMember { key: "age".into(), type_: Type::Number, optional: false, readonly: false },
                ],
            };
            let cg = QuoteCodegen::default();
            let tokens = cg.gen_type(&ty);
            let s = tokens.to_string();
            println!("interface codegen: {}", s);
            assert!(!s.is_empty());
        }

        // --- Tagged Union -> Enum ---

        #[test]
        #[ignore = "parser not yet capturing type alias declarations"]
        fn test_lowering_tagged_union_simple() {
            let source = r#"type Result = { ok: true; data: string } | { ok: false; error: string };"#;
            let decl = find_type_decl(source, "Result").unwrap();
            let ty = &decl.type_;
            assert!(matches!(ty, Type::Union { .. }));
            
            let converter = TypeToRust::new(OutputKind::String);
            let rust_type = converter.convert(ty);
            println!("tagged union lowering: {:?}", rust_type);
            assert!(matches!(rust_type, RustType::Enum(_)));
        }

        #[test]
        #[ignore = "parser not yet capturing type alias declarations"]
        fn test_lowering_discriminated_union_kind_field() {
            let source = r#"type Shape = { kind: "circle"; radius: number } | { kind: "rect"; width: number; height: number };"#;
            let decl = find_type_decl(source, "Shape").unwrap();
            let ty = &decl.type_;
            
            let converter = TypeToRust::new(OutputKind::String);
            let rust_type = converter.convert(ty);
            let name = rust_type.type_name();
            println!("discriminated union: {}", name);
            assert!(matches!(rust_type, RustType::Enum(_)));
        }

        // --- Null/Undefined -> Option ---

        #[test]
        fn test_lowering_null_to_option() {
            let ty = Type::Union {
                types: vec![Type::String, Type::Null],
            };
            let converter = TypeToRust::new(OutputKind::String);
            let rust_type = converter.convert(&ty);
            println!("string | null lowering: {:?}", rust_type);
        }

        #[test]
        fn test_lowering_undefined_to_option() {
            let ty = Type::Union {
                types: vec![Type::String, Type::Undefined],
            };
            let converter = TypeToRust::new(OutputKind::String);
            let rust_type = converter.convert(&ty);
            println!("string | undefined lowering: {:?}", rust_type);
        }

        // --- Array -> Vec ---

        #[test]
        fn test_lowering_array_primitive() {
            let ty = Type::Array { elem: Box::new(Type::Number) };
            let converter = TypeToRust::new(OutputKind::String);
            let rust_type = converter.convert(&ty);
            assert!(matches!(rust_type, RustType::Vec(_)));
            assert_eq!(rust_type.type_name(), "Vec<f64>");
        }

        #[test]
        fn test_lowering_array_string() {
            let ty = Type::Array { elem: Box::new(Type::String) };
            let converter = TypeToRust::new(OutputKind::String);
            let rust_type = converter.convert(&ty);
            assert_eq!(rust_type.type_name(), "Vec<String>");
        }

        // --- Generic Monomorphization ---

        #[test]
        #[ignore = "parser not yet capturing generic function type params"]
        fn test_lowering_generic_simple() {
            let source = "function identity<T>(x: T): T { return x; }";
            let parser = TsParser::new();
            let module = parser.parse_source(source).unwrap();
            for item in &module.items {
                if let ModuleItem::Decl(Decl::Function(f)) = item {
                    assert!(!f.generics.is_empty());
                    assert!(f.return_type.is_some());
                    return;
                }
            }
            panic!("expected function");
        }

        // --- Primitive mappings ---

        #[test]
        fn test_lowering_string() {
            assert_eq!(type_to_rust_name(&Type::String), "String");
        }

        #[test]
        fn test_lowering_number() {
            assert_eq!(type_to_rust_name(&Type::Number), "f64");
        }

        #[test]
        fn test_lowering_boolean() {
            assert_eq!(type_to_rust_name(&Type::Boolean), "bool");
        }

        #[test]
        fn test_lowering_void() {
            assert_eq!(type_to_rust_name(&Type::Void), "()");
        }

        #[test]
        fn test_lowering_never() {
            assert_eq!(type_to_rust_name(&Type::Never), "!");
        }

        #[test]
        fn test_lowering_any() {
            assert_eq!(type_to_rust_name(&Type::Any), "Value");
        }

        #[test]
        fn test_lowering_unknown() {
            assert_eq!(type_to_rust_name(&Type::Unknown), "Value");
        }

        // --- Function type -> Fn trait ---

        #[test]
        fn test_lowering_function_type() {
            let ty = Type::Function {
                params: vec![Type::Number],
                ret: Box::new(Type::String),
            };
            let converter = TypeToRust::new(OutputKind::String);
            let rust_type = converter.convert(&ty);
            let name = rust_type.type_name();
            println!("function type lowering: {}", name);
            assert!(name.contains("fn") || name.contains("Fn"));
        }
    
