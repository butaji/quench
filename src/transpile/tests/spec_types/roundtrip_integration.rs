use super::helpers::*;
    mod roundtrip_integration {
        use super::*;

        #[test]
        #[ignore = "parser not yet capturing type alias declarations"]
        fn test_roundtrip_type_alias_string_union() {
            let source = r#"type Status = "ok" | "err" | "pending";"#;
            let decl = find_type_decl(source, "Status").unwrap();
            let name = type_to_rust_name(&decl.type_);
            println!("Status type alias: {}", name);
            assert!(!name.is_empty());
        }

        #[test]
        #[ignore = "parser not yet capturing interface declarations"]
        fn test_roundtrip_interface_with_all_primitives() {
            let source = "interface AllPrimitives { s: string; n: number; b: boolean; }";
            let decl = find_type_decl(source, "AllPrimitives").unwrap();
            let name = type_to_rust_name(&decl.type_);
            assert!(name.contains("s:") && name.contains("String"));
            assert!(name.contains("n:") && name.contains("f64"));
            assert!(name.contains("b:") && name.contains("bool"));
        }

        #[test]
        #[ignore = "parser not yet capturing function type annotations"]
        fn test_roundtrip_function_with_type_annotations() {
            let source = "function greet(name: string, age: number): string { return ''; }";
            let parser = TsParser::new();
            let module = parser.parse_source(source).unwrap();
            for item in &module.items {
                if let ModuleItem::Decl(Decl::Function(f)) = item {
                    assert!(f.params[0].type_.is_some());
                    assert!(f.params[1].type_.is_some());
                    assert!(f.return_type.is_some());
                    assert_eq!(type_to_rust_name(f.params[0].type_.as_ref().unwrap()), "String");
                    assert_eq!(type_to_rust_name(f.params[1].type_.as_ref().unwrap()), "f64");
                    assert_eq!(type_to_rust_name(f.return_type.as_ref().unwrap()), "String");
                    return;
                }
            }
            panic!("expected function");
        }

        #[test]
        #[ignore = "parser not yet capturing generic function type params"]
        fn test_roundtrip_generic_function_identity() {
            let source = "function identity<T>(value: T): T { return value; }";
            let parser = TsParser::new();
            let module = parser.parse_source(source).unwrap();
            for item in &module.items {
                if let ModuleItem::Decl(Decl::Function(f)) = item {
                    assert!(!f.generics.is_empty());
                    assert_eq!(f.generics[0].name, "T");
                    println!("generic param: {}", type_to_rust_name(f.params[0].type_.as_ref().unwrap()));
                    println!("generic return: {}", type_to_rust_name(f.return_type.as_ref().unwrap()));
                    return;
                }
            }
            panic!("expected function");
        }

        #[test]
        #[ignore = "parser not yet capturing type alias declarations"]
        fn test_roundtrip_tagged_union_result() {
            let source = r#"type Result = { ok: true; data: string } | { ok: false; error: string };"#;
            let decl = find_type_decl(source, "Result").unwrap();
            let name = type_to_rust_name(&decl.type_);
            println!("Result tagged union: {}", name);
        }

        #[test]
        #[ignore = "parser not yet capturing type alias declarations"]
        fn test_roundtrip_shape_discriminated_union() {
            let source = r#"type Shape = { kind: "circle"; radius: number } | { kind: "rect"; width: number; height: number };"#;
            let decl = find_type_decl(source, "Shape").unwrap();
            let name = type_to_rust_name(&decl.type_);
            println!("Shape discriminated union: {}", name);
        }
    

}
