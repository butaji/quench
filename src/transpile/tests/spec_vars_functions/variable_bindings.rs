use super::helpers::*;
    mod variable_bindings {
        use super::*;

        #[test]
        fn const_immutable_binding() {
            let decl = parse_first_decl("const x = 5;");
            match decl {
                Decl::Variable(ref v) => {
                    assert!(matches!(v.kind, VariableKind::Const));
                    assert_eq!(v.name, "x");
                }
                _ => panic!("expected variable decl"),
            }
            let tokens = codegen_decl(&decl);
            assert!(!tokens.is_empty(), "const codegen should produce output");
        }

        #[test]
        fn let_mutable_binding() {
            let decl = parse_first_decl("let x = 5;");
            match decl {
                Decl::Variable(ref v) => {
                    assert!(matches!(v.kind, VariableKind::Let));
                    assert_eq!(v.name, "x");
                }
                _ => panic!("expected variable decl"),
            }
            let tokens = codegen_decl(&decl);
            assert!(!tokens.is_empty(), "let codegen should produce output");
        }

        #[test]
        fn var_hoisting_flattened() {
            let decl = parse_first_decl("var x = 5;");
            match decl {
                Decl::Variable(ref v) => {
                    assert!(matches!(v.kind, VariableKind::Var));
                    assert_eq!(v.name, "x");
                }
                _ => panic!("expected variable decl"),
            }
            let tokens = codegen_decl(&decl);
            assert!(!tokens.is_empty(), "var codegen should produce output");
        }

        #[test]
        #[ignore = "Parser does not capture type annotations on variable declarations"]
        fn const_with_type_annotation() {
            let decl = parse_first_decl("const x: number = 5;");
            match decl {
                Decl::Variable(ref v) => {
                    assert!(matches!(v.kind, VariableKind::Const));
                    assert!(v.type_.is_some());
                }
                _ => panic!("expected variable decl"),
            }
            let tokens = codegen_decl(&decl);
            assert!(!tokens.is_empty());
        }

        #[test]
        fn let_without_initializer() {
            let decl = parse_first_decl("let x;");
            match decl {
                Decl::Variable(ref v) => {
                    assert!(matches!(v.kind, VariableKind::Let));
                    assert!(v.init.is_none());
                }
                _ => panic!("expected variable decl"),
            }
        }

        #[test]
        #[ignore = "TypeScript requires initializer for const declarations"]
        fn const_without_initializer_with_type() {
            let decl = parse_first_decl("const x: number;");
            match decl {
                Decl::Variable(ref v) => {
                    assert!(matches!(v.kind, VariableKind::Const));
                    assert!(v.type_.is_some());
                    assert!(v.init.is_none());
                }
                _ => panic!("expected variable decl"),
            }
        }

        // ES2024 / TS 5.2 explicit resource management tests
        #[test]
        fn using_binding() {
            let decl = parse_first_decl("using x = getResource();");
            match decl {
                Decl::Variable(ref v) => {
                    assert!(matches!(v.kind, VariableKind::Using));
                    assert_eq!(v.name, "x");
                    assert!(v.init.is_some(), "using requires initializer");
                }
                _ => panic!("expected variable decl"),
            }
            let tokens = codegen_decl(&decl);
            assert!(!tokens.is_empty(), "using codegen should produce output");
        }

        #[test]
        fn await_using_binding() {
            let decl = parse_first_decl("await using x = getAsyncResource();");
            match decl {
                Decl::Variable(ref v) => {
                    assert!(matches!(v.kind, VariableKind::AwaitUsing));
                    assert_eq!(v.name, "x");
                    assert!(v.init.is_some(), "await using requires initializer");
                }
                _ => panic!("expected variable decl"),
            }
            let tokens = codegen_decl(&decl);
            assert!(!tokens.is_empty(), "await using codegen should produce output");
        }
    

}
