use runts_plugin::{CargoDep, DevAction, DevContext, DevState, Plugin, PluginError};

pub struct ReactPlugin;

struct ReactDevState;

impl DevState for ReactDevState {}

impl Plugin for ReactPlugin {
    fn name(&self) -> &str { "react" }

    fn help_text(&self) -> &str {
        "React SSR with streaming — renderToPipeableStream, Suspense, lazy loading"
    }

    fn codegen_module(&self, hir_str: &str) -> Result<String, PluginError> {
        let hir: runts_plugin::hir::Module = serde_json::from_str(hir_str)
            .map_err(|e| PluginError::codegen("react", "unknown", format!("{e}")))?;

        let source_path = hir.source_path.as_deref().unwrap_or("");

        // Check if this is a component file
        if source_path.contains("/component/") || source_path.ends_with(".jsx") {
            Ok(self.codegen_component_module(source_path))
        } else if source_path.contains("server") || source_path.contains("main") {
            Ok(self.codegen_server_module(source_path))
        } else {
            Ok(self.codegen_generic_module())
        }
    }

    fn cargo_deps(&self) -> Vec<CargoDep> {
        vec![
            CargoDep { name: "serde".into(), version: Some("1.0".into()), path: None, features: vec!["derive".into()] },
            CargoDep { name: "serde_json".into(), version: Some("1.0".into()), path: None, features: vec![] },
        ]
    }

    fn codegen_entry(&self, _modules: &[runts_plugin::hir::Module]) -> Result<String, PluginError> {
        Ok(r#"// React SSR entry point
// This would set up the HTTP server with renderToPipeableStream
// For v0.1, this is a placeholder

fn main() {
    println!("React SSR server starting...");
    println!("Full React streaming SSR coming in v0.2");
}
"#.to_string())
    }

    fn dev_init(&self, _ctx: &mut DevContext) -> Result<Box<dyn DevState>, PluginError> {
        Ok(Box::new(ReactDevState))
    }

    fn dev_run_once(&self, _state: &mut dyn DevState) -> Result<DevAction, PluginError> {
        Ok(DevAction::Continue)
    }

    fn dev_reload(&self, _ctx: &mut DevContext, _state: &mut dyn DevState) -> Result<(), PluginError> {
        Ok(())
    }
}

impl ReactPlugin {
    fn codegen_component_module(&self, file_path: &str) -> String {
        let name = file_path.split('/').last()
            .unwrap_or("Component")
            .replace(".jsx", "")
            .replace(".js", "");

        format!(r#"//! React component: {name}
//! Source: {file_path}
//!
//! For v0.1, React components run via QuickJS interpreter.
//! Native compilation to Rust is planned for v0.2.

// Placeholder for React component module
pub struct {name};

impl {name} {{
    pub fn render() -> String {{
        "<div>React Component: {name} (rendered via QuickJS)</div>".to_string()
    }}
}}
"#, name = name, file_path = file_path)
    }

    fn codegen_server_module(&self, file_path: &str) -> String {
        format!(r#"//! React SSR server: {file_path}
//!
//! This would set up HTTP server with renderToPipeableStream.
//! QuickJS runtime with React polyfills for v0.1.

fn main() {{
    println!("React SSR Server");
    println!("Streaming SSR with Suspense + lazy loading");
    println!("Full implementation coming in v0.2");
}}
"#, file_path = file_path)
    }

    fn codegen_generic_module(&self) -> String {
        "// Generic React module\n".to_string()
    }
}
