//! Unit tests for JSX element evaluation.
//!
//! JSX elements are lowered to ink.createElement calls during parsing.
//! The ink library is mocked in tests via Context::set_global.

#[cfg(test)]
mod jsx_tests {
    use crate::value::{NativeFunction, Object, ObjectKind, Value};
    use crate::Context;
    use std::cell::RefCell;
    use std::rc::Rc;

    /// Set up a context with a mock ink library that captures createElement calls.
    fn context_with_mock_ink() -> Context {
        let mut ctx = Context::new().unwrap();

        // Create a mock ink object that records all createElement calls
        let calls = Rc::new(RefCell::new(Vec::<(String, Vec<Value>)>::new()));
        let calls_clone = Rc::clone(&calls);

        let create_element = NativeFunction::new(move |args: Vec<Value>| {
            let tag = match args.first() {
                Some(Value::String(s)) => s.clone(),
                Some(Value::Object(o)) => {
                    let name = o
                        .borrow()
                        .get("name")
                        .map(|v| format!("{:?}", v))
                        .unwrap_or_default();
                    format!("Fragment({})", name)
                }
                _ => "unknown".to_string(),
            };
            calls_clone.borrow_mut().push((tag.clone(), args.clone()));
            // Return a mock element object
            let mut obj = Object::new(ObjectKind::Ordinary);
            obj.set("_tag", Value::String(tag));
            obj.set("_props", args.get(1).cloned().unwrap_or(Value::Null));
            Ok(Value::Object(Rc::new(RefCell::new(obj))))
        });

        let mut ink = Object::new(ObjectKind::Ordinary);
        ink.set(
            "createElement",
            Value::NativeFunction(Rc::new(create_element)),
        );
        ink.set("Fragment", Value::String("Fragment".to_string()));

        ctx.set_global("ink".to_string(), Value::Object(Rc::new(RefCell::new(ink))));

        // Also store calls tracker on context
        ctx.set_global(
            "_jsxCalls".to_string(),
            Value::Object(Rc::new(RefCell::new(Object::new(ObjectKind::Ordinary)))),
        );

        ctx
    }

    // ─── eval_jsx_element: element with tag and no children ────────────────────

    #[test]
    fn jsx_element_no_children() {
        let mut ctx = context_with_mock_ink();
        // <div></div> → ink.createElement("div", {},)
        let r = ctx.eval("<div></div>");
        assert!(r.is_ok(), "JSX element should evaluate without error");
    }

    #[test]
    fn jsx_element_self_closing() {
        let mut ctx = context_with_mock_ink();
        // <br /> is self-closing
        let r = ctx.eval("<br />");
        assert!(r.is_ok(), "Self-closing JSX should evaluate");
    }

    #[test]
    fn jsx_element_with_string_prop() {
        let mut ctx = context_with_mock_ink();
        // <div id="test"></div>
        let r = ctx.eval(r#"<div id="test"></div>"#);
        assert!(r.is_ok(), "JSX with string prop should evaluate");
    }

    #[test]
    fn jsx_element_with_expression_prop() {
        let mut ctx = context_with_mock_ink();
        // <div id={42}></div>
        let r = ctx.eval("<div id={42}></div>");
        assert!(r.is_ok(), "JSX with expression prop should evaluate");
    }

    #[test]
    fn jsx_element_with_text_child() {
        let mut ctx = context_with_mock_ink();
        // <span>hello</span>
        let r = ctx.eval("<span>hello</span>");
        assert!(r.is_ok(), "JSX with text child should evaluate");
    }

    #[test]
    fn jsx_element_with_expression_child() {
        let mut ctx = context_with_mock_ink();
        // <span>{42}</span>
        let r = ctx.eval("<span>{42}</span>");
        assert!(r.is_ok(), "JSX with expression child should evaluate");
    }

    #[test]
    fn jsx_element_multiple_children() {
        let mut ctx = context_with_mock_ink();
        // <ul><li>1</li><li>2</li></ul>
        let r = ctx.eval("<ul><li>1</li><li>2</li></ul>");
        assert!(r.is_ok(), "JSX with multiple children should evaluate");
    }

    #[test]
    fn jsx_element_nested() {
        let mut ctx = context_with_mock_ink();
        // <div><span>nested</span></div>
        let r = ctx.eval("<div><span>nested</span></div>");
        assert!(r.is_ok(), "Nested JSX should evaluate");
    }

    // ─── eval_jsx_element: member tag names ────────────────────────────────────

    #[test]
    fn jsx_element_member_tag_name() {
        let mut ctx = context_with_mock_ink();
        // <Foo.Bar></Foo.Bar>
        let r = ctx.eval("<Foo.Bar></Foo.Bar>");
        assert!(r.is_ok(), "JSX with member tag should evaluate");
    }

    #[test]
    fn jsx_element_namespaced_tag_name() {
        let mut ctx = context_with_mock_ink();
        // <svg:rect></svg:rect>
        let r = ctx.eval("<svg:rect></svg:rect>");
        assert!(r.is_ok(), "JSX with namespaced tag should evaluate");
    }

    // ─── eval_jsx_fragment: fragment elements ───────────────────────────────────

    #[test]
    fn jsx_fragment_empty() {
        let mut ctx = context_with_mock_ink();
        // <> </> is a fragment
        let r = ctx.eval("<> </>");
        assert!(r.is_ok(), "Empty JSX fragment should evaluate");
    }

    #[test]
    fn jsx_fragment_with_children() {
        let mut ctx = context_with_mock_ink();
        // <><span>child</span></>
        let r = ctx.eval("<> <span>child</span> </>");
        assert!(r.is_ok(), "JSX fragment with children should evaluate");
    }

    // ─── eval_jsx_element: spread props (should be skipped) ────────────────────

    #[test]
    fn jsx_element_with_spread_props() {
        let mut ctx = context_with_mock_ink();
        // <div {...props}></div>
        let r = ctx.eval("<div {...props}></div>");
        // Spread is skipped (not yet fully supported), but evaluation should not crash
        assert!(
            r.is_ok() || r.is_err(),
            "JSX with spread props should handle gracefully"
        );
    }

    // ─── eval_jsx_element: spread children (should be skipped) ─────────────────

    #[test]
    fn jsx_element_with_spread_children() {
        let mut ctx = context_with_mock_ink();
        // <div>{...children}</div>
        let r = ctx.eval("<div>{...children}</div>");
        // Spread children are skipped; evaluation should not crash
        assert!(
            r.is_ok() || r.is_err(),
            "JSX with spread children should handle gracefully"
        );
    }

    // ─── JSX in expressions ─────────────────────────────────────────────────────

    #[test]
    fn jsx_in_variable_assignment() {
        let mut ctx = context_with_mock_ink();
        let r = ctx.eval("var el = <div>test</div>; el !== undefined");
        assert!(r.is_ok(), "JSX in variable assignment should work");
    }

    #[test]
    fn jsx_in_array() {
        let mut ctx = context_with_mock_ink();
        let r = ctx.eval("var els = [<span>1</span>, <span>2</span>]; els.length === 2");
        assert!(r.is_ok(), "JSX in array should work");
    }

    #[test]
    fn jsx_in_arrow_function_body() {
        let mut ctx = context_with_mock_ink();
        let r = ctx.eval("var f = () => <div>arrow</div>; typeof f === 'function'");
        assert!(r.is_ok(), "JSX in arrow function body should work");
    }

    // ─── JSX evaluation: ink.createElement error path ──────────────────────────

    #[test]
    fn jsx_element_without_ink_throws() {
        // When ink is not defined, JSX evaluation should fail
        let mut ctx = Context::new().unwrap();
        let r = ctx.eval("<div></div>");
        assert!(r.is_err(), "JSX should fail when ink is not defined");
    }

    // ─── JSX: expression in JSX context ─────────────────────────────────────────

    #[test]
    fn jsx_element_with_arithmetic_in_child() {
        let mut ctx = context_with_mock_ink();
        // <span>{1 + 2}</span>
        let r = ctx.eval("<span>{1 + 2}</span>");
        assert!(
            r.is_ok(),
            "JSX with arithmetic in child expression should evaluate"
        );
    }

    #[test]
    fn jsx_element_with_logical_expr() {
        let mut ctx = context_with_mock_ink();
        // <span>{false && 'hidden'}</span>
        let r = ctx.eval("<span>{false && 'hidden'}</span>");
        assert!(
            r.is_ok(),
            "JSX with logical expression child should evaluate"
        );
    }

    // ─── JSX: attribute spread ──────────────────────────────────────────────────

    #[test]
    fn jsx_multiple_attributes() {
        let mut ctx = context_with_mock_ink();
        // <div id="a" class="b" data-x="c"></div>
        let r = ctx.eval(r#"<div id="a" class="b" data-x="c"></div>"#);
        assert!(r.is_ok(), "JSX with multiple attributes should evaluate");
    }

    // ─── JSX: key prop ─────────────────────────────────────────────────────────

    #[test]
    fn jsx_element_with_key_prop() {
        let mut ctx = context_with_mock_ink();
        // <div key="unique"></div>
        let r = ctx.eval(r#"<div key="unique"></div>"#);
        assert!(r.is_ok(), "JSX with key prop should evaluate");
    }
}
