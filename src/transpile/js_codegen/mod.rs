//! JavaScript code generator

pub mod expr;
pub mod jsx;
pub mod stmt;

use super::hir::*;

pub fn generate_island_js(name: &str, module: &Module) -> String {
    let mut js = String::new();
    js.push_str("// Generated island JS\n");
    js.push_str("export function ");
    js.push_str(name);
    js.push_str("(props) {\n");
    // Find the function in the module
    for item in &module.items {
        if let ModuleItem::Decl(Decl::Function(func)) = item {
            if func.name == name || func.name.is_empty() {
                js.push_str("  // Function body\n");
                break;
            }
        }
    }
    js.push_str("}\n");
    js
}

fn import_spec_to_js(spec: &ImportSpecifier) -> String {
    match spec {
        ImportSpecifier::Named { name, alias } => alias
            .as_ref()
            .map_or_else(|| name.clone(), |a| format!("{} as {}", name, a)),
        ImportSpecifier::Default { name } => name.clone(),
        ImportSpecifier::Namespace { name } => name.clone(),
    }
}

pub fn module_to_js(module: &Module) -> String {
    let mut js = String::new();
    for item in &module.items {
        if let ModuleItem::Import(import) = item {
            let specs: Vec<String> = import
                .specifiers
                .iter()
                .map(|s| import_spec_to_js(s))
                .collect();
            js.push_str(&format!(
                "import {} from '{}';\n",
                specs.join(", "),
                import.source
            ));
        } else if let ModuleItem::Decl(Decl::Function(func)) = item {
            js.push_str(&format!("function {}() {{\n}}\n", func.name));
        }
    }
    js
}
