use super::helpers::*;
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
    
