//! Spec 2.1-2.2: Variables & Functions parser + codegen tests
//!
//! allow:too_many_lines,complexity
//!
//! Covers:
//! - Variable bindings (const, let, var)
//! - Destructuring (object, array, nested, rest, defaults)
//! - Function declarations and expressions
//! - Arrow functions (sync/async)
//! - Parameter forms (default, rest, multiple)
//! - Return type annotations

#[cfg(test)]
mod spec_vars_functions_tests {
    use crate::transpile::hir::{
        Decl, Expr, FunctionDecl, ModuleItem, ObjectPatProp, Param, Pat, Pat::*,
        QuoteCodegen, Stmt, Type, VariableDecl, VariableKind,
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

    fn parse_first_decl(source: &str) -> Decl {
        let items = parse_source(source);
        let item = items.first().expect("no items");
        match item {
            ModuleItem::Decl(d) => d.clone(),
            ModuleItem::Stmt(Stmt::Variable(v)) => Decl::Variable(VariableDecl {
                name: v.name.clone(),
                kind: v.kind.clone(),
                type_: v.type_.clone(),
                init: v.init.clone(),
                pattern: v.pattern.clone(),
            }),
            ModuleItem::Stmt(Stmt::FunctionDecl(f)) => Decl::Function(FunctionDecl {
                name: f.name.clone(),
                generics: f.generics.clone(),
                params: f.params.clone(),
                return_type: f.return_type.clone(),
                body: f.body.clone(),
                is_async: f.is_async,
                is_generator: f.is_generator,
                decorators: f.decorators.clone(),
                throws: f.throws,
                error_type: f.error_type.clone(),
            }),
            _ => panic!("expected decl, got {:?}", item),
        }
    }

    fn find_function(source: &str) -> FunctionDecl {
        let items = parse_source(source);
        for item in &items {
            if let ModuleItem::Decl(Decl::Function(f)) = item {
                return f.clone();
            }
        }
        panic!("no function found in: {}", source);
    }

    fn find_variable(source: &str) -> VariableDecl {
        let items = parse_source(source);
        for item in &items {
            if let ModuleItem::Decl(Decl::Variable(v)) = item {
                return v.clone();
            }
            if let ModuleItem::Stmt(Stmt::Variable(v)) = item {
                return v.clone();
            }
        }
        panic!("no variable found in: {}", source);
    }

    fn find_expr_in_var(source: &str) -> Expr {
        let v = find_variable(source);
        v.init.expect("no init")
    }

    // =============================================================================
    // Codegen helpers
    // =============================================================================

    fn codegen_expr(expr: &Expr) -> TokenStream {
        QuoteCodegen::default().gen_expr(expr)
    }

    fn codegen_stmt(stmt: &Stmt) -> Option<TokenStream> {
        QuoteCodegen::default().gen_stmt(stmt)
    }

    fn codegen_fn(func: &FunctionDecl) -> TokenStream {
        QuoteCodegen::default().gen_fn(func)
    }

    fn codegen_decl(decl: &Decl) -> TokenStream {
        match decl {
            Decl::Function(f) => codegen_fn(f),
            Decl::Variable(v) => {
                let stmt = Stmt::Variable(v.clone());
                codegen_stmt(&stmt).unwrap_or_default()
            }
            _ => TokenStream::new(),
        }
    }

    fn assert_codegen_not_null(expr: &Expr, label: &str) {
        let tokens = codegen_expr(expr);
        let s = tokens.to_string();
        assert!(
            !s.contains("Value :: Null") && !s.contains("Value::Null"),
            "{}: codegen produced Value::Null: {}",
            label,
            s
        );
    }

    // =============================================================================
    // SECTION 2.1: VARIABLES & BINDING
    // =============================================================================

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
    }

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

    // =============================================================================
    // SECTION 2.2: FUNCTIONS
    // =============================================================================

    mod function_declarations {
        use super::*;

        #[test]
        fn basic_function_decl() {
            let func = find_function("function foo() {}");
            assert_eq!(func.name, "foo");
            assert!(func.params.is_empty());
            assert!(!func.is_async);
            let tokens = codegen_fn(&func);
            assert!(!tokens.is_empty(), "function decl codegen should produce output");
        }

        #[test]
        fn function_with_body() {
            let func = find_function("function foo() { return 1; }");
            assert!(func.body.is_some());
            let tokens = codegen_fn(&func);
            let s = tokens.to_string();
            assert!(s.contains("fn foo") || s.contains("foo"), "should contain function name");
        }

        #[test]
        fn function_with_single_param() {
            let func = find_function("function foo(x) {}");
            assert_eq!(func.params.len(), 1);
            assert_eq!(func.params[0].name, "x");
            let tokens = codegen_fn(&func);
            assert!(!tokens.is_empty());
        }

        #[test]
        fn function_with_multiple_params() {
            let func = find_function("function f(a, b, c) {}");
            assert_eq!(func.params.len(), 3);
            assert_eq!(func.params[0].name, "a");
            assert_eq!(func.params[1].name, "b");
            assert_eq!(func.params[2].name, "c");
            let tokens = codegen_fn(&func);
            assert!(!tokens.is_empty());
        }

        #[test]
        fn function_with_return_type() {
            let func = find_function("function f(): number { return 1; }");
            assert!(func.return_type.is_some());
            if let Some(Type::Number) = func.return_type.as_ref() {
                // correct
            } else {
                panic!("expected number return type, got {:?}", func.return_type);
            }
            let tokens = codegen_fn(&func);
            let s = tokens.to_string();
            assert!(!s.is_empty());
        }

        #[test]
        fn function_with_void_return() {
            let func = find_function("function f(): void { return; }");
            assert!(func.return_type.is_some());
            let tokens = codegen_fn(&func);
            assert!(!tokens.is_empty());
        }

        #[test]
        fn function_with_string_return_type() {
            let func = find_function("function f(): string { return 'hi'; }");
            assert!(func.return_type.is_some());
            let tokens = codegen_fn(&func);
            assert!(!tokens.is_empty());
        }

        #[test]
        fn function_with_boolean_return_type() {
            let func = find_function("function f(): boolean { return true; }");
            assert!(func.return_type.is_some());
            let tokens = codegen_fn(&func);
            assert!(!tokens.is_empty());
        }

        #[test]
        fn function_no_return_type_annotation() {
            let func = find_function("function f() { return 1; }");
            assert!(func.return_type.is_none());
            let tokens = codegen_fn(&func);
            assert!(!tokens.is_empty());
        }

        #[test]
        fn function_generator() {
            let func = find_function("function* g() { yield 1; }");
            assert!(func.is_generator);
            let tokens = codegen_fn(&func);
            assert!(!tokens.is_empty());
        }
    }

    mod arrow_functions {
        use super::*;

        #[test]
        fn arrow_no_params() {
            let expr = find_expr_in_var("const f = () => 1;");
            assert!(matches!(expr, Expr::ArrowFunction { .. }));
            if let Expr::ArrowFunction { params, .. } = &expr {
                assert!(params.is_empty());
            }
            assert_codegen_not_null(&expr, "arrow no params");
        }

        #[test]
        fn arrow_single_param() {
            let expr = find_expr_in_var("const f = x => x + 1;");
            assert!(matches!(expr, Expr::ArrowFunction { .. }));
            if let Expr::ArrowFunction { params, .. } = &expr {
                assert_eq!(params.len(), 1);
                assert_eq!(params[0].name, "x");
            }
            assert_codegen_not_null(&expr, "arrow single param");
        }

        #[test]
        fn arrow_with_parens_single_param() {
            let expr = find_expr_in_var("const f = (x) => x + 1;");
            assert!(matches!(expr, Expr::ArrowFunction { .. }));
            assert_codegen_not_null(&expr, "arrow with parens");
        }

        #[test]
        fn arrow_multiple_params() {
            let expr = find_expr_in_var("const f = (a, b, c) => a + b + c;");
            assert!(matches!(expr, Expr::ArrowFunction { .. }));
            if let Expr::ArrowFunction { params, .. } = &expr {
                assert_eq!(params.len(), 3);
            }
            assert_codegen_not_null(&expr, "arrow multiple params");
        }

        #[test]
        fn arrow_block_body() {
            let expr = find_expr_in_var("const f = () => { return 1; };");
            assert!(matches!(expr, Expr::ArrowFunction { .. }));
            assert_codegen_not_null(&expr, "arrow block body");
        }

        #[test]
        fn arrow_expr_body() {
            let expr = find_expr_in_var("const f = () => 42;");
            assert!(matches!(expr, Expr::ArrowFunction { .. }));
            assert_codegen_not_null(&expr, "arrow expr body");
        }

        #[test]
        fn arrow_async() {
            let expr = find_expr_in_var("const f = async () => {};");
            assert!(matches!(expr, Expr::ArrowFunction { .. }));
            if let Expr::ArrowFunction { is_async, .. } = &expr {
                assert!(*is_async);
            }
            assert_codegen_not_null(&expr, "arrow async");
        }

        #[test]
        fn arrow_async_with_param() {
            let expr = find_expr_in_var("const f = async x => await x;");
            assert!(matches!(expr, Expr::ArrowFunction { .. }));
            if let Expr::ArrowFunction { is_async, params, .. } = &expr {
                assert!(*is_async);
                assert_eq!(params.len(), 1);
            }
            assert_codegen_not_null(&expr, "arrow async with param");
        }
    }

    mod async_functions {
        use super::*;

        #[test]
        fn async_function_basic() {
            let func = find_function("async function foo() {}");
            assert!(func.is_async);
            let tokens = codegen_fn(&func);
            assert!(!tokens.is_empty(), "async function codegen should produce output");
        }

        #[test]
        fn async_function_with_await() {
            let func = find_function("async function foo() { return await Promise.resolve(1); }");
            assert!(func.is_async);
            assert!(func.body.is_some());
            let tokens = codegen_fn(&func);
            assert!(!tokens.is_empty());
        }

        #[test]
        fn async_function_no_params() {
            let func = find_function("async function f() {}");
            assert!(func.is_async);
            assert!(func.params.is_empty());
            let tokens = codegen_fn(&func);
            assert!(!tokens.is_empty());
        }

        #[test]
        fn async_function_with_params() {
            let func = find_function("async function f(x, y) {}");
            assert!(func.is_async);
            assert_eq!(func.params.len(), 2);
            let tokens = codegen_fn(&func);
            assert!(!tokens.is_empty());
        }

        #[test]
        fn async_function_return_type() {
            let func = find_function("async function f(): Promise<number> { return Promise.resolve(1); }");
            assert!(func.is_async);
            assert!(func.return_type.is_some());
            let tokens = codegen_fn(&func);
            assert!(!tokens.is_empty());
        }
    }

    mod function_parameters {
        use super::*;

        #[test]
        fn function_default_param() {
            let func = find_function("function f(x = 1) {}");
            assert_eq!(func.params.len(), 1);
            assert!(func.params[0].default.is_some());
            let tokens = codegen_fn(&func);
            assert!(!tokens.is_empty(), "default param codegen should produce output");
        }

        #[test]
        fn function_multiple_default_params() {
            let func = find_function("function f(a = 1, b = 2) {}");
            assert_eq!(func.params.len(), 2);
            assert!(func.params[0].default.is_some());
            assert!(func.params[1].default.is_some());
            let tokens = codegen_fn(&func);
            assert!(!tokens.is_empty());
        }

        #[test]
        fn function_mixed_params() {
            let func = find_function("function f(a, b = 1, c) {}");
            assert_eq!(func.params.len(), 3);
            assert!(func.params[0].default.is_none());
            assert!(func.params[1].default.is_some());
            assert!(func.params[2].default.is_none());
            let tokens = codegen_fn(&func);
            assert!(!tokens.is_empty());
        }

        #[test]
        fn function_rest_param() {
            let func = find_function("function f(...args) {}");
            assert_eq!(func.params.len(), 1);
            assert!(func.params[0].pattern.is_some());
            let tokens = codegen_fn(&func);
            assert!(!tokens.is_empty(), "rest param codegen should produce output");
        }

        #[test]
        fn function_rest_with_other_params() {
            let func = find_function("function f(a, b, ...rest) {}");
            assert_eq!(func.params.len(), 3);
            let tokens = codegen_fn(&func);
            assert!(!tokens.is_empty());
        }

        #[test]
        fn function_optional_param() {
            let func = find_function("function f(x?) {}");
            assert_eq!(func.params.len(), 1);
            assert!(func.params[0].optional);
            let tokens = codegen_fn(&func);
            assert!(!tokens.is_empty());
        }

        #[test]
        fn function_param_with_type() {
            let func = find_function("function f(x: number) {}");
            assert_eq!(func.params.len(), 1);
            assert!(func.params[0].type_.is_some());
            let tokens = codegen_fn(&func);
            assert!(!tokens.is_empty());
        }

        #[test]
        fn function_param_with_type_and_default() {
            let func = find_function("function f(x: number = 1) {}");
            assert_eq!(func.params.len(), 1);
            assert!(func.params[0].type_.is_some());
            assert!(func.params[0].default.is_some());
            let tokens = codegen_fn(&func);
            assert!(!tokens.is_empty());
        }

        #[test]
        fn function_param_array_pattern() {
            let func = find_function("function f([a, b]) {}");
            assert_eq!(func.params.len(), 1);
            assert!(func.params[0].pattern.is_some());
            let tokens = codegen_fn(&func);
            assert!(!tokens.is_empty());
        }

        #[test]
        fn function_param_object_pattern() {
            let func = find_function("function f({a, b}) {}");
            assert_eq!(func.params.len(), 1);
            assert!(func.params[0].pattern.is_some());
            let tokens = codegen_fn(&func);
            assert!(!tokens.is_empty());
        }

        #[test]
        fn function_param_rest_array_pattern() {
            let func = find_function("function f(...[a, b]) {}");
            assert_eq!(func.params.len(), 1);
            assert!(func.params[0].pattern.is_some());
            let tokens = codegen_fn(&func);
            assert!(!tokens.is_empty());
        }
    }

    // =============================================================================
    // INTEGRATION: round-trip codegen for all constructs
    // =============================================================================

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

    // =============================================================================
    // Edge cases and combined patterns
    // =============================================================================

    mod edge_cases {
        use super::*;

        #[test]
        fn function_in_block() {
            let source = "if (true) { function f() {} }";
            let items = parse_source(source);
            assert!(!items.is_empty());
        }

        #[test]
        fn multiple_variables() {
            let source = "const a = 1, b = 2, c = 3;";
            let items = parse_source(source);
            assert!(!items.is_empty());
        }

        #[test]
        fn destructure_function_return() {
            let source = "const {a, b} = foo();";
            let decl = parse_first_decl(source);
            match decl {
                Decl::Variable(ref v) => {
                    assert!(v.pattern.is_some());
                }
                _ => panic!("expected variable decl"),
            }
            let tokens = codegen_decl(&decl);
            assert!(!tokens.is_empty());
        }

        #[test]
        fn nested_arrow_in_function() {
            let func = find_function("function outer() { const f = () => 1; }");
            assert!(func.body.is_some());
            let tokens = codegen_fn(&func);
            assert!(!tokens.is_empty());
        }

        #[test]
        fn function_calling_function() {
            let source = "function a() {} function b() { a(); }";
            let items = parse_source(source);
            assert!(items.len() >= 2, "should parse multiple functions");
            let func_b = find_function("function b() { a(); }");
            assert!(func_b.body.is_some());
        }

        #[test]
        fn arrow_with_complex_body() {
            let expr = find_expr_in_var("const f = (x: number): number => { return x * 2; };");
            assert!(matches!(expr, Expr::ArrowFunction { .. }));
            assert_codegen_not_null(&expr, "arrow complex body");
        }

        #[test]
        fn function_with_throws() {
            let source = "function f() throws { }";
            let func = find_function(source);
            let tokens = codegen_fn(&func);
            assert!(!tokens.is_empty());
        }
    }
}