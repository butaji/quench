//! TSX → JS transpiler for the rquickjs dev path.
//!
//! Uses oxc to parse TSX, strip TypeScript types, transform JSX to
//! `React.createElement`, and emit plain JS.  Post-processing rewrites
//! `import { … } from 'ink'` to string-tag constants and injects a
//! minimal React shim so the bundle evals cleanly in rquickjs.

pub mod react_shim;
pub mod runtime_shim;

#[cfg(test)]
mod tests {
    use crate::transpile::bundler::transpile_to_js_bundled;

    #[test]
    fn transpile_ink_text_props_evals() {
        let path = std::path::Path::new("examples/ink-text-props/tui/app.tsx");
        if !path.exists() {
            eprintln!("Skipping test: {} not found", path.display());
            return;
        }

        let js = transpile_to_js_bundled(path).unwrap();

        let runtime = rquickjs::Runtime::new().unwrap();
        let ctx = rquickjs::Context::full(&runtime).unwrap();
        ctx.with(|ctx| {
            runts_ink::js_bridge::install(&ctx).unwrap();
            let result: Result<rquickjs::Value, _> = ctx.eval(js.as_str());
            assert!(result.is_ok(), "Eval failed: {:?}", result.err());

            let default_export: rquickjs::Value = ctx.globals().get("__runts_default").unwrap();
            assert!(default_export.is_function(), "__runts_default should be a function");
        });
    }

    #[test]
    fn bundler_handles_ink_imports() {
        let path = std::path::Path::new("examples/ink-text-props/tui/app.tsx");
        if !path.exists() {
            eprintln!("Skipping test: {} not found", path.display());
            return;
        }

        let js = transpile_to_js_bundled(path).unwrap();
        // Verify ink components are converted to string constants
        assert!(js.contains("var Box = 'Box'"), "Box should be string constant");
        assert!(js.contains("var Text = 'Text'"), "Text should be string constant");
        // Verify React shim is included
        assert!(js.contains("React._withHooks"), "React._withHooks should be present");
    }
}
