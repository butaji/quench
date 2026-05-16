//! # Subset Validator
//!
//! Validates that TypeScript code uses only the zero-overhead subset.
//! Rejects forbidden features like `any`, `class`, `try/catch`, etc.

use swc_ecma_ast::*;
use super::context::AnalysisContext;

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
    /// Current depth for complexity tracking
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
        match decl {
            Decl::Fn(f) => self.validate_function(&f.function, ctx),
            Decl::Var(v) => {
                self.validate_var_decl(v, ctx)?;
                Ok(())
            }
            Decl::Class(_) => Err(self.error(ctx, "Classes are forbidden. Use plain objects.")),
            Decl::TsInterface(_) => Ok(()),
            Decl::TsTypeAlias(t) => self.validate_ts_type(&t.type_ann, ctx),
            Decl::TsEnum(e) => self.validate_ts_enum(e, ctx),
            Decl::TsModule(_) => Ok(()),
        }
    }

    /// Validate a variable declaration.
    fn validate_var_decl(&mut self, v: &VarDecl, ctx: &mut AnalysisContext) -> crate::Result<()> {
        if v.kind != VarDeclKind::Var {
            // const and let are allowed
        }
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
        match stmt {
            Stmt::Expr(e) => self.validate_expr(&e.expr, ctx),
            Stmt::If(i) => {
                self.validate_expr(&i.test, ctx)?;
                self.validate_stmt(&i.cons, ctx)?;
                if let Some(alt) = &i.alt {
                    self.validate_stmt(alt, ctx)?;
                }
                Ok(())
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
                // Only allowed for arrays with Object.keys() pattern
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
            Stmt::Try(t) => Err(self.error(ctx, "try/catch is forbidden. Use Result<T,E> pattern.")),
            Stmt::Throw(_) => Err(self.error(ctx, "throw is forbidden. Use Result<T,E> pattern.")),
            Stmt::Return(r) => {
                r.value.as_ref().map_or(Ok(()), |e| self.validate_expr(e, ctx))
            }
            Stmt::Break(_) | Stmt::Continue(_) => Ok(()),
            Stmt::With(_) => Err(self.error(ctx, "with is forbidden.")),
            Stmt::Labeled(l) => self.validate_stmt(&l.body, ctx),
            Stmt::Empty(_) => Ok(()),
            Stmt::Debugger(_) => Ok(()),
            Stmt::Decl(d) => self.validate_decl(d, ctx),
        }
    }

    /// Validate an expression.
    fn validate_expr(&mut self, expr: &Expr, ctx: &mut AnalysisContext) -> crate::Result<()> {
        match expr {
            Expr::Ident(_) => Ok(()),
            Expr::Lit(l) => self.validate_lit(l, ctx),
            Expr::Bin(b) => {
                self.validate_expr(&b.left, ctx)?;
                self.validate_expr(&b.right, ctx)?;
                // Check for loose equality
                if matches!(b.op, BinaryOp::EqEq | BinaryOp::NotEq) {
                    return Err(self.error(ctx, "Use === instead of ==".into()));
                }
                // Warn about potential integer division
                if matches!(b.op, BinaryOp::Div) {
                    self.check_integer_division(b, ctx);
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
                    // Dynamic property access - only allowed for arrays
                    let obj_type = ctx.infer_type(&m.obj);
                    if !matches!(obj_type, Some(super::TypeInfo::Array(_))) {
                        return Err(self.error(ctx, "Dynamic property access is forbidden. Use Map<K,V>.".into()));
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
            Expr::New(n) => Err(self.error(ctx, "new is forbidden. Use factory functions.".into())),
            Expr::Arrow(f) => {
                for param in &f.params {
                    if let Pat::Assign(a) = &param.pat {
                        self.validate_expr(&a.right, ctx)?;
                    }
                }
                self.validate_expr(&f.body, ctx)
            }
            Expr::Fn(f) => self.validate_function(&f.function, ctx),
            Expr::Class(c) => Err(self.error(ctx, "Classes are forbidden. Use plain objects.".into())),
            Expr::Seq(s) => {
                for expr in &s.exprs {
                    self.validate_expr(expr, ctx)?;
                }
                Ok(())
            }
            Expr::Cond(c) => {
                self.validate_expr(&c.test, ctx)?;
                self.validate_expr(&c.cons, ctx)?;
                self.validate_expr(&c.alt, ctx)
            }
            Expr::Await(a) => self.validate_expr(&a.arg, ctx),
            Expr::Paren(p) => self.validate_expr(&p.expr, ctx),
            Expr::Tpl(t) => {
                for expr in &t.exprs {
                    self.validate_expr(expr, ctx)?;
                }
                Ok(())
            }
            Expr::TaggedTpl(t) => {
                self.validate_expr(&t.tag, ctx)?;
                for expr in &t.tpl.exprs {
                    self.validate_expr(expr, ctx)?;
                }
                Ok(())
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
            Expr::This(_) => Err(self.error(ctx, "this is forbidden. Use explicit parameters.".into())),
            Expr::Yield(_) => Err(self.error(ctx, "yield is forbidden.".into())),
            Expr::MetaProp(_) => Err(self.error(ctx, "Meta properties are forbidden.".into())),
            Expr::Super(_) => Err(self.error(ctx, "super is forbidden.".into())),
            Expr::TsTypeAssertion(t) => self.validate_expr(&t.expr, ctx),
            Expr::TsAs(t) => self.validate_expr(&t.expr, ctx),
            Expr::TsNonNull(t) => self.validate_expr(&t.expr, ctx),
            Expr::TsSatisfies(t) => self.validate_expr(&t.expr, ctx),
            Expr::Jsx(_) => Ok(()), // Handled by JSX-specific validation
            Expr::Invalid(_) => Err(self.error(ctx, "Invalid expression.".into())),
        }
    }

    /// Validate a literal.
    fn validate_lit(&self, lit: &Lit, ctx: &mut AnalysisContext) -> crate::Result<()> {
        match lit {
            Lit::Null(_) => Ok(()),
            Lit::Bool(_) | Lit::Num(_) | Lit::Str(_) | Lit::BigInt(_) => Ok(()),
            Lit::Regex(_) => Err(self.error(ctx, "Regex literals are forbidden.".into())),
            Lit::JSXText(_) => Ok(()),
        }
    }

    /// Validate an object property.
    fn validate_prop(&self, prop: &Prop, ctx: &mut AnalysisContext) -> crate::Result<()> {
        match prop {
            Prop::Shorthand(i) => Ok(()),
            Prop::KeyValue(k) => {
                self.validate_expr(&k.value, ctx)
            }
            Prop::Assign(a) => self.validate_expr(&a.value, ctx),
            Prop::Getter(g) => self.validate_expr(&g.body.as_ref().unwrap_or(&BlockStmtOrExpr::Invalid(Invalid { span: Default::default() })), ctx),
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
            Pat::Ident(i) => Ok(()),
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
                        ObjectPatProp::KeyValue(k) => self.validate_pat(&k.value, ctx),
                        ObjectPatProp::Rest(r) => self.validate_pat(&r.arg, ctx),
                    }
                }
                Ok(())
            }
            Pat::Assign(a) => {
                self.validate_pat(&a.left, ctx)?;
                self.validate_expr(&a.right, ctx)
            }
            Pat::Invalid(_) => Ok(()),
            Pat::Expr(e) => self.validate_expr(e, ctx),
        }
    }

    /// Validate a module declaration.
    fn validate_module_decl(&self, decl: &ModuleDecl, ctx: &mut AnalysisContext) -> crate::Result<()> {
        match decl {
            ModuleDecl::Import(i) => {
                for spec in &i.specifiers {
                    if let ImportSpecifier::TypeOnly(_) = spec {
                        // Type-only imports are fine
                    }
                }
                Ok(())
            }
            ModuleDecl::Export(e) => self.validate_export(e, ctx),
            ModuleDecl::TsImportEquals(_) => Ok(()),
            ModuleDecl::ExportDefault(_) => Ok(()),
            ModuleDecl::ExportAll(_) => Ok(()),
            ModuleDecl::ModuleASCII(_) => Ok(()),
            ModuleDecl::UseAs(_) => Ok(()),
            ModuleDecl::ImportStar(_) => Err(self.error(ctx, "Wildcard imports are forbidden.".into())),
        }
    }

    /// Validate an export.
    fn validate_export(&self, export: &ExportDecl, ctx: &mut AnalysisContext) -> crate::Result<()> {
        match export {
            ExportDecl::Decl(d) => self.validate_decl(d, ctx),
            ExportDecl::Named(n) => {
                for spec in &n.specifiers {
                    match spec {
                        ExportSpecifier::Named(_) => {}
                        ExportSpecifier::Default(_) => {}
                        ExportSpecifier::Namespace(_) => {}
                    }
                }
                Ok(())
            }
        }
    }

    /// Validate a TypeScript type annotation.
    fn validate_ts_type(&self, type_: &TsType, ctx: &mut AnalysisContext) -> crate::Result<()> {
        match type_ {
            TsType::TsKeywordType(k) => {
                if matches!(k.kind, TsKeywordTypeKind::TsAnyType | TsKeywordTypeKind::TsUnknownType) {
                    return Err(self.error(ctx, "any and unknown are forbidden. Use concrete types.".into()));
                }
                Ok(())
            }
            TsType::TsLitType(_) => Ok(()),
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
            TsType::TsConstructorType(_) => Ok(()),
            TsType::TsMappedType(_) => Err(self.error(ctx, "Mapped types are forbidden.".into())),
            TsType::TsTemplateLiteralType(_) => Ok(()),
            TsType::TsInferType(_) => Ok(()),
            TsType::TspreadType(_) => Ok(()),
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
            TsType::TsTypeOperatorType(t) => self.validate_ts_type(&t.type_ann, ctx),
            TsType::TsConditionalType(c) => {
                self.validate_ts_type(&c.check_type, ctx)?;
                self.validate_ts_type(&c.extends_type, ctx)?;
                self.validate_ts_type(&c.true_type, ctx)?;
                self.validate_ts_type(&c.false_type, ctx)
            }
        }
    }

    /// Validate a TypeScript enum.
    fn validate_ts_enum(&self, e: &TsEnumDecl, ctx: &mut AnalysisContext) -> crate::Result<()> {
        // Check for duplicate members
        let mut tags: std::collections::HashSet<&str> = std::collections::HashSet::new();
        for member in &e.members {
            let tag = match &member.id {
                TsEnumMemberId::Str(s) => s.value.as_str(),
                TsEnumMemberId::Computed(c) => return Err(self.error(ctx, "Computed enum members are forbidden.".into())),
            };
            if !tags.insert(tag) {
                return Err(self.error(ctx, format!("Duplicate enum member: {}", tag)));
            }
        }
        Ok(())
    }

    /// Check for integer division warning.
    fn check_integer_division(&self, bin: &BinExpr, ctx: &mut AnalysisContext) {
        // This is a simplified check - full implementation would
        // track inferred types through the expression tree
        ctx.add_warning(
            ctx.current_location(),
            "Integer division inferred. Use explicit float (e.g., x / 2.0) if float division intended.".into(),
            "integer-division",
        );
    }

    /// Create a validation error.
    fn error(&self, ctx: &mut AnalysisContext, message: String) -> crate::RuneError {
        crate::RuneError::Analysis {
            location: ctx.current_location(),
            message,
        }
    }
}
