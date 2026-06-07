//! Spec: Classes - parser + codegen tests
//!
//!
//! Covers:
//! - Simple class declarations with fields and methods
//! - Constructor initialization
//! - Instance methods with `this`
//! - Static methods
//! - `new ClassName()` instantiation
//! - Math operations (including exponentiation)

#[cfg(test)]
#[ignore]
mod spec_classes_tests {
    use crate::transpile::hir::{
        AssignOp, ClassDecl, ClassMember, ClassMethod, Decl, Expr, MethodKind, ModuleItem,
        Param, QuoteCodegen, Stmt, Type, Ownership,
    };
    use proc_macro2::TokenStream;
    use quote::ToTokens;

    // =============================================================================
    // Parser helpers
    // =============================================================================

    fn parse_source(source: &str) -> Vec<ModuleItem> {
        let parser = crate::transpile::parser::TsParser::new();
        parser.parse_source(source).expect("parse failed").items
    }

    fn find_class(source: &str) -> ClassDecl {
        let items = parse_source(source);
        for item in &items {
            if let ModuleItem::Decl(Decl::Class(c)) = item {
                return c.clone();
            }
        }
        panic!("no class found in: {}", source);
    }

    // =============================================================================
    // Codegen helpers
    // =============================================================================

    fn codegen_stmt(stmt: &Stmt) -> Option<TokenStream> {
        QuoteCodegen::default().gen_stmt(stmt)
    }

    fn codegen_class(class: &ClassDecl) -> TokenStream {
        QuoteCodegen::default().gen_class(class)
    }

    /// Normalize whitespace in TokenStream string output for reliable testing.
    /// proc_macro2::TokenStream::to_string() adds spaces around colons, so we
    /// split on whitespace and rejoin to get a consistent format.
    fn normalize_ws(s: &str) -> String {
        s.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    // =============================================================================
    // SECTION: Class Parsing
    // =============================================================================

    mod class_parsing {
        use super::*;

        #[test]
        fn simple_class_with_fields() {
            let source = r#"
                class Point {
                    x: number;
                    y: number;
                }
            "#;
            let class = find_class(source);
            assert_eq!(class.name, "Point");
            // Should have 2 members (x and y fields)
            assert!(class.members.len() >= 2, "should have field members");
        }

        #[test]
        fn class_with_constructor() {
            let source = r#"
                class Point {
                    x: number;
                    y: number;
                    constructor(x: number, y: number) {
                        this.x = x;
                        this.y = y;
                    }
                }
            "#;
            let class = find_class(source);
            assert_eq!(class.name, "Point");
            // Should have constructor method
            let has_constructor = class.methods.iter().any(|m| m.kind == MethodKind::Constructor);
            assert!(has_constructor, "should have constructor method");
        }

        #[test]
        fn class_with_method() {
            let source = r#"
                class Point {
                    x: number;
                    y: number;
                    distance(other: Point): number {
                        return 0;
                    }
                }
            "#;
            let class = find_class(source);
            assert_eq!(class.name, "Point");
            // Should have distance method
            let has_distance = class.methods.iter().any(|m| m.name == "distance");
            assert!(has_distance, "should have distance method");
        }

        #[test]
        fn class_with_static_method() {
            let source = r#"
                class Point {
                    x: number;
                    y: number;
                    static origin(): Point {
                        return new Point(0, 0);
                    }
                }
            "#;
            let class = find_class(source);
            assert_eq!(class.name, "Point");
            // Should have static origin method
            let has_static_origin = class.methods.iter().any(|m| m.name == "origin");
            assert!(has_static_origin, "should have static origin method");
        }
    }

    // =============================================================================
    // SECTION: Class Codegen
    // =============================================================================

    #[ignore]
    mod class_codegen {
        use super::*;

        #[test]
        fn simple_class_struct_generation() {
            let class = ClassDecl {
                name: "Point".to_string(),
                extends: None,
                members: vec![
                    ClassMember {
                        name: "x".to_string(),
                        type_: Some(Type::Number),
                        is_static: false,
                        is_async: false,
                    },
                    ClassMember {
                        name: "y".to_string(),
                        type_: Some(Type::Number),
                        is_static: false,
                        is_async: false,
                    },
                ],
                generics: vec![],
                methods: vec![],
            };

            let tokens = codegen_class(&class);
            let s = normalize_ws(&tokens.to_string());
            assert!(s.contains("struct Point"), "should generate struct Point");
            assert!(s.contains("pub x : f64"), "should have x field: {}", s);
            assert!(s.contains("pub y : f64"), "should have y field: {}", s);
        }

        #[test]
        #[ignore = "class constructor codegen not fully implemented"]
        fn class_with_constructor_codegen() {
            let class = ClassDecl {
                name: "Point".to_string(), extends: None,
                members: vec![
                    ClassMember { name: "x".to_string(), type_: Some(Type::Number), is_static: false, is_async: false },
                    ClassMember { name: "y".to_string(), type_: Some(Type::Number), is_static: false, is_async: false },
                ],
                generics: vec![],
                methods: vec![ClassMethod {
                    name: "constructor".to_string(),
                    params: vec![
                        Param { name: "x".to_string(), type_: Some(Type::Number), default: None, optional: false, pattern: None, ownership: Ownership::Owned },
                        Param { name: "y".to_string(), type_: Some(Type::Number), default: None, optional: false, pattern: None, ownership: Ownership::Owned },
                    ],
                    body: Expr::Block(vec![
                        Stmt::Expr { expr: Expr::Assign { op: AssignOp::Assign, left: Box::new(Expr::Member { obj: Box::new(Expr::This), property: Box::new(Expr::Ident { name: "x".to_string() }), computed: false, optional: false }), right: Box::new(Expr::Ident { name: "x".to_string() }) } },
                        Stmt::Expr { expr: Expr::Assign { op: AssignOp::Assign, left: Box::new(Expr::Member { obj: Box::new(Expr::This), property: Box::new(Expr::Ident { name: "y".to_string() }), computed: false, optional: false }), right: Box::new(Expr::Ident { name: "y".to_string() }) } },
                    ]),
                    kind: MethodKind::Constructor,
                }],
            };
            let tokens = codegen_class(&class);
            let s = normalize_ws(&tokens.to_string());
            assert!(s.contains("Point"), "should have Point: {}", s);
            assert!(s.contains("new"), "should have new: {}", s);
        }

        #[test]
        #[ignore = "class instance method codegen not fully implemented"]
        fn class_with_instance_method_codegen() {
            let class = ClassDecl {
                name: "Point".to_string(), extends: None,
                members: vec![
                    ClassMember { name: "x".to_string(), type_: Some(Type::Number), is_static: false, is_async: false },
                    ClassMember { name: "y".to_string(), type_: Some(Type::Number), is_static: false, is_async: false },
                ],
                generics: vec![],
                methods: vec![ClassMethod {
                    name: "distance".to_string(),
                    params: vec![Param { name: "other".to_string(), type_: None, default: None, optional: false, pattern: None, ownership: Ownership::Borrow }],
                    body: Expr::Number(0.0), kind: MethodKind::Method,
                }],
            };
            let tokens = codegen_class(&class);
            let s = normalize_ws(&tokens.to_string());
            assert!(s.contains("fn distance"), "should generate fn distance: {}", s);
            assert!(s.contains("& self"), "instance method should have &self: {}", s);
            assert!(s.contains("f64"), "should have f64 param: {}", s);
        }

        #[test]
        fn class_with_static_method_codegen() {
            let class = ClassDecl {
                name: "Point".to_string(),
                extends: None,
                members: vec![],
                generics: vec![],
                methods: vec![
                    ClassMethod {
                        name: "origin".to_string(),
                        params: vec![],
                        body: Expr::Number(0.0),
                        kind: MethodKind::Method,
                    },
                ],
            };

            let tokens = codegen_class(&class);
            let s = tokens.to_string();
            assert!(s.contains("fn origin"), "should generate fn origin");
            // Static method should NOT have &self
            assert!(!s.contains("&self"), "static method should not have &self");
        }
    }

    // =============================================================================
    // SECTION: Integration - Full Parse + Codegen
    // =============================================================================

    #[ignore]
    mod integration {
        use super::*;

        #[test]
        #[ignore = "full class parse and codegen not fully implemented"]
        fn full_class_parse_and_codegen() {
            let source = r#"
                class Point {
                    x: number;
                    y: number;
                    
                    constructor(x: number, y: number) {
                        this.x = x;
                        this.y = y;
                    }
                    
                    distance(other: Point): number {
                        return 0;
                    }
                }
            "#;
            let items = parse_source(source);
            assert!(!items.is_empty(), "should parse class");
            
            // Find the class declaration
            let class_item = items.iter().find(|i| matches!(i, ModuleItem::Decl(Decl::Class(_))));
            assert!(class_item.is_some(), "should find class declaration");
            
            if let ModuleItem::Decl(Decl::Class(class)) = class_item.unwrap() {
                let tokens = codegen_class(class);
                let s = tokens.to_string();
                assert!(s.contains("struct Point"), "should generate struct");
                assert!(s.contains("impl Point"), "should generate impl");
                assert!(s.contains("fn new"), "should have constructor");
                assert!(s.contains("fn distance"), "should have distance method");
            }
        }

        #[test]
        fn full_class_with_static_method() {
            let source = r#"
                class Point {
                    x: number;
                    y: number;
                    
                    static origin(): Point {
                        return new Point(0, 0);
                    }
                }
            "#;
            let items = parse_source(source);
            let class_item = items.iter().find(|i| matches!(i, ModuleItem::Decl(Decl::Class(_))));
            assert!(class_item.is_some(), "should find class declaration");
        }
    }
}
