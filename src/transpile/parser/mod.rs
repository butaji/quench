//! TypeScript parser using oxc
//!
//! allow:too_many_lines,complexity

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
// allow:complexity,too_many_lines
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
            hir::ModuleItem::Decl(hir::Decl::Class(ref mut class)) => {
                // Analyze class methods
                for method in &mut class.methods {
                    hir::infer_method_ownership(method);
                    hir::analyze_method_effects(method);
                }
            }
            hir::ModuleItem::Decl(hir::Decl::Variable(ref mut var)) => {
                // Analyze arrow functions in variable initializers
                if let Some(ref mut expr) = var.init {
                    analyze_expr_passes(expr);
                }
            }
            hir::ModuleItem::Stmt(hir::Stmt::Expr { ref mut expr }) => {
                analyze_expr_passes(expr);
            }
            _ => {}
        }
    }
}

/// Analyze expression passes for arrow functions and other nested expressions
// allow:complexity,too_many_lines
fn analyze_expr_passes(expr: &mut hir::Expr) {
    match expr {
        hir::Expr::ArrowFunction { ref mut params, body, is_async: _ } => {
            // Create a temporary FunctionDecl for analysis
            let mut func = hir::FunctionDecl {
                name: String::new(),
                generics: vec![],
                params: params.clone(),
                return_type: None,
                body: None,
                is_async: false,
                is_generator: false,
                decorators: vec![],
                throws: false,
                error_type: None,
            };
            // Set body from the arrow function body expression
            if let hir::Expr::Block(stmts) = &**body {
                func.body = Some(hir::Block(stmts.clone()));
            }
            hir::infer_function_ownership(&mut func);
            hir::analyze_effects(&mut func);
            // Update params with inferred ownership
            *params = func.params;
        }
        hir::Expr::Function(ref mut func) => {
            hir::infer_function_ownership(func);
            hir::analyze_effects(func);
        }
        hir::Expr::Block(stmts) => {
            for stmt in stmts {
                analyze_stmt_passes(stmt);
            }
        }
        hir::Expr::Call { callee, arguments: _ } => {
            analyze_expr_passes(callee);
        }
        hir::Expr::Member { obj, property, computed: _ } => {
            analyze_expr_passes(obj);
            analyze_expr_passes(property);
        }
        hir::Expr::StaticMember { obj, property: _ } => {
            analyze_expr_passes(obj);
        }
        hir::Expr::JSX(ref mut jsx) => {
            for attr in &mut jsx.opening.attrs {
                if let hir::JSXAttr::Spread { expr } = attr {
                    analyze_expr_passes(expr);
                }
            }
            for child in &mut jsx.children {
                if let hir::JSXChild::Spread { expr } = child {
                    analyze_expr_passes(expr);
                }
            }
        }
        _ => {}
    }
}

/// Analyze statement passes for nested expressions
// allow:complexity,too_many_lines
fn analyze_stmt_passes(stmt: &mut hir::Stmt) {
    match stmt {
        hir::Stmt::If { test, consequent, alternate } => {
            analyze_expr_passes(test);
            analyze_stmt_passes(consequent);
            if let Some(alt) = alternate {
                analyze_stmt_passes(alt);
            }
        }
        hir::Stmt::While { test, body } => {
            analyze_expr_passes(test);
            analyze_stmt_passes(body);
        }
        hir::Stmt::For { init, test, update, body } => {
            if let Some(ref mut for_init) = init {
                match for_init {
                    hir::ForInit::Variable(_, vars) => {
                        for (_, init) in vars {
                            if let Some(ref mut e) = init {
                                analyze_expr_passes(e);
                            }
                        }
                    }
                    hir::ForInit::Expr(ref mut e) => {
                        analyze_expr_passes(e);
                    }
                }
            }
            if let Some(ref mut t) = test {
                analyze_expr_passes(t);
            }
            if let Some(ref mut u) = update {
                analyze_expr_passes(u);
            }
            analyze_stmt_passes(body);
        }
        hir::Stmt::Block { stmts } => {
            for s in stmts {
                analyze_stmt_passes(s);
            }
        }
        hir::Stmt::Return { arg } => {
            if let Some(ref mut a) = arg {
                analyze_expr_passes(a);
            }
        }
        hir::Stmt::Expr { expr } => {
            analyze_expr_passes(expr);
        }
        hir::Stmt::Class(ref mut class) => {
            for method in &mut class.methods {
                hir::infer_method_ownership(method);
                hir::analyze_method_effects(method);
            }
        }
        _ => {}
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
