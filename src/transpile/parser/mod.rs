//! TypeScript parser using oxc

pub mod expr;
pub mod jsx;
pub mod stmt;

#[cfg(test)]
pub mod tests;

use crate::transpile::hir;
use anyhow::Result;
use std::path::Path;

pub fn parse_source(source: &str, is_tsx: bool) -> Result<hir::Module> {
    use oxc_allocator::Allocator;
    use oxc_parser::Parser as OxcParser;
    use oxc_span::SourceType;
    let allocator = Allocator::default();
    let mut source_type = SourceType::default()
        .with_module(true)
        .with_typescript(true);
    if is_tsx {
        source_type = source_type.with_jsx(true);
    }
    let ret = OxcParser::new(&allocator, source, source_type).parse();
    if !ret.errors.is_empty() {
        let error_messages: Vec<String> = ret
            .errors
            .iter()
            .map(|e| format!("{:?}", e))
            .collect();
        anyhow::bail!("Parse errors:\n{}", error_messages.join("\n"));
    }
    let items: Vec<_> = ret
        .program
        .body
        .iter()
        .flat_map(stmt::convert_module_item)
        .collect();
    let mut module = hir::Module {
        source: String::new(),
        items,
        types: std::collections::HashMap::new(),
    };
    // Run ownership and effect analysis passes
    run_analysis_passes(&mut module);
    Ok(module)
}

/// Run ownership and effect analysis on parsed module
fn run_analysis_passes(module: &mut hir::Module) {
    for item in &mut module.items {
        match item {
            hir::ModuleItem::Stmt(hir::Stmt::FunctionDecl(ref mut func)) => {
                hir::infer_function_ownership(func);
                hir::analyze_effects(func);
            }
            hir::ModuleItem::Decl(hir::Decl::Function(ref mut func)) => {
                hir::infer_function_ownership(func);
                hir::analyze_effects(func);
            }
            _ => {}
        }
    }
}

#[allow(dead_code)]
pub fn parse_file(path: &Path) -> Result<hir::Module> {
    let source = std::fs::read_to_string(path)?;
    parse_source(
        &source,
        path.extension().and_then(|e| e.to_str()) == Some("tsx"),
    )
}

pub struct TsParser;
#[allow(dead_code)]
impl TsParser {
    pub fn new() -> Self {
        Self
    }
    pub fn parse_source(&self, s: &str) -> Result<hir::Module> {
        parse_source(s, false)
    }
    pub fn parse_tsx(&self, s: &str) -> Result<hir::Module> {
        parse_source(s, true)
    }
    pub fn parse_file(&self, p: &Path) -> Result<hir::Module> {
        parse_file(p)
    }
}
impl Default for TsParser {
    fn default() -> Self {
        Self::new()
    }
}
