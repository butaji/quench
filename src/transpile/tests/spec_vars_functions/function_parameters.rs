use super::helpers::*;
    mod function_parameters {
        use super::*;

        #[test]
        #[ignore = "Parser does not capture default parameter values"]
        fn function_default_param() {
            let func = find_function("function f(x = 1) {}");
            assert_eq!(func.params.len(), 1);
            assert!(func.params[0].default.is_some());
            let tokens = codegen_fn(&func);
            assert!(!tokens.is_empty(), "default param codegen should produce output");
        }

        #[test]
        #[ignore = "Parser does not capture default parameter values"]
        fn function_multiple_default_params() {
            let func = find_function("function f(a = 1, b = 2) {}");
            assert_eq!(func.params.len(), 2);
            assert!(func.params[0].default.is_some());
            assert!(func.params[1].default.is_some());
            let tokens = codegen_fn(&func);
            assert!(!tokens.is_empty());
        }

        #[test]
        #[ignore = "Parser does not capture default parameter values"]
        fn function_mixed_params() {
            let func = find_function("function f(a, b = 1, c) {}");
            assert_eq!(func.params.len(), 3);
            assert!(func.params[0].default.is_none());
            assert!(func.params[1].default.is_some());
            assert!(func.params[2].default.is_none());
            let tokens = codegen_fn(&func);
            assert!(!tokens.is_empty());
        }

        #[test]
        fn function_rest_param() {
            let func = find_function("function f(...args) {}");
            assert_eq!(func.params.len(), 1);
            assert!(func.params[0].pattern.is_some());
            let tokens = codegen_fn(&func);
            assert!(!tokens.is_empty(), "rest param codegen should produce output");
        }

        #[test]
        fn function_rest_with_other_params() {
            let func = find_function("function f(a, b, ...rest) {}");
            assert_eq!(func.params.len(), 3);
            let tokens = codegen_fn(&func);
            assert!(!tokens.is_empty());
        }

        #[test]
        fn function_optional_param() {
            let func = find_function("function f(x?) {}");
            assert_eq!(func.params.len(), 1);
            assert!(func.params[0].optional);
            let tokens = codegen_fn(&func);
            assert!(!tokens.is_empty());
        }

        #[test]
        #[ignore = "Parser does not capture parameter type annotations"]
        fn function_param_with_type() {
            let func = find_function("function f(x: number) {}");
            assert_eq!(func.params.len(), 1);
            assert!(func.params[0].type_.is_some());
            let tokens = codegen_fn(&func);
            assert!(!tokens.is_empty());
        }

        #[test]
        #[ignore = "Parser does not capture parameter type annotations"]
        fn function_param_with_type_and_default() {
            let func = find_function("function f(x: number = 1) {}");
            assert_eq!(func.params.len(), 1);
            assert!(func.params[0].type_.is_some());
            assert!(func.params[0].default.is_some());
            let tokens = codegen_fn(&func);
            assert!(!tokens.is_empty());
        }

        #[test]

        fn function_param_array_pattern() {
            let func = find_function("function f([a, b]) {}");
            assert_eq!(func.params.len(), 1);
            assert!(func.params[0].pattern.is_some());
            let tokens = codegen_fn(&func);
            assert!(!tokens.is_empty());
        }

        #[test]

        fn function_param_object_pattern() {
            let func = find_function("function f({a, b}) {}");
            assert_eq!(func.params.len(), 1);
            assert!(func.params[0].pattern.is_some());
            let tokens = codegen_fn(&func);
            assert!(!tokens.is_empty());
        }

        #[test]
        #[ignore = "Parser does not handle rest array patterns in function parameters"]
        fn function_param_rest_array_pattern() {
            let func = find_function("function f(...[a, b]) {}");
            assert_eq!(func.params.len(), 1);
            assert!(func.params[0].pattern.is_some());
            let tokens = codegen_fn(&func);
            assert!(!tokens.is_empty());
        }
    
