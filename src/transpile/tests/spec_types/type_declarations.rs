use super::helpers::*;
    mod type_declarations {
        use super::*;

        #[test]
        #[ignore = "parser not yet capturing interface declarations"]
        fn test_parse_interface_simple() {
            let source = "interface Foo { bar: number; }";
            let decl = find_type_decl(source, "Foo");
            assert!(decl.is_some());
            assert!(matches!(decl.unwrap().type_, Type::Object { .. }));
        }

        #[test]
        #[ignore = "parser not yet capturing interface declarations"]
        fn test_parse_interface_with_optional() {
            let source = "interface Config { port?: number; host?: string; }";
            let decl = find_type_decl(source, "Config").unwrap();
            if let Type::Object { members } = decl.type_ {
                assert!(members.iter().any(|m| m.optional));
            }
        }

        #[test]
        #[ignore = "parser not yet capturing type alias declarations"]
        fn test_parse_type_alias_primitive() {
            let source = "type Status = string;";
            let decl = find_type_decl(source, "Status");
            assert!(decl.is_some());
            assert!(matches!(decl.unwrap().type_, Type::String));
        }

        #[test]
        #[ignore = "parser not yet capturing type alias declarations"]
        fn test_parse_type_alias_union() {
            let source = r#"type Status = "ok" | "err";"#;
            let decl = find_type_decl(source, "Status").unwrap();
            assert!(matches!(decl.type_, Type::Union { .. }));
        }

        #[test]
        #[ignore = "parser not yet capturing type alias declarations"]
        fn test_parse_type_alias_object() {
            let source = "type Point = { x: number; y: number; };";
            let decl = find_type_decl(source, "Point").unwrap();
            assert!(matches!(decl.type_, Type::Object { .. }));
        }

        #[test]
        #[ignore = "parser not yet capturing type alias declarations"]
        fn test_parse_type_alias_intersection() {
            let source = "type Combined = { a: string } & { b: number };";
            let decl = find_type_decl(source, "Combined").unwrap();
            assert!(matches!(decl.type_, Type::Intersection { .. }));
        }

        #[test]
        #[ignore = "parser not yet capturing generic function type params"]
        fn test_parse_generic_function() {
            let source = "function identity<T>(x: T): T { return x; }";
            let parser = TsParser::new();
            let result = parser.parse_source(source).unwrap();
            for item in &result.items {
                if let ModuleItem::Decl(Decl::Function(f)) = item {
                    assert!(!f.generics.is_empty());
                    assert_eq!(f.generics[0].name, "T");
                    return;
                }
            }
            panic!("expected function declaration");
        }

        #[test]
        fn test_codegen_generic_function() {
            let source = "function identity<T>(x: T): T { return x; }";
            let parser = TsParser::new();
            let module = parser.parse_source(source).unwrap();
            let cg = QuoteCodegen::default();
            for item in &module.items {
                if let ModuleItem::Decl(Decl::Function(f)) = item {
                    let tokens = cg.gen_fn(f);
                    let s = tokens.to_string();
                    assert!(s.contains("fn") || s.contains("Fn"), "got: {}", s);
                    return;
                }
            }
        }

        #[test]
        #[ignore = "parser not yet capturing interface declarations"]
        fn test_parse_generic_interface() {
            let source = "interface Container<T> { value: T; }";
            let decl = find_type_decl(source, "Container");
            assert!(decl.is_some());
            let decl = decl.unwrap();
            assert!(!decl.generics.is_empty());
            assert_eq!(decl.generics[0].name, "T");
        }

        #[test]
        #[ignore = "parser not yet capturing type alias declarations"]
        fn test_parse_generic_type_alias() {
            let source = "type Pair<A, B> = { first: A; second: B; };";
            let decl = find_type_decl(source, "Pair");
            assert!(decl.is_some());
            assert_eq!(decl.unwrap().generics.len(), 2);
        }
    

}
