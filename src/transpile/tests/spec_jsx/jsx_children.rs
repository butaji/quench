use super::helpers::*;
    mod jsx_children {
        use super::*;

        // Text child

        #[test]
        fn child_text() {
            let jsx = assert_jsx_parses(r#"const x = <div>hello</div>;"#);
            assert!(!jsx.children.is_empty());
            assert!(matches!(&jsx.children[0], JSXChild::Text(t) if t == "hello"));
            assert_codegen_not_empty(&jsx);
        }

        #[test]
        fn child_text_with_spaces() {
            let jsx = assert_jsx_parses(r#"const x = <div>hello world</div>;"#);
            assert!(matches!(&jsx.children[0], JSXChild::Text(t) if t == "hello world"));
            assert_codegen_not_empty(&jsx);
        }

        #[test]
        fn child_text_multiline() {
            let jsx = assert_jsx_parses("const x = <div>line1\nline2</div>;");
            assert!(matches!(&jsx.children[0], JSXChild::Text(_)));
            assert_codegen_not_empty(&jsx);
        }

        // Expression child

        #[test]
        fn child_expr_ident() {
            let jsx = assert_jsx_parses(r#"const x = <div>{name}</div>;"#);
            assert!(!jsx.children.is_empty());
            assert!(matches!(&jsx.children[0], JSXChild::Expr(expr) if matches!(expr, Expr::Ident { name } if name == "name")));
            assert_codegen_not_empty(&jsx);
        }

        #[test]
        fn child_expr_number() {
            let jsx = assert_jsx_parses(r#"const x = <div>{42}</div>;"#);
            assert!(matches!(&jsx.children[0], JSXChild::Expr(expr) if matches!(expr, Expr::Number(n) if *n == 42.0)));
            assert_codegen_not_empty(&jsx);
        }

        #[test]
        fn child_expr_string() {
            let jsx = assert_jsx_parses(r#"const x = <div>{"hello"}</div>;"#);
            assert!(matches!(&jsx.children[0], JSXChild::Expr(expr) if matches!(expr, Expr::String(s) if s == "hello")));
            assert_codegen_not_empty(&jsx);
        }

        // Element child

        #[test]
        fn child_element() {
            let jsx = assert_jsx_parses(r#"const x = <div><span /></div>;"#);
            assert!(!jsx.children.is_empty());
            assert!(matches!(&jsx.children[0], JSXChild::JSX(inner) if matches!(inner.opening.name, JSXName::Ident(ref n) if n == "span")));
            assert_codegen_not_empty(&jsx);
        }

        #[test]
        fn child_element_with_children() {
            let jsx = assert_jsx_parses(r#"const x = <div><span>inner</span></div>;"#);
            let child = &jsx.children[0];
            assert!(matches!(child, JSXChild::JSX(inner) if !inner.children.is_empty()));
            assert_codegen_not_empty(&jsx);
        }

        // Multiple children

        #[test]
        fn child_multiple_exprs() {
            let jsx = assert_jsx_parses(r#"const x = <div>{a}{b}{c}</div>;"#);
            assert_eq!(jsx.children.len(), 3);
            assert!(matches!(&jsx.children[0], JSXChild::Expr(expr) if matches!(expr, Expr::Ident { name } if name == "a")));
            assert!(matches!(&jsx.children[1], JSXChild::Expr(expr) if matches!(expr, Expr::Ident { name } if name == "b")));
            assert!(matches!(&jsx.children[2], JSXChild::Expr(expr) if matches!(expr, Expr::Ident { name } if name == "c")));
            assert_codegen_not_empty(&jsx);
        }

        #[test]
        fn child_text_and_expr() {
            let jsx = assert_jsx_parses(r#"const x = <div>Hello, {name}!</div>;"#);
            // oxc may split text nodes: "Hello, " and "!" become separate text nodes
            assert!(jsx.children.len() >= 2, "Expected at least 2 children, got {}", jsx.children.len());
            // Check that we have an expression child for {name}
            assert!(jsx.children.iter().any(|c| matches!(c, JSXChild::Expr(expr) if matches!(expr, Expr::Ident { name } if name == "name"))),
                "Should have expression child for name");
            assert_codegen_not_empty(&jsx);
        }

        // Conditional rendering (logical AND)

        #[test]
        fn child_conditional_and() {
            let jsx = assert_jsx_parses(r#"const x = <div>{flag && <A />}</div>;"#);
            assert!(!jsx.children.is_empty());
            // The && expression should be in an Expr child
            assert!(matches!(&jsx.children[0], JSXChild::Expr(expr) if matches!(expr, Expr::Logical { op: crate::transpile::hir::LogicalOp::And, .. })));
            assert_codegen_not_empty(&jsx);
        }

        #[test]
        fn child_conditional_and_with_expr() {
            let jsx = assert_jsx_parses(r#"const x = <div>{show && <span>visible</span>}</div>;"#);
            assert!(matches!(&jsx.children[0], JSXChild::Expr(Expr::Logical { .. })));
            assert_codegen_not_empty(&jsx);
        }

        // Ternary rendering

        #[test]
        fn child_ternary() {
            let jsx = assert_jsx_parses(r#"const x = <div>{flag ? <A /> : <B />}</div>;"#);
            assert!(!jsx.children.is_empty());
            assert!(matches!(&jsx.children[0], JSXChild::Expr(expr) if matches!(expr, Expr::Cond { .. })));
            assert_codegen_not_empty(&jsx);
        }

        #[test]
        fn child_ternary_with_text() {
            let jsx = assert_jsx_parses(r#"const x = <div>{ok ? <span>yes</span> : <span>no</span>}</div>;"#);
            assert!(matches!(&jsx.children[0], JSXChild::Expr(Expr::Cond { .. })));
            assert_codegen_not_empty(&jsx);
        }

        // Null child

        #[test]
        fn child_null() {
            let jsx = assert_jsx_parses(r#"const x = <div>{null}</div>;"#);
            assert!(!jsx.children.is_empty());
            assert!(matches!(&jsx.children[0], JSXChild::Expr(expr) if matches!(expr, Expr::Null)));
            assert_codegen_not_empty(&jsx);
        }

        // Array map

        #[test]
        fn child_array_map() {
            let jsx = assert_jsx_parses(r#"const x = <div>{items.map(x => <X />)}</div>;"#);
            assert!(!jsx.children.is_empty());
            assert!(matches!(&jsx.children[0], JSXChild::Expr(Expr::Call { .. })));
            assert_codegen_not_empty(&jsx);
        }

        #[test]
        fn child_array_map_with_key() {
            let jsx = assert_jsx_parses(r#"const x = <ul>{items.map(item => <li key={item.id}>{item.name}</li>)}</ul>;"#);
            assert!(!jsx.children.is_empty());
            // The call expression contains the map
            assert!(matches!(&jsx.children[0], JSXChild::Expr(Expr::Call { .. })));
            assert_codegen_not_empty(&jsx);
        }

        // Inline arrow chains

        #[test]
        fn child_inline_arrow_chain() {
            let jsx = assert_jsx_parses(r#"const x = <div>{items.filter(x => x.active).map(x => <X />)}</div>;"#);
            assert!(!jsx.children.is_empty());
            // Should be a call expression (the final .map())
            assert!(matches!(&jsx.children[0], JSXChild::Expr(Expr::Call { .. })));
            assert_codegen_not_empty(&jsx);
        }

        // Fragment as child

        #[test]
        fn child_fragment() {
            let jsx = assert_jsx_parses(r#"const x = <div><>inner</></div>;"#);
            assert!(!jsx.children.is_empty());
            // Fragment child is stored as JSX with Fragment name
            assert!(matches!(&jsx.children[0], JSXChild::JSX(inner) if matches!(inner.opening.name, JSXName::Fragment)));
            assert_codegen_not_empty(&jsx);
        }

        // Spread child

        #[test]
        fn child_spread() {
            let jsx = assert_jsx_parses(r#"const x = <div>{...children}</div>;"#);
            assert!(!jsx.children.is_empty());
            assert!(matches!(&jsx.children[0], JSXChild::Spread { expr } if matches!(expr, Expr::Ident { name } if name == "children")));
            assert_codegen_not_empty(&jsx);
        }
    
