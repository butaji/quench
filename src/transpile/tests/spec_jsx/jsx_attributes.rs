use super::helpers::*;
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
    
