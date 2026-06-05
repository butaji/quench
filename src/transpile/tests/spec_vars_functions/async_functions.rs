use super::helpers::*;
    mod async_functions {
        use super::*;

        #[test]
        fn async_function_basic() {
            let func = find_function("async function foo() {}");
            assert!(func.is_async);
            let tokens = codegen_fn(&func);
            assert!(!tokens.is_empty(), "async function codegen should produce output");
        }

        #[test]
        fn async_function_with_await() {
            let func = find_function("async function foo() { return await Promise.resolve(1); }");
            assert!(func.is_async);
            assert!(func.body.is_some());
            let tokens = codegen_fn(&func);
            assert!(!tokens.is_empty());
        }

        #[test]
        fn async_function_no_params() {
            let func = find_function("async function f() {}");
            assert!(func.is_async);
            assert!(func.params.is_empty());
            let tokens = codegen_fn(&func);
            assert!(!tokens.is_empty());
        }

        #[test]
        fn async_function_with_params() {
            let func = find_function("async function f(x, y) {}");
            assert!(func.is_async);
            assert_eq!(func.params.len(), 2);
            let tokens = codegen_fn(&func);
            assert!(!tokens.is_empty());
        }

        #[test]
        #[ignore = "Parser does not capture return type annotations"]
        fn async_function_return_type() {
            let func = find_function("async function f(): Promise<number> { return Promise.resolve(1); }");
            assert!(func.is_async);
            assert!(func.return_type.is_some());
            let tokens = codegen_fn(&func);
            assert!(!tokens.is_empty());
        }
    
