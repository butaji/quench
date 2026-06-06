//! TypeScript parser using oxc
//!

pub mod expr;
pub mod expr_ops;
pub mod jsx;
pub mod stmt;

#[cfg(test)]
pub mod tests;

use crate::transpile::hir;
use anyhow::Result;
use std::path::Path;

#[cfg(test)]
mod debug_tests {
    use super::*;
    #[test]
    fn debug_json() {
        let src = std::fs::read_to_string("examples/ink-text-props/tui/app.tsx").unwrap();
        let module = parse_source(&src, true).unwrap();
        let json = serde_json::to_string_pretty(&module.items).unwrap();
        panic!("{}", json);
    }

    #[test]
    fn debug_raw() {
        let src = std::fs::read_to_string("examples/ink-text-props/tui/app.tsx").unwrap();
        use oxc_allocator::Allocator;
        use oxc_parser::Parser as OxcParser;
        use oxc_span::SourceType;
        let allocator = Allocator::default();
        let source_type = SourceType::default().with_module(true).with_typescript(true).with_jsx(true);
        let ret = OxcParser::new(&allocator, &src, source_type).parse();
        let program = ret.program;
        for stmt in &program.body {
            if let oxc_ast::ast::Statement::ExportDefaultDeclaration(ed) = stmt {
                if let oxc_ast::ast::ExportDefaultDeclarationKind::FunctionDeclaration(f) = &ed.declaration {
                    if let Some(body) = &f.body {
                        for s in &body.statements {
                            if let oxc_ast::ast::Statement::ReturnStatement(r) = s {
                                let arg = r.argument.as_ref();
                                eprintln!("RETURN ARG: {:?}", arg.map(|a| format!("{:?}", a)));
                            }
                        }
                    }
                }
            }
        }
        panic!("done");
    }
}

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
        .flat_map(stmt::convert_statement)
        .collect();
    let mut module = hir::Module {
        source: String::new(),
        source_path: None,
        route_info: None,
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
        run_pass_on_item(item);
    }
}

fn run_pass_on_item(item: &mut hir::ModuleItem) {
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
            for method in &mut class.methods {
                hir::infer_method_ownership(method);
                hir::analyze_method_effects(method);
            }
        }
        hir::ModuleItem::Decl(hir::Decl::Variable(ref mut var)) => {
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

/// Analyze expression passes for arrow functions and other nested expressions
fn analyze_expr_passes(expr: &mut hir::Expr) {
    match expr {
        hir::Expr::ArrowFunction { ref mut params, body, is_async: _ } => {
            analyze_arrow_fn(params, body);
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
        hir::Expr::Call { callee, .. } => analyze_expr_passes(callee),
        hir::Expr::Member { .. } | hir::Expr::StaticMember { .. } => analyze_member_static_expr(expr),
        hir::Expr::JSX(ref mut jsx) => analyze_jsx_spreads(jsx),
        _ => {}
    }
}

fn analyze_member_static_expr(expr: &mut hir::Expr) {
    match expr {
        hir::Expr::Member { obj, property, .. } => {
            analyze_expr_passes(obj);
            analyze_expr_passes(property);
        }
        hir::Expr::StaticMember { obj, .. } => {
            analyze_expr_passes(obj);
        }
        _ => {}
    }
}

fn analyze_arrow_fn(params: &mut Vec<hir::Param>, body: &Box<hir::Expr>) {
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
    if let hir::Expr::Block(stmts) = &**body {
        func.body = Some(hir::Block(stmts.clone()));
    }
    hir::infer_function_ownership(&mut func);
    hir::analyze_effects(&mut func);
    *params = func.params;
}

fn analyze_jsx_spreads(jsx: &mut hir::JSXExpr) {
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

/// Analyze statement passes for nested expressions
fn analyze_stmt_passes(stmt: &mut hir::Stmt) {
    match stmt {
        hir::Stmt::If { test, consequent, alternate } => analyze_if_stmt(test, consequent, alternate),
        hir::Stmt::While { test, body } => analyze_while_stmt(test, body),
        hir::Stmt::For { init, test, update, body } => analyze_for_loop(init, test, update, body),
        hir::Stmt::Block { stmts } => analyze_block_stmts(stmts),
        hir::Stmt::Return { arg } => analyze_return_stmt(arg),
        hir::Stmt::Expr { expr } => analyze_expr_passes(expr),
        hir::Stmt::Class(ref mut class) => analyze_class_methods(class),
        _ => {}
    }
}

fn analyze_if_stmt(test: &mut hir::Expr, consequent: &mut Box<hir::Stmt>, alternate: &mut Option<Box<hir::Stmt>>) {
    analyze_expr_passes(test);
    analyze_stmt_passes(consequent);
    if let Some(alt) = alternate {
        analyze_stmt_passes(alt);
    }
}

fn analyze_while_stmt(test: &mut hir::Expr, body: &mut Box<hir::Stmt>) {
    analyze_expr_passes(test);
    analyze_stmt_passes(body);
}

fn analyze_block_stmts(stmts: &mut Vec<hir::Stmt>) {
    for s in stmts {
        analyze_stmt_passes(s);
    }
}

fn analyze_return_stmt(arg: &mut Option<hir::Expr>) {
    if let Some(ref mut a) = arg {
        analyze_expr_passes(a);
    }
}

fn analyze_for_loop(
    init: &mut Option<hir::ForInit>,
    test: &mut Option<hir::Expr>,
    update: &mut Option<hir::Expr>,
    body: &mut Box<hir::Stmt>,
) {
    if let Some(ref mut for_init) = init {
        analyze_for_init(for_init);
    }
    if let Some(ref mut t) = test {
        analyze_expr_passes(t);
    }
    if let Some(ref mut u) = update {
        analyze_expr_passes(u);
    }
    analyze_stmt_passes(body);
}

fn analyze_for_init(for_init: &mut hir::ForInit) {
    match for_init {
        hir::ForInit::Variable(_, vars) => {
            for (_, init) in vars {
                if let Some(ref mut e) = init {
                    analyze_expr_passes(e);
                }
            }
        }
        hir::ForInit::Expr(ref mut e) => analyze_expr_passes(e),
    }
}

fn analyze_class_methods(class: &mut hir::ClassDecl) {
    for method in &mut class.methods {
        hir::infer_method_ownership(method);
        hir::analyze_method_effects(method);
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
