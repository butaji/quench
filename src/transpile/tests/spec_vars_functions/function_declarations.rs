use super::helpers::*;
    mod function_declarations {
        use super::*;

        #[test]
        fn basic_function_decl() {
            let func = find_function("function foo() {}");
            assert_eq!(func.name, "foo");
            assert!(func.params.is_empty());
            assert!(!func.is_async);
            let tokens = codegen_fn(&func);
            assert!(!tokens.is_empty(), "function decl codegen should produce output");
        }

        #[test]
        fn function_with_body() {
            let func = find_function("function foo() { return 1; }");
            assert!(func.body.is_some());
            let tokens = codegen_fn(&func);
            let s = tokens.to_string();
            assert!(s.contains("fn foo") || s.contains("foo"), "should contain function name");
        }

        #[test]
        fn function_with_single_param() {
            let func = find_function("function foo(x) {}");
            assert_eq!(func.params.len(), 1);
            assert_eq!(func.params[0].name, "x");
            let tokens = codegen_fn(&func);
            assert!(!tokens.is_empty());
        }

        #[test]
        fn function_with_multiple_params() {
            let func = find_function("function f(a, b, c) {}");
            assert_eq!(func.params.len(), 3);
            assert_eq!(func.params[0].name, "a");
            assert_eq!(func.params[1].name, "b");
            assert_eq!(func.params[2].name, "c");
            let tokens = codegen_fn(&func);
            assert!(!tokens.is_empty());
        }

        #[test]
        #[ignore = "Parser does not capture return type annotations"]
        fn function_with_return_type() {
            let func = find_function("function f(): number { return 1; }");
            assert!(func.return_type.is_some());
            if let Some(Type::Number) = func.return_type.as_ref() {
                // correct
            } else {
                panic!("expected number return type, got {:?}", func.return_type);
            }
            let tokens = codegen_fn(&func);
            let s = tokens.to_string();
            assert!(!s.is_empty());
        }

        #[test]
        #[ignore = "Parser does not capture return type annotations"]
        fn function_with_void_return() {
            let func = find_function("function f(): void { return; }");
            assert!(func.return_type.is_some());
            let tokens = codegen_fn(&func);
            assert!(!tokens.is_empty());
        }

        #[test]
        #[ignore = "Parser does not capture return type annotations"]
        fn function_with_string_return_type() {
            let func = find_function("function f(): string { return 'hi'; }");
            assert!(func.return_type.is_some());
            let tokens = codegen_fn(&func);
            assert!(!tokens.is_empty());
        }

        #[test]
        #[ignore = "Parser does not capture return type annotations"]
        fn function_with_boolean_return_type() {
            let func = find_function("function f(): boolean { return true; }");
            assert!(func.return_type.is_some());
            let tokens = codegen_fn(&func);
            assert!(!tokens.is_empty());
        }

        #[test]
        fn function_no_return_type_annotation() {
            let func = find_function("function f() { return 1; }");
            assert!(func.return_type.is_none());
            let tokens = codegen_fn(&func);
            assert!(!tokens.is_empty());
        }

        #[test]
        fn function_generator() {
            let func = find_function("function* g() { yield 1; }");
            assert!(func.is_generator);
            let tokens = codegen_fn(&func);
            assert!(!tokens.is_empty());
        }
    
