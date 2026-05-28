//! Function generation

use anyhow::Result;
use crate::transpile::codegen::CodeGenerator;
use crate::transpile::hir::*;

pub struct FnGen;

impl FnGen {
    pub fn generate_function(cg: &mut CodeGenerator, decl: &FunctionDecl, is_component: bool) -> Result<String> {
        let async_prefix = if decl.is_async { "async " } else { "" };
        let mut output = String::new();
        if is_component { if let Some(param) = decl.params.first() { if let Some(Type::Ref { name, generics }) = &param.type_ { if name == "PageProps" && !generics.is_empty() { output.push_str(&Self::generate_page_props_struct(cg, &decl.name, generics)?); output.push('\n'); } } } }
        let params: Vec<String> = decl.params.iter().map(|p| { let n = cg.to_snake_case(&p.name); let t = p.type_.as_ref().map(|t| cg.type_to_rust(t)).unwrap_or_else(|| "serde_json::Value".to_string()); if p.optional { format!("{}: Option<{}>", n, t) } else { format!("{}: {}", n, t) } }).collect();
        let return_type = if is_component { "VNode".to_string() } else { decl.return_type.as_ref().map(|t| cg.type_to_rust(t)).unwrap_or_else(|| "serde_json::Value".to_string()) };
        output.push_str(&format!("pub {}fn {}({}) -> {} {{\n", async_prefix, decl.name, params.join(", "), return_type));
        if let Some(body) = &decl.body { cg.indent += 1; for stmt in &body.0 { let stmt_code = cg.stmt_to_rust(stmt)?; if !stmt_code.trim().is_empty() { output.push_str(&format!("{}{}\n", cg.indent_str(), stmt_code.trim())); } } cg.indent -= 1; }
        output.push_str("}\n"); Ok(output)
    }

    fn generate_page_props_struct(cg: &CodeGenerator, _name: &str, generics: &[Type]) -> Result<String> { let _data_type = generics.first().map(|g| cg.type_to_rust(g)).unwrap_or_else(|| "()".to_string()); Ok("#[derive(Serialize, Deserialize, Debug, Clone)]\npub struct PageProps<T> {\n    pub params: HashMap<String, String>,\n    pub url: String,\n    pub data: T,\n}\n".to_string()) }
}
