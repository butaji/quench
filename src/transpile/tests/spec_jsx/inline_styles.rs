use super::helpers::*;
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
    
