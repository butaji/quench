use super::helpers::*;
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
