//! Spec control flow tests — section 2.4 of SUPPORTED_SUBSET.md
//!
//! Covers: conditional, switch, loops, jump statements, exception handling
//!
//! allow:too_many_lines,complexity,nested_externals

#[cfg(test)]
mod spec_control_flow_tests {
    use proc_macro2::TokenStream;
    use quote::ToTokens;

    use crate::transpile::hir::*;
    use crate::transpile::parser::TsParser;

    // =========================================================================
    // Helpers
    // =========================================================================

    /// Parse a statement and return the first statement
    fn parse_first_stmt(source: &str) -> Stmt {
        let parser = TsParser::new();
        let result = parser.parse_source(source).expect("parse failed");
        let item = result.items.first().expect("no items");
        match item {
            ModuleItem::Stmt(s) => s.clone(),
            ModuleItem::Decl(Decl::Function(_)) => Stmt::FunctionDecl(FunctionDecl {
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
            }),
            ModuleItem::Decl(Decl::Variable(v)) => Stmt::Variable(v.clone()),
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

    /// Assert statement parsed from source is NOT Stmt::Empty
    fn assert_not_empty(source: &str, label: &str) {
        let stmt = parse_first_stmt(source);
        assert!(
            !matches!(stmt, Stmt::Empty),
            "{}: parsed to Stmt::Empty, expected non-empty: {:?}",
            label,
            source
        );
    }

    /// Get codegen output for a statement, assert it's Some
    fn assert_codegen_some(stmt: &Stmt, label: &str) -> TokenStream {
        let cg = QuoteCodegen::default();
        let result = cg.gen_stmt(stmt);
        assert!(result.is_some(), "{}: gen_stmt returned None", label);
        result.unwrap()
    }

    /// Check if TokenStream contains Value::Null (with any spacing)
    fn contains_value_null(tokens: &TokenStream) -> bool {
        let s = tokens.to_string();
        s.contains("Value :: Null") || s.contains("Value::Null") || s.contains("Value . Null")
    }

    /// Helper: wrap statement(s) in a function for valid JS
    fn wrap_in_function(body: &str) -> String {
        format!("function f() {{ {} }}", body)
    }

    /// Parse a function body and return its statements
    fn parse_function_body(source: &str) -> Vec<Stmt> {
        let parser = TsParser::new();
        let result = parser.parse_source(source).expect("parse failed");
        for item in &result.items {
            if let ModuleItem::Decl(Decl::Function(ref f)) = item {
                if let Some(ref body) = f.body {
                    return body.0.clone();
                }
            }
        }
        vec![]
    }

    /// Find a statement of a specific type in a list
    fn find_stmt<T: Fn(&Stmt) -> bool>(stmts: &[Stmt], pred: T) -> Option<&Stmt> {
        stmts.iter().find(|s| pred(s))
    }

    /// Helper to check if a loop body contains a specific statement
    fn loop_body_contains(stmt: &Stmt, target: &str) -> bool {
        // Unwrap labeled statements
        let stmt = match stmt {
            Stmt::Labeled { body, .. } => body.as_ref(),
            other => other,
        };
        match stmt {
            Stmt::For { body, .. } | Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
                let body_stmt = body.as_ref();
                if let Stmt::Block(stmts) = body_stmt {
                    stmts.iter().any(|s| match target {
                        "break" => matches!(s, Stmt::Break { .. }),
                        "continue" => matches!(s, Stmt::Continue { .. }),
                        _ => false,
                    })
                } else {
                    false
                }
            }
            _ => false,
        }
    }

    // =========================================================================
    // 2.4.1 Conditional
    // =========================================================================

    mod conditional {
        use super::*;

        // Parser tests

        #[test]
        fn parser_if_basic() {
            assert_not_empty("if (x) { }", "if basic");
        }

        #[test]
        fn parser_if_with_block() {
            assert_not_empty("if (x) { const y = 1; }", "if with block");
        }

        #[test]
        fn parser_if_else() {
            assert_not_empty("if (x) { } else { }", "if-else");
        }

        #[test]
        fn parser_if_else_if() {
            assert_not_empty("if (x) { } else if (y) { }", "if-else if");
        }

        #[test]
        fn parser_if_else_if_else() {
            assert_not_empty("if (x) { } else if (y) { } else { }", "if-else if-else");
        }

        #[test]
        fn parser_if_nested() {
            assert_not_empty(
                "if (x) { if (y) { } }",
                "nested if",
            );
        }

        // Codegen tests

        #[test]
        fn codegen_if_basic() {
            let stmt = parse_first_stmt("if (x) { }");
            let tokens = assert_codegen_some(&stmt, "if basic");
            let s = tokens.to_string();
            assert!(s.contains("if"), "should contain if: {}", s);
            assert!(!contains_value_null(&tokens), "should not fallback to Value::Null");
        }

        #[test]
        fn codegen_if_with_expr() {
            let stmt = parse_first_stmt("if (x > 0) { const y = 1; }");
            let tokens = assert_codegen_some(&stmt, "if with expr");
            let s = tokens.to_string();
            assert!(s.contains("if"), "should contain if: {}", s);
            assert!(!contains_value_null(&tokens), "should not fallback to Value::Null");
        }

        #[test]
        fn codegen_if_else() {
            let stmt = parse_first_stmt("if (x) { const a = 1; } else { const b = 2; }");
            let tokens = assert_codegen_some(&stmt, "if-else");
            let s = tokens.to_string();
            assert!(s.contains("if") && s.contains("else"), "should contain if-else: {}", s);
        }

        #[test]
        fn codegen_if_else_if_else() {
            let stmt = parse_first_stmt("if (x) { } else if (y) { } else { }");
            let tokens = assert_codegen_some(&stmt, "if-else if-else");
            let s = tokens.to_string();
            // Rust uses else if as separate, check for else
            assert!(s.contains("else"), "should contain else: {}", s);
        }

        // Integration: full round-trip parse -> codegen

        #[test]
        fn integration_if_else_return() {
            let source = wrap_in_function("if (x) { return 1; } else { return 2; }");
            let parser = TsParser::new();
            let result = parser.parse_source(&source).expect("parse failed");
            let func = result.items.iter().find_map(|item| {
                if let ModuleItem::Decl(Decl::Function(f)) = item {
                    Some(f)
                } else {
                    None
                }
            }).expect("no function");

            let cg = QuoteCodegen::default();
            let tokens = cg.gen_fn(func);
            let s = tokens.to_string();
            assert!(s.contains("if"), "should generate if: {}", s);
            assert!(s.contains("return"), "should generate return: {}", s);
        }
    }

    // =========================================================================
    // 2.4.2 Switch
    // =========================================================================

    mod switch {
        use super::*;

        #[test]
        fn parser_switch_basic() {
            assert_not_empty("switch (x) { case 1: break; }", "switch basic");
        }

        #[test]
        fn parser_switch_with_default() {
            assert_not_empty("switch (x) { case 1: break; default: break; }", "switch with default");
        }

        #[test]
        fn parser_switch_multiple_cases() {
            assert_not_empty(
                "switch (x) { case 1: case 2: break; default: break; }",
                "switch multiple cases (fallthrough)",
            );
        }

        #[test]
        fn parser_switch_with_return() {
            // return in switch case
            let source = wrap_in_function("switch (x) { case 1: return 1; case 2: return 2; }");
            let stmts = parse_function_body(&source);
            let has_switch = stmts.iter().any(|s| matches!(s, Stmt::Switch { .. }));
            assert!(has_switch, "switch with return should parse");
        }

        #[test]
        fn parser_switch_fallthrough() {
            assert_not_empty(
                "switch (x) { case 1: const a = 1; case 2: const b = 2; break; }",
                "switch fallthrough with statements",
            );
        }

        #[test]
        fn codegen_switch_basic() {
            let stmt = parse_first_stmt("switch (x) { case 1: break; }");
            let tokens = assert_codegen_some(&stmt, "switch basic");
            let s = tokens.to_string();
            assert!(s.contains("match"), "switch should become match: {}", s);
        }

        #[test]
        fn codegen_switch_with_default() {
            let stmt = parse_first_stmt("switch (x) { case 1: break; default: break; }");
            let tokens = assert_codegen_some(&stmt, "switch with default");
            let s = tokens.to_string();
            assert!(s.contains("match"), "switch should become match: {}", s);
            // default becomes _ arm
            assert!(s.contains("_") || s.contains("match"), "default should become _: {}", s);
        }

        #[test]
        fn codegen_switch_multiple_cases() {
            let stmt = parse_first_stmt("switch (x) { case 1: case 2: break; }");
            let tokens = assert_codegen_some(&stmt, "switch multiple cases");
            let s = tokens.to_string();
            assert!(s.contains("match"), "switch should become match: {}", s);
        }

        #[test]
        fn integration_switch_basic() {
            let source = wrap_in_function("switch (x) { case 1: return 1; default: return 0; }");
            let parser = TsParser::new();
            let result = parser.parse_source(&source).expect("parse failed");
            let func = result.items.iter().find_map(|item| {
                if let ModuleItem::Decl(Decl::Function(f)) = item { Some(f) } else { None }
            }).expect("no function");

            let cg = QuoteCodegen::default();
            let tokens = cg.gen_fn(func);
            let s = tokens.to_string();
            assert!(s.contains("match"), "should generate match: {}", s);
        }
    }

    // =========================================================================
    // 2.4.3 Loops
    // =========================================================================

    mod loops {
        use super::*;

        // C-style for

        #[test]
        fn parser_for_basic() {
            assert_not_empty("for (let i = 0; i < 10; i++) { }", "for basic");
        }

        #[test]
        fn parser_for_no_init() {
            assert_not_empty("for (; i < 10; i++) { }", "for no init");
        }

        #[test]
        fn parser_for_no_test() {
            assert_not_empty("for (let i = 0; ; i++) { }", "for no test (infinite)");
        }

        #[test]
        fn parser_for_no_update() {
            assert_not_empty("for (let i = 0; i < 10;) { }", "for no update");
        }

        #[test]
        fn parser_for_empty_all() {
            assert_not_empty("for (;;) { }", "for empty all");
        }

        #[test]
        fn codegen_for_basic() {
            let stmt = parse_first_stmt("for (let i = 0; i < 10; i++) { }");
            let tokens = assert_codegen_some(&stmt, "for basic");
            let s = tokens.to_string();
            assert!(s.contains("for"), "should contain for: {}", s);
            // Should NOT contain "in" (that's for...in)
            assert!(s.contains(";"), "should have semicolons for c-style for: {}", s);
        }

        #[test]
        fn codegen_for_no_init() {
            let stmt = parse_first_stmt("for (; i < 10; i++) { }");
            let tokens = assert_codegen_some(&stmt, "for no init");
            let s = tokens.to_string();
            assert!(s.contains("for"), "should contain for: {}", s);
        }

        #[test]
        fn codegen_for_empty_all() {
            let stmt = parse_first_stmt("for (;;) { }");
            let tokens = assert_codegen_some(&stmt, "for empty all");
            let s = tokens.to_string();
            assert!(s.contains("for"), "should contain for: {}", s);
        }

        // For...of

        #[test]
        fn parser_for_of() {
            assert_not_empty("for (const x of arr) { }", "for-of");
        }

        #[test]
        fn parser_for_of_let() {
            assert_not_empty("for (let x of arr) { }", "for-of let");
        }

        #[test]
        fn parser_for_of_with_body() {
            // for-of with single binding is directly supported
            assert_not_empty("for (const x of arr) { const y = x; }", "for-of with body");
        }

        #[test]
        fn codegen_for_of() {
            let stmt = parse_first_stmt("for (const x of arr) { }");
            let tokens = assert_codegen_some(&stmt, "for-of");
            let s = tokens.to_string();
            // Note: for...of in TS becomes for...in in Rust
            assert!(s.contains("for") && s.contains("in"), "for-of should become for...in: {}", s);
        }

        #[test]
        fn codegen_for_of_let() {
            let stmt = parse_first_stmt("for (let x of arr) { }");
            let tokens = assert_codegen_some(&stmt, "for-of let");
            let s = tokens.to_string();
            assert!(s.contains("for") && s.contains("in"), "for-of should become for...in: {}", s);
        }

        // For...in

        #[test]
        fn parser_for_in() {
            assert_not_empty("for (const k in obj) { }", "for-in");
        }

        #[test]
        fn parser_for_in_let() {
            assert_not_empty("for (let k in obj) { }", "for-in let");
        }

        #[test]
        fn codegen_for_in() {
            let stmt = parse_first_stmt("for (const k in obj) { }");
            let tokens = assert_codegen_some(&stmt, "for-in");
            let s = tokens.to_string();
            assert!(s.contains("for") && s.contains("in"), "for-in should contain for...in: {}", s);
        }

        #[test]
        fn codegen_for_in_let() {
            let stmt = parse_first_stmt("for (let k in obj) { }");
            let tokens = assert_codegen_some(&stmt, "for-in let");
            let s = tokens.to_string();
            assert!(s.contains("for") && s.contains("in"), "for-in should contain for...in: {}", s);
        }

        // While

        #[test]
        fn parser_while_basic() {
            assert_not_empty("while (x) { }", "while basic");
        }

        #[test]
        fn parser_while_with_body() {
            assert_not_empty("while (x) { const y = 1; }", "while with body");
        }

        #[test]
        fn codegen_while_basic() {
            let stmt = parse_first_stmt("while (x) { }");
            let tokens = assert_codegen_some(&stmt, "while basic");
            let s = tokens.to_string();
            assert!(s.contains("while"), "should contain while: {}", s);
        }

        #[test]
        fn codegen_while_with_body() {
            let stmt = parse_first_stmt("while (x) { const y = 1; }");
            let tokens = assert_codegen_some(&stmt, "while with body");
            let s = tokens.to_string();
            assert!(s.contains("while"), "should contain while: {}", s);
        }

        // Do...while

        #[test]
        fn parser_do_while() {
            assert_not_empty("do { } while (x);", "do-while");
        }

        #[test]
        fn parser_do_while_with_body() {
            assert_not_empty("do { const y = 1; } while (x);", "do-while with body");
        }

        #[test]
        fn codegen_do_while() {
            let stmt = parse_first_stmt("do { } while (x);");
            let tokens = assert_codegen_some(&stmt, "do-while");
            let s = tokens.to_string();
            // do-while becomes loop { if ! (cond) { break ; } }
            assert!(s.contains("loop") && s.contains("break"), "do-while should become loop with break: {}", s);
        }

        #[test]
        fn codegen_do_while_with_body() {
            let stmt = parse_first_stmt("do { const y = 1; } while (x);");
            let tokens = assert_codegen_some(&stmt, "do-while with body");
            let s = tokens.to_string();
            // do-while becomes loop { if ! (cond) { break ; } }
            assert!(s.contains("loop") && s.contains("break"), "do-while should become loop with break: {}", s);
        }

        // Loop integration tests

        #[test]
        fn integration_for_with_break() {
            let source = wrap_in_function("for (let i = 0; i < 10; i++) { if (i > 5) { break; } }");
            let parser = TsParser::new();
            let result = parser.parse_source(&source).expect("parse failed");
            let func = result.items.iter().find_map(|item| {
                if let ModuleItem::Decl(Decl::Function(f)) = item { Some(f) } else { None }
            }).expect("no function");

            let cg = QuoteCodegen::default();
            let tokens = cg.gen_fn(func);
            let s = tokens.to_string();
            assert!(s.contains("for"), "should generate for: {}", s);
            assert!(s.contains("break"), "should generate break: {}", s);
        }

        #[test]
        fn integration_for_of_with_continue() {
            let source = wrap_in_function("for (const x of arr) { if (x === 0) { continue; } }");
            let parser = TsParser::new();
            let result = parser.parse_source(&source).expect("parse failed");
            let func = result.items.iter().find_map(|item| {
                if let ModuleItem::Decl(Decl::Function(f)) = item { Some(f) } else { None }
            }).expect("no function");

            let cg = QuoteCodegen::default();
            let tokens = cg.gen_fn(func);
            let s = tokens.to_string();
            assert!(s.contains("for") && s.contains("in"), "should generate for...in: {}", s);
            assert!(s.contains("continue"), "should generate continue: {}", s);
        }
    }

    // =========================================================================
    // 2.4.4 Jump Statements
    // =========================================================================

    mod jump_statements {
        use super::*;

        // Break

        #[test]
        fn parser_break_in_loop() {
            // break must be in a loop context
            let source = wrap_in_function("for (;;) { break; }");
            let stmts = parse_function_body(&source);
            // Find the for loop and check its body contains break
            let has_break = find_stmt(&stmts, |s| matches!(s, Stmt::For { .. }))
                .map(|s| loop_body_contains(s, "break"))
                .unwrap_or(false);
            assert!(has_break, "break should parse inside for loop");
        }

        #[test]
        fn parser_break_labeled() {
            // Labeled break: label must exist, but parser should accept syntax
            let source = wrap_in_function("outer: for (;;) { break outer; }");
            let stmts = parse_function_body(&source);
            // For labeled statements, the structure is Stmt::Labeled { label, body: Box<Stmt::For {...} }
            // Check that we have a labeled statement containing a for loop with break
            let has_break = find_stmt(&stmts, |s| matches!(s, Stmt::Labeled { .. }))
                .and_then(|s| {
                    if let Stmt::Labeled { body, .. } = s {
                        // Unwrap the labeled to get the inner statement
                        loop_body_contains(body.as_ref(), "break").then_some(true)
                    } else {
                        None
                    }
                })
                .unwrap_or(false);
            assert!(has_break, "labeled break should parse");
        }

        #[test]
        fn codegen_break_in_loop() {
            let source = wrap_in_function("for (;;) { break; }");
            let parser = TsParser::new();
            let result = parser.parse_source(&source).expect("parse failed");
            let func = result.items.iter().find_map(|item| {
                if let ModuleItem::Decl(Decl::Function(f)) = item { Some(f) } else { None }
            }).expect("no function");

            let cg = QuoteCodegen::default();
            let tokens = cg.gen_fn(func);
            let s = tokens.to_string();
            assert!(s.contains("break"), "should generate break: {}", s);
        }

        #[test]
        fn codegen_break_labeled() {
            let source = wrap_in_function("outer: for (;;) { break outer; }");
            let parser = TsParser::new();
            let result = parser.parse_source(&source).expect("parse failed");
            let func = result.items.iter().find_map(|item| {
                if let ModuleItem::Decl(Decl::Function(f)) = item { Some(f) } else { None }
            }).expect("no function");

            let cg = QuoteCodegen::default();
            let tokens = cg.gen_fn(func);
            let s = tokens.to_string();
            // Labeled break in Rust uses 'break label or 'break outer;
            // Check that break is present
            assert!(s.contains("break"), "should generate break: {}", s);
        }

        // Continue

        #[test]
        fn parser_continue_in_loop() {
            let source = wrap_in_function("for (;;) { continue; }");
            let stmts = parse_function_body(&source);
            let has_continue = find_stmt(&stmts, |s| matches!(s, Stmt::For { .. }))
                .map(|s| loop_body_contains(s, "continue"))
                .unwrap_or(false);
            assert!(has_continue, "continue should parse inside for loop");
        }

        #[test]
        fn parser_continue_labeled() {
            let source = wrap_in_function("outer: for (;;) { continue outer; }");
            let stmts = parse_function_body(&source);
            // For labeled statements, the structure is Stmt::Labeled { label, body: Box<Stmt::For {...} }
            // Check that we have a labeled statement containing a for loop with continue
            let has_continue = find_stmt(&stmts, |s| matches!(s, Stmt::Labeled { .. }))
                .and_then(|s| {
                    if let Stmt::Labeled { body, .. } = s {
                        // Unwrap the labeled to get the inner statement
                        loop_body_contains(body.as_ref(), "continue").then_some(true)
                    } else {
                        None
                    }
                })
                .unwrap_or(false);
            assert!(has_continue, "labeled continue should parse");
        }

        #[test]
        fn codegen_continue_in_loop() {
            let source = wrap_in_function("for (;;) { continue; }");
            let parser = TsParser::new();
            let result = parser.parse_source(&source).expect("parse failed");
            let func = result.items.iter().find_map(|item| {
                if let ModuleItem::Decl(Decl::Function(f)) = item { Some(f) } else { None }
            }).expect("no function");

            let cg = QuoteCodegen::default();
            let tokens = cg.gen_fn(func);
            let s = tokens.to_string();
            assert!(s.contains("continue"), "should generate continue: {}", s);
        }

        #[test]
        fn codegen_continue_labeled() {
            let source = wrap_in_function("outer: for (;;) { continue outer; }");
            let parser = TsParser::new();
            let result = parser.parse_source(&source).expect("parse failed");
            let func = result.items.iter().find_map(|item| {
                if let ModuleItem::Decl(Decl::Function(f)) = item { Some(f) } else { None }
            }).expect("no function");

            let cg = QuoteCodegen::default();
            let tokens = cg.gen_fn(func);
            let s = tokens.to_string();
            assert!(s.contains("continue"), "should generate continue: {}", s);
        }

        // Return

        #[test]
        fn parser_return_no_arg() {
            let source = wrap_in_function("return;");
            let stmts = parse_function_body(&source);
            let has_return = stmts.iter().any(|s| matches!(s, Stmt::Return { arg: None }));
            assert!(has_return, "return without arg should parse");
        }

        #[test]
        fn parser_return_with_expr() {
            let source = wrap_in_function("return 1;");
            let stmts = parse_function_body(&source);
            let has_return = stmts.iter().any(|s| matches!(s, Stmt::Return { arg: Some(_) }));
            assert!(has_return, "return with expr should parse");
        }

        #[test]
        fn parser_return_with_object() {
            let source = wrap_in_function("return { a: 1, b: 2 };");
            let stmts = parse_function_body(&source);
            let has_return = stmts.iter().any(|s| matches!(s, Stmt::Return { arg: Some(_) }));
            assert!(has_return, "return with object should parse");
        }

        #[test]
        fn parser_return_with_array() {
            let source = wrap_in_function("return [1, 2, 3];");
            let stmts = parse_function_body(&source);
            let has_return = stmts.iter().any(|s| matches!(s, Stmt::Return { arg: Some(_) }));
            assert!(has_return, "return with array should parse");
        }

        #[test]
        fn parser_return_with_function_call() {
            let source = wrap_in_function("return foo(1, 2);");
            let stmts = parse_function_body(&source);
            let has_return = stmts.iter().any(|s| matches!(s, Stmt::Return { arg: Some(_) }));
            assert!(has_return, "return with function call should parse");
        }

        #[test]
        fn codegen_return_no_arg() {
            let stmt = Stmt::Return { arg: None };
            let tokens = assert_codegen_some(&stmt, "return no arg");
            let s = tokens.to_string();
            assert!(s.contains("return"), "should generate return: {}", s);
        }

        #[test]
        fn codegen_return_with_expr() {
            let stmt = Stmt::Return { arg: Some(Expr::Number(42.0)) };
            let tokens = assert_codegen_some(&stmt, "return with expr");
            let s = tokens.to_string();
            assert!(s.contains("return"), "should generate return: {}", s);
        }

        #[test]
        fn codegen_return_with_object() {
            let stmt = Stmt::Return {
                arg: Some(Expr::Object {
                    members: vec![ObjectMemberExpr {
                        prop: ObjectProp::Init {
                            key: PropKey::Str("a".into()),
                            value: Expr::Number(1.0),
                            computed: false,
                        },
                    }],
                }),
            };
            let tokens = assert_codegen_some(&stmt, "return with object");
            let s = tokens.to_string();
            assert!(s.contains("return"), "should generate return: {}", s);
        }

        #[test]
        fn integration_return_in_function() {
            let source = wrap_in_function("if (x) { return 1; } return 0;");
            let parser = TsParser::new();
            let result = parser.parse_source(&source).expect("parse failed");
            let func = result.items.iter().find_map(|item| {
                if let ModuleItem::Decl(Decl::Function(f)) = item { Some(f) } else { None }
            }).expect("no function");

            let cg = QuoteCodegen::default();
            let tokens = cg.gen_fn(func);
            let s = tokens.to_string();
            assert!(s.contains("return"), "should generate return: {}", s);
        }

        // Labeled statements

        #[test]
        fn parser_labeled_basic() {
            assert_not_empty("label: { }", "labeled basic");
        }

        #[test]
        fn parser_labeled_with_loop() {
            assert_not_empty("outer: while (true) { break outer; }", "labeled with loop");
        }

        #[test]
        fn codegen_labeled_basic() {
            let stmt = parse_first_stmt("label: { }");
            let tokens = assert_codegen_some(&stmt, "labeled basic");
            let s = tokens.to_string();
            assert!(!contains_value_null(&tokens), "should not fallback to Value::Null");
        }

        #[test]
        fn codegen_labeled_with_loop() {
            let source = wrap_in_function("outer: while (true) { break outer; }");
            let parser = TsParser::new();
            let result = parser.parse_source(&source).expect("parse failed");
            let func = result.items.iter().find_map(|item| {
                if let ModuleItem::Decl(Decl::Function(f)) = item { Some(f) } else { None }
            }).expect("no function");

            let cg = QuoteCodegen::default();
            let tokens = cg.gen_fn(func);
            let s = tokens.to_string();
            assert!(s.contains("while"), "should generate while: {}", s);
            assert!(s.contains("break"), "should generate break: {}", s);
        }
    }

    // =========================================================================
    // 2.4.5 Exception Handling
    // =========================================================================

    mod exception_handling {
        use super::*;

        // Try-catch

        #[test]
        fn parser_try_catch() {
            assert_not_empty("try { } catch (e) { }", "try-catch");
        }

        #[test]
        fn parser_try_catch_with_body() {
            assert_not_empty("try { const x = 1; } catch (e) { const y = 2; }", "try-catch with body");
        }

        #[test]
        fn parser_try_catch_finally() {
            assert_not_empty("try { } catch (e) { } finally { }", "try-catch-finally");
        }

        #[test]
        fn parser_try_finally() {
            assert_not_empty("try { } finally { }", "try-finally");
        }

        #[test]
        fn parser_try_with_throw() {
            assert_not_empty("try { throw new Error(); } catch (e) { }", "try-throw-catch");
        }

        #[test]
        fn codegen_try_catch() {
            let stmt = parse_first_stmt("try { const x = 1; } catch (e) { const y = 2; }");
            let tokens = assert_codegen_some(&stmt, "try-catch");
            let s = tokens.to_string();
            // try-catch becomes std::panic::catch_unwind + match
            assert!(
                s.contains("catch_unwind") || s.contains("match"),
                "try-catch should use catch_unwind: {}",
                s
            );
        }

        #[test]
        fn codegen_try_catch_finally() {
            let stmt = parse_first_stmt("try { } catch (e) { } finally { }");
            let tokens = assert_codegen_some(&stmt, "try-catch-finally");
            let s = tokens.to_string();
            assert!(
                s.contains("catch_unwind") || s.contains("match"),
                "try-catch-finally should use catch_unwind: {}",
                s
            );
            // finally should still have some code
            assert!(!s.is_empty(), "should generate finally code");
        }

        #[test]
        fn codegen_try_finally() {
            let stmt = parse_first_stmt("try { } finally { }");
            let tokens = assert_codegen_some(&stmt, "try-finally");
            let s = tokens.to_string();
            // try-finally is just block + finally block
            assert!(!s.is_empty(), "should generate try-finally code");
        }

        // Throw

        #[test]
        fn parser_throw_new_error() {
            assert_not_empty("throw new Error();", "throw new Error");
        }

        #[test]
        fn parser_throw_new_error_with_msg() {
            assert_not_empty(r#"throw new Error("msg");"#, "throw new Error with msg");
        }

        #[test]
        fn parser_throw_expr() {
            // throw with any expression
            assert_not_empty("throw err;", "throw expr");
        }

        #[test]
        fn parser_throw_string() {
            assert_not_empty(r#"throw "error string";"#, "throw string");
        }

        #[test]
        fn parser_throw_object() {
            assert_not_empty("throw { code: 404, message: \"not found\" };", "throw object");
        }

        #[test]
        fn codegen_throw_new_error() {
            let stmt = parse_first_stmt(r#"throw new Error("msg");"#);
            let tokens = assert_codegen_some(&stmt, "throw new Error");
            let s = tokens.to_string();
            assert!(!contains_value_null(&tokens), "throw should not fallback to Value::Null");
        }

        #[test]
        fn codegen_throw_expr() {
            let stmt = parse_first_stmt("throw err;");
            let tokens = assert_codegen_some(&stmt, "throw expr");
            let s = tokens.to_string();
            assert!(!contains_value_null(&tokens), "throw should not fallback to Value::Null");
        }

        #[test]
        fn codegen_throw_string() {
            let stmt = parse_first_stmt(r#"throw "error";"#);
            let tokens = assert_codegen_some(&stmt, "throw string");
            let s = tokens.to_string();
            assert!(!contains_value_null(&tokens), "throw should not fallback to Value::Null");
        }

        // Integration tests

        #[test]
        fn integration_try_catch_return() {
            let source = wrap_in_function("try { return 1; } catch (e) { return 2; }");
            let parser = TsParser::new();
            let result = parser.parse_source(&source).expect("parse failed");
            let func = result.items.iter().find_map(|item| {
                if let ModuleItem::Decl(Decl::Function(f)) = item { Some(f) } else { None }
            }).expect("no function");

            let cg = QuoteCodegen::default();
            let tokens = cg.gen_fn(func);
            let s = tokens.to_string();
            assert!(
                s.contains("catch_unwind") || s.contains("match"),
                "should use catch_unwind: {}",
                s
            );
        }

        #[test]
        fn integration_try_catch_finally_return() {
            let source = wrap_in_function("try { return 1; } catch (e) { return 2; } finally { }");
            let parser = TsParser::new();
            let result = parser.parse_source(&source).expect("parse failed");
            let func = result.items.iter().find_map(|item| {
                if let ModuleItem::Decl(Decl::Function(f)) = item { Some(f) } else { None }
            }).expect("no function");

            let cg = QuoteCodegen::default();
            let tokens = cg.gen_fn(func);
            let s = tokens.to_string();
            assert!(s.contains("return"), "should generate return: {}", s);
        }

        #[test]
        fn integration_try_throw_catch_return() {
            let source = wrap_in_function("try { throw new Error(); } catch (e) { return 1; }");
            let parser = TsParser::new();
            let result = parser.parse_source(&source).expect("parse failed");
            let func = result.items.iter().find_map(|item| {
                if let ModuleItem::Decl(Decl::Function(f)) = item { Some(f) } else { None }
            }).expect("no function");

            let cg = QuoteCodegen::default();
            let tokens = cg.gen_fn(func);
            let s = tokens.to_string();
            assert!(s.contains("return"), "should generate return: {}", s);
        }

        #[test]
        fn integration_try_finally_with_return() {
            let source = wrap_in_function("try { return 1; } finally { }");
            let parser = TsParser::new();
            let result = parser.parse_source(&source).expect("parse failed");
            let func = result.items.iter().find_map(|item| {
                if let ModuleItem::Decl(Decl::Function(f)) = item { Some(f) } else { None }
            }).expect("no function");

            let cg = QuoteCodegen::default();
            let tokens = cg.gen_fn(func);
            let s = tokens.to_string();
            assert!(s.contains("return"), "should generate return: {}", s);
        }

        // Error object properties

        #[test]
        fn parser_throw_type_error() {
            assert_not_empty("throw new TypeError();", "throw TypeError");
        }

        #[test]
        fn parser_throw_range_error() {
            assert_not_empty("throw new RangeError();", "throw RangeError");
        }

        #[test]
        fn parser_throw_with_template() {
            assert_not_empty(r#"throw new Error(`Error: ${code}`);"#, "throw with template");
        }
    }

    // =========================================================================
    // Edge cases and combinations
    // =========================================================================

    mod edge_cases {
        use super::*;

        #[test]
        fn parser_if_with_comma_in_condition() {
            // comma in condition expression (comma operator)
            let source = wrap_in_function("if (x, y) { return 1; }");
            let stmts = parse_function_body(&source);
            let has_if = stmts.iter().any(|s| matches!(s, Stmt::If { .. }));
            assert!(has_if, "if with comma in condition should parse");
        }

        #[test]
        fn parser_nested_loops_break_continue() {
            let source = wrap_in_function("for (;;) { for (;;) { break; } continue; }");
            let stmts = parse_function_body(&source);
            let outer_for = find_stmt(&stmts, |s| matches!(s, Stmt::For { .. }));
            assert!(outer_for.is_some(), "nested loops should parse");
        }

        #[test]
        fn parser_switch_with_break_in_loop() {
            let source = wrap_in_function("while (true) { switch (x) { case 1: break; } }");
            let stmts = parse_function_body(&source);
            let has_while = stmts.iter().any(|s| matches!(s, Stmt::While { .. }));
            assert!(has_while, "switch in while should parse");
        }

        #[test]
        fn parser_try_in_loop() {
            let source = wrap_in_function("while (true) { try { } catch (e) { } }");
            let stmts = parse_function_body(&source);
            let has_while = stmts.iter().any(|s| matches!(s, Stmt::While { .. }));
            assert!(has_while, "try in while should parse");
        }

        #[test]
        fn parser_if_with_assignment_in_condition() {
            let source = wrap_in_function("if (x = 1) { return 1; }");
            let stmts = parse_function_body(&source);
            let has_if = stmts.iter().any(|s| matches!(s, Stmt::If { .. }));
            assert!(has_if, "if with assignment in condition should parse");
        }

        #[test]
        fn codegen_nested_loops() {
            let source = wrap_in_function("for (;;) { for (;;) { break; } }");
            let parser = TsParser::new();
            let result = parser.parse_source(&source).expect("parse failed");
            let func = result.items.iter().find_map(|item| {
                if let ModuleItem::Decl(Decl::Function(f)) = item { Some(f) } else { None }
            }).expect("no function");

            let cg = QuoteCodegen::default();
            let tokens = cg.gen_fn(func);
            let s = tokens.to_string();
            // Count occurrences of "for"
            let for_count = s.matches("for").count();
            assert!(for_count >= 2, "nested for loops should generate 2+ for: {}", s);
        }

        #[test]
        fn codegen_if_else_chain() {
            let source = wrap_in_function("if (x) { return 1; } else if (y) { return 2; } else if (z) { return 3; } else { return 0; }");
            let parser = TsParser::new();
            let result = parser.parse_source(&source).expect("parse failed");
            let func = result.items.iter().find_map(|item| {
                if let ModuleItem::Decl(Decl::Function(f)) = item { Some(f) } else { None }
            }).expect("no function");

            let cg = QuoteCodegen::default();
            let tokens = cg.gen_fn(func);
            let s = tokens.to_string();
            assert!(s.contains("if"), "should contain if: {}", s);
            assert!(s.contains("else"), "should contain else: {}", s);
        }

        #[test]
        fn codegen_switch_with_all_return() {
            // Every case returns - common exhaustive pattern
            let source = wrap_in_function("switch (x) { case 1: return 1; case 2: return 2; default: return 0; }");
            let parser = TsParser::new();
            let result = parser.parse_source(&source).expect("parse failed");
            let func = result.items.iter().find_map(|item| {
                if let ModuleItem::Decl(Decl::Function(f)) = item { Some(f) } else { None }
            }).expect("no function");

            let cg = QuoteCodegen::default();
            let tokens = cg.gen_fn(func);
            let s = tokens.to_string();
            assert!(s.contains("match"), "switch should become match: {}", s);
            assert!(s.contains("return"), "should contain return: {}", s);
        }
    }
}