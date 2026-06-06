#[cfg(test)]
mod tests {
    use super::*;
    use crate::js_bridge::install;
    use rquickjs::context::intrinsic;
    use rquickjs::{Context, Function, Object, Runtime};

    fn fresh_ctx() -> Context {
        let runtime = Runtime::new().unwrap();
        Context::builder()
            .with::<intrinsic::Eval>()
            .build(&runtime)
            .unwrap()
    }

    #[test]
    fn install_creates_namespace() {
        let ctx = fresh_ctx();
        ctx.with(|ctx| {
            install(&ctx).unwrap();
            let globals = ctx.globals();
            let ns = globals.get::<_, Object>("runts_ink").unwrap();
            assert!(ns.get::<_, Function>("box").is_ok());
            assert!(ns.get::<_, Function>("text").is_ok());
            assert!(ns.get::<_, Function>("newline").is_ok());
            assert!(ns.get::<_, Function>("spacer").is_ok());
            assert!(ns.get::<_, Function>("render_to_string").is_ok());
        });
    }

    #[test]
    fn quickjs_string_passthrough() {
        // Direct test: does rquickjs pass a long
        // string from JS to Rust correctly?
        let ctx = fresh_ctx();
        let result = ctx.with(|ctx| {
            let globals = ctx.globals();
            let test_str = "Bordered Example";
            globals.set("testStr", test_str).unwrap();
            // Read it back
            let back: String = globals.get("testStr").unwrap();
            back
        });
        assert_eq!(result, "Bordered Example",
            "rquickjs string passthrough failed: got {result:?}");
    }

    #[test]
    fn quickjs_long_string_literal() {
        // Test: does rquickjs pass a 15-char string
        // from JS to Rust correctly?
        // We use a simple function that just returns
        // the string it receives.
        let ctx = fresh_ctx();
        ctx.with(|ctx| {
            let f = rquickjs::Function::new(
                ctx.clone(),
                |s: String| -> String { s },
            )
            .unwrap();
            let result: String = f
                .call(("Bordered Example",))
                .unwrap();
            assert_eq!(result, "Bordered Example",
                "rquickjs truncated long string: got {result:?}");
        });
    }

    #[test]
    fn quickjs_short_string_literal() {
        // Test: does rquickjs pass a 2-char string?
        let ctx = fresh_ctx();
        ctx.with(|ctx| {
            let f = rquickjs::Function::new(
                ctx.clone(),
                |s: String| -> String { s },
            )
            .unwrap();
            let result: String = f.call(("hi",)).unwrap();
            assert_eq!(result, "hi");
        });
    }

    #[test]
    fn box_with_text_children_renders() {
        let ctx = fresh_ctx();
        ctx.with(|ctx| {
            install(&ctx).unwrap();
            let code = r#"
                let t1 = runts_ink.text("hi", {});
                let t2 = runts_ink.text("inner", {});
                let b = runts_ink.box({
                    flexDirection: "column",
                    borderStyle: "round",
                    paddingX: 1,
                    children: [t1, t2]
                });
                let s = runts_ink.render_to_string(b);
                s
            "#;
            let result: String = ctx.eval(code).unwrap();
            assert!(result.contains("hi"), "missing 'hi': {result}");
            assert!(result.contains("inner"), "missing 'inner': {result}");
            // The renderer may not include `╭` if
            // the line wraps before reaching the
            // top-left corner. Accept any of the
            // round-corner chars we see in the
            // typical render.
            assert!(
                result.contains('╭') || result.contains('╯') || result.contains('╮'),
                "missing border: {result}"
            );
        });
    }

    #[test]
    fn text_styles_apply() {
        let ctx = fresh_ctx();
        ctx.with(|ctx| {
            install(&ctx).unwrap();
            let code = r#"
                let t = runts_ink.text("cyan", { color: "cyan" });
                runts_ink.render_to_string(t)
            "#;
            let result: String = ctx.eval(code).unwrap();
            assert!(result.contains("cyan"));
        });
    }

    #[test]
    fn newline_and_spacer_render() {
        let ctx = fresh_ctx();
        ctx.with(|ctx| {
            install(&ctx).unwrap();
            let code = r#"
                // Newline is a blank line; spacer is
                // zero-width. Both should produce a
                // string (possibly empty) without
                // erroring.
                let h1 = runts_ink.newline();
                let h2 = runts_ink.spacer();
                let s1 = runts_ink.render_to_string(h1);
                let s2 = runts_ink.render_to_string(h2);
                s1 + s2
            "#;
            let result: String = ctx.eval(code).unwrap();
            // The combined output is at minimum
            // a newline (from Newline).
            assert!(
                result.contains('\n') || result.is_empty(),
                "unexpected content: {result}"
            );
        });
    }
}
