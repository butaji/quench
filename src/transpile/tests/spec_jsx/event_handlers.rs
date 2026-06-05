use super::helpers::*;
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
    
