use super::helpers::*;
    mod integration_codegen {
        use super::*;

        #[test]
        fn roundtrip_const_binding() {
            let decl = parse_first_decl("const x = 5;");
            let tokens = codegen_decl(&decl);
            let s = tokens.to_string();
            assert!(!s.is_empty(), "const binding codegen failed");
        }

        #[test]
        fn roundtrip_let_binding() {
            let decl = parse_first_decl("let x = 5;");
            let tokens = codegen_decl(&decl);
            let s = tokens.to_string();
            assert!(!s.is_empty(), "let binding codegen failed");
        }

        #[test]
        fn roundtrip_var_binding() {
            let decl = parse_first_decl("var x = 5;");
            let tokens = codegen_decl(&decl);
            let s = tokens.to_string();
            assert!(!s.is_empty(), "var binding codegen failed");
        }

        #[test]

        fn roundtrip_object_destructure() {
            let decl = parse_first_decl("const {a, b} = obj;");
            let tokens = codegen_decl(&decl);
            let s = tokens.to_string();
            assert!(!s.is_empty(), "object destructure codegen failed");
        }

        #[test]

        fn roundtrip_array_destructure() {
            let decl = parse_first_decl("const [a, b] = arr;");
            let tokens = codegen_decl(&decl);
            let s = tokens.to_string();
            assert!(!s.is_empty(), "array destructure codegen failed");
        }

        #[test]
        fn roundtrip_function_decl() {
            let func = find_function("function foo() {}");
            let tokens = codegen_fn(&func);
            let s = tokens.to_string();
            assert!(!s.is_empty(), "function decl codegen failed");
        }

        #[test]
        fn roundtrip_arrow_function() {
            let expr = find_expr_in_var("const f = () => {};");
            let tokens = codegen_expr(&expr);
            let s = tokens.to_string();
            assert!(!s.is_empty(), "arrow function codegen failed");
        }

        #[test]
        fn roundtrip_async_function() {
            let func = find_function("async function foo() {}");
            let tokens = codegen_fn(&func);
            let s = tokens.to_string();
            assert!(!s.is_empty(), "async function codegen failed");
        }

        #[test]
        fn roundtrip_function_with_all_param_types() {
            let func = find_function("function f(a: number, b = 1, ...rest: number[]) {}");
            let tokens = codegen_fn(&func);
            let s = tokens.to_string();
            assert!(!s.is_empty(), "complex function codegen failed");
        }
    

}
