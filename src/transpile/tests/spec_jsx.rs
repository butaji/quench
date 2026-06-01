//! Comprehensive JSX/TSX spec tests covering SUPPORTED_SUBSET.md sections 3.1-3.2
//!
//! Tests parse correctness, HIR structure, and codegen output for all JSX variants.
//!
//! allow:too_many_lines,complexity

#[cfg(test)]
mod spec_jsx_tests {
    use proc_macro2::TokenStream;
    use quote::ToTokens;

    use crate::transpile::hir::{
        Decl, Expr, JSXAttr, JSXAttrValue, JSXChild, JSXExpr, JSXName, Module,
        ModuleItem, QuoteCodegen,
    };

    // =============================================================================
    // Helpers
    // =============================================================================

    fn parse_jsx(source: &str) -> Module {
        let parser = crate::transpile::parser::TsParser::new();
        parser.parse_tsx(source).expect("parse failed")
    }

    fn find_jsx_expr(module: &Module) -> Option<JSXExpr> {
        for item in &module.items {
            if let ModuleItem::Decl(Decl::Variable(var)) = item {
                if let Some(Expr::JSX(jsx)) = &var.init {
                    return Some(jsx.clone());
                }
            }
        }
        None
    }

    fn find_jsx_expr_in_stmt(module: &Module) -> Option<JSXExpr> {
        for item in &module.items {
            match item {
                ModuleItem::Decl(Decl::Variable(var)) => {
                    if let Some(Expr::JSX(jsx)) = &var.init {
                        return Some(jsx.clone());
                    }
                }
                ModuleItem::Decl(Decl::Function(func)) => {
                    // Look inside function body for return statements with JSX
                    if let Some(ref body) = func.body {
                        for stmt in &body.0 {
                            if let crate::transpile::hir::Stmt::Return { arg: Some(Expr::JSX(jsx)) } = stmt {
                                return Some(jsx.clone());
                            }
                        }
                    }
                }
                ModuleItem::Stmt(stmt) => {
                    if let crate::transpile::hir::Stmt::Return { arg: Some(Expr::JSX(jsx)) } = stmt {
                        return Some(jsx.clone());
                    }
                }
                _ => {}
            }
        }
        None
    }

    /// Check if TokenStream contains Value::Null (with various quote spacing)
    fn contains_value_null(tokens: &TokenStream) -> bool {
        let s = tokens.to_string();
        s.contains("Value :: Null") || s.contains("Value::Null") || s.contains("Value . Null")
    }

    /// Assert JSX parses and is not Expr::Invalid
    fn assert_jsx_parses(source: &str) -> JSXExpr {
        let module = parse_jsx(source);
        let jsx = find_jsx_expr(&module).expect(&format!(
            "Expected to find JSX expr in: {}\nModule: {:#?}",
            source, module
        ));
        jsx
    }

    /// Assert JSX codegen produces non-empty tokens
    fn assert_codegen_not_empty(jsx: &JSXExpr) {
        let expr = Expr::JSX(jsx.clone());
        let tokens = QuoteCodegen::default().gen_expr(&expr);
        assert!(!tokens.is_empty(), "Codegen should produce tokens for JSX");
    }

    // =============================================================================
    // 3.1 JSX Elements
    // =============================================================================

    mod jsx_elements {
        use super::*;

        // HTML elements

        #[test]
        fn html_element_self_closing() {
            let jsx = assert_jsx_parses(r#"const x = <div />;"#);
            assert!(matches!(jsx.opening.name, JSXName::Ident(ref n) if n == "div"));
            assert!(jsx.opening.self_closing);
            assert!(jsx.closing.is_none());
            assert_codegen_not_empty(&jsx);
        }

        #[test]
        fn html_element_with_closing_tag() {
            let jsx = assert_jsx_parses(r#"const x = <div></div>;"#);
            assert!(matches!(jsx.opening.name, JSXName::Ident(ref n) if n == "div"));
            assert!(!jsx.opening.self_closing);
            assert!(jsx.closing.is_some());
            assert_codegen_not_empty(&jsx);
        }

        #[test]
        fn html_element_img_self_closing() {
            let jsx = assert_jsx_parses(r#"const x = <img />;"#);
            assert!(matches!(jsx.opening.name, JSXName::Ident(ref n) if n == "img"));
            assert!(jsx.opening.self_closing);
            assert_codegen_not_empty(&jsx);
        }

        #[test]
        fn html_element_input_self_closing() {
            let jsx = assert_jsx_parses(r#"const x = <input />;"#);
            assert!(matches!(jsx.opening.name, JSXName::Ident(ref n) if n == "input"));
            assert!(jsx.opening.self_closing);
            assert_codegen_not_empty(&jsx);
        }

        #[test]
        fn html_element_span() {
            let jsx = assert_jsx_parses(r#"const x = <span></span>;"#);
            assert!(matches!(jsx.opening.name, JSXName::Ident(ref n) if n == "span"));
            assert_codegen_not_empty(&jsx);
        }

        #[test]
        fn html_element_p() {
            let jsx = assert_jsx_parses(r#"const x = <p>paragraph</p>;"#);
            assert!(matches!(jsx.opening.name, JSXName::Ident(ref n) if n == "p"));
            assert!(!jsx.children.is_empty());
            assert_codegen_not_empty(&jsx);
        }

        // Components (PascalCase)

        #[test]
        fn component_counter() {
            let jsx = assert_jsx_parses(r#"const x = <Counter />;"#);
            assert!(matches!(jsx.opening.name, JSXName::Ident(ref n) if n == "Counter"));
            assert!(jsx.opening.self_closing);
            assert_codegen_not_empty(&jsx);
        }

        #[test]
        fn component_my_component() {
            let jsx = assert_jsx_parses(r#"const x = <MyComponent />;"#);
            assert!(matches!(jsx.opening.name, JSXName::Ident(ref n) if n == "MyComponent"));
            assert_codegen_not_empty(&jsx);
        }

        #[test]
        fn component_with_closing_tag() {
            let jsx = assert_jsx_parses(r#"const x = <MyComponent></MyComponent>;"#);
            assert!(matches!(jsx.opening.name, JSXName::Ident(ref n) if n == "MyComponent"));
            assert!(!jsx.opening.self_closing);
            assert_codegen_not_empty(&jsx);
        }

        #[test]
        fn component_member_expression() {
            // React.Foo style component
            let jsx = assert_jsx_parses(r#"const x = <React.Foo />;"#);
            assert!(matches!(jsx.opening.name, JSXName::Member { object: ref o, property: ref p }
                if o == "React" && p == "Foo"));
            assert_codegen_not_empty(&jsx);
        }

        // Fragments

        #[test]
        fn fragment_empty() {
            let jsx = assert_jsx_parses(r#"const x = <></>;"#);
            assert!(matches!(jsx.opening.name, JSXName::Fragment));
            assert!(!jsx.opening.self_closing);
            assert!(jsx.closing.is_none()); // Fragment has no closing tag in HIR
            assert_codegen_not_empty(&jsx);
        }

        #[test]
        fn fragment_with_content() {
            let jsx = assert_jsx_parses(r#"const x = <>hello</>;"#);
            assert!(matches!(jsx.opening.name, JSXName::Fragment));
            assert!(!jsx.children.is_empty());
            assert_codegen_not_empty(&jsx);
        }

        #[test]
        fn fragment_explicit_tag() {
            let jsx = assert_jsx_parses(r#"const x = <Fragment></Fragment>;"#);
            assert!(matches!(jsx.opening.name, JSXName::Ident(ref n) if n == "Fragment"));
            assert_codegen_not_empty(&jsx);
        }

        // Nested elements

        #[test]
        fn nested_simple() {
            let jsx = assert_jsx_parses(r#"const x = <div><span>text</span></div>;"#);
            assert!(matches!(jsx.opening.name, JSXName::Ident(ref n) if n == "div"));
            assert!(!jsx.children.is_empty());
            // First child should be JSX element (span)
            let child = &jsx.children[0];
            assert!(matches!(child, JSXChild::JSX(inner) if matches!(inner.opening.name, JSXName::Ident(ref n) if n == "span")));
            assert_codegen_not_empty(&jsx);
        }

        #[test]
        fn nested_deep() {
            let jsx = assert_jsx_parses(r#"const x = <div><span><em>deep</em></span></div>;"#);
            assert!(!jsx.children.is_empty());
            let child = &jsx.children[0];
            assert!(matches!(child, JSXChild::JSX(inner) if matches!(inner.opening.name, JSXName::Ident(ref n) if n == "span")));
            assert_codegen_not_empty(&jsx);
        }

        #[test]
        fn nested_multiple_children() {
            let jsx = assert_jsx_parses(r#"const x = <div><span /><p /></div>;"#);
            assert_eq!(jsx.children.len(), 2);
            assert!(matches!(&jsx.children[0], JSXChild::JSX(inner) if matches!(inner.opening.name, JSXName::Ident(ref n) if n == "span")));
            assert!(matches!(&jsx.children[1], JSXChild::JSX(inner) if matches!(inner.opening.name, JSXName::Ident(ref n) if n == "p")));
            assert_codegen_not_empty(&jsx);
        }
    }

    // =============================================================================
    // 3.1 JSX Attributes/Props
    // =============================================================================

    mod jsx_attributes {
        use super::*;

        // String value

        #[test]
        fn attr_string_value() {
            let jsx = assert_jsx_parses(r#"const x = <div class="home" />;"#);
            assert!(!jsx.opening.attrs.is_empty());
            let attr = &jsx.opening.attrs[0];
            assert!(matches!(attr, JSXAttr::Attr { name, value: Some(JSXAttrValue::String(s)) }
                if name == "class" && s == "home"));
            assert_codegen_not_empty(&jsx);
        }

        #[test]
        fn attr_string_value_id() {
            let jsx = assert_jsx_parses(r#"const x = <div id="main" />;"#);
            let attr = &jsx.opening.attrs[0];
            assert!(matches!(attr, JSXAttr::Attr { name, value: Some(JSXAttrValue::String(s)) }
                if name == "id" && s == "main"));
            assert_codegen_not_empty(&jsx);
        }

        #[test]
        fn attr_string_value_title() {
            let jsx = assert_jsx_parses(r#"const x = <div title="tooltip" />;"#);
            let attr = &jsx.opening.attrs[0];
            assert!(matches!(attr, JSXAttr::Attr { name, value: Some(JSXAttrValue::String(s)) }
                if name == "title" && s == "tooltip"));
            assert_codegen_not_empty(&jsx);
        }

        // Expression value

        #[test]
        fn attr_expr_value() {
            let jsx = assert_jsx_parses(r#"const x = <div id={myId} />;"#);
            let attr = &jsx.opening.attrs[0];
            assert!(matches!(attr, JSXAttr::Attr { name, value: Some(JSXAttrValue::Expr(expr)) }
                if name == "id" && matches!(expr, Expr::Ident { name } if name == "myId")));
            assert_codegen_not_empty(&jsx);
        }

        #[test]
        fn attr_expr_value_complex() {
            let jsx = assert_jsx_parses(r#"const x = <div style={{color: "red"}} />;"#);
            let attr = &jsx.opening.attrs[0];
            assert!(matches!(attr, JSXAttr::Attr { name, value: Some(JSXAttrValue::Expr(expr)) }
                if name == "style"));
            assert_codegen_not_empty(&jsx);
        }

        #[test]
        fn attr_expr_value_number() {
            let jsx = assert_jsx_parses(r#"const x = <div tabIndex={0} />;"#);
            let attr = &jsx.opening.attrs[0];
            assert!(matches!(attr, JSXAttr::Attr { name, value: Some(JSXAttrValue::Expr(expr)) }
                if name == "tabIndex"));
            assert_codegen_not_empty(&jsx);
        }

        // Boolean attribute (shorthand)

        #[test]
        fn attr_boolean() {
            let jsx = assert_jsx_parses(r#"const x = <input disabled />;"#);
            let attr = &jsx.opening.attrs[0];
            // Boolean attrs have value: None or Empty
            assert!(matches!(attr, JSXAttr::Attr { name, value }
                if name == "disabled"));
            assert_codegen_not_empty(&jsx);
        }

        #[test]
        fn attr_boolean_checked() {
            let jsx = assert_jsx_parses(r#"const x = <input checked />;"#);
            let attr = &jsx.opening.attrs[0];
            assert!(matches!(attr, JSXAttr::Attr { name, .. } if name == "checked"));
            assert_codegen_not_empty(&jsx);
        }

        #[test]
        fn attr_boolean_autofocus() {
            let jsx = assert_jsx_parses(r#"const x = <input autofocus />;"#);
            let attr = &jsx.opening.attrs[0];
            assert!(matches!(attr, JSXAttr::Attr { name, .. } if name == "autofocus"));
            assert_codegen_not_empty(&jsx);
        }

        // Spread attribute

        #[test]
        fn attr_spread() {
            let jsx = assert_jsx_parses(r#"const x = <div {...props} />;"#);
            let attr = &jsx.opening.attrs[0];
            assert!(matches!(attr, JSXAttr::Spread { expr } if matches!(expr, Expr::Ident { name } if name == "props")));
            assert_codegen_not_empty(&jsx);
        }

        #[test]
        fn attr_spread_with_other_attrs() {
            let jsx = assert_jsx_parses(r#"const x = <div class="home" {...props} />;"#);
            assert_eq!(jsx.opening.attrs.len(), 2);
            assert!(matches!(&jsx.opening.attrs[0], JSXAttr::Attr { name, .. } if name == "class"));
            assert!(matches!(&jsx.opening.attrs[1], JSXAttr::Spread { .. }));
            assert_codegen_not_empty(&jsx);
        }

        #[test]
        fn attr_spread_after_expr() {
            let jsx = assert_jsx_parses(r#"const x = <div id={id} {...props} />;"#);
            assert_eq!(jsx.opening.attrs.len(), 2);
            assert!(matches!(&jsx.opening.attrs[1], JSXAttr::Spread { .. }));
            assert_codegen_not_empty(&jsx);
        }

        // Mixed attributes

        #[test]
        fn attr_mixed_all_types() {
            let jsx = assert_jsx_parses(r#"const x = <div class="home" {...props} id={id} />;"#);
            assert_eq!(jsx.opening.attrs.len(), 3);
            assert!(matches!(&jsx.opening.attrs[0], JSXAttr::Attr { name, value: Some(JSXAttrValue::String(_)) } if name == "class"));
            assert!(matches!(&jsx.opening.attrs[1], JSXAttr::Spread { .. }));
            assert!(matches!(&jsx.opening.attrs[2], JSXAttr::Attr { name, value: Some(JSXAttrValue::Expr(_)) } if name == "id"));
            assert_codegen_not_empty(&jsx);
        }

        // Data attributes

        #[test]
        fn attr_data_id() {
            let jsx = assert_jsx_parses(r#"const x = <div data-id="x" />;"#);
            let attr = &jsx.opening.attrs[0];
            assert!(matches!(attr, JSXAttr::Attr { name, value: Some(JSXAttrValue::String(s)) }
                if name == "data-id" && s == "x"));
            assert_codegen_not_empty(&jsx);
        }

        #[test]
        fn attr_data_custom() {
            let jsx = assert_jsx_parses(r#"const x = <div data-value="test" />;"#);
            let attr = &jsx.opening.attrs[0];
            assert!(matches!(attr, JSXAttr::Attr { name, value: Some(JSXAttrValue::String(s)) }
                if name == "data-value" && s == "test"));
            assert_codegen_not_empty(&jsx);
        }

        // ARIA attributes

        #[test]
        fn attr_aria_label() {
            let jsx = assert_jsx_parses(r#"const x = <div aria-label="x" />;"#);
            let attr = &jsx.opening.attrs[0];
            assert!(matches!(attr, JSXAttr::Attr { name, value: Some(JSXAttrValue::String(s)) }
                if name == "aria-label" && s == "x"));
            assert_codegen_not_empty(&jsx);
        }

        #[test]
        fn attr_aria_hidden() {
            let jsx = assert_jsx_parses(r#"const x = <div aria-hidden="true" />;"#);
            let attr = &jsx.opening.attrs[0];
            assert!(matches!(attr, JSXAttr::Attr { name, value: Some(JSXAttrValue::String(s)) }
                if name == "aria-hidden" && s == "true"));
            assert_codegen_not_empty(&jsx);
        }

        #[test]
        fn attr_aria_role() {
            let jsx = assert_jsx_parses(r#"const x = <div role="button" />;"#);
            let attr = &jsx.opening.attrs[0];
            assert!(matches!(attr, JSXAttr::Attr { name, value: Some(JSXAttrValue::String(s)) }
                if name == "role" && s == "button"));
            assert_codegen_not_empty(&jsx);
        }

        // key prop

        #[test]
        fn attr_key() {
            let jsx = assert_jsx_parses(r#"const x = <div key={i} />;"#);
            let attr = &jsx.opening.attrs[0];
            assert!(matches!(attr, JSXAttr::Attr { name, value: Some(JSXAttrValue::Expr(expr)) }
                if name == "key" && matches!(expr, Expr::Ident { name } if name == "i")));
            assert_codegen_not_empty(&jsx);
        }

        #[test]
        fn attr_key_with_other_props() {
            let jsx = assert_jsx_parses(r#"const x = <div key={i} class="item" />;"#);
            assert_eq!(jsx.opening.attrs.len(), 2);
            assert!(matches!(&jsx.opening.attrs[0], JSXAttr::Attr { name, .. } if name == "key"));
            assert_codegen_not_empty(&jsx);
        }
    }

    // =============================================================================
    // 3.1 JSX Children
    // =============================================================================

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
    }

    // =============================================================================
    // Inline Styles
    // =============================================================================

    mod inline_styles {
        use super::*;

        #[test]
        fn style_single_prop() {
            let jsx = assert_jsx_parses(r#"const x = <div style={{color: "red"}} />;"#);
            let attr = jsx.opening.attrs.iter().find(|a| {
                matches!(a, JSXAttr::Attr { name, .. } if name == "style")
            });
            assert!(attr.is_some(), "Should have style attribute");
            assert_codegen_not_empty(&jsx);
        }

        #[test]
        fn style_multiple_props() {
            let jsx = assert_jsx_parses(r#"const x = <div style={{color: "red", fontSize: 14}} />;"#);
            let attr = jsx.opening.attrs.iter().find(|a| {
                matches!(a, JSXAttr::Attr { name, .. } if name == "style")
            });
            assert!(attr.is_some(), "Should have style attribute");
            assert_codegen_not_empty(&jsx);
        }

        #[test]
        fn style_with_camel_case() {
            let jsx = assert_jsx_parses(r#"const x = <div style={{backgroundColor: "blue"}} />;"#);
            let attr = jsx.opening.attrs.iter().find(|a| {
                matches!(a, JSXAttr::Attr { name, .. } if name == "style")
            });
            assert!(attr.is_some(), "Should have style attribute");
            assert_codegen_not_empty(&jsx);
        }

        #[test]
        fn style_with_unit() {
            let jsx = assert_jsx_parses(r#"const x = <div style={{margin: "10px"}} />;"#);
            let attr = jsx.opening.attrs.iter().find(|a| {
                matches!(a, JSXAttr::Attr { name, .. } if name == "style")
            });
            assert!(attr.is_some(), "Should have style attribute");
            assert_codegen_not_empty(&jsx);
        }
    }

    // =============================================================================
    // class / className
    // =============================================================================

    mod class_classname {
        use super::*;

        #[test]
        fn class_attribute() {
            let jsx = assert_jsx_parses(r#"const x = <div class="home" />;"#);
            let attr = jsx.opening.attrs.iter().find(|a| {
                matches!(a, JSXAttr::Attr { name, .. } if name == "class")
            });
            assert!(attr.is_some(), "Should have class attribute");
            assert_codegen_not_empty(&jsx);
        }

        #[test]
        fn className_attribute() {
            let jsx = assert_jsx_parses(r#"const x = <div className="home" />;"#);
            let attr = jsx.opening.attrs.iter().find(|a| {
                matches!(a, JSXAttr::Attr { name, .. } if name == "className")
            });
            assert!(attr.is_some(), "Should have className attribute");
            assert_codegen_not_empty(&jsx);
        }

        #[test]
        fn class_multiple_values() {
            let jsx = assert_jsx_parses(r#"const x = <div class="foo bar baz" />;"#);
            let attr = jsx.opening.attrs.iter().find(|a| {
                matches!(a, JSXAttr::Attr { name, value: Some(JSXAttrValue::String(s)), .. } if name == "class" && s.contains("foo"))
            });
            assert!(attr.is_some(), "Should have class attribute with multiple values");
            assert_codegen_not_empty(&jsx);
        }
    }

    // =============================================================================
    // Event Handlers (parse only, codegen is framework-specific)
    // =============================================================================

    mod event_handlers {
        use super::*;

        #[test]
        fn onClick_handler() {
            let jsx = assert_jsx_parses(r#"const x = <div onClick={handler} />;"#);
            let attr = jsx.opening.attrs.iter().find(|a| {
                matches!(a, JSXAttr::Attr { name, .. } if name == "onClick")
            });
            assert!(attr.is_some(), "Should have onClick attribute");
            assert_codegen_not_empty(&jsx);
        }

        #[test]
        fn onClick_arrow() {
            let jsx = assert_jsx_parses(r#"const x = <div onClick={() => alert("hi")} />;"#);
            let attr = jsx.opening.attrs.iter().find(|a| {
                matches!(a, JSXAttr::Attr { name, .. } if name == "onClick")
            });
            assert!(attr.is_some(), "Should have onClick attribute");
            assert_codegen_not_empty(&jsx);
        }

        #[test]
        fn onInput_handler() {
            let jsx = assert_jsx_parses(r#"const x = <input onInput={e => console.log(e)} />;"#);
            let attr = jsx.opening.attrs.iter().find(|a| {
                matches!(a, JSXAttr::Attr { name, .. } if name == "onInput")
            });
            assert!(attr.is_some(), "Should have onInput attribute");
            assert_codegen_not_empty(&jsx);
        }

        #[test]
        fn onChange_handler() {
            let jsx = assert_jsx_parses(r#"const x = <input onChange={handleChange} />;"#);
            let attr = jsx.opening.attrs.iter().find(|a| {
                matches!(a, JSXAttr::Attr { name, .. } if name == "onChange")
            });
            assert!(attr.is_some(), "Should have onChange attribute");
            assert_codegen_not_empty(&jsx);
        }

        #[test]
        fn onSubmit_handler() {
            let jsx = assert_jsx_parses(r#"const x = <form onSubmit={onSubmit} />;"#);
            let attr = jsx.opening.attrs.iter().find(|a| {
                matches!(a, JSXAttr::Attr { name, .. } if name == "onSubmit")
            });
            assert!(attr.is_some(), "Should have onSubmit attribute");
            assert_codegen_not_empty(&jsx);
        }

        #[test]
        fn onMouseEnter_handler() {
            let jsx = assert_jsx_parses(r#"const x = <div onMouseEnter={handleEnter} />;"#);
            let attr = jsx.opening.attrs.iter().find(|a| {
                matches!(a, JSXAttr::Attr { name, .. } if name == "onMouseEnter")
            });
            assert!(attr.is_some(), "Should have onMouseEnter attribute");
            assert_codegen_not_empty(&jsx);
        }
    }

    // =============================================================================
    // Codegen: Verify JSX produces non-empty output (placeholder behavior)
    // =============================================================================

    mod codegen {
        use super::*;

        #[test]
        fn codegen_jsx_simple_div() {
            let jsx = assert_jsx_parses(r#"const x = <div />;"#);
            let tokens = QuoteCodegen::default().gen_expr(&Expr::JSX(jsx));
            assert!(!tokens.is_empty(), "JSX codegen should produce tokens");
            // JSX should produce VNode, not Value::Null
            let s = tokens.to_string();
            assert!(s.contains("VNode"), "JSX codegen should produce VNode: {}", s);
        }

        #[test]
        fn codegen_jsx_with_attrs() {
            let jsx = assert_jsx_parses(r#"const x = <div class="home" id="main" />;"#);
            let tokens = QuoteCodegen::default().gen_expr(&Expr::JSX(jsx));
            assert!(!tokens.is_empty(), "JSX codegen should produce tokens");
        }

        #[test]
        fn codegen_jsx_with_children() {
            let jsx = assert_jsx_parses(r#"const x = <div><span>text</span></div>;"#);
            let tokens = QuoteCodegen::default().gen_expr(&Expr::JSX(jsx));
            assert!(!tokens.is_empty(), "JSX codegen should produce tokens");
        }

        #[test]
        fn codegen_jsx_fragment() {
            let jsx = assert_jsx_parses(r#"const x = <></>;"#);
            let tokens = QuoteCodegen::default().gen_expr(&Expr::JSX(jsx));
            assert!(!tokens.is_empty(), "JSX fragment codegen should produce tokens");
        }

        #[test]
        fn codegen_jsx_component() {
            let jsx = assert_jsx_parses(r#"const x = <Counter />;"#);
            let tokens = QuoteCodegen::default().gen_expr(&Expr::JSX(jsx));
            assert!(!tokens.is_empty(), "JSX component codegen should produce tokens");
        }

        #[test]
        fn codegen_jsx_expr_child() {
            let jsx = assert_jsx_parses(r#"const x = <div>{name}</div>;"#);
            let tokens = QuoteCodegen::default().gen_expr(&Expr::JSX(jsx));
            assert!(!tokens.is_empty(), "JSX with expr child codegen should produce tokens");
        }

        #[test]
        fn codegen_jsx_conditional() {
            let jsx = assert_jsx_parses(r#"const x = <div>{flag && <A />}</div>;"#);
            let tokens = QuoteCodegen::default().gen_expr(&Expr::JSX(jsx));
            assert!(!tokens.is_empty(), "JSX with conditional codegen should produce tokens");
        }

        #[test]
        fn codegen_jsx_ternary() {
            let jsx = assert_jsx_parses(r#"const x = <div>{flag ? <A /> : <B />}</div>;"#);
            let tokens = QuoteCodegen::default().gen_expr(&Expr::JSX(jsx));
            assert!(!tokens.is_empty(), "JSX with ternary codegen should produce tokens");
        }
    }

    // =============================================================================
    // JSX in function return position
    // =============================================================================

    mod jsx_in_function {
        use super::*;

        #[test]
        fn jsx_return_simple() {
            let source = r#"function Component() { return <div>hello</div>; }"#;
            let module = parse_jsx(source);
            // Find JSX in return statement
            let jsx_opt = find_jsx_expr_in_stmt(&module);
            assert!(jsx_opt.is_some(), "Should find JSX in return statement");
            let jsx = jsx_opt.unwrap();
            assert!(matches!(jsx.opening.name, JSXName::Ident(ref n) if n == "div"));
            assert_codegen_not_empty(&jsx);
        }

        #[test]
        fn jsx_return_with_props() {
            let source = r#"function Component() { return <div class="home">Welcome</div>; }"#;
            let module = parse_jsx(source);
            let jsx = find_jsx_expr_in_stmt(&module).unwrap();
            assert!(matches!(jsx.opening.name, JSXName::Ident(ref n) if n == "div"));
            assert!(!jsx.opening.attrs.is_empty());
            assert_codegen_not_empty(&jsx);
        }

        #[test]
        fn jsx_return_fragment() {
            let source = r#"function Component() { return <>fragment</>; }"#;
            let module = parse_jsx(source);
            let jsx = find_jsx_expr_in_stmt(&module).unwrap();
            assert!(matches!(jsx.opening.name, JSXName::Fragment));
            assert_codegen_not_empty(&jsx);
        }
    }

    // =============================================================================
    // Edge cases and combinations
    // =============================================================================

    mod edge_cases {
        use super::*;

        #[test]
        fn self_closing_with_children_attribute() {
            // Self-closing with children in attributes shouldn't happen, but parser should handle
            let jsx = assert_jsx_parses(r#"const x = <input type="text" value="hi" />;"#);
            assert!(jsx.opening.self_closing);
            assert_eq!(jsx.opening.attrs.len(), 2);
            assert_codegen_not_empty(&jsx);
        }

        #[test]
        fn namespaced_attribute() {
            // namespaced attrs like xml:lang
            let jsx = assert_jsx_parses(r#"const x = <div xml:lang="en" />;"#);
            // The parser handles this as a regular attribute name
            assert!(!jsx.opening.attrs.is_empty());
            assert_codegen_not_empty(&jsx);
        }

        #[test]
        fn mixed_content() {
            // Complex mixed content with text, expression, and element
            let jsx = assert_jsx_parses(r#"const x = <div>count: {count}</div>;"#);
            assert!(!jsx.children.is_empty());
            // Check that we have text and expression children
            assert!(jsx.children.iter().any(|c| matches!(c, JSXChild::Text(_))), "Should have text child");
            assert!(jsx.children.iter().any(|c| matches!(c, JSXChild::Expr(_))), "Should have expr child");
            assert_codegen_not_empty(&jsx);
        }

        #[test]
        fn component_with_all_props() {
            let jsx = assert_jsx_parses(
                r#"const x = <Counter initial={0} step={1} label="Count" onUpdate={handleUpdate} />;"#,
            );
            assert!(jsx.opening.self_closing);
            assert!(jsx.opening.attrs.len() >= 4);
            assert_codegen_not_empty(&jsx);
        }

        #[test]
        fn empty_expression_in_child() {
            // {} in child position
            let jsx = assert_jsx_parses(r#"const x = <div>{}</div>;"#);
            // Empty expression becomes Null
            assert!(!jsx.children.is_empty());
            assert_codegen_not_empty(&jsx);
        }

        #[test]
        fn jsx_whitespace() {
            let jsx = assert_jsx_parses(r#"const x = <div>   spaced   </div>;"#);
            assert!(matches!(&jsx.children[0], JSXChild::Text(t) if t.contains("spaced")));
            assert_codegen_not_empty(&jsx);
        }
    }
}
