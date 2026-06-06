//! React plugin implementation
#![allow(clippy::too_many_lines)]

use runts_plugin::{CargoDep, DevAction, DevContext, DevState, Plugin, PluginError};

pub struct ReactPlugin;

struct ReactDevState;

impl DevState for ReactDevState {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

impl Plugin for ReactPlugin {
    fn name(&self) -> &str { "react" }

    fn help_text(&self) -> &str {
        "React SSR with streaming — renderToPipeableStream, Suspense, lazy loading"
    }

    fn codegen_module(&self, hir_str: &str) -> Result<String, PluginError> {
        let module: runts_plugin::hir::Module = serde_json::from_str(hir_str)
            .map_err(|e| PluginError::codegen("react", "unknown", format!("{e}")))?;

        let source_path = module.source_path.as_deref().unwrap_or("");
        let has_hir_items = module.items_json.as_ref()
            .map_or(false, |v| v.as_array().map_or(false, |a| !a.is_empty()));

        if has_hir_items {
            self.codegen_module_with_hir(source_path)
        } else {
            self.codegen_module_without_hir(source_path)
        }
    }

    fn codegen_module_with_hir(&self, source_path: &str) -> Result<String, PluginError> {
        if source_path.contains("/component/") || source_path.ends_with(".jsx") || source_path.ends_with(".tsx") {
            return Ok(self.codegen_component_module(source_path));
        }
        if source_path.contains("server") || source_path.contains("main") {
            return Ok(self.codegen_server_module(source_path));
        }
        Ok(self.codegen_generic_module())
    }

    fn codegen_module_without_hir(&self, source_path: &str) -> Result<String, PluginError> {
        if source_path.contains("/component/") || source_path.ends_with(".jsx") || source_path.ends_with(".tsx") {
            Ok(self.codegen_component_module(source_path))
        } else if source_path.contains("server") || source_path.contains("main") {
            Ok(self.codegen_server_module(source_path))
        } else {
            Ok(self.codegen_generic_module())
        }
    }

    fn cargo_deps(&self) -> Vec<CargoDep> {
        vec![
            CargoDep { name: "axum".into(), version: Some("0.7".into()), path: None, features: vec!["macros".into()] },
            CargoDep { name: "tokio".into(), version: Some("1.0".into()), path: None, features: vec!["full".into()] },
            CargoDep { name: "serde".into(), version: Some("1.0".into()), path: None, features: vec!["derive".into()] },
            CargoDep { name: "serde_json".into(), version: Some("1.0".into()), path: None, features: vec![] },
        ]
    }

    fn codegen_entry(&self, modules: &[runts_plugin::hir::Module]) -> Result<String, PluginError> {
        let routes_code = collect_routes_code(modules);
        let component_name = find_first_component_name(modules)
            .unwrap_or_else(|| "App".to_string());
        Ok(generate_axum_main(&routes_code, &component_name))
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
        let name = extract_component_name(file_path);
        component_template(&name, file_path)
    }

    fn codegen_server_module(&self, file_path: &str) -> String {
        let handler_name = extract_handler_name(file_path);
        let route_path = extract_route_path(file_path);
        let component_name = extract_component_from_imports(file_path);
        generate_server_handler(&handler_name, &route_path, &component_name, file_path)
    }

    fn codegen_generic_module(&self) -> String {
        r#"//! Generic React module
//!
//! Default code generation for unclassified React files

pub struct GenericComponent;

impl GenericComponent {
    pub fn render() -> String {
        "<div>Generic Component</div>".to_string()
    }
}
"#.to_string()
    }
}

mod helpers;
use helpers::*;

#[cfg(test)]
mod tests;
