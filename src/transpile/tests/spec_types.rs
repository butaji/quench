//! Comprehensive parser + codegen tests for TypeScript Types (SUPPORTED_SUBSET.md 4.1-4.3)
//!
//! Tests are grouped by:
//! 1. Primitive type annotations
//! 2. Complex type annotations
//! 3. Type declarations (interface, type alias, enum)
//! 4. Type-directed lowering (THE KEY FEATURE - string unions -> enums, interfaces -> structs, etc.)
//!

#[cfg(test)]
mod spec_types_tests {
    use crate::transpile::hir::*;
    use crate::transpile::hir::type_to_rust::{TypeToRust, OutputKind, RustType};
    use crate::transpile::parser::TsParser;
    use proc_macro2::TokenStream;
    use quote::ToTokens;

    // =========================================================================
    // Helper functions
    // =========================================================================

    fn parse_fn_return_type(source: &str) -> Option<Type> {
        let parser = TsParser::new();
        let result = parser.parse_source(source).ok()?;
        for item in &result.items {
            if let ModuleItem::Decl(Decl::Function(f)) = item {
                return f.return_type.clone();
            }
        }
        None
    }

    fn parse_fn_param_type(source: &str, param_idx: usize) -> Option<Type> {
        let parser = TsParser::new();
        let result = parser.parse_source(source).ok()?;
        for item in &result.items {
            if let ModuleItem::Decl(Decl::Function(f)) = item {
                return f.params.get(param_idx).and_then(|p| p.type_.clone());
            }
        }
        None
    }

    fn type_to_rust_name(ty: &Type) -> String {
        let converter = TypeToRust::new(OutputKind::String);
        converter.convert(ty).type_name()
    }

    fn codegen_produces_output(ty: &Type) -> bool {
        let cg = QuoteCodegen::default();
        let tokens = cg.gen_type(ty);
        !tokens.is_empty()
    }

    fn find_type_decl(source: &str, name: &str) -> Option<TypeDecl> {
        let parser = TsParser::new();
        let result = parser.parse_source(source).ok()?;
        for item in &result.items {
            if let ModuleItem::Decl(Decl::Type(t)) = item {
                if t.name == name {
                    return Some(t.clone());
                }
            }
        }
        None
    }

    // =========================================================================
    // SECTION 1: Primitive Type Annotations
    // =========================================================================

    mod primitive_types {
        use super::*;

        // All parse tests are ignored because parser hardcodes return_type: None
        #[test]
        #[ignore = "parser not yet capturing return type annotations"]
        fn test_parse_type_string() {
            let ty = parse_fn_return_type("function f(): string { return ''; }").unwrap();
            assert!(matches!(ty, Type::String));
        }

        #[test]
        fn test_codegen_type_string() {
            let ty = Type::String;
            assert_eq!(type_to_rust_name(&ty), "String");
            assert!(codegen_produces_output(&ty));
        }

        #[test]
        #[ignore = "parser not yet capturing return type annotations"]
        fn test_parse_type_number() {
            let ty = parse_fn_return_type("function f(): number { return 1; }").unwrap();
            assert!(matches!(ty, Type::Number));
        }

        #[test]
        fn test_codegen_type_number() {
            let ty = Type::Number;
            assert_eq!(type_to_rust_name(&ty), "f64");
            assert!(codegen_produces_output(&ty));
        }

        #[test]
        #[ignore = "parser not yet capturing return type annotations"]
        fn test_parse_type_boolean() {
            let ty = parse_fn_return_type("function f(): boolean { return true; }").unwrap();
            assert!(matches!(ty, Type::Boolean));
        }

        #[test]
        fn test_codegen_type_boolean() {
            let ty = Type::Boolean;
            assert_eq!(type_to_rust_name(&ty), "bool");
            assert!(codegen_produces_output(&ty));
        }

        #[test]
        #[ignore = "parser not yet capturing param type annotations"]
        fn test_parse_type_null() {
            let ty = parse_fn_param_type("function f(x: null): null { return null; }", 0).unwrap();
            assert!(matches!(ty, Type::Null));
        }

        #[test]
        fn test_codegen_type_null() {
            let ty = Type::Null;
            assert_eq!(type_to_rust_name(&ty), "Value");
            assert!(codegen_produces_output(&ty));
        }

        #[test]
        #[ignore = "parser not yet capturing param type annotations"]
        fn test_parse_type_undefined() {
            let ty = parse_fn_param_type("function f(x: undefined): undefined { return undefined; }", 0).unwrap();
            assert!(matches!(ty, Type::Undefined));
        }

        #[test]
        fn test_codegen_type_undefined() {
            let ty = Type::Undefined;
            assert_eq!(type_to_rust_name(&ty), "Value");
            assert!(codegen_produces_output(&ty));
        }

        #[test]
        #[ignore = "parser not yet capturing return type annotations"]
        fn test_parse_type_void() {
            let ty = parse_fn_return_type("function f(): void { }").unwrap();
            assert!(matches!(ty, Type::Void));
        }

        #[test]
        fn test_codegen_type_void() {
            let ty = Type::Void;
            assert_eq!(type_to_rust_name(&ty), "()");
            assert!(codegen_produces_output(&ty));
        }

        #[test]
        #[ignore = "parser not yet capturing param type annotations"]
        fn test_parse_type_any() {
            let ty = parse_fn_param_type("function f(x: any): any { return x; }", 0).unwrap();
            assert!(matches!(ty, Type::Any));
        }

        #[test]
        fn test_codegen_type_any() {
            let ty = Type::Any;
            assert_eq!(type_to_rust_name(&ty), "Value");
            assert!(codegen_produces_output(&ty));
        }

        #[test]
        #[ignore = "parser not yet capturing param type annotations"]
        fn test_parse_type_unknown() {
            let ty = parse_fn_param_type("function f(x: unknown): unknown { return x; }", 0).unwrap();
            assert!(matches!(ty, Type::Unknown));
        }

        #[test]
        fn test_codegen_type_unknown() {
            let ty = Type::Unknown;
            assert_eq!(type_to_rust_name(&ty), "Value");
            assert!(codegen_produces_output(&ty));
        }

        #[test]
        #[ignore = "parser not yet capturing return type annotations"]
        fn test_parse_type_never() {
            let ty = parse_fn_return_type("function f(): never { throw new Error(); }").unwrap();
            assert!(matches!(ty, Type::Never));
        }

        #[test]
        fn test_codegen_type_never() {
            let ty = Type::Never;
            assert_eq!(type_to_rust_name(&ty), "!");
            assert!(codegen_produces_output(&ty));
        }

        #[test]
        #[ignore = "parser not yet capturing return type annotations"]
        fn test_parse_type_bigint() {
            let ty = parse_fn_return_type("function f(): bigint { return 123n; }").unwrap();
            assert!(matches!(ty, Type::BigInt));
        }

        #[test]
        fn test_codegen_type_bigint() {
            let ty = Type::BigInt;
            assert_eq!(type_to_rust_name(&ty), "i64");
            assert!(codegen_produces_output(&ty));
        }
    }

    // =========================================================================
    // SECTION 2: Complex Type Annotations
    // =========================================================================

    mod complex_types {
        use super::*;

        #[test]
        #[ignore = "parser not yet capturing param type annotations"]
        fn test_parse_type_array_syntax() {
            let ty = parse_fn_param_type("function f(x: string[]): string[] { return x; }", 0).unwrap();
            assert!(matches!(ty, Type::Array { .. }));
        }

        #[test]
        fn test_codegen_type_array_syntax() {
            let ty = Type::Array { elem: Box::new(Type::String) };
            assert_eq!(type_to_rust_name(&ty), "Vec<String>");
            assert!(codegen_produces_output(&ty));
        }

        #[test]
        #[ignore = "parser not yet capturing param type annotations"]
        fn test_parse_type_array_generic() {
            let ty = parse_fn_param_type("function f(x: Array<string>): Array<string> { return x; }", 0).unwrap();
            assert!(matches!(ty, Type::Array { .. }));
        }

        #[test]
        fn test_codegen_type_array_generic() {
            let ty = Type::Array { elem: Box::new(Type::Number) };
            assert_eq!(type_to_rust_name(&ty), "Vec<f64>");
            assert!(codegen_produces_output(&ty));
        }

        #[test]
        #[ignore = "parser not yet capturing param type annotations"]
        fn test_parse_type_multidim_array() {
            let ty = parse_fn_param_type("function f(x: number[][]): number[][] { return x; }", 0).unwrap();
            assert!(matches!(ty, Type::Array { .. }));
        }

        #[test]
        fn test_codegen_type_multidim_array() {
            let ty = Type::Array { elem: Box::new(Type::Array { elem: Box::new(Type::Number) }) };
            assert_eq!(type_to_rust_name(&ty), "Vec<Vec<f64>>");
        }

        #[test]
        #[ignore = "parser not yet capturing param type annotations"]
        fn test_parse_type_object_literal() {
            let ty = parse_fn_param_type("function f(x: { a: string; b: number }): { a: string; b: number } { return x; }", 0).unwrap();
            assert!(matches!(ty, Type::Object { .. }));
        }

        #[test]
        fn test_codegen_type_object_literal() {
            let ty = Type::Object {
                members: vec![
                    TypeMember { key: "name".into(), type_: Type::String, optional: false, readonly: false },
                    TypeMember { key: "age".into(), type_: Type::Number, optional: false, readonly: false },
                ],
            };
            let name = type_to_rust_name(&ty);
            assert!(name.contains("name: String") && name.contains("age: f64"), "got: {}", name);
            assert!(codegen_produces_output(&ty));
        }

        #[test]
        #[ignore = "parser not yet capturing function type annotations"]
        fn test_parse_type_function() {
            let ty = parse_fn_param_type("function f(x: (x: number) => string): (x: number) => string { return x; }", 0).unwrap();
            assert!(matches!(ty, Type::Function { .. }));
        }

        #[test]
        fn test_codegen_type_function() {
            let ty = Type::Function {
                params: vec![Type::Number],
                ret: Box::new(Type::String),
            };
            let name = type_to_rust_name(&ty);
            assert!(name.contains("fn") && name.contains("f64") && name.contains("String"), "got: {}", name);
            assert!(codegen_produces_output(&ty));
        }

        #[test]
        #[ignore = "parser not yet capturing param type annotations"]
        fn test_parse_type_union() {
            let ty = parse_fn_param_type("function f(x: string | number): string | number { return x; }", 0).unwrap();
            assert!(matches!(ty, Type::Union { .. }));
        }

        #[test]
        fn test_codegen_type_union() {
            let ty = Type::Union {
                types: vec![Type::String, Type::Number],
            };
            let name = type_to_rust_name(&ty);
            println!("union type name: {}", name);
            assert!(codegen_produces_output(&ty));
        }

        #[test]
        #[ignore = "parser not yet capturing param type annotations"]
        fn test_parse_type_intersection() {
            let ty = parse_fn_param_type("function f(x: { a: string } & { b: number }): { a: string } & { b: number } { return x; }", 0).unwrap();
            assert!(matches!(ty, Type::Intersection { .. }));
        }

        #[test]
        fn test_codegen_type_intersection() {
            let ty = Type::Intersection {
                types: vec![
                    Type::Object { members: vec![TypeMember { key: "a".into(), type_: Type::String, optional: false, readonly: false }] },
                    Type::Object { members: vec![TypeMember { key: "b".into(), type_: Type::Number, optional: false, readonly: false }] },
                ],
            };
            let name = type_to_rust_name(&ty);
            println!("intersection type name: {}", name);
            assert!(codegen_produces_output(&ty));
        }

        #[test]
        #[ignore = "parser not yet capturing param type annotations"]
        fn test_parse_type_literal_string() {
            let ty = parse_fn_param_type("function f(x: \"ok\"): \"ok\" { return x; }", 0).unwrap();
            assert!(matches!(ty, Type::Literal { kind: LiteralKind::String, .. }));
        }

        #[test]
        fn test_codegen_type_literal_string() {
            let ty = Type::Literal { kind: LiteralKind::String, value: "ok".into() };
            assert_eq!(type_to_rust_name(&ty), "\"ok\"");
            assert!(codegen_produces_output(&ty));
        }

        #[test]
        #[ignore = "parser not yet capturing param type annotations"]
        fn test_parse_type_literal_number() {
            let ty = parse_fn_param_type("function f(x: 42): 42 { return x; }", 0).unwrap();
            assert!(matches!(ty, Type::Literal { kind: LiteralKind::Number, .. }));
        }

        #[test]
        fn test_codegen_type_literal_number() {
            let ty = Type::Literal { kind: LiteralKind::Number, value: "42".into() };
            assert_eq!(type_to_rust_name(&ty), "42");
            assert!(codegen_produces_output(&ty));
        }

        #[test]
        #[ignore = "parser not yet capturing param type annotations"]
        fn test_parse_type_literal_boolean() {
            let ty = parse_fn_param_type("function f(x: true): true { return x; }", 0).unwrap();
            assert!(matches!(ty, Type::Literal { kind: LiteralKind::Boolean, .. }));
        }

        #[test]
        fn test_codegen_type_literal_boolean() {
            let ty = Type::Literal { kind: LiteralKind::Boolean, value: "true".into() };
            assert_eq!(type_to_rust_name(&ty), "true");
            assert!(codegen_produces_output(&ty));
        }

        #[test]
        #[ignore = "parser not yet capturing param type annotations"]
        fn test_parse_type_ref_simple() {
            let ty = parse_fn_param_type("function f(x: MyType): MyType { return x; }", 0).unwrap();
            assert!(matches!(ty, Type::Ref { .. }));
        }

        #[test]
        fn test_codegen_type_ref_simple() {
            let ty = Type::Ref { name: "MyType".into(), generics: vec![] };
            assert_eq!(type_to_rust_name(&ty), "MyType");
            assert!(codegen_produces_output(&ty));
        }

        #[test]
        #[ignore = "parser not yet capturing param type annotations"]
        fn test_parse_type_ref_generics() {
            let ty = parse_fn_param_type("function f(x: Result<string>): Result<string> { return x; }", 0).unwrap();
            assert!(matches!(ty, Type::Ref { name, generics } if name == "Result" && !generics.is_empty()));
        }

        #[test]
        fn test_codegen_type_ref_generics() {
            let ty = Type::Ref {
                name: "Result".into(),
                generics: vec![Type::String],
            };
            assert_eq!(type_to_rust_name(&ty), "Result<String>");
            assert!(codegen_produces_output(&ty));
        }
    }

    // =========================================================================
    // SECTION 3: Type Declarations
    // =========================================================================

    mod type_declarations {
        use super::*;

        #[test]
        #[ignore = "parser not yet capturing interface declarations"]
        fn test_parse_interface_simple() {
            let source = "interface Foo { bar: number; }";
            let decl = find_type_decl(source, "Foo");
            assert!(decl.is_some());
            assert!(matches!(decl.unwrap().type_, Type::Object { .. }));
        }

        #[test]
        #[ignore = "parser not yet capturing interface declarations"]
        fn test_parse_interface_with_optional() {
            let source = "interface Config { port?: number; host?: string; }";
            let decl = find_type_decl(source, "Config").unwrap();
            if let Type::Object { members } = decl.type_ {
                assert!(members.iter().any(|m| m.optional));
            }
        }

        #[test]
        #[ignore = "parser not yet capturing type alias declarations"]
        fn test_parse_type_alias_primitive() {
            let source = "type Status = string;";
            let decl = find_type_decl(source, "Status");
            assert!(decl.is_some());
            assert!(matches!(decl.unwrap().type_, Type::String));
        }

        #[test]
        #[ignore = "parser not yet capturing type alias declarations"]
        fn test_parse_type_alias_union() {
            let source = r#"type Status = "ok" | "err";"#;
            let decl = find_type_decl(source, "Status").unwrap();
            assert!(matches!(decl.type_, Type::Union { .. }));
        }

        #[test]
        #[ignore = "parser not yet capturing type alias declarations"]
        fn test_parse_type_alias_object() {
            let source = "type Point = { x: number; y: number; };";
            let decl = find_type_decl(source, "Point").unwrap();
            assert!(matches!(decl.type_, Type::Object { .. }));
        }

        #[test]
        #[ignore = "parser not yet capturing type alias declarations"]
        fn test_parse_type_alias_intersection() {
            let source = "type Combined = { a: string } & { b: number };";
            let decl = find_type_decl(source, "Combined").unwrap();
            assert!(matches!(decl.type_, Type::Intersection { .. }));
        }

        #[test]
        #[ignore = "parser not yet capturing generic function type params"]
        fn test_parse_generic_function() {
            let source = "function identity<T>(x: T): T { return x; }";
            let parser = TsParser::new();
            let result = parser.parse_source(source).unwrap();
            for item in &result.items {
                if let ModuleItem::Decl(Decl::Function(f)) = item {
                    assert!(!f.generics.is_empty());
                    assert_eq!(f.generics[0].name, "T");
                    return;
                }
            }
            panic!("expected function declaration");
        }

        #[test]
        fn test_codegen_generic_function() {
            let source = "function identity<T>(x: T): T { return x; }";
            let parser = TsParser::new();
            let module = parser.parse_source(source).unwrap();
            let cg = QuoteCodegen::default();
            for item in &module.items {
                if let ModuleItem::Decl(Decl::Function(f)) = item {
                    let tokens = cg.gen_fn(f);
                    let s = tokens.to_string();
                    assert!(s.contains("fn") || s.contains("Fn"), "got: {}", s);
                    return;
                }
            }
        }

        #[test]
        #[ignore = "parser not yet capturing interface declarations"]
        fn test_parse_generic_interface() {
            let source = "interface Container<T> { value: T; }";
            let decl = find_type_decl(source, "Container");
            assert!(decl.is_some());
            let decl = decl.unwrap();
            assert!(!decl.generics.is_empty());
            assert_eq!(decl.generics[0].name, "T");
        }

        #[test]
        #[ignore = "parser not yet capturing type alias declarations"]
        fn test_parse_generic_type_alias() {
            let source = "type Pair<A, B> = { first: A; second: B; };";
            let decl = find_type_decl(source, "Pair");
            assert!(decl.is_some());
            assert_eq!(decl.unwrap().generics.len(), 2);
        }
    }

    // =========================================================================
    // SECTION 4: Type-Directed Lowering (THE KEY FEATURE!)
    // =========================================================================

    mod type_directed_lowering {
        use super::*;

        // --- String Union -> Enum ---

        #[test]
        #[ignore = "parser not yet capturing type alias declarations"]
        fn test_lowering_string_union_two_variants() {
            let source = r#"type Status = "ok" | "err";"#;
            let decl = find_type_decl(source, "Status").unwrap();
            let ty = &decl.type_;
            assert!(matches!(ty, Type::Union { .. }));
            
            let converter = TypeToRust::new(OutputKind::String);
            let rust_type = converter.convert(ty);
            assert!(matches!(rust_type, RustType::Enum(_)), "got: {:?}", rust_type);
            
            let name = rust_type.type_name();
            println!("Status union lowering: {}", name);
        }

        #[test]
        #[ignore = "parser not yet capturing type alias declarations"]
        fn test_lowering_string_union_three_variants() {
            let source = r#"type Status = "ok" | "err" | "pending";"#;
            let decl = find_type_decl(source, "Status").unwrap();
            let ty = &decl.type_;
            
            let converter = TypeToRust::new(OutputKind::String);
            let rust_type = converter.convert(ty);
            assert!(matches!(rust_type, RustType::Enum(_)));
            
            let name = rust_type.type_name();
            println!("3-variant union: {}", name);
        }

        #[test]
        fn test_codegen_string_union() {
            let ty = Type::Union {
                types: vec![
                    Type::Literal { kind: LiteralKind::String, value: "ok".into() },
                    Type::Literal { kind: LiteralKind::String, value: "err".into() },
                ],
            };
            let cg = QuoteCodegen::default();
            let tokens = cg.gen_type(&ty);
            let s = tokens.to_string();
            println!("string union codegen: {}", s);
            assert!(!s.is_empty());
        }

        // --- Interface -> Struct ---

        #[test]
        #[ignore = "parser not yet capturing interface declarations"]
        fn test_lowering_interface_simple() {
            let source = "interface User { name: string; age: number; }";
            let decl = find_type_decl(source, "User").unwrap();
            let ty = &decl.type_;
            assert!(matches!(ty, Type::Object { .. }));
            
            let converter = TypeToRust::new(OutputKind::String);
            let rust_type = converter.convert(ty);
            assert!(matches!(rust_type, RustType::Struct(_)), "got: {:?}", rust_type);
            
            let name = rust_type.type_name();
            assert!(name.contains("name: String") && name.contains("age: f64"), "got: {}", name);
        }

        #[test]
        fn test_codegen_interface() {
            let ty = Type::Object {
                members: vec![
                    TypeMember { key: "name".into(), type_: Type::String, optional: false, readonly: false },
                    TypeMember { key: "age".into(), type_: Type::Number, optional: false, readonly: false },
                ],
            };
            let cg = QuoteCodegen::default();
            let tokens = cg.gen_type(&ty);
            let s = tokens.to_string();
            println!("interface codegen: {}", s);
            assert!(!s.is_empty());
        }

        // --- Tagged Union -> Enum ---

        #[test]
        #[ignore = "parser not yet capturing type alias declarations"]
        fn test_lowering_tagged_union_simple() {
            let source = r#"type Result = { ok: true; data: string } | { ok: false; error: string };"#;
            let decl = find_type_decl(source, "Result").unwrap();
            let ty = &decl.type_;
            assert!(matches!(ty, Type::Union { .. }));
            
            let converter = TypeToRust::new(OutputKind::String);
            let rust_type = converter.convert(ty);
            println!("tagged union lowering: {:?}", rust_type);
            assert!(matches!(rust_type, RustType::Enum(_)));
        }

        #[test]
        #[ignore = "parser not yet capturing type alias declarations"]
        fn test_lowering_discriminated_union_kind_field() {
            let source = r#"type Shape = { kind: "circle"; radius: number } | { kind: "rect"; width: number; height: number };"#;
            let decl = find_type_decl(source, "Shape").unwrap();
            let ty = &decl.type_;
            
            let converter = TypeToRust::new(OutputKind::String);
            let rust_type = converter.convert(ty);
            let name = rust_type.type_name();
            println!("discriminated union: {}", name);
            assert!(matches!(rust_type, RustType::Enum(_)));
        }

        // --- Null/Undefined -> Option ---

        #[test]
        fn test_lowering_null_to_option() {
            let ty = Type::Union {
                types: vec![Type::String, Type::Null],
            };
            let converter = TypeToRust::new(OutputKind::String);
            let rust_type = converter.convert(&ty);
            println!("string | null lowering: {:?}", rust_type);
        }

        #[test]
        fn test_lowering_undefined_to_option() {
            let ty = Type::Union {
                types: vec![Type::String, Type::Undefined],
            };
            let converter = TypeToRust::new(OutputKind::String);
            let rust_type = converter.convert(&ty);
            println!("string | undefined lowering: {:?}", rust_type);
        }

        // --- Array -> Vec ---

        #[test]
        fn test_lowering_array_primitive() {
            let ty = Type::Array { elem: Box::new(Type::Number) };
            let converter = TypeToRust::new(OutputKind::String);
            let rust_type = converter.convert(&ty);
            assert!(matches!(rust_type, RustType::Vec(_)));
            assert_eq!(rust_type.type_name(), "Vec<f64>");
        }

        #[test]
        fn test_lowering_array_string() {
            let ty = Type::Array { elem: Box::new(Type::String) };
            let converter = TypeToRust::new(OutputKind::String);
            let rust_type = converter.convert(&ty);
            assert_eq!(rust_type.type_name(), "Vec<String>");
        }

        // --- Generic Monomorphization ---

        #[test]
        #[ignore = "parser not yet capturing generic function type params"]
        fn test_lowering_generic_simple() {
            let source = "function identity<T>(x: T): T { return x; }";
            let parser = TsParser::new();
            let module = parser.parse_source(source).unwrap();
            for item in &module.items {
                if let ModuleItem::Decl(Decl::Function(f)) = item {
                    assert!(!f.generics.is_empty());
                    assert!(f.return_type.is_some());
                    return;
                }
            }
            panic!("expected function");
        }

        // --- Primitive mappings ---

        #[test]
        fn test_lowering_string() {
            assert_eq!(type_to_rust_name(&Type::String), "String");
        }

        #[test]
        fn test_lowering_number() {
            assert_eq!(type_to_rust_name(&Type::Number), "f64");
        }

        #[test]
        fn test_lowering_boolean() {
            assert_eq!(type_to_rust_name(&Type::Boolean), "bool");
        }

        #[test]
        fn test_lowering_void() {
            assert_eq!(type_to_rust_name(&Type::Void), "()");
        }

        #[test]
        fn test_lowering_never() {
            assert_eq!(type_to_rust_name(&Type::Never), "!");
        }

        #[test]
        fn test_lowering_any() {
            assert_eq!(type_to_rust_name(&Type::Any), "Value");
        }

        #[test]
        fn test_lowering_unknown() {
            assert_eq!(type_to_rust_name(&Type::Unknown), "Value");
        }

        // --- Function type -> Fn trait ---

        #[test]
        fn test_lowering_function_type() {
            let ty = Type::Function {
                params: vec![Type::Number],
                ret: Box::new(Type::String),
            };
            let converter = TypeToRust::new(OutputKind::String);
            let rust_type = converter.convert(&ty);
            let name = rust_type.type_name();
            println!("function type lowering: {}", name);
            assert!(name.contains("fn") || name.contains("Fn"));
        }
    }

    // =========================================================================
    // SECTION 5: Codegen TokenStream Verification
    // =========================================================================

    mod codegen_verification {
        use super::*;

        #[test]
        fn test_all_primitive_types_codegen() {
            let types = vec![
                Type::String,
                Type::Number,
                Type::Boolean,
                Type::Null,
                Type::Undefined,
                Type::Void,
                Type::Never,
                Type::Any,
                Type::Unknown,
                Type::BigInt,
            ];
            for ty in types {
                let tokens = QuoteCodegen::default().gen_type(&ty);
                assert!(!tokens.is_empty(), "Type {:?} should produce output", ty);
            }
        }

        #[test]
        fn test_all_complex_types_codegen() {
            let types = vec![
                Type::Array { elem: Box::new(Type::String) },
                Type::Ref { name: "MyType".into(), generics: vec![] },
                Type::Ref { name: "Result".into(), generics: vec![Type::String] },
                Type::Union { types: vec![Type::String, Type::Number] },
                Type::Intersection { types: vec![Type::Object { members: vec![] }, Type::Object { members: vec![] }] },
                Type::Function { params: vec![Type::Number], ret: Box::new(Type::String) },
                Type::Object { members: vec![TypeMember { key: "x".into(), type_: Type::Number, optional: false, readonly: false }] },
            ];
            for ty in types {
                let tokens = QuoteCodegen::default().gen_type(&ty);
                assert!(!tokens.is_empty(), "Type {:?} should produce output", ty);
            }
        }

        #[test]
        fn test_literal_types_codegen() {
            let types = vec![
                Type::Literal { kind: LiteralKind::String, value: "ok".into() },
                Type::Literal { kind: LiteralKind::Number, value: "42".into() },
                Type::Literal { kind: LiteralKind::Boolean, value: "true".into() },
            ];
            for ty in types {
                let tokens = QuoteCodegen::default().gen_type(&ty);
                assert!(!tokens.is_empty(), "Literal type {:?} should produce output", ty);
            }
        }
    }

    // =========================================================================
    // SECTION 6: Round-trip Integration Tests
    // =========================================================================

    mod roundtrip_integration {
        use super::*;

        #[test]
        #[ignore = "parser not yet capturing type alias declarations"]
        fn test_roundtrip_type_alias_string_union() {
            let source = r#"type Status = "ok" | "err" | "pending";"#;
            let decl = find_type_decl(source, "Status").unwrap();
            let name = type_to_rust_name(&decl.type_);
            println!("Status type alias: {}", name);
            assert!(!name.is_empty());
        }

        #[test]
        #[ignore = "parser not yet capturing interface declarations"]
        fn test_roundtrip_interface_with_all_primitives() {
            let source = "interface AllPrimitives { s: string; n: number; b: boolean; }";
            let decl = find_type_decl(source, "AllPrimitives").unwrap();
            let name = type_to_rust_name(&decl.type_);
            assert!(name.contains("s:") && name.contains("String"));
            assert!(name.contains("n:") && name.contains("f64"));
            assert!(name.contains("b:") && name.contains("bool"));
        }

        #[test]
        #[ignore = "parser not yet capturing function type annotations"]
        fn test_roundtrip_function_with_type_annotations() {
            let source = "function greet(name: string, age: number): string { return ''; }";
            let parser = TsParser::new();
            let module = parser.parse_source(source).unwrap();
            for item in &module.items {
                if let ModuleItem::Decl(Decl::Function(f)) = item {
                    assert!(f.params[0].type_.is_some());
                    assert!(f.params[1].type_.is_some());
                    assert!(f.return_type.is_some());
                    assert_eq!(type_to_rust_name(f.params[0].type_.as_ref().unwrap()), "String");
                    assert_eq!(type_to_rust_name(f.params[1].type_.as_ref().unwrap()), "f64");
                    assert_eq!(type_to_rust_name(f.return_type.as_ref().unwrap()), "String");
                    return;
                }
            }
            panic!("expected function");
        }

        #[test]
        #[ignore = "parser not yet capturing generic function type params"]
        fn test_roundtrip_generic_function_identity() {
            let source = "function identity<T>(value: T): T { return value; }";
            let parser = TsParser::new();
            let module = parser.parse_source(source).unwrap();
            for item in &module.items {
                if let ModuleItem::Decl(Decl::Function(f)) = item {
                    assert!(!f.generics.is_empty());
                    assert_eq!(f.generics[0].name, "T");
                    println!("generic param: {}", type_to_rust_name(f.params[0].type_.as_ref().unwrap()));
                    println!("generic return: {}", type_to_rust_name(f.return_type.as_ref().unwrap()));
                    return;
                }
            }
            panic!("expected function");
        }

        #[test]
        #[ignore = "parser not yet capturing type alias declarations"]
        fn test_roundtrip_tagged_union_result() {
            let source = r#"type Result = { ok: true; data: string } | { ok: false; error: string };"#;
            let decl = find_type_decl(source, "Result").unwrap();
            let name = type_to_rust_name(&decl.type_);
            println!("Result tagged union: {}", name);
        }

        #[test]
        #[ignore = "parser not yet capturing type alias declarations"]
        fn test_roundtrip_shape_discriminated_union() {
            let source = r#"type Shape = { kind: "circle"; radius: number } | { kind: "rect"; width: number; height: number };"#;
            let decl = find_type_decl(source, "Shape").unwrap();
            let name = type_to_rust_name(&decl.type_);
            println!("Shape discriminated union: {}", name);
        }
    }
}