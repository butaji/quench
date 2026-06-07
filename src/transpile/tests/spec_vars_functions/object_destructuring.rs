use super::helpers::*;
    mod object_destructuring {
        use super::*;

        #[test]
        fn simple_object_destructure() {
            let decl = parse_first_decl("const {a, b} = obj;");
            match decl {
                Decl::Variable(ref v) => {
                    assert!(v.pattern.is_some(), "should have pattern");
                    let pat = v.pattern.as_ref().unwrap();
                    assert!(matches!(pat, Pat::Object { .. }));
                }
                _ => panic!("expected variable decl"),
            }
            let tokens = codegen_decl(&decl);
            assert!(!tokens.is_empty(), "object destructure codegen should produce output");
        }

        #[test]
        #[ignore = "nested object destructuring not fully implemented"]
        fn nested_object_destructure() {
            let decl = parse_first_decl("const {a: {b}} = obj;");
            match decl {
                Decl::Variable(ref v) => {
                    assert!(v.pattern.is_some());
                    let pat = v.pattern.as_ref().unwrap();
                    if let Pat::Object { props, .. } = pat {
                        let has_nested = props.iter().any(|p| {
                            if let ObjectPatProp::Init { value: inner, .. } = p {
                                matches!(*inner, Pat::Object { .. })
                            } else {
                                false
                            }
                        });
                        assert!(has_nested, "should have nested object pattern");
                    }
                }
                _ => panic!("expected variable decl"),
            }
            let tokens = codegen_decl(&decl);
            assert!(!tokens.is_empty());
        }

        #[test]

        fn object_destructure_with_rest() {
            let decl = parse_first_decl("const {a, ...rest} = obj;");
            match decl {
                Decl::Variable(ref v) => {
                    assert!(v.pattern.is_some());
                    let pat = v.pattern.as_ref().unwrap();
                    if let Pat::Object { props, rest } = pat {
                        let has_rest_prop = props.iter().any(|p| matches!(p, ObjectPatProp::Rest { .. }));
                        assert!(has_rest_prop || rest.is_some(), "should have rest in pattern");
                    }
                }
                _ => panic!("expected variable decl"),
            }
            let tokens = codegen_decl(&decl);
            assert!(!tokens.is_empty());
        }

        #[test]
        #[ignore = "object destructuring with default not fully implemented"]
        fn object_destructure_with_default() {
            let decl = parse_first_decl("const {a = 1} = obj;");
            match decl {
                Decl::Variable(ref v) => {
                    assert!(v.pattern.is_some());
                    let pat = v.pattern.as_ref().unwrap();
                    if let Pat::Object { props, .. } = pat {
                        let has_default = props.iter().any(|p| {
                            if let ObjectPatProp::Init { value, .. } = p {
                                matches!(*value, Pat::Default { .. })
                            } else {
                                false
                            }
                        });
                        assert!(has_default, "should have default value in pattern");
                    }
                }
                _ => panic!("expected variable decl"),
            }
            let tokens = codegen_decl(&decl);
            assert!(!tokens.is_empty());
        }

        #[test]

        fn object_destructure_rename() {
            let decl = parse_first_decl("const {a: b} = obj;");
            match decl {
                Decl::Variable(ref v) => {
                    assert!(v.pattern.is_some());
                    let pat = v.pattern.as_ref().unwrap();
                    if let Pat::Object { props, .. } = pat {
                        assert!(!props.is_empty(), "should have property");
                    }
                }
                _ => panic!("expected variable decl"),
            }
            let tokens = codegen_decl(&decl);
            assert!(!tokens.is_empty());
        }

        #[test]

        fn object_destructure_complex() {
            let decl = parse_first_decl("const {a: {b: c}, d = 2, ...rest} = obj;");
            match decl {
                Decl::Variable(ref v) => {
                    assert!(v.pattern.is_some());
                }
                _ => panic!("expected variable decl"),
            }
            let tokens = codegen_decl(&decl);
            assert!(!tokens.is_empty());
        }
    

}
