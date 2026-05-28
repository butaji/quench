#[cfg(test)]
mod codegen_tests {
    use crate::transpile::codegen::CodeGenerator;
    use crate::transpile::hir::*;

    fn create_codegen() -> CodeGenerator { CodeGenerator::new() }

    #[test]
    fn test_generate_interface_to_struct() {
        let mut cg = create_codegen();
        let decl = TypeDecl {
            name: "CounterProps".to_string(),
            generics: vec![],
            type_: Type::Object {
                members: vec![
                    ObjectMember { key: "initial".to_string(), type_: Type::Number, optional: true, readonly: false },
                    ObjectMember { key: "step".to_string(), type_: Type::Number, optional: true, readonly: false },
                ],
            },
        };
        let result = cg.generate_type_decl(&decl).unwrap();
        assert!(result.len() > 0);
    }
    
    #[test]
    fn test_generate_string_union_to_enum() {
        let mut cg = create_codegen();
        let decl = TypeDecl {
            name: "Status".to_string(),
            generics: vec![],
            type_: Type::Union {
                types: vec![
                    Type::Literal { kind: LiteralKind::String, value: "ok".to_string() },
                    Type::Literal { kind: LiteralKind::String, value: "err".to_string() },
                    Type::Literal { kind: LiteralKind::String, value: "pending".to_string() },
                ],
            },
        };
        let result = cg.generate_type_decl(&decl).unwrap();
        assert!(result.contains("pub enum Status"));
    }

    #[test]
    fn test_generate_function_to_rust() {
        let mut cg = create_codegen();
        let func = FunctionDecl {
            name: "add".to_string(),
            generics: vec![],
            params: vec![
                Param { name: "a".to_string(), type_: Some(Type::Number), default: None, optional: false, pattern: None },
                Param { name: "b".to_string(), type_: Some(Type::Number), default: None, optional: false, pattern: None },
            ],
            return_type: Some(Type::Number),
            body: Some(Block(vec![Stmt::Return { arg: Some(Expr::Bin { op: BinaryOp::Add, left: Box::new(Expr::Ident { name: "a".to_string() }), right: Box::new(Expr::Ident { name: "b".to_string() }) })])),
            is_async: false,
            is_generator: false,
            decorators: vec![],
        };
        let result = cg.generate_function(&func, false).unwrap();
        assert!(result.contains("pub fn add"));
        assert!(result.contains("a: f64"));
        assert!(result.contains("b: f64"));
    }
    
    #[test]
    fn test_jsx_to_html_macro() {
        let mut cg = create_codegen();
        let jsx = JSXExpr {
            opening: JSXOpening { name: JSXName::Ident("div".to_string()), attrs: vec![JSXAttr::Attr { name: "class".to_string(), value: Some(JSXAttrValue::String("container".to_string())) }], self_closing: false },
            children: vec![JSXChild::Text("Hello World".to_string())],
            closing: None,
        };
        let result = cg.jsx_to_rust(&jsx);
        assert!(result.contains("html!("));
        assert!(result.contains("<div"));
    }

    #[test]
    fn test_jsx_fragment_to_rust() {
        let mut cg = create_codegen();
        let jsx = JSXExpr {
            opening: JSXOpening { name: JSXName::Fragment, attrs: vec![], self_closing: false },
            children: vec![JSXChild::Text("Hello".to_string()), JSXChild::JSX(JSXExpr { opening: JSXOpening { name: JSXName::Ident("span".to_string()), attrs: vec![], self_closing: false }, children: vec![JSXChild::Text("world".to_string())], closing: None })],
            closing: None,
        };
        let result = cg.jsx_to_rust(&jsx);
        assert!(result.len() > 0);
    }

    #[test]
    fn test_jsx_fragment_empty_to_rust() {
        let mut cg = create_codegen();
        let jsx = JSXExpr { opening: JSXOpening { name: JSXName::Fragment, attrs: vec![], self_closing: false }, children: vec![], closing: None };
        let result = cg.jsx_to_rust(&jsx);
        assert!(result.len() > 0);
    }
    
    #[test]
    fn test_event_handler_conversion() {
        let mut cg = create_codegen();
        assert_eq!(cg.jsx_attr_to_rust("onClick"), "on_click");
        assert_eq!(cg.jsx_attr_to_rust("onChange"), "on_change");
        assert_eq!(cg.jsx_attr_to_rust("onKeyDown"), "on_key_down");
    }
    
    #[test]
    fn test_attribute_conversion() {
        let mut cg = create_codegen();
        assert_eq!(cg.jsx_attr_to_rust("class"), "class_name");
        assert_eq!(cg.jsx_attr_to_rust("className"), "class_name");
        assert_eq!(cg.jsx_attr_to_rust("htmlFor"), "for_id");
    }
    
    #[test]
    fn test_expression_conversion() {
        let mut cg = create_codegen();
        let expr = Expr::Bin { op: BinaryOp::Add, left: Box::new(Expr::Ident { name: "a".to_string() }), right: Box::new(Expr::Ident { name: "b".to_string() }) };
        let result = cg.expr_to_rust(&expr);
        assert!(result.contains("(a + b)"));
    }
    
    #[test]
    fn test_logical_conversion() {
        let mut cg = create_codegen();
        let expr = Expr::Logical { op: LogicalOp::And, left: Box::new(Expr::Ident { name: "a".to_string() }), right: Box::new(Expr::Ident { name: "b".to_string() }) };
        let result = cg.expr_to_rust(&expr);
        assert!(result.contains("if a"));
        assert!(result.contains("else"));
    }
    
    #[test]
    fn test_ternary_conversion() {
        let mut cg = create_codegen();
        let expr = Expr::Cond { test: Box::new(Expr::Ident { name: "count".to_string() }), consequent: Box::new(Expr::String("positive".to_string())), alternate: Box::new(Expr::String("negative".to_string())) };
        let result = cg.expr_to_rust(&expr);
        assert!(result.contains("if count"));
        assert!(result.contains("else"));
    }
    
    #[test]
    fn test_type_to_rust_primitives() {
        let mut cg = create_codegen();
        assert_eq!(cg.type_to_rust(&Type::String), "String");
        assert_eq!(cg.type_to_rust(&Type::Number), "f64");
        assert_eq!(cg.type_to_rust(&Type::Boolean), "bool");
    }
    
    #[test]
    fn test_type_to_rust_option() {
        let mut cg = create_codegen();
        let t = Type::Union { types: vec![Type::String, Type::Null] };
        assert_eq!(cg.type_to_rust(&t), "Option<String>");
    }
    
    #[test]
    fn test_type_to_rust_array() {
        let mut cg = create_codegen();
        let t = Type::Array { elem: Box::new(Type::String) };
        assert_eq!(cg.type_to_rust(&t), "Vec<String>");
    }
    
    #[test]
    fn test_snake_case() {
        let mut cg = create_codegen();
        assert_eq!(cg.to_snake_case("useState"), "use_state");
        assert_eq!(cg.to_snake_case("onClick"), "on_click");
        assert_eq!(cg.to_snake_case("className"), "class_name");
    }
    
    #[test]
    fn test_component_with_destructured_props() {
        let mut cg = create_codegen();
        let func = FunctionDecl {
            name: "Counter".to_string(),
            generics: vec![],
            params: vec![Param {
                name: "_props".to_string(),
                type_: Some(Type::Object { members: vec![ObjectMember { key: "initial".to_string(), type_: Type::Number, optional: true, readonly: false }, ObjectMember { key: "step".to_string(), type_: Type::Number, optional: true, readonly: false }] }),
                default: None,
                optional: false,
                pattern: Some(Pat::Object { props: vec![ObjectPatProp::Init { key: "initial".to_string(), value: Pat::Ident { name: "initial".to_string(), type_: None } }, ObjectPatProp::Init { key: "step".to_string(), value: Pat::Ident { name: "step".to_string(), type_: None } }], rest: None }),
            }],
            return_type: None,
            body: Some(Block(vec![Stmt::Return { arg: Some(Expr::JSX(JSXExpr { opening: JSXOpening { name: JSXName::Ident("div".to_string()), attrs: vec![], self_closing: false }, closing: Some(JSXClosing { name: JSXName::Ident("div".to_string()) }), children: vec![] }))])),
            is_async: false,
            is_generator: false,
            decorators: vec![],
        };
        let result = cg.generate_function(&func, true).unwrap();
        assert!(result.contains("fn"));
    }
}
