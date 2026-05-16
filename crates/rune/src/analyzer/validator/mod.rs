//! # Subset Validator
//!
//! Validates that TypeScript code uses only the zero-overhead subset.
//! Rejects forbidden features like `any`, `class`, `try/catch`, etc.

mod rules;

use swc_ecma_ast::*;
use super::context::AnalysisContext;
pub use rules::{
    validate_expr_is_allowed, validate_lit_is_allowed,
    validate_bin_op_is_allowed, validate_stmt_is_allowed,
    validate_decl_is_allowed, validate_ts_keyword_is_allowed,
    validate_module_decl_is_allowed,
};

/// Validation error with source location.
#[derive(Debug, Clone)]
pub struct ValidationError {
    pub location: String,
    pub message: String,
    pub code: &'static str,
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.location, self.message)
    }
}

/// Validates the zero-overhead TypeScript subset.
#[derive(Debug, Default)]
pub struct SubsetValidator {
    complexity: usize,
}

impl SubsetValidator {
    /// Create a new validator.
    pub fn new() -> Self {
        Self { complexity: 0 }
    }

    /// Validate an entire module.
    pub fn validate_module(&mut self, module: &Module, ctx: &mut AnalysisContext) -> crate::Result<()> {
        for item in &module.body {
            self.validate_module_item(item, ctx)?;
        }
        Ok(())
    }

    /// Validate a module item.
    fn validate_module_item(&mut self, item: &ModuleItem, ctx: &mut AnalysisContext) -> crate::Result<()> {
        match item {
            ModuleItem::Stmt(Stmt::Decl(decl)) => self.validate_decl(decl, ctx),
            ModuleItem::Stmt(Stmt::Expr(expr)) => self.validate_expr(&expr.expr, ctx),
            ModuleItem::ModuleDecl(decl) => self.validate_module_decl(decl, ctx),
            _ => Ok(()),
        }
    }

    /// Validate a declaration.
    fn validate_decl(&mut self, decl: &Decl, ctx: &mut AnalysisContext) -> crate::Result<()> {
        if let Some(err) = validate_decl_is_allowed(decl, ctx) {
            return Err(crate::RuneError::Analysis {
                location: err.location,
                message: err.message,
            });
        }

        match decl {
            Decl::Fn(f) => self.validate_function(&f.function, ctx),
            Decl::Var(v) => {
                self.validate_var_decl(v, ctx)?;
                Ok(())
            }
            Decl::TsInterface(_) => Ok(()),
            Decl::TsTypeAlias(t) => self.validate_ts_type(&t.type_ann, ctx),
            Decl::TsEnum(e) => self.validate_ts_enum(e, ctx),
            Decl::TsModule(_) => Ok(()),
            _ => Ok(()),
        }
    }

    /// Validate a variable declaration.
    fn validate_var_decl(&mut self, v: &VarDecl, ctx: &mut AnalysisContext) -> crate::Result<()> {
        for decl in &v.decls {
            if let Some(init) = &decl.init {
                self.validate_expr(init, ctx)?;
            }
        }
        Ok(())
    }

    /// Validate a function.
    fn validate_function(&mut self, f: &Function, ctx: &mut AnalysisContext) -> crate::Result<()> {
        self.complexity += 1;
        if self.complexity > 10 {
            ctx.add_warning(
                ctx.current_location(),
                "High cyclomatic complexity detected".into(),
                "complexity",
            );
        }

        let result = f.body.as_ref().map_or(Ok(()), |b| self.validate_stmt(b, ctx));
        self.complexity -= 1;
        result
    }

    /// Validate a statement.
    fn validate_stmt(&mut self, stmt: &Stmt, ctx: &mut AnalysisContext) -> crate::Result<()> {
        if let Some(err) = validate_stmt_is_allowed(stmt, ctx) {
            return Err(crate::RuneError::Analysis {
                location: err.location,
                message: err.message,
            });
        }

        match stmt {
            Stmt::Expr(e) => self.validate_expr(&e.expr, ctx),
            Stmt::If(i) => {
                self.validate_expr(&i.test, ctx)?;
                self.validate_stmt(&i.cons, ctx)?;
                i.alt.as_ref().map_or(Ok(()), |a| self.validate_stmt(a, ctx))
            }
            Stmt::While(w) => {
                self.validate_expr(&w.test, ctx)?;
                self.validate_stmt(&w.body, ctx)
            }
            Stmt::For(f) => {
                if let Some(init) = &f.init {
                    match init {
                        VarDeclOrExpr::VarDecl(v) => self.validate_var_decl(v, ctx)?,
                        VarDeclOrExpr::Expr(e) => self.validate_expr(e, ctx)?,
                    }
                }
                if let Some(test) = &f.test {
                    self.validate_expr(test, ctx)?;
                }
                if let Some(update) = &f.update {
                    self.validate_expr(update, ctx)?;
                }
                self.validate_stmt(&f.body, ctx)
            }
            Stmt::ForIn(f) => {
                self.validate_expr(&f.right, ctx)?;
                self.validate_stmt(&f.body, ctx)
            }
            Stmt::ForOf(f) => {
                self.validate_expr(&f.right, ctx)?;
                self.validate_stmt(&f.body, ctx)
            }
            Stmt::DoWhile(d) => {
                self.validate_stmt(&d.body, ctx)?;
                self.validate_expr(&d.test, ctx)
            }
            Stmt::Switch(s) => {
                self.validate_expr(&s.discriminant, ctx)?;
                for case in &s.cases {
                    if let Some(test) = &case.test {
                        self.validate_expr(test, ctx)?;
                    }
                    for item in &case.cons {
                        self.validate_stmt(item, ctx)?;
                    }
                }
                Ok(())
            }
            Stmt::Block(b) => {
                for stmt in &b.stmts {
                    self.validate_stmt(stmt, ctx)?;
                }
                Ok(())
            }
            Stmt::Return(r) => r.value.as_ref().map_or(Ok(()), |e| self.validate_expr(e, ctx)),
            Stmt::Break(_) | Stmt::Continue(_) | Stmt::Empty(_) | Stmt::Debugger(_) => Ok(()),
            Stmt::Labeled(l) => self.validate_stmt(&l.body, ctx),
            Stmt::Decl(d) => self.validate_decl(d, ctx),
            _ => Ok(()),
        }
    }

    /// Validate an expression.
    fn validate_expr(&mut self, expr: &Expr, ctx: &mut AnalysisContext) -> crate::Result<()> {
        if let Some(err) = validate_expr_is_allowed(expr, ctx) {
            return Err(crate::RuneError::Analysis {
                location: err.location,
                message: err.message,
            });
        }

        match expr {
            Expr::Ident(_) | Expr::Await(_) | Expr::Paren(_) => Ok(()),
            Expr::Lit(l) => {
                if let Some(err) = validate_lit_is_allowed(l, ctx) {
                    return Err(crate::RuneError::Analysis {
                        location: err.location,
                        message: err.message,
                    });
                }
                Ok(())
            }
            Expr::Bin(b) => {
                self.validate_expr(&b.left, ctx)?;
                self.validate_expr(&b.right, ctx)?;
                if let Some(err) = validate_bin_op_is_allowed(b.op, ctx) {
                    return Err(crate::RuneError::Analysis {
                        location: err.location,
                        message: err.message,
                    });
                }
                if matches!(b.op, BinaryOp::Div) {
                    ctx.add_warning(
                        ctx.current_location(),
                        "Integer division inferred. Use explicit float (e.g., x / 2.0) if float division intended.".into(),
                        "integer-division",
                    );
                }
                Ok(())
            }
            Expr::Unary(u) => self.validate_expr(&u.arg, ctx),
            Expr::Update(u) => self.validate_expr(&u.arg, ctx),
            Expr::Assign(a) => {
                self.validate_expr(&a.left, ctx)?;
                self.validate_expr(&a.value, ctx)
            }
            Expr::Member(m) => {
                self.validate_expr(&m.obj, ctx)?;
                if m.computed {
                    let obj_type = ctx.infer_type(&m.obj);
                    if !matches!(obj_type, Some(super::TypeInfo::Array(_))) {
                        return Err(crate::RuneError::Analysis {
                            location: ctx.current_location(),
                            message: "Dynamic property access is forbidden. Use Map<K,V>.".into(),
                        });
                    }
                }
                if let Some(prop) = &m.prop {
                    self.validate_expr(prop, ctx)?;
                }
                Ok(())
            }
            Expr::Call(c) => {
                self.validate_expr(&c.callee, ctx)?;
                for arg in &c.args {
                    self.validate_expr(&arg.expr, ctx)?;
                }
                Ok(())
            }
            Expr::New(_) => Err(crate::RuneError::Analysis {
                location: ctx.current_location(),
                message: "new is forbidden. Use factory functions.".into(),
            }),
            Expr::Arrow(a) => {
                for param in &a.params {
                    if let Pat::Assign(assign) = &param.pat {
                        self.validate_expr(&assign.right, ctx)?;
                    }
                }
                self.validate_expr(&a.body, ctx)
            }
            Expr::Fn(f) => self.validate_function(&f.function, ctx),
            Expr::Class(_) => Err(crate::RuneError::Analysis {
                location: ctx.current_location(),
                message: "Classes are forbidden. Use plain objects.".into(),
            }),
            Expr::Seq(s) => {
                for e in &s.exprs {
                    self.validate_expr(e, ctx)?;
                }
                Ok(())
            }
            Expr::Cond(c) => {
                self.validate_expr(&c.test, ctx)?;
                self.validate_expr(&c.cons, ctx)?;
                self.validate_expr(&c.alt, ctx)
            }
            Expr::Array(a) => {
                for elem in &a.elems {
                    if let Some(e) = elem {
                        self.validate_expr(&e.expr, ctx)?;
                    }
                }
                Ok(())
            }
            Expr::Object(o) => {
                for prop in &o.props {
                    match prop {
                        PropOrSpread::Prop(p) => self.validate_prop(p, ctx),
                        PropOrSpread::Spread(s) => self.validate_expr(&s.expr, ctx),
                    }
                }
                Ok(())
            }
            Expr::Tpl(t) => {
                for e in &t.exprs {
                    self.validate_expr(e, ctx)?;
                }
                Ok(())
            }
            Expr::TaggedTpl(t) => {
                self.validate_expr(&t.tag, ctx)?;
                for e in &t.tpl.exprs {
                    self.validate_expr(e, ctx)?;
                }
                Ok(())
            }
            Expr::TsTypeAssertion(t) => self.validate_expr(&t.expr, ctx),
            Expr::TsAs(t) => self.validate_expr(&t.expr, ctx),
            Expr::TsNonNull(t) => self.validate_expr(&t.expr, ctx),
            Expr::TsSatisfies(t) => self.validate_expr(&t.expr, ctx),
            Expr::Jsx(_) => Ok(()),
            Expr::Invalid(_) => Err(crate::RuneError::Analysis {
                location: ctx.current_location(),
                message: "Invalid expression.".into(),
            }),
            _ => Ok(()),
        }
    }

    /// Validate a property.
    fn validate_prop(&self, prop: &Prop, ctx: &mut AnalysisContext) -> crate::Result<()> {
        match prop {
            Prop::Shorthand(_) => Ok(()),
            Prop::KeyValue(kv) => self.validate_expr(&kv.value, ctx),
            Prop::Assign(a) => self.validate_expr(&a.value, ctx),
            Prop::Getter(g) => g.body.as_ref()
                .map_or(Ok(()), |b| self.validate_expr(b, ctx)),
            Prop::Setter(s) => {
                if let Some(param) = &s.param {
                    self.validate_pat(param, ctx)?;
                }
                Ok(())
            }
            Prop::Method(m) => self.validate_function(&m.function, ctx),
        }
    }

    /// Validate a pattern.
    fn validate_pat(&self, pat: &Pat, ctx: &mut AnalysisContext) -> crate::Result<()> {
        match pat {
            Pat::Ident(_) => Ok(()),
            Pat::Array(a) => {
                for elem in &a.elems {
                    if let Some(p) = elem {
                        self.validate_pat(p, ctx)?;
                    }
                }
                Ok(())
            }
            Pat::Rest(r) => self.validate_pat(&r.arg, ctx),
            Pat::Object(o) => {
                for prop in &o.props {
                    match prop {
                        ObjectPatProp::Assign(a) => {
                            if let Some(def) = &a.value {
                                self.validate_expr(def, ctx)?;
                            }
                        }
                        ObjectPatProp::KeyValue(kv) => self.validate_pat(&kv.value, ctx),
                        ObjectPatProp::Rest(r) => self.validate_pat(&r.arg, ctx),
                    }
                }
                Ok(())
            }
            Pat::Assign(a) => {
                self.validate_pat(&a.left, ctx)?;
                self.validate_expr(&a.right, ctx)
            }
            _ => Ok(()),
        }
    }

    /// Validate a module declaration.
    fn validate_module_decl(&self, decl: &ModuleDecl, ctx: &mut AnalysisContext) -> crate::Result<()> {
        if let Some(err) = validate_module_decl_is_allowed(decl, ctx) {
            return Err(crate::RuneError::Analysis {
                location: err.location,
                message: err.message,
            });
        }

        match decl {
            ModuleDecl::Import(_) => Ok(()),
            ModuleDecl::Export(e) => self.validate_export(e, ctx),
            _ => Ok(()),
        }
    }

    /// Validate an export.
    fn validate_export(&self, export: &ExportDecl, ctx: &mut AnalysisContext) -> crate::Result<()> {
        match export {
            ExportDecl::Decl(d) => self.validate_decl(d, ctx),
            ExportDecl::Named(_) => Ok(()),
        }
    }

    /// Validate a TypeScript type annotation.
    fn validate_ts_type(&self, type_: &TsType, ctx: &mut AnalysisContext) -> crate::Result<()> {
        match type_ {
            TsType::TsKeywordType(k) => {
                if let Some(err) = validate_ts_keyword_is_allowed(k.kind, ctx) {
                    return Err(crate::RuneError::Analysis {
                        location: err.location,
                        message: err.message,
                    });
                }
                Ok(())
            }
            TsType::TsArrayType(a) => self.validate_ts_type(&a.elem_type, ctx),
            TsType::TsTupleType(t) => {
                for elem in &t.elem_types {
                    self.validate_ts_type(&elem.ty, ctx)?;
                }
                Ok(())
            }
            TsType::TsUnionOrIntersectionType(t) => {
                for ty in &t.types {
                    self.validate_ts_type(ty, ctx)?;
                }
                Ok(())
            }
            TsType::TsParenthesizedType(p) => self.validate_ts_type(&p.type_ann, ctx),
            TsType::TsFunctionType(f) => {
                for param in &f.params {
                    self.validate_pat(&param.pat, ctx)?;
                    if let Some(type_ann) = &param.type_ann {
                        self.validate_ts_type(&type_ann.type_ann, ctx)?;
                    }
                }
                if let Some(ret) = &f.type_ann {
                    self.validate_ts_type(&ret.type_ann, ctx)?;
                }
                Ok(())
            }
            TsType::TsTypeRef(t) => {
                if let Some(type_params) = &t.type_params {
                    for param in &type_params.params {
                        self.validate_ts_type(param, ctx)?;
                    }
                }
                Ok(())
            }
            TsType::TsIndexedAccessType(i) => {
                self.validate_ts_type(&i.obj_type, ctx)?;
                self.validate_ts_type(&i.index_type, ctx)
            }
            TsType::TsMappedType(_) => Err(crate::RuneError::Analysis {
                location: ctx.current_location(),
                message: "Mapped types are forbidden.".into(),
            }),
            _ => Ok(()),
        }
    }

    /// Validate a TypeScript enum.
    fn validate_ts_enum(&self, e: &TsEnumDecl, ctx: &mut AnalysisContext) -> crate::Result<()> {
        let mut tags: std::collections::HashSet<&str> = std::collections::HashSet::new();
        for member in &e.members {
            let tag = match &member.id {
                TsEnumMemberId::Str(s) => s.value.as_str(),
                TsEnumMemberId::Computed(_) => {
                    return Err(crate::RuneError::Analysis {
                        location: ctx.current_location(),
                        message: "Computed enum members are forbidden.".into(),
                    });
                }
            };
            if !tags.insert(tag) {
                return Err(crate::RuneError::Analysis {
                    location: ctx.current_location(),
                    message: format!("Duplicate enum member: {}", tag),
                });
            }
        }
        Ok(())
    }
}
