//! JavaScript code generator for island client bundles

pub mod stmt;
pub mod expr;
pub mod jsx;

use super::hir::*;

pub use stmt::*;
pub use expr::*;
pub use jsx::*;

/// Generate a Preact island JS bundle from HIR.
pub fn generate_island_js(name: &str, module: &Module) -> String {
    let mut js = String::new();

    let component = module.items.iter().find_map(|item| {
        if let ModuleItem::Export(Export::Default { expr }) = item {
            if let Expr::Function { decl } = expr {
                return Some(decl.clone());
            }
        }
        None
    });

    let Some(component) = component else {
        return format!("console.error('[runts] No default export found for island: {}');", name);
    };

    js.push_str("import { h, render } from 'preact';\n");

    let body_stmts = component.body.as_ref().map(|b| b.0.as_slice()).unwrap_or(&[]);
    let mut hooks = Vec::new();
    if stmts_have_call(body_stmts, "useState") { hooks.push("useState"); }
    if stmts_have_call(body_stmts, "useEffect") { hooks.push("useEffect"); }
    if stmts_have_call(body_stmts, "useRef") { hooks.push("useRef"); }
    if stmts_have_call(body_stmts, "useMemo") { hooks.push("useMemo"); }
    if stmts_have_call(body_stmts, "useCallback") { hooks.push("useCallback"); }
    if stmts_have_call(body_stmts, "useSignal") { hooks.push("useSignal"); }
    if stmts_have_call(body_stmts, "useComputed") { hooks.push("useComputed"); }
    if !hooks.is_empty() {
        js.push_str(&format!("import {{ {} }} from 'preact/hooks';\n", hooks.join(", ")));
    }

    js.push('\n');
    js.push_str(&format!("// Island: {}\n", name));
    js.push_str("export default function ");
    js.push_str(name);
    js.push_str("Component(");
    js.push_str(&params_to_js(&component.params));
    js.push_str(") {\n");

    if let Some(body) = &component.body {
        for stmt in &body.0 {
            let stmt_js = stmt_to_js(stmt);
            if !stmt_js.is_empty() {
                for line in stmt_js.lines() {
                    js.push_str("  ");
                    js.push_str(line);
                    js.push('\n');
                }
            }
        }
    }

    js.push_str("}\n\n");
    js.push_str("// Auto-hydrate on client\n");
    js.push_str("const el = document.querySelector('[data-island=\"");
    js.push_str(name);
    js.push_str("\"]');\n");
    js.push_str("if (el && typeof Runts !== 'undefined') {\n");
    js.push_str("  Runts.registerIsland('");
    js.push_str(name);
    js.push_str("', ");
    js.push_str(name);
    js.push_str("Component);\n}\n");

    js
}
