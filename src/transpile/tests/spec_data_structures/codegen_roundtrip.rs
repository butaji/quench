use super::helpers::*;
    mod codegen_roundtrip {
        use super::*;

        /// Verify object codegen doesn't panic
        #[test]
        fn codegen_object() {
            let source = r#"const x = { a: 1, b: "two" };"#;
            let expr = parse_expr(source);
            let tokens = QuoteCodegen::default().gen_expr(&expr);
            let s = tokens.to_string();
            assert!(!s.is_empty(), "codegen should produce output");
        }

        /// Verify array codegen doesn't panic
        #[test]
        fn codegen_array() {
            let source = r#"const x = [1, 2, 3];"#;
            let expr = parse_expr(source);
            let tokens = QuoteCodegen::default().gen_expr(&expr);
            let s = tokens.to_string();
            assert!(!s.is_empty(), "codegen should produce output");
        }

        /// Verify nested object codegen doesn't panic
        #[test]
        fn codegen_nested_object() {
            let source = r#"const x = { a: { b: 1 } };"#;
            let expr = parse_expr(source);
            let tokens = QuoteCodegen::default().gen_expr(&expr);
            let s = tokens.to_string();
            assert!(!s.is_empty(), "codegen should produce output");
        }

        /// Verify empty object codegen doesn't panic
        #[test]
        fn codegen_empty_object() {
            let source = r#"const x = {};"#;
            let expr = parse_expr(source);
            let tokens = QuoteCodegen::default().gen_expr(&expr);
            let s = tokens.to_string();
            assert!(!s.is_empty(), "codegen should produce output");
        }

        /// Verify empty array codegen doesn't panic
        #[test]
        fn codegen_empty_array() {
            let source = r#"const x = [];"#;
            let expr = parse_expr(source);
            let tokens = QuoteCodegen::default().gen_expr(&expr);
            let s = tokens.to_string();
            assert!(!s.is_empty(), "codegen should produce output");
        }

        /// Verify array with spread codegen doesn't panic
        #[test]
        fn codegen_array_spread() {
            let source = r#"const x = [...arr, 1];"#;
            let expr = parse_expr(source);
            let tokens = QuoteCodegen::default().gen_expr(&expr);
            let s = tokens.to_string();
            assert!(!s.is_empty(), "codegen should produce output");
        }

        /// Verify object with spread codegen doesn't panic
        #[test]
        fn codegen_object_spread() {
            let source = r#"const x = { ...obj, c: 3 };"#;
            let expr = parse_expr(source);
            let tokens = QuoteCodegen::default().gen_expr(&expr);
            let s = tokens.to_string();
            assert!(!s.is_empty(), "codegen should produce output");
        }
    

}
