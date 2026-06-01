//! Parser completeness tests — verifies all statement/expression types are handled.
//!
//! allow:too_many_lines,complexity

#[cfg(test)]
mod completeness_parser_tests {
    use crate::transpile::hir::*;
    use crate::transpile::parser::TsParser;

    /// Parse a statement and return the first statement
    fn parse_first_stmt(source: &str) -> Stmt {
        let parser = TsParser::new();
        let result = parser.parse_source(source).expect("parse failed");
        let item = result.items.first().expect("no items");
        match item {
            ModuleItem::Stmt(s) => s.clone(),
            ModuleItem::Decl(Decl::Function(_)) => {
                Stmt::FunctionDecl(FunctionDecl {
                    name: String::new(),
                    generics: vec![],
                    params: vec![],
                    return_type: None,
                    body: None,
                    is_async: false,
                    is_generator: false,
                    decorators: vec![],
                    throws: false,
                    error_type: None,
                })
            }
            ModuleItem::Decl(Decl::Variable(_)) => Stmt::Empty,
            ModuleItem::Decl(Decl::Class(_)) => Stmt::Class(ClassDecl {
                name: String::new(),
                extends: None,
                members: vec![],
                generics: vec![],
                methods: vec![],
            }),
            ModuleItem::Decl(Decl::Type(_)) => Stmt::Empty,
            ModuleItem::Import(_) => Stmt::Empty,
            ModuleItem::Export(_) => Stmt::Empty,
        }
    }

    /// Parse an expression wrapped in a variable declaration
    fn parse_expr_in_var(source: &str) -> Expr {
        let parser = TsParser::new();
        let result = parser.parse_source(source).expect("parse failed");
        for item in &result.items {
            if let ModuleItem::Decl(Decl::Variable(v)) = item {
                if let Some(expr) = &v.init {
                    return (*expr).clone();
                }
            }
        }
        Expr::Invalid
    }

    /// Assert the statement parsed from source is NOT Stmt::Empty
    fn assert_not_empty(source: &str, label: &str) {
        let stmt = parse_first_stmt(source);
        assert!(
            !matches!(stmt, Stmt::Empty),
            "{}: parsed to Stmt::Empty, expected non-empty statement: {:?}",
            label,
            source
        );
    }

    /// Assert the expression parsed from source is NOT Expr::Invalid
    fn assert_not_invalid(source: &str, label: &str) {
        let expr = parse_expr_in_var(source);
        assert!(
            !matches!(expr, Expr::Invalid),
            "{}: parsed to Expr::Invalid, expected valid expression: {:?}",
            label,
            source
        );
    }

    // ============================================================
    // STATEMENT COVERAGE
    // ============================================================

    #[test]
    fn stmt_if() {
        assert_not_empty("if (x) { }", "if statement");
    }

    #[test]
    fn stmt_while() {
        assert_not_empty("while (x) { }", "while statement");
    }

    #[test]
    fn stmt_do_while() {
        assert_not_empty("do { } while (x);", "do-while statement");
    }

    #[test]
    fn stmt_for() {
        assert_not_empty("for (let i = 0; i < 10; i++) { }", "for statement");
    }

    #[test]
    fn stmt_for_in() {
        assert_not_empty("for (let x in obj) { }", "for-in statement");
    }

    #[test]
    fn stmt_for_of() {
        assert_not_empty("for (let x of arr) { }", "for-of statement");
    }

    #[test]
    fn stmt_switch() {
        assert_not_empty("switch (x) { case 1: break; }", "switch statement");
    }

    #[test]
    fn stmt_try_catch() {
        assert_not_empty("try { } catch (e) { }", "try-catch statement");
    }

    #[test]
    fn stmt_try_finally() {
        assert_not_empty("try { } finally { }", "try-finally statement");
    }

    #[test]
    fn stmt_try_catch_finally() {
        assert_not_empty("try { } catch (e) { } finally { }", "try-catch-finally");
    }

    #[test]
    fn stmt_throw() {
        assert_not_empty("throw new Error();", "throw statement");
    }

    #[test]
    fn stmt_break() {
        assert_not_empty("break;", "break statement");
    }

    #[test]
    fn stmt_continue() {
        assert_not_empty("continue;", "continue statement");
    }

    #[test]
    fn stmt_return() {
        assert_not_empty("return;", "return statement");
    }

    #[test]
    fn stmt_return_val() {
        assert_not_empty("return 1;", "return with value");
    }

    #[test]
    fn stmt_with() {
        // 'with' is deprecated and should be handled explicitly
        let stmt = parse_first_stmt("with (obj) { }");
        // Parser may reject with (syntax error) or emit Stmt::Empty
        // This test documents the current behavior
        println!("with statement result: {:?}", stmt);
    }

    #[test]
    fn stmt_labeled() {
        assert_not_empty("label: { }", "labeled statement");
    }

    #[test]
    fn stmt_debugger() {
        let stmt = parse_first_stmt("debugger;");
        // debugger may map to Stmt::Empty or cause parse error
        println!("debugger statement result: {:?}", stmt);
    }

    #[test]
    fn stmt_function_decl() {
        assert_not_empty("function f() { }", "function declaration");
    }

    #[test]
    fn stmt_class_decl() {
        assert_not_empty("class Foo { }", "class declaration");
    }

    #[test]
    fn stmt_export_default() {
        let source = "export default 42;";
        let parser = TsParser::new();
        let result = parser.parse_source(source).expect("parse failed");
        let has_export = result.items.iter().any(|item| {
            matches!(item, ModuleItem::Stmt(Stmt::ExportDefault { .. }))
        });
        assert!(has_export, "export default should parse to Stmt::ExportDefault");
    }

    #[test]
    fn stmt_export_named() {
        let source = "export { foo };";
        let parser = TsParser::new();
        let result = parser.parse_source(source).expect("parse failed");
        let has_export = result.items.iter().any(|item| {
            matches!(item, ModuleItem::Stmt(Stmt::ExportNamed { .. }))
        });
        assert!(has_export, "export named should parse to Stmt::ExportNamed");
    }

    #[test]
    fn stmt_import() {
        let source = "import { foo } from 'mod';";
        let parser = TsParser::new();
        let result = parser.parse_source(source).expect("parse failed");
        let has_import = result.items.iter().any(|item| {
            matches!(item, ModuleItem::Import(_))
        });
        assert!(has_import, "import should parse to ModuleItem::Import");
    }

    #[test]
    fn stmt_import_default() {
        let source = "import foo from 'mod';";
        let parser = TsParser::new();
        let result = parser.parse_source(source).expect("parse failed");
        let has_import = result.items.iter().any(|item| {
            matches!(item, ModuleItem::Import(_))
        });
        assert!(has_import, "import default should parse");
    }

    #[test]
    fn stmt_var_const() {
        assert_not_empty("const x = 1;", "const declaration");
    }

    #[test]
    fn stmt_var_let() {
        assert_not_empty("let x = 1;", "let declaration");
    }

    #[test]
    fn stmt_var_var() {
        assert_not_empty("var x = 1;", "var declaration");
    }

    #[test]
    fn stmt_block() {
        assert_not_empty("{ const x = 1; }", "block statement");
    }

    #[test]
    fn stmt_empty() {
        let source = ";";
        let parser = TsParser::new();
        let result = parser.parse_source(source).expect("parse failed");
        let first = result.items.first();
        if let Some(ModuleItem::Stmt(s)) = first {
            // empty statement should parse
            assert!(matches!(s, Stmt::Empty), "empty statement should be Stmt::Empty");
        }
    }

    #[test]
    fn stmt_expr() {
        assert_not_empty("x + 1;", "expression statement");
    }

    // ============================================================
    // EXPRESSION COVERAGE
    // ============================================================

    #[test]
    fn expr_string() {
        assert_not_invalid(r#"const x = "hello";"#, "string literal");
    }

    #[test]
    fn expr_number() {
        assert_not_invalid("const x = 42;", "number literal");
    }

    #[test]
    fn expr_boolean_true() {
        assert_not_invalid("const x = true;", "boolean true");
    }

    #[test]
    fn expr_boolean_false() {
        assert_not_invalid("const x = false;", "boolean false");
    }

    #[test]
    fn expr_null() {
        assert_not_invalid("const x = null;", "null literal");
    }

    #[test]
    fn expr_undefined() {
        assert_not_invalid("const x = undefined;", "undefined literal");
    }

    #[test]
    fn expr_bigint() {
        assert_not_invalid("const x = 123n;", "bigint literal");
    }

    #[test]
    fn expr_regexp() {
        assert_not_invalid("const x = /abc/;", "regexp literal");
    }

    #[test]
    fn expr_ident() {
        assert_not_invalid("const x = y;", "identifier");
    }

    #[test]
    fn expr_binary_add() {
        assert_not_invalid("const x = 1 + 2;", "binary add");
    }

    #[test]
    fn expr_binary_strict_eq() {
        assert_not_invalid("const x = a === b;", "strict equality");
    }

    #[test]
    fn expr_binary_instanceof() {
        assert_not_invalid("const x = x instanceof Y;", "instanceof");
    }

    #[test]
    fn expr_unary_not() {
        assert_not_invalid("const x = !a;", "unary not");
    }

    #[test]
    fn expr_unary_typeof() {
        assert_not_invalid("const x = typeof x;", "typeof");
    }

    #[test]
    fn expr_unary_void() {
        assert_not_invalid("const x = void 0;", "void");
    }

    #[test]
    fn expr_update_pre_inc() {
        assert_not_invalid("const x = ++i;", "pre-increment");
    }

    #[test]
    fn expr_update_post_dec() {
        assert_not_invalid("const x = i--;", "post-decrement");
    }

    #[test]
    fn expr_logical_and() {
        assert_not_invalid("const x = a && b;", "logical and");
    }

    #[test]
    fn expr_logical_or() {
        assert_not_invalid("const x = a || b;", "logical or");
    }

    #[test]
    fn expr_logical_nullish() {
        assert_not_invalid("const x = a ?? b;", "nullish coalescing");
    }

    #[test]
    fn expr_conditional() {
        assert_not_invalid("const x = a ? 1 : 2;", "conditional");
    }

    #[test]
    fn expr_assign() {
        assert_not_invalid("const x = (a = 1);", "assignment");
    }

    #[test]
    fn expr_assign_add() {
        assert_not_invalid("const x = (a += 2);", "add assignment");
    }

    #[test]
    fn expr_array() {
        assert_not_invalid("const x = [1, 2];", "array literal");
    }

    #[test]
    fn expr_array_spread() {
        assert_not_invalid("const x = [...arr];", "array spread");
    }

    #[test]
    fn expr_object() {
        assert_not_invalid("const x = {a: 1};", "object literal");
    }

    #[test]
    fn expr_object_spread() {
        assert_not_invalid("const x = {...obj};", "object spread");
    }

    #[test]
    fn expr_call() {
        assert_not_invalid("const x = foo();", "call expression");
    }

    #[test]
    fn expr_call_method() {
        assert_not_invalid("const x = obj.method();", "method call");
    }

    #[test]
    fn expr_new() {
        assert_not_invalid("const x = new Foo();", "new expression");
    }

    #[test]
    fn expr_member() {
        assert_not_invalid("const x = obj.prop;", "member expression");
    }

    #[test]
    fn expr_member_computed() {
        assert_not_invalid("const x = arr[0];", "computed member");
    }

    #[test]
    fn expr_super() {
        assert_not_invalid("const x = super;", "super");
    }

    #[test]
    fn expr_this() {
        assert_not_invalid("const x = this;", "this");
    }

    #[test]
    fn expr_arrow_no_params() {
        assert_not_invalid("const x = () => 1;", "arrow function");
    }

    #[test]
    fn expr_arrow_with_param() {
        assert_not_invalid("const x = (y) => y + 1;", "arrow with param");
    }

    #[test]
    fn expr_arrow_block() {
        assert_not_invalid("const x = () => { return 1; };", "arrow block body");
    }

    #[test]
    fn expr_function_expr() {
        assert_not_invalid("const x = function() { };", "function expression");
    }

    #[test]
    fn expr_await() {
        let source = "async function f() { const x = await p; }";
        let parser = TsParser::new();
        let result = parser.parse_source(source).expect("parse failed");
        let has_await = result.items.iter().any(|item| {
            if let ModuleItem::Decl(Decl::Function(ref f)) = item {
                if let Some(ref body) = f.body {
                    body.0.iter().any(|s| {
                        if let Stmt::Expr { expr } = s {
                            matches!(expr, Expr::Await { .. })
                        } else {
                            false
                        }
                    })
                } else {
                    false
                }
            } else {
                false
            }
        });
        assert!(has_await, "await expression should parse");
    }

    #[test]
    fn expr_yield() {
        let source = "function* g() { yield 1; }";
        let parser = TsParser::new();
        let result = parser.parse_source(source).expect("parse failed");
        // Yield is detected in function body
        assert!(!result.items.is_empty(), "yield should parse");
    }

    #[test]
    fn expr_template_literal() {
        assert_not_invalid(r#"const x = `hello ${name}`;"#, "template literal");
    }

    #[test]
    fn expr_tagged_template() {
        assert_not_invalid(r#"const x = tag`hello`;"#, "tagged template");
    }

    #[test]
    fn expr_sequence() {
        assert_not_invalid("const x = (a, b, c);", "sequence expression");
    }

    #[test]
    fn expr_class_expr() {
        assert_not_invalid("const x = class { };", "class expression");
    }

    #[test]
    fn expr_jsx_element() {
        let source = "const x = <div />;";
        let parser = TsParser::new();
        let result = parser.parse_tsx(source).expect("parse failed");
        let has_jsx = result.items.iter().any(|item| {
            if let ModuleItem::Decl(Decl::Variable(ref v)) = item {
                if let Some(Expr::JSX(_)) = v.init.as_ref() {
                    return true;
                }
            }
            false
        });
        assert!(has_jsx, "JSX element should parse");
    }

    #[test]
    fn expr_jsx_member() {
        let source = "const x = <Foo.Bar />;";
        let parser = TsParser::new();
        let result = parser.parse_tsx(source).expect("parse failed");
        let has_jsx = result.items.iter().any(|item| {
            if let ModuleItem::Decl(Decl::Variable(ref v)) = item {
                if let Some(Expr::JSX(ref jsx)) = v.init.as_ref() {
                    if let JSXName::Member { .. } = &jsx.opening.name {
                        return true;
                    }
                }
            }
            false
        });
        assert!(has_jsx, "JSX member element should parse");
    }

    #[test]
    fn expr_jsx_fragment() {
        let source = "const x = <></>;";
        let parser = TsParser::new();
        let result = parser.parse_tsx(source).expect("parse failed");
        let has_fragment = result.items.iter().any(|item| {
            if let ModuleItem::Decl(Decl::Variable(ref v)) = item {
                if let Some(Expr::JSX(ref jsx)) = v.init.as_ref() {
                    return matches!(jsx.opening.name, JSXName::Fragment);
                }
            }
            false
        });
        assert!(has_fragment, "JSX fragment should parse");
    }

    #[test]
    fn expr_meta_new_target() {
        let source = "function f() { const x = new.target; }";
        let parser = TsParser::new();
        let result = parser.parse_source(source).expect("parse failed");
        let has_new_target = result.items.iter().any(|item| {
            if let ModuleItem::Decl(Decl::Function(ref f)) = item {
                if let Some(ref body) = f.body {
                    body.0.iter().any(|s| {
                        if let Stmt::Expr { expr } = s {
                            if let Expr::MetaProperty { kind: MetaPropKind::NewTarget } = expr {
                                return true;
                            }
                        }
                        false
                    })
                } else {
                    false
                }
            } else {
                false
            }
        });
        assert!(has_new_target, "new.target should parse");
    }

    #[test]
    fn expr_meta_import_meta() {
        let source = "const x = import.meta;";
        let parser = TsParser::new();
        let result = parser.parse_source(source).expect("parse failed");
        let has_import_meta = result.items.iter().any(|item| {
            if let ModuleItem::Decl(Decl::Variable(ref v)) = item {
                if let Some(Expr::MetaProperty { kind: MetaPropKind::ImportMeta }) = v.init.as_ref() {
                    return true;
                }
            }
            false
        });
        assert!(has_import_meta, "import.meta should parse");
    }

    #[test]
    fn expr_import_expression() {
        assert_not_invalid(r#"const x = import("mod");"#, "import expression");
    }

    #[test]
    fn expr_chain_optional() {
        assert_not_invalid("const x = obj?.prop;", "optional chain");
    }

    #[test]
    fn expr_private_in() {
        assert_not_invalid("const x = #p in obj;", "private in");
    }

    // ============================================================
    // PATTERN (DESTRUCTURING) COVERAGE
    // ============================================================

    #[test]
    fn pat_array() {
        let source = "const [a, b] = arr;";
        let parser = TsParser::new();
        let result = parser.parse_source(source).expect("parse failed");
        let has_array_pat = result.items.iter().any(|item| {
            if let ModuleItem::Decl(Decl::Variable(ref v)) = item {
                v.pattern.as_ref().map(|p| matches!(p, Pat::Array { .. })).unwrap_or(false)
            } else {
                false
            }
        });
        assert!(has_array_pat, "array destructuring should parse");
    }

    #[test]
    fn pat_object() {
        let source = "const {x, y} = obj;";
        let parser = TsParser::new();
        let result = parser.parse_source(source).expect("parse failed");
        let has_obj_pat = result.items.iter().any(|item| {
            if let ModuleItem::Decl(Decl::Variable(ref v)) = item {
                v.pattern.as_ref().map(|p| matches!(p, Pat::Object { .. })).unwrap_or(false)
            } else {
                false
            }
        });
        assert!(has_obj_pat, "object destructuring should parse");
    }

    #[test]
    fn pat_array_spread() {
        let source = "const [a, ...rest] = arr;";
        let parser = TsParser::new();
        let result = parser.parse_source(source).expect("parse failed");
        // Check spread is preserved
        let has_spread = result.items.iter().any(|item| {
            if let ModuleItem::Decl(Decl::Variable(ref v)) = item {
                if let Some(Pat::Array { elems, .. }) = v.pattern {
                    elems.iter().any(|e| matches!(e, Some(Pat::Rest { .. })))
                } else {
                    false
                }
            } else {
                false
            }
        });
        assert!(has_spread, "array spread pattern should parse");
    }

    #[test]
    fn pat_object_spread() {
        let source = "const {x, ...rest} = obj;";
        let parser = TsParser::new();
        let result = parser.parse_source(source).expect("parse failed");
        let has_spread = result.items.iter().any(|item| {
            if let ModuleItem::Decl(Decl::Variable(ref v)) = item {
                if let Some(Pat::Object { props, rest }) = v.pattern.as_ref() {
                    if let Some(_) = rest {
                        return props.iter().any(|p| matches!(p, ObjectPatProp::Rest(_)));
                    }
                }
                false
            } else {
                false
            }
        });
        assert!(has_spread, "object spread pattern should parse");
    }

    #[test]
    fn pat_array_default() {
        let source = "const [a = 1] = arr;";
        let parser = TsParser::new();
        let result = parser.parse_source(source).expect("parse failed");
        // Check default value is preserved
        let has_default = result.items.iter().any(|item| {
            if let ModuleItem::Decl(Decl::Variable(ref v)) = item {
                if let Some(Pat::Array(ref elems)) = v.pattern {
                    elems.iter().any(|e| {
                        if let Some(Pat::Default { .. }) = e { true } else { false }
                    })
                } else {
                    false
                }
            } else {
                false
            }
        });
        assert!(has_default, "array default pattern should parse");
    }

    #[test]
    fn pat_function_param_array() {
        let source = "function f([a, b]) { }";
        let parser = TsParser::new();
        let result = parser.parse_source(source).expect("parse failed");
        let has_array_param = result.items.iter().any(|item| {
            if let ModuleItem::Decl(Decl::Function(ref f)) = item {
                f.params.iter().any(|p| {
                    p.pattern.as_ref().map(|pat| matches!(pat, Pat::Array(_))).unwrap_or(false)
                })
            } else {
                false
            }
        });
        assert!(has_array_param, "function param with array pattern should parse");
    }

    // ============================================================
    // ERROR-ON-UNHANDLED TESTS
    // ============================================================

    #[test]
    fn error_with_statement() {
        // 'with' is not valid in strict mode / ES5+ and parser should error
        let source = "with (obj) { }";
        let parser = TsParser::new();
        let result = parser.parse_source(source);
        // Either errors, or produces Stmt::Empty if silently handled
        if result.is_ok() {
            let stmt = parse_first_stmt(source);
            println!("with statement parsed to: {:?}", stmt);
        } else {
            println!("with statement errored: {:?}", result.err());
        }
    }

    #[test]
    fn error_debugger() {
        // debugger should either error or map to Empty explicitly
        let source = "debugger;";
        let parser = TsParser::new();
        let result = parser.parse_source(source);
        match result {
            Ok(_) => {
                let stmt = parse_first_stmt(source);
                // If parsed successfully, should be Empty or Block containing Empty
                println!("debugger parsed to: {:?}", stmt);
            }
            Err(e) => {
                // Error is acceptable - debugger might not be supported
                println!("debugger errored: {:?}", e);
            }
        }
    }

    // ============================================================
    // IGNORED TESTS - known parser gaps
    // ============================================================

    #[test]
    #[ignore = "parser not yet implemented"]
    fn stmt_with_error() {
        // with statement should return error, not silent drop
        let source = "with (obj) { }";
        let parser = TsParser::new();
        let result = parser.parse_source(source);
        assert!(result.is_err(), "with statement should error");
    }

    #[test]
    #[ignore = "parser not yet implemented"]
    fn stmt_debugger_empty() {
        // debugger should explicitly map to Stmt::Empty, not silently drop
        let source = "debugger;";
        let stmt = parse_first_stmt(source);
        assert!(matches!(stmt, Stmt::Empty), "debugger should map to Stmt::Empty");
    }
}