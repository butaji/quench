#[cfg(test)]
mod tests {
    use super::*;
    use crate::transpile::hir::*;

    #[test]
    fn test_gen_fn() {
        let cg = QuoteCodegen::default();
        let func = FunctionDecl {
            name: "greet".into(),
            generics: vec![],
            params: vec![Param {
                name: "name".into(),
                type_: Some(Type::String),
                default: None,
                optional: false,
                pattern: None,
                ownership: Ownership::Borrow,
            }],
            return_type: Some(Type::String),
            body: Some(Block(vec![Stmt::Return {
                arg: Some(Expr::Ident { name: "name".into() }),
            }])),
            is_async: false,
            is_generator: false,
            decorators: vec![],
            throws: false,
            error_type: None,
        };

        let tokens = cg.gen_fn(&func);
        let s = tokens.to_string();
        assert!(s.contains("fn greet"));
        assert!(s.contains("& String") || s.contains("&String"));
        assert!(s.contains("-> String"));
    }

    #[test]
    #[ignore = "for loop codegen emits invalid Rust: 'for let i = 0' not valid syntax"]
    fn test_gen_for_loop() {
        let cg = QuoteCodegen::default();
        let stmt = Stmt::For {
            init: Some(ForInit::Variable(VariableKind::Let, vec![("i".to_string(), Some(Expr::Number(0.0)))])),
            test: Some(Expr::Bin {
                op: BinaryOp::Lt,
                left: Box::new(Expr::Ident { name: "i".into() }),
                right: Box::new(Expr::Number(10.0)),
            }),
            update: Some(Expr::Update {
                op: UpdateOp::PlusPlus,
                arg: Box::new(Expr::Ident { name: "i".into() }),
                prefix: true,
            }),
            body: Box::new(Stmt::Block { stmts: vec![] }),
        };

        let tokens = cg.gen_stmt(&stmt);
        assert!(tokens.is_some());
        let s = tokens.unwrap().to_string();
        assert!(s.contains("for"));
        assert!(s.contains("let i = 0"));
        assert!(s.contains("i < 10"));
    }

    #[test]
    fn test_gen_while_loop() {
        let cg = QuoteCodegen::default();
        let stmt = Stmt::While {
            test: Expr::Boolean(true),
            body: Box::new(Stmt::Block { stmts: vec![] }),
        };

        let tokens = cg.gen_stmt(&stmt);
        assert!(tokens.is_some());
        let s = tokens.unwrap().to_string();
        assert!(s.contains("while true"));
    }

    #[test]
    fn test_gen_do_while_loop() {
        let cg = QuoteCodegen::default();
        let stmt = Stmt::DoWhile {
            body: Box::new(Stmt::Block { stmts: vec![] }),
            test: Expr::Boolean(true),
        };

        let tokens = cg.gen_stmt(&stmt);
        assert!(tokens.is_some());
        let s = tokens.unwrap().to_string();
        assert!(s.contains("loop"));
    }

    #[test]
    #[ignore = "switch codegen uses match incorrectly: no fallthrough support, === in arms"]
    fn test_gen_switch() {
        let cg = QuoteCodegen::default();
        let stmt = Stmt::Switch {
            discriminant: Expr::Ident { name: "x".into() },
            cases: vec![
                SwitchCase {
                    test: Some(Expr::Number(1.0)),
                    consequent: vec![Stmt::Return { arg: Some(Expr::String("one".into())) }],
                },
                SwitchCase {
                    test: Some(Expr::Number(2.0)),
                    consequent: vec![Stmt::Return { arg: Some(Expr::String("two".into())) }],
                },
                SwitchCase {
                    test: None,
                    consequent: vec![Stmt::Return { arg: Some(Expr::String("other".into())) }],
                },
            ],
        };

        let tokens = cg.gen_stmt(&stmt);
        assert!(tokens.is_some());
        let s = tokens.unwrap().to_string();
        assert!(s.contains("match x"));
    }

    #[test]
    #[ignore = "try/catch codegen uses std::panic::catch_unwind with wrong semantics"]
    fn test_gen_try_catch() {
        let cg = QuoteCodegen::default();
        let stmt = Stmt::Try {
            block: Block(vec![Stmt::Return { arg: Some(Expr::Number(1.0)) }]),
            handler: Some(CatchClause {
                param: "e".into(),
                body: Box::new(Block(vec![Stmt::Return { arg: Some(Expr::Number(0.0)) }])),
            }),
            finalizer: None,
        };

        let tokens = cg.gen_stmt(&stmt);
        assert!(tokens.is_some());
        let s = tokens.unwrap().to_string();
        assert!(s.contains("catch_unwind"));
    }

    #[test]
    fn test_gen_break_continue() {
        let cg = QuoteCodegen::default();
        let break_stmt = Stmt::Break { label: None };
        let continue_stmt = Stmt::Continue { label: None };

        let break_tokens = cg.gen_stmt(&break_stmt);
        assert!(break_tokens.is_some());
        assert!(break_tokens.unwrap().to_string().contains("break"));

        let continue_tokens = cg.gen_stmt(&continue_stmt);
        assert!(continue_tokens.is_some());
        assert!(continue_tokens.unwrap().to_string().contains("continue"));
    }

    #[test]
    #[ignore = "throw codegen uses std::panic::panic_any instead of Result propagation"]
    fn test_gen_throw() {
        let cg = QuoteCodegen::default();
        let stmt = Stmt::Throw {
            arg: Expr::String("error".into()),
        };

        let tokens = cg.gen_stmt(&stmt);
        assert!(tokens.is_some());
        let s = tokens.unwrap().to_string();
        assert!(s.contains("panic_any"));
    }

    #[test]
    fn test_gen_return() {
        let cg = QuoteCodegen::default();
        let stmt = Stmt::Return { arg: Some(Expr::Number(42.0)) };

        let tokens = cg.gen_stmt(&stmt);
        assert!(tokens.is_some());
        let s = tokens.unwrap().to_string();
        assert!(s.contains("return"));
    }

    #[test]
    fn test_gen_labeled() {
        let cg = QuoteCodegen::default();
        let stmt = Stmt::Labeled {
            label: "loop".into(),
            body: Box::new(Stmt::While {
                test: Expr::Boolean(true),
                body: Box::new(Stmt::Break { label: None }),
            }),
        };

        let tokens = cg.gen_stmt(&stmt);
        assert!(tokens.is_some());
        let s = tokens.unwrap().to_string();
        // Rust labels are lifetimes like 'loop : (with spaces around punctuation in token stream)
        assert!(s.contains("'loop"), "Expected string to contain 'loop, got: {:?}", s);
    }

    #[test]
    fn test_gen_union_type() {
        let cg = QuoteCodegen::default();
        let union_type = Type::Union {
            types: vec![Type::String, Type::Number],
        };

        let tokens = cg.gen_type(&union_type);
        let s = tokens.to_string();
        assert!(s.contains("enum"));
        assert!(s.contains("Variant"));
    }

    #[test]
    fn test_gen_intersection_type() {
        let cg = QuoteCodegen::default();
        let intersection_type = Type::Intersection {
            types: vec![
                Type::Object { members: vec![TypeMember { key: "a".into(), type_: Type::String, optional: false, readonly: false }] },
                Type::Object { members: vec![TypeMember { key: "b".into(), type_: Type::Number, optional: false, readonly: false }] },
            ],
        };

        let tokens = cg.gen_type(&intersection_type);
        let s = tokens.to_string();
        assert!(s.contains("struct"));
    }

    #[test]
    fn test_gen_literal_type() {
        let cg = QuoteCodegen::default();
        let literal_type = Type::Literal {
            kind: LiteralKind::String,
            value: "hello".into(),
        };

        let tokens = cg.gen_type(&literal_type);
        let s = tokens.to_string();
        assert!(s.contains("hello"));
    }

    #[test]
    fn test_gen_console_log() {
        let cg = QuoteCodegen::default();
        let expr = Expr::Call {
            callee: Box::new(Expr::StaticMember {
                obj: Box::new(Expr::Ident { name: "console".into() }),
                property: "log".into(),
                optional: false,
            }),
            arguments: vec![Expr::String("hello".into())],
            optional: false,
        };
        let tokens = cg.gen_expr(&expr);
        let s = tokens.to_string();
        // proc_macro may add spaces: "println ! (...)" is still println!
        assert!(s.contains("println") && s.contains("hello"), "console.log should generate println with hello, got: {}", s);
    }

    #[test]
    fn test_gen_string_concat() {
        let cg = QuoteCodegen::default();
        let expr = Expr::Bin {
            op: BinaryOp::Add,
            left: Box::new(Expr::String("Hello, ".into())),
            right: Box::new(Expr::Ident { name: "name".into() }),
        };
        let tokens = cg.gen_expr(&expr);
        let s = tokens.to_string();
        // quote may add spaces: "format ! (...)" is still format!
        assert!(s.contains("format"), "string concat should use format!, got: {}", s);
    }

    #[test]
    fn test_gen_import_named() {
        let cg = QuoteCodegen::default();
        let stmt = Stmt::ImportNamed {
            source: "react".into(),
            specifiers: vec![
                ImportSpecifier::Named { name: "useState".into(), alias: None },
                ImportSpecifier::Named { name: "Component".into(), alias: Some("Comp".into()) },
            ],
        };

        let tokens = cg.gen_stmt(&stmt);
        assert!(tokens.is_some());
        let s = tokens.unwrap().to_string();
        assert!(s.contains("use"), "import should produce use statement");
        assert!(s.contains("react"), "import should contain module name");
        assert!(s.contains("useState"), "import should contain imported name");
        assert!(s.contains("as"), "import with alias should contain 'as'");
    }

    #[test]
    fn test_gen_import_default() {
        let cg = QuoteCodegen::default();
        let stmt = Stmt::ImportDefault {
            source: "react".into(),
            local: "React".into(),
        };

        let tokens = cg.gen_stmt(&stmt);
        assert!(tokens.is_some());
        let s = tokens.unwrap().to_string();
        assert!(s.contains("use"), "import should produce use statement");
        assert!(s.contains("react"), "import should contain module name");
        assert!(s.contains("React"), "import should contain local name");
    }

    #[test]
    fn test_gen_import_namespace() {
        let cg = QuoteCodegen::default();
        let stmt = Stmt::ImportNamed {
            source: "lodash".into(),
            specifiers: vec![
                ImportSpecifier::Namespace { name: "_".into() },
            ],
        };

        let tokens = cg.gen_stmt(&stmt);
        assert!(tokens.is_some());
        let s = tokens.unwrap().to_string();
        assert!(s.contains("use"), "import should produce use statement");
        assert!(s.contains("lodash"), "import should contain module name");
        assert!(s.contains("*"), "namespace import should contain *");
    }

    #[test]
    fn test_gen_export_named() {
        let cg = QuoteCodegen::default();
        let stmt = Stmt::ExportNamed {
            specifiers: vec![
                Export::Named { name: "x".into() },
            ],
        };

        let tokens = cg.gen_stmt(&stmt);
        assert!(tokens.is_some());
        let s = tokens.unwrap().to_string();
        assert!(s.contains("pub"), "export named should produce pub");
        assert!(s.contains("x"), "export named should contain name");
    }

    #[test]
    fn test_gen_export_reexport() {
        let cg = QuoteCodegen::default();
        let stmt = Stmt::ExportNamed {
            specifiers: vec![
                Export::ReExport { source: "mod".into(), names: vec!["a".into(), "b".into()] },
            ],
        };

        let tokens = cg.gen_stmt(&stmt);
        assert!(tokens.is_some());
        let s = tokens.unwrap().to_string();
        assert!(s.contains("pub use"), "re-export should produce pub use");
        assert!(s.contains("mod"), "re-export should contain module name");
        assert!(s.contains("a"), "re-export should contain names");
    }

    #[test]
    fn test_gen_export_all() {
        let cg = QuoteCodegen::default();
        let stmt = Stmt::ExportNamed {
            specifiers: vec![
                Export::All { source: "mod".into() },
            ],
        };

        let tokens = cg.gen_stmt(&stmt);
        assert!(tokens.is_some());
        let s = tokens.unwrap().to_string();
        assert!(s.contains("pub use"), "export all should produce pub use");
        assert!(s.contains("mod"), "export all should contain module name");
        assert!(s.contains("*"), "export all should contain *");
    }

    #[test]
    fn test_gen_export_default_expr() {
        let cg = QuoteCodegen::default();
        let stmt = Stmt::ExportDefault {
            expr: Expr::Number(42.0),
        };

        let tokens = cg.gen_stmt(&stmt);
        assert!(tokens.is_some());
        let s = tokens.unwrap().to_string();
        assert!(s.contains("pub const"), "export default number should produce pub const");
        assert!(s.contains("default"), "export default should contain 'default'");
    }

    #[test]
    fn test_gen_export_default_function() {
        let cg = QuoteCodegen::default();
        let stmt = Stmt::ExportDefault {
            expr: Expr::Function(FunctionDecl {
                name: "myFunc".into(),
                generics: vec![],
                params: vec![],
                return_type: None,
                body: Some(Block(vec![])),
                is_async: false,
                is_generator: false,
                decorators: vec![],
                throws: false,
                error_type: None,
            }),
        };

        let tokens = cg.gen_stmt(&stmt);
        assert!(tokens.is_some());
        let s = tokens.unwrap().to_string();
        assert!(s.contains("fn myFunc"), "export default function should produce fn with name");
    }

    #[test]
    fn test_gen_export_default_arrow() {
        let cg = QuoteCodegen::default();
        let stmt = Stmt::ExportDefault {
            expr: Expr::ArrowFunction {
                params: vec![Param {
                    name: "x".into(),
                    type_: Some(Type::Number),
                    default: None,
                    optional: false,
                    pattern: None,
                    ownership: Ownership::Owned,
                }],
                body: Box::new(Expr::Number(42.0)),
                is_async: false,
            },
        };

        let tokens = cg.gen_stmt(&stmt);
        assert!(tokens.is_some());
        let s = tokens.unwrap().to_string();
        assert!(s.contains("fn default"), "export default arrow should produce fn default");
    }

    #[test]
    fn test_gen_module_path_simplification() {
        let cg = QuoteCodegen::default();
        // Test with a path like "./utils/helper"
        let stmt = Stmt::ImportNamed {
            source: "./utils/helper".into(),
            specifiers: vec![
                ImportSpecifier::Named { name: "foo".into(), alias: None },
            ],
        };

        let tokens = cg.gen_stmt(&stmt);
        assert!(tokens.is_some());
        let s = tokens.unwrap().to_string();
        // Should use last path component "helper", not full path
        assert!(s.contains("helper"), "module path should be simplified to last component");
        assert!(!s.contains("./utils/helper"), "full path should not appear");
    }
}
