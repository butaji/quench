//! # Ownership Analysis
//!
//! Infers Rust ownership patterns from TypeScript usage.
//! Determines whether bindings should be `&T`, `&mut T`, or owned `T`.

use swc_ecma_ast::*;
use super::{BorrowMode, OwnershipAnalysis, TypeInfo};
use super::context::AnalysisContext;

/// Borrow mode for a binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BorrowMode {
    /// Immutable borrow `&T`
    Shared,
    /// Mutable borrow `&mut T`
    Mut,
    /// Owned value `T`
    Owned,
    /// Unknown mode
    Unknown,
}

impl BorrowMode {
    /// Check if this mode allows mutation.
    pub fn is_mutable(&self) -> bool {
        matches!(self, BorrowMode::Mut | BorrowMode::Owned)
    }

    /// Combine two borrow modes.
    pub fn combine(self, other: BorrowMode) -> BorrowMode {
        use BorrowMode::*;
        match (self, other) {
            (Unknown, m) | (m, Unknown) => m,
            (Shared, Shared) => Shared,
            (Mut, _) | (_, Mut) => Mut,
            (Owned, Owned) => Owned,
            (Shared, Owned) | (Owned, Shared) => Owned,
        }
    }
}

/// Analyzes ownership and borrowing patterns.
#[derive(Debug)]
pub struct OwnershipAnalyzer {
    /// Analysis results
    analysis: OwnershipAnalysis,
    /// Current scope's bindings
    bindings: Vec<(String, BorrowMode)>,
}

impl OwnershipAnalyzer {
    /// Create a new ownership analyzer.
    pub fn new() -> Self {
        Self {
            analysis: OwnershipAnalysis::default(),
            bindings: Vec::new(),
        }
    }

    /// Analyze a module and produce ownership information.
    pub fn analyze(&mut self, module: &Module, ctx: &AnalysisContext) -> crate::Result<OwnershipAnalysis> {
        for item in &module.body {
            self.analyze_module_item(item, ctx)?;
        }
        Ok(std::mem::take(&mut self.analysis))
    }

    /// Analyze a module item.
    fn analyze_module_item(&mut self, item: &ModuleItem, ctx: &AnalysisContext) -> crate::Result<()> {
        match item {
            ModuleItem::Stmt(Stmt::Decl(decl)) => self.analyze_decl(decl, ctx),
            _ => Ok(()),
        }
    }

    /// Analyze a declaration.
    fn analyze_decl(&mut self, decl: &Decl, ctx: &AnalysisContext) -> crate::Result<()> {
        match decl {
            Decl::Fn(f) => {
                // Analyze function body
                if let Some(body) = f.function.body.as_ref() {
                    self.push_scope();
                    // Add parameters as bindings
                    for param in &f.function.params {
                        let name = param.pat.as_ident()
                            .map(|i| i.id.sym.to_string())
                            .unwrap_or_else(|| "_".to_string());
                        self.bindings.push((name.clone(), BorrowMode::Unknown));
                    }
                    self.analyze_stmt(body, ctx)?;
                    self.pop_scope();
                }
            }
            Decl::Var(v) => {
                for declarator in &v.decls {
                    let (name, mode) = self.analyze_var_declarator(declarator, ctx)?;
                    self.analysis.set(name, mode);
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// Analyze a variable declarator.
    fn analyze_var_declarator(&mut self, declarator: &VarDeclarator, ctx: &AnalysisContext) -> crate::Result<(String, BorrowMode)> {
        let name = match &declarator.name {
            Pat::Ident(ident) => ident.id.sym.to_string(),
            _ => return Ok((String::new(), BorrowMode::Unknown)),
        };

        let mode = match &declarator.init {
            Some(init) => self.analyze_expr_usage(init, ctx)?,
            None => BorrowMode::Owned,
        };

        self.bindings.push((name.clone(), mode));
        Ok((name, mode))
    }

    /// Analyze expression and infer borrow mode from usage.
    fn analyze_expr_usage(&mut self, expr: &Expr, ctx: &AnalysisContext) -> crate::Result<BorrowMode> {
        match expr {
            Expr::Ident(i) => {
                // Look up the binding
                self.find_binding(&i.sym.to_string())
                    .map(|(_, m)| *m)
                    .unwrap_or(BorrowMode::Unknown)
            }
            Expr::Call(c) => {
                // Check if it's a consuming call
                for arg in &c.args {
                    self.analyze_expr_usage(&arg.expr, ctx)?;
                }
                // Return value from function call is typically owned
                BorrowMode::Owned
            }
            Expr::Member(m) => {
                // Accessing a member typically needs shared reference
                self.analyze_expr_usage(&m.obj, ctx)?;
                BorrowMode::Shared
            }
            Expr::Bin(b) => {
                self.analyze_expr_usage(&b.left, ctx)?;
                self.analyze_expr_usage(&b.right, ctx)?;
                BorrowMode::Owned
            }
            Expr::Object(o) => {
                for prop in &o.props {
                    if let PropOrSpread::Prop(Prop::KeyValue(kv)) = prop {
                        self.analyze_expr_usage(&kv.value, ctx)?;
                    }
                }
                BorrowMode::Owned
            }
            Expr::Array(a) => {
                for elem in &a.elems {
                    if let Some(e) = elem {
                        self.analyze_expr_usage(&e.expr, ctx)?;
                    }
                }
                BorrowMode::Owned
            }
            Expr::Arrow(a) => {
                if let BlockStmtOrExpr::BlockStmt(b) = &a.body {
                    self.push_scope();
                    for param in &a.params {
                        let name = param.pat.as_ident()
                            .map(|i| i.id.sym.to_string())
                            .unwrap_or_else(|| "_".to_string());
                        self.bindings.push((name.clone(), BorrowMode::Unknown));
                    }
                    for stmt in &b.stmts {
                        self.analyze_stmt(stmt, ctx)?;
                    }
                    self.pop_scope();
                }
                BorrowMode::Owned
            }
            _ => BorrowMode::Owned,
        }
    }

    /// Analyze a statement for ownership patterns.
    fn analyze_stmt(&mut self, stmt: &Stmt, ctx: &AnalysisContext) -> crate::Result<()> {
        match stmt {
            Stmt::Expr(e) => {
                self.analyze_expr_usage(&e.expr, ctx)?;
            }
            Stmt::If(i) => {
                self.analyze_expr_usage(&i.test, ctx)?;
                self.analyze_stmt(&i.cons, ctx)?;
                if let Some(alt) = &i.alt {
                    self.analyze_stmt(alt, ctx)?;
                }
            }
            Stmt::While(w) => {
                self.analyze_expr_usage(&w.test, ctx)?;
                self.analyze_stmt(&w.body, ctx)
            }
            Stmt::For(f) => {
                if let Some(init) = &f.init {
                    match init {
                        VarDeclOrExpr::VarDecl(v) => {
                            for decl in &v.decls {
                                let (name, mode) = self.analyze_var_declarator(decl, ctx)?;
                                self.analysis.set(name, mode);
                            }
                        }
                        VarDeclOrExpr::Expr(e) => {
                            self.analyze_expr_usage(e, ctx)?;
                        }
                    }
                }
                if let Some(test) = &f.test {
                    self.analyze_expr_usage(test, ctx)?;
                }
                if let Some(update) = &f.update {
                    self.analyze_expr_usage(update, ctx)?;
                }
                self.analyze_stmt(&f.body, ctx)
            }
            Stmt::ForOf(f) => {
                self.push_scope();
                let item_name = match &f.left {
                    VarDeclOrPat::VarDecl(v) => {
                        v.decls.first().and_then(|d| d.name.as_ident())
                            .map(|i| i.id.sym.to_string())
                    }
                    VarDeclOrPat::Pat(p) => {
                        p.as_ident().map(|i| i.id.sym.to_string())
                    }
                    _ => None,
                };
                self.analyze_expr_usage(&f.right, ctx)?;
                if let Some(name) = item_name {
                    self.bindings.push((name.clone(), BorrowMode::Shared));
                    self.analysis.set(name, BorrowMode::Shared);
                }
                self.analyze_stmt(&f.body, ctx)?;
                self.pop_scope()
            }
            Stmt::DoWhile(d) => {
                self.analyze_stmt(&d.body, ctx)?;
                self.analyze_expr_usage(&d.test, ctx)
            }
            Stmt::Switch(s) => {
                self.analyze_expr_usage(&s.discriminant, ctx)?;
                for case in &s.cases {
                    for item in &case.cons {
                        self.analyze_stmt(item, ctx)?;
                    }
                }
                Ok(())
            }
            Stmt::Block(b) => {
                self.push_scope();
                for stmt in &b.stmts {
                    self.analyze_stmt(stmt, ctx)?;
                }
                self.pop_scope();
                Ok(())
            }
            Stmt::Return(r) => {
                if let Some(expr) = &r.value {
                    self.analyze_expr_usage(expr, ctx)?;
                }
                Ok(())
            }
            Stmt::Break(_) | Stmt::Continue(_) | Stmt::Empty(_) | Stmt::Debugger(_) => Ok(()),
            Stmt::Labeled(l) => self.analyze_stmt(&l.body, ctx),
            Stmt::With(_) => Ok(()), // Forbidden - caught by validator
            Stmt::Try(_) | Stmt::Throw(_) => Ok(()), // Forbidden - caught by validator
            Stmt::Decl(d) => self.analyze_decl(d, ctx),
        }
    }

    /// Find a binding in the current scope chain.
    fn find_binding(&self, name: &str) -> Option<&(String, BorrowMode)> {
        self.bindings.iter().rev().find(|(n, _)| n == name)
    }

    /// Push a new scope.
    fn push_scope(&mut self) {
        let depth = self.bindings.len();
        self.bindings.push((format!("__scope_{}", depth), BorrowMode::Unknown));
    }

    /// Pop the current scope.
    fn pop_scope(&mut self) {
        while let Some((name, _)) = self.bindings.pop() {
            if name.starts_with("__scope_") {
                break;
            }
        }
    }
}

impl Default for OwnershipAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}
