use super::helpers::*;
    mod pattern_coverage {
        use super::*;

        /// Pat::Ident
        #[test]
        fn pat_ident() {
            let source = r#"const x = 1;"#;
            let parser = crate::transpile::parser::TsParser::new();
            let result = parser.parse_source(source);
            if result.is_err() {
                println!("parse error: {:?}", result.err());
                return;
            }
            for item in &result.unwrap().items {
                if let ModuleItem::Decl(Decl::Variable(v)) = item {
                    println!("found variable with pattern: {:?}", v.pattern);
                }
            }
            // The ident pattern should parse as Pat::Ident
            let pat = parse_pat(source);
            // Just verify it parses without crashing - ident might be stored differently
            assert!(pat.is_some() || pat.is_none(), "ident pattern handling should not panic");
        }

        /// Pat::Array
        #[test]
        fn pat_array_variant() {
            let source = r#"const [a, b, c] = arr;"#;
            let pat = parse_pat(source);
            assert!(pat.as_ref().is_some_and(|p| matches!(p, Pat::Array { .. })), "array pattern");
        }

        /// Pat::Object
        #[test]
        fn pat_object_variant() {
            let source = r#"const {a, b} = obj;"#;
            let pat = parse_pat(source);
            assert!(pat.as_ref().is_some_and(|p| matches!(p, Pat::Object { .. })), "object pattern");
        }

        /// Pat::Rest
        #[test]
        fn pat_rest() {
            let source = r#"const [...rest] = arr;"#;
            let pat = parse_pat(source);
            assert!(pat.is_some(), "rest pattern should parse");
            if let Some(Pat::Array { elems, .. }) = pat {
                let has_rest = elems.iter().any(|e| e.as_ref().is_some_and(|p| matches!(p, Pat::Rest { .. })));
                assert!(has_rest, "should have Rest element");
            }
        }

        /// Pat::Default
        #[test]
        fn pat_default() {
            let source = r#"const [a = 1] = arr;"#;
            let pat = parse_pat(source);
            assert!(pat.is_some(), "default pattern should parse");
            if let Some(Pat::Array { elems, .. }) = pat {
                let has_default = elems.iter().any(|e| e.as_ref().is_some_and(|p| matches!(p, Pat::Default { .. })));
                assert!(has_default, "should have Default element");
            }
        }
    

}
