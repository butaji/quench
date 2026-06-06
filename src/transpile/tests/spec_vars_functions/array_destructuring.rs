use super::helpers::*;
    mod array_destructuring {
        use super::*;

        #[test]

        fn simple_array_destructure() {
            let decl = parse_first_decl("const [a, b] = arr;");
            match decl {
                Decl::Variable(ref v) => {
                    assert!(v.pattern.is_some());
                    let pat = v.pattern.as_ref().unwrap();
                    assert!(matches!(pat, Pat::Array { .. }));
                }
                _ => panic!("expected variable decl"),
            }
            let tokens = codegen_decl(&decl);
            assert!(!tokens.is_empty(), "array destructure codegen should produce output");
        }

        #[test]

        fn array_destructure_with_rest() {
            let decl = parse_first_decl("const [a, ...rest] = arr;");
            match decl {
                Decl::Variable(ref v) => {
                    assert!(v.pattern.is_some());
                    let pat = v.pattern.as_ref().unwrap();
                    if let Pat::Array { elems, rest } = pat {
                        let has_rest = rest.is_some() || elems.iter().any(|e| matches!(e, Some(Pat::Rest { .. })));
                        assert!(has_rest, "should have rest element");
                    }
                }
                _ => panic!("expected variable decl"),
            }
            let tokens = codegen_decl(&decl);
            assert!(!tokens.is_empty());
        }

        #[test]

        fn array_destructure_with_default() {
            let decl = parse_first_decl("const [a = 1] = arr;");
            match decl {
                Decl::Variable(ref v) => {
                    assert!(v.pattern.is_some());
                    let pat = v.pattern.as_ref().unwrap();
                    if let Pat::Array { elems, .. } = pat {
                        let has_default = elems.iter().any(|e| matches!(e, Some(Pat::Default { .. })));
                        assert!(has_default, "should have default element");
                    }
                }
                _ => panic!("expected variable decl"),
            }
            let tokens = codegen_decl(&decl);
            assert!(!tokens.is_empty());
        }

        #[test]

        fn array_destructure_nested() {
            let decl = parse_first_decl("const [[a], b] = arr;");
            match decl {
                Decl::Variable(ref v) => {
                    assert!(v.pattern.is_some());
                    let pat = v.pattern.as_ref().unwrap();
                    if let Pat::Array { elems, .. } = pat {
                        let has_nested = elems.iter().any(|e| matches!(e, Some(Pat::Array { .. })));
                        assert!(has_nested, "should have nested array");
                    }
                }
                _ => panic!("expected variable decl"),
            }
            let tokens = codegen_decl(&decl);
            assert!(!tokens.is_empty());
        }

        #[test]

        fn array_destructure_sparse() {
            let decl = parse_first_decl("const [a, , b] = arr;");
            match decl {
                Decl::Variable(ref v) => {
                    assert!(v.pattern.is_some());
                    let pat = v.pattern.as_ref().unwrap();
                    if let Pat::Array { elems, .. } = pat {
                        assert_eq!(elems.len(), 3, "should have 3 elements (with hole)");
                        assert!(elems[1].is_none(), "middle element should be None (hole)");
                    }
                }
                _ => panic!("expected variable decl"),
            }
            let tokens = codegen_decl(&decl);
            assert!(!tokens.is_empty());
        }

        #[test]
        fn array_destructure_ignore_first() {
            let decl = parse_first_decl("const [, b] = arr;");
            match decl {
                Decl::Variable(ref v) => {
                    assert!(v.pattern.is_some());
                    let pat = v.pattern.as_ref().unwrap();
                    if let Pat::Array { elems, .. } = pat {
                        assert!(elems[0].is_none(), "first element should be None (ignored)");
                    }
                }
                _ => panic!("expected variable decl"),
            }
        }
    

}
