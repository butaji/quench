//! TSX → JS transpiler for the rquickjs dev path.
//!
//! Uses oxc to parse TSX, strip TypeScript types, transform JSX to
//! `React.createElement`, and emit plain JS.  Post-processing rewrites
//! `import { … } from 'ink'` to string-tag constants and injects a
//! minimal React shim so the bundle evals cleanly in rquickjs.

pub mod react_shim;

pub use crate::transpile::bundler::transpile_to_js_bundled;
pub use crate::transpile::postprocess::transpile_to_js;

#[cfg(test)]
mod tests {
    use crate::transpile::postprocess::transpile_to_js;
    use rquickjs::{Context, Runtime};

    #[test]
    fn transpile_basic_tsx() {
        let source = r#"
import React from 'react';
import { Box, Text } from 'ink';

export default function App() {
  return <Box><Text bold>hi</Text></Box>;
}
"#;
        let js = transpile_to_js(source).unwrap();
        assert!(js.contains("function App()"));
        assert!(js.contains("React.createElement"));
        assert!(js.contains(r#"const Box = "Box";"#));
        assert!(js.contains(r#"const Text = "Text";"#));
        assert!(js.contains("var __runts_default = React._withHooks(App);"));
        assert!(!js.contains("import "));
    }

    #[test]
    fn transpile_strips_types() {
        let source = r#"
import React from 'react';
import { Box } from 'ink';

interface Props { name: string; }

export default function App(props: Props) {
  const x: number = 42;
  return <Box>{props.name}</Box>;
}
"#;
        let js = transpile_to_js(source).unwrap();
        assert!(!js.contains("interface Props"));
        assert!(!js.contains("props: Props"));
        assert!(js.contains("const x = 42;"));
    }

    #[test]
    fn transpile_ink_text_props_evals() {
        let path = std::path::Path::new("examples/ink-text-props/tui/app.tsx");
        if !path.exists() {
            eprintln!("Skipping test: {} not found", path.display());
            return;
        }
        let source = std::fs::read_to_string(path).unwrap();
        let js = transpile_to_js(&source).unwrap();

        let runtime = Runtime::new().unwrap();
        let ctx = Context::full(&runtime).unwrap();
        ctx.with(|ctx| {
            runts_ink::js_bridge::install(&ctx).unwrap();
            let result: Result<rquickjs::Value, _> = ctx.eval(js.as_str());
            assert!(result.is_ok(), "Eval failed: {:?}", result.err());

            let default_export: rquickjs::Value = ctx.globals().get("__runts_default").unwrap();
            assert!(default_export.is_function(), "__runts_default should be a function");
        });
    }
}
