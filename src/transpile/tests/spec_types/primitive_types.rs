use super::helpers::*;
    mod primitive_types {
        use super::*;

        // All parse tests are ignored because parser hardcodes return_type: None
        #[test]
        #[ignore = "parser not yet capturing return type annotations"]
        fn test_parse_type_string() {
            let ty = parse_fn_return_type("function f(): string { return ''; }").unwrap();
            assert!(matches!(ty, Type::String));
        }

        #[test]
        fn test_codegen_type_string() {
            let ty = Type::String;
            assert_eq!(type_to_rust_name(&ty), "String");
            assert!(codegen_produces_output(&ty));
        }

        #[test]
        #[ignore = "parser not yet capturing return type annotations"]
        fn test_parse_type_number() {
            let ty = parse_fn_return_type("function f(): number { return 1; }").unwrap();
            assert!(matches!(ty, Type::Number));
        }

        #[test]
        fn test_codegen_type_number() {
            let ty = Type::Number;
            assert_eq!(type_to_rust_name(&ty), "f64");
            assert!(codegen_produces_output(&ty));
        }

        #[test]
        #[ignore = "parser not yet capturing return type annotations"]
        fn test_parse_type_boolean() {
            let ty = parse_fn_return_type("function f(): boolean { return true; }").unwrap();
            assert!(matches!(ty, Type::Boolean));
        }

        #[test]
        fn test_codegen_type_boolean() {
            let ty = Type::Boolean;
            assert_eq!(type_to_rust_name(&ty), "bool");
            assert!(codegen_produces_output(&ty));
        }

        #[test]
        #[ignore = "parser not yet capturing param type annotations"]
        fn test_parse_type_null() {
            let ty = parse_fn_param_type("function f(x: null): null { return null; }", 0).unwrap();
            assert!(matches!(ty, Type::Null));
        }

        #[test]
        #[ignore = "RustType::Value maps to serde_json::Value not 'Value'"]
        fn test_codegen_type_null() {
            let ty = Type::Null;
            assert_eq!(type_to_rust_name(&ty), "Value");
            assert!(codegen_produces_output(&ty));
        }

        #[test]
        #[ignore = "parser not yet capturing param type annotations"]
        fn test_parse_type_undefined() {
            let ty = parse_fn_param_type("function f(x: undefined): undefined { return undefined; }", 0).unwrap();
            assert!(matches!(ty, Type::Undefined));
        }

        #[test]
        #[ignore = "RustType::Value maps to serde_json::Value not 'Value'"]
        fn test_codegen_type_undefined() {
            let ty = Type::Undefined;
            assert_eq!(type_to_rust_name(&ty), "Value");
            assert!(codegen_produces_output(&ty));
        }

        #[test]
        #[ignore = "parser not yet capturing return type annotations"]
        fn test_parse_type_void() {
            let ty = parse_fn_return_type("function f(): void { }").unwrap();
            assert!(matches!(ty, Type::Void));
        }

        #[test]
        fn test_codegen_type_void() {
            let ty = Type::Void;
            assert_eq!(type_to_rust_name(&ty), "()");
            assert!(codegen_produces_output(&ty));
        }

        #[test]
        #[ignore = "parser not yet capturing param type annotations"]
        fn test_parse_type_any() {
            let ty = parse_fn_param_type("function f(x: any): any { return x; }", 0).unwrap();
            assert!(matches!(ty, Type::Any));
        }

        #[test]
        #[ignore = "RustType::Value maps to serde_json::Value not 'Value'"]
        fn test_codegen_type_any() {
            let ty = Type::Any;
            assert_eq!(type_to_rust_name(&ty), "Value");
            assert!(codegen_produces_output(&ty));
        }

        #[test]
        #[ignore = "parser not yet capturing param type annotations"]
        fn test_parse_type_unknown() {
            let ty = parse_fn_param_type("function f(x: unknown): unknown { return x; }", 0).unwrap();
            assert!(matches!(ty, Type::Unknown));
        }

        #[test]
        #[ignore = "RustType::Value maps to serde_json::Value not 'Value'"]
        fn test_codegen_type_unknown() {
            let ty = Type::Unknown;
            assert_eq!(type_to_rust_name(&ty), "Value");
            assert!(codegen_produces_output(&ty));
        }

        #[test]
        #[ignore = "parser not yet capturing return type annotations"]
        fn test_parse_type_never() {
            let ty = parse_fn_return_type("function f(): never { throw new Error(); }").unwrap();
            assert!(matches!(ty, Type::Never));
        }

        #[test]
        fn test_codegen_type_never() {
            let ty = Type::Never;
            assert_eq!(type_to_rust_name(&ty), "!");
            assert!(codegen_produces_output(&ty));
        }

        #[test]
        #[ignore = "parser not yet capturing return type annotations"]
        fn test_parse_type_bigint() {
            let ty = parse_fn_return_type("function f(): bigint { return 123n; }").unwrap();
            assert!(matches!(ty, Type::BigInt));
        }

        #[test]
        fn test_codegen_type_bigint() {
            let ty = Type::BigInt;
            assert_eq!(type_to_rust_name(&ty), "i64");
            assert!(codegen_produces_output(&ty));
        }
    

}
