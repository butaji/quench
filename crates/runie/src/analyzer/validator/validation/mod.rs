//! # Subset Validation
//!
//! Validates the zero-overhead TypeScript subset is being used.
//!
//! This validator parses the source with SWC and walks the AST to detect
//! forbidden language features, eliminating false positives from string literals
//! and comments that plague line-based regex approaches.

use super::ValidationError;
use crate::parser::SourceFile;
use swc_common::Spanned;
use swc_ecma_ast::{
    BinaryOp, Callee, Decl, Expr, ForHead, Lit, MemberProp, Module, ModuleItem, Stmt,
    TsKeywordTypeKind, UnaryOp, VarDeclKind,
};

/// Validator for the Rune TypeScript subset.
#[derive(Debug)]
pub struct SubsetValidator {
    /// Errors found during validation
    errors: Vec<ValidationError>,
    /// Source being validated (for accurate line numbers)
    source: Option<SourceFile>,
}

impl SubsetValidator {
    /// Create a new validator.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            errors: Vec::new(),
            source: None,
        }
    }

    /// Validate a source file by walking its AST.
    ///
    /// # Errors
    /// Returns all validation errors if validation fails.
    pub fn validate(&mut self, source: &SourceFile) -> Result<(), Vec<ValidationError>> {
        self.errors.clear();
        self.source = Some(source.clone());

        // Parse the source to get an AST for validation.
        let ast = if source.is_tsx() {
            crate::parser::swc_parser::SwcAst::parse_tsx(&source.source, &source.name)
        } else {
            crate::parser::swc_parser::SwcAst::parse_ts(&source.source, &source.name)
        };

        if let Ok(ast) = ast {
            self.walk_module(&ast.module);
        }

        self.source = None;

        if self.errors.is_empty() {
            Ok(())
        } else {
            Err(self.errors.clone())
        }
    }

    // ------------------------------------------------------------------
    // AST walkers
    // ------------------------------------------------------------------

    fn walk_module(&mut self, module: &Module) {
        for item in &module.body {
            self.walk_module_item(item);
        }
    }

    fn walk_module_item(&mut self, item: &ModuleItem) {
        match item {
            ModuleItem::Stmt(stmt) => self.walk_stmt(stmt),
            ModuleItem::ModuleDecl(decl) => self.walk_module_decl(decl),
        }
    }

    fn walk_module_decl(&mut self, decl: &swc_ecma_ast::ModuleDecl) {
        match decl {
            swc_ecma_ast::ModuleDecl::Import(_) => {}
            swc_ecma_ast::ModuleDecl::ExportDecl(decl) => self.walk_decl(&decl.decl),
            swc_ecma_ast::ModuleDecl::ExportDefaultExpr(expr) => {
                self.walk_expr(&expr.expr);
            }
            swc_ecma_ast::ModuleDecl::ExportNamed(_) => {}
            swc_ecma_ast::ModuleDecl::ExportAll(_) => {}
            swc_ecma_ast::ModuleDecl::TsImportEquals(_) => {}
            swc_ecma_ast::ModuleDecl::TsExportAssignment(expr) => {
                self.walk_expr(&expr.expr);
            }
            swc_ecma_ast::ModuleDecl::TsNamespaceExport(_) => {}
            swc_ecma_ast::ModuleDecl::ExportDefaultDecl(_) => {}
        }
    }

    fn walk_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Block(block) => {
                for stmt in &block.stmts {
                    self.walk_stmt(stmt);
                }
            }
            Stmt::Empty(_) => {}
            Stmt::Debugger(_) => {}
            Stmt::With(with) => {
                self.push_error(
                    "no-with",
                    "with statement is forbidden.",
                    with.span.lo.0,
                );
                self.walk_stmt(&with.body);
            }
            Stmt::Return(ret) => {
                if let Some(arg) = &ret.arg {
                    self.walk_expr(arg);
                }
            }
            Stmt::Labeled(labeled) => self.walk_stmt(&labeled.body),
            Stmt::Break(_) => {}
            Stmt::Continue(_) => {}
            Stmt::If(if_stmt) => {
                self.check_implicit_coercion(&if_stmt.test);
                self.walk_expr(&if_stmt.test);
                self.walk_stmt(&if_stmt.cons);
                if let Some(alt) = &if_stmt.alt {
                    self.walk_stmt(alt);
                }
            }
            Stmt::Switch(switch) => {
                self.walk_expr(&switch.discriminant);
                for case in &switch.cases {
                    if let Some(test) = &case.test {
                        self.walk_expr(test);
                    }
                    for stmt in &case.cons {
                        self.walk_stmt(stmt);
                    }
                }
            }
            Stmt::Throw(throw) => {
                self.push_error(
                    "no-exceptions",
                    "Use Result<T,E> return pattern instead of throw.",
                    throw.span.lo.0,
                );
                self.walk_expr(&throw.arg);
            }
            Stmt::Try(try_stmt) => {
                self.push_error(
                    "no-exceptions",
                    "Use Result<T,E> return pattern instead of try/catch.",
                    try_stmt.span.lo.0,
                );
                for stmt in &try_stmt.block.stmts {
                    self.walk_stmt(stmt);
                }
                if let Some(handler) = &try_stmt.handler {
                    if let Some(param) = &handler.param {
                        self.walk_pat(param);
                    }
                    for stmt in &handler.body.stmts {
                        self.walk_stmt(stmt);
                    }
                }
                if let Some(finalizer) = &try_stmt.finalizer {
                    for stmt in &finalizer.stmts {
                        self.walk_stmt(stmt);
                    }
                }
            }
            Stmt::While(while_stmt) => {
                self.check_implicit_coercion(&while_stmt.test);
                self.walk_expr(&while_stmt.test);
                self.walk_stmt(&while_stmt.body);
            }
            Stmt::DoWhile(do_while) => {
                self.walk_stmt(&do_while.body);
                self.check_implicit_coercion(&do_while.test);
                self.walk_expr(&do_while.test);
            }
            Stmt::For(for_stmt) => {
                if let Some(init) = &for_stmt.init {
                    match init {
                        swc_ecma_ast::VarDeclOrExpr::VarDecl(decl) => self.walk_var_decl(decl),
                        swc_ecma_ast::VarDeclOrExpr::Expr(expr) => self.walk_expr(expr),
                    }
                }
                if let Some(test) = &for_stmt.test {
                    self.walk_expr(test);
                }
                if let Some(update) = &for_stmt.update {
                    self.walk_expr(update);
                }
                self.walk_stmt(&for_stmt.body);
            }
            Stmt::ForIn(for_in) => {
                self.push_error(
                    "no-for-in",
                    "for...in is forbidden. Use for...of with Object.keys() or Map.",
                    for_in.span.lo.0,
                );
                match &for_in.left {
                    ForHead::Pat(pat) => self.walk_pat(pat),
                    ForHead::VarDecl(decl) => self.walk_var_decl(decl),
                    ForHead::UsingDecl(_) => {}
                }
                self.walk_expr(&for_in.right);
                self.walk_stmt(&for_in.body);
            }
            Stmt::ForOf(for_of) => {
                match &for_of.left {
                    ForHead::Pat(pat) => self.walk_pat(pat),
                    ForHead::VarDecl(decl) => self.walk_var_decl(decl),
                    ForHead::UsingDecl(_) => {}
                }
                self.walk_expr(&for_of.right);
                self.walk_stmt(&for_of.body);
            }
            Stmt::Decl(decl) => self.walk_decl(decl),
            Stmt::Expr(expr) => self.walk_expr(&expr.expr),
        }
    }

    fn walk_decl(&mut self, decl: &Decl) {
        match decl {
            Decl::Class(class) => {
                self.push_error(
                    "no-class",
                    "Classes are forbidden. Use plain objects and functions.",
                    class.class.span.lo.0,
                );
            }
            Decl::Fn(func) => {
                if let Some(body) = &func.function.body {
                    for stmt in &body.stmts {
                        self.walk_stmt(stmt);
                    }
                }
            }
            Decl::Var(var) => self.walk_var_decl(var),
            Decl::TsInterface(intf) => {
                for ext in &intf.extends {
                    self.walk_expr(&ext.expr);
                }
                for member in &intf.body.body {
                    self.walk_ts_type_element(member);
                }
            }
            Decl::TsTypeAlias(alias) => {
                self.walk_ts_type(&alias.type_ann);
            }
            Decl::TsEnum(enm) => {
                for member in &enm.members {
                    if let Some(init) = &member.init {
                        self.walk_expr(init);
                    }
                }
            }
            Decl::TsModule(module) => {
                if let Some(body) = &module.body {
                    self.walk_ts_namespace_body(body);
                }
            }
            Decl::Using(_) => {}
        }
    }

    fn walk_var_decl(&mut self, decl: &swc_ecma_ast::VarDecl) {
        if decl.kind == VarDeclKind::Var {
            self.push_error(
                "no-var",
                "Use 'const' or 'let' instead of 'var'.",
                decl.span.lo.0,
            );
        }
        for decl_item in &decl.decls {
            self.walk_pat(&decl_item.name);
            if let Some(init) = &decl_item.init {
                self.walk_expr(init);
            }
            if decl_item.definite {
                // definite flag set without explicit type annotation
            }
        }
    }

    fn walk_pat(&mut self, pat: &swc_ecma_ast::Pat) {
        match pat {
            swc_ecma_ast::Pat::Ident(ident) => {
                if let Some(type_ann) = &ident.type_ann {
                    self.walk_ts_type(&type_ann.type_ann);
                }
            }
            swc_ecma_ast::Pat::Array(arr) => {
                for elem in arr.elems.iter().flatten() {
                    self.walk_pat(elem);
                }
            }
            swc_ecma_ast::Pat::Rest(rest) => {
                self.walk_pat(&rest.arg);
            }
            swc_ecma_ast::Pat::Object(obj) => {
                for prop in &obj.props {
                    match prop {
                        swc_ecma_ast::ObjectPatProp::KeyValue(kv) => {
                            self.walk_pat(&kv.value);
                        }
                        swc_ecma_ast::ObjectPatProp::Assign(assign) => {
                            if let Some(value) = &assign.value {
                                self.walk_expr(value);
                            }
                        }
                        swc_ecma_ast::ObjectPatProp::Rest(rest) => {
                            self.walk_pat(&rest.arg);
                        }
                    }
                }
            }
            swc_ecma_ast::Pat::Assign(assign) => {
                self.walk_pat(&assign.left);
                self.walk_expr(&assign.right);
            }
            swc_ecma_ast::Pat::Invalid(_) | swc_ecma_ast::Pat::Expr(_) => {}
        }
    }

    fn walk_expr(&mut self, expr: &Expr) {
        match expr {
            Expr::This(this) => {
                self.push_error(
                    "no-this",
                    "'this' keyword is forbidden. Use explicit parameters.",
                    this.span.lo.0,
                );
            }
            Expr::Array(arr) => {
                for elem in arr.elems.iter().flatten() {
                    self.walk_expr(&elem.expr);
                }
            }
            Expr::Object(obj) => {
                for prop in &obj.props {
                    match prop {
                        swc_ecma_ast::PropOrSpread::Prop(prop) => self.walk_prop(prop),
                        swc_ecma_ast::PropOrSpread::Spread(spread) => {
                            self.walk_expr(&spread.expr);
                        }
                    }
                }
            }
            Expr::Fn(func) => {
                if let Some(body) = &func.function.body {
                    for stmt in &body.stmts {
                        self.walk_stmt(stmt);
                    }
                }
            }
            Expr::Unary(unary) => {
                match unary.op {
                    UnaryOp::TypeOf => {
                        self.push_error(
                            "no-typeof",
                            "typeof is forbidden. Runtime type inspection is not allowed.",
                            unary.span.lo.0,
                        );
                    }
                    UnaryOp::Delete => {
                        self.push_error(
                            "no-delete",
                            "delete is forbidden. Use ownership and explicit drops.",
                            unary.span.lo.0,
                        );
                    }
                    _ => {}
                }
                self.walk_expr(&unary.arg);
            }
            Expr::Update(update) => {
                self.walk_expr(&update.arg);
            }
            Expr::Bin(bin) => {
                match bin.op {
                    BinaryOp::EqEq | BinaryOp::NotEq => {
                        self.push_error(
                            "no-loose-equality",
                            "Use strict equality (=== or !==).",
                            bin.span.lo.0,
                        );
                    }
                    BinaryOp::In => {
                        self.push_error(
                            "no-for-in",
                            "in operator is forbidden. Use Map<K,V> for key lookup.",
                            bin.span.lo.0,
                        );
                    }
                    BinaryOp::InstanceOf => {
                        self.push_error(
                            "no-instanceof",
                            "instanceof is forbidden. Use explicit type checking.",
                            bin.span.lo.0,
                        );
                    }
                    _ => {}
                }
                self.walk_expr(&bin.left);
                self.walk_expr(&bin.right);
            }
            Expr::Assign(assign) => {
                match &assign.left {
                    swc_ecma_ast::AssignTarget::Simple(target) => {
                        self.walk_simple_assign_target(target);
                    }
                    swc_ecma_ast::AssignTarget::Pat(pat) => {
                        self.walk_assign_target_pat(pat);
                    }
                }
                self.walk_expr(&assign.right);
            }
            Expr::Member(member) => {
                self.walk_expr(&member.obj);
                if let MemberProp::Computed(comp) = &member.prop {
                    if !self.is_allowed_indexing(&member.obj, &comp.expr) {
                        self.push_error(
                            "no-dynamic-access",
                            "Dynamic property access (obj[key]) is forbidden. Use Map<K,V> for dynamic keys.",
                            member.span.lo.0,
                        );
                    }
                    self.walk_expr(&comp.expr);
                }
            }
            Expr::SuperProp(sup) => {
                if let swc_ecma_ast::SuperProp::Computed(comp) = &sup.prop {
                    self.walk_expr(&comp.expr);
                }
            }
            Expr::Cond(cond) => {
                self.check_implicit_coercion(&cond.test);
                self.walk_expr(&cond.test);
                self.walk_expr(&cond.cons);
                self.walk_expr(&cond.alt);
            }
            Expr::Call(call) => {
                if let Callee::Expr(callee) = &call.callee {
                    if let Expr::Ident(ident) = &**callee {
                        if ident.sym.as_ref() == "eval" {
                            self.push_error(
                                "no-eval",
                                "eval() is forbidden.",
                                call.span.lo.0,
                            );
                        }
                    }
                    self.walk_expr(callee);
                }
                for arg in &call.args {
                    self.walk_expr(&arg.expr);
                }
            }
            Expr::New(new_expr) => {
                // Allow built-in constructors: Array, Map, Set, Date
                let is_builtin = if let Expr::Ident(ident) = &*new_expr.callee {
                    matches!(ident.sym.as_ref(), "Array" | "Map" | "Set" | "Date")
                } else {
                    false
                };
                if !is_builtin {
                    self.push_error(
                        "no-new",
                        "Constructors (new) are forbidden. Use factory functions.",
                        new_expr.span.lo.0,
                    );
                }
                self.walk_expr(&new_expr.callee);
                if let Some(args) = &new_expr.args {
                    for arg in args {
                        self.walk_expr(&arg.expr);
                    }
                }
            }
            Expr::Seq(seq) => {
                for expr in &seq.exprs {
                    self.walk_expr(expr);
                }
            }
            Expr::Ident(ident)
                if ident.sym.as_ref() == "arguments" =>
            {
                self.push_error(
                    "no-arguments",
                    "Use rest parameters (...args) instead of 'arguments'.",
                    ident.span.lo.0,
                );
            }
            Expr::Ident(_) => {}
            Expr::Lit(_) => {}
            Expr::Paren(paren) => self.walk_expr(&paren.expr),
            Expr::Arrow(arrow) => {
                match arrow.body.as_ref() {
                    swc_ecma_ast::BlockStmtOrExpr::BlockStmt(block) => {
                        for stmt in &block.stmts {
                            self.walk_stmt(stmt);
                        }
                    }
                    swc_ecma_ast::BlockStmtOrExpr::Expr(expr) => self.walk_expr(expr),
                }
            }
            Expr::Await(await_expr) => self.walk_expr(&await_expr.arg),
            Expr::Yield(yield_expr) => {
                if let Some(arg) = &yield_expr.arg {
                    self.walk_expr(arg);
                }
            }
            Expr::MetaProp(_) => {}
            Expr::JSXMember(_) => {}
            Expr::JSXNamespacedName(_) => {}
            Expr::JSXEmpty(_) => {}
            Expr::JSXElement(elem) => {
                self.walk_jsx_element(elem);
            }
            Expr::JSXFragment(frag) => {
                for child in &frag.children {
                    self.walk_jsx_element_child(child);
                }
            }
            Expr::TsTypeAssertion(assertion) => {
                self.walk_ts_type(&assertion.type_ann);
                self.walk_expr(&assertion.expr);
            }
            Expr::TsConstAssertion(assertion) => {
                self.walk_expr(&assertion.expr);
            }
            Expr::TsNonNull(non_null) => {
                self.walk_expr(&non_null.expr);
            }
            Expr::TsAs(as_expr) => {
                self.walk_ts_type(&as_expr.type_ann);
                self.walk_expr(&as_expr.expr);
            }
            Expr::TsInstantiation(inst) => {
                self.walk_expr(&inst.expr);
            }
            Expr::TsSatisfies(sat) => {
                self.walk_ts_type(&sat.type_ann);
                self.walk_expr(&sat.expr);
            }
            Expr::PrivateName(_) => {}
            Expr::OptChain(opt) => {
                match opt.base.as_ref() {
                    swc_ecma_ast::OptChainBase::Member(member) => {
                        self.walk_expr(&member.obj);
                        if let MemberProp::Computed(comp) = &member.prop {
                            self.walk_expr(&comp.expr);
                        }
                    }
                    swc_ecma_ast::OptChainBase::Call(call) => {
                        self.walk_expr(&call.callee);
                        for arg in &call.args {
                            self.walk_expr(&arg.expr);
                        }
                    }
                }
            }
            Expr::Invalid(_) => {}
            Expr::Tpl(tpl) => {
                for expr in &tpl.exprs {
                    self.walk_expr(expr);
                }
            }
            Expr::TaggedTpl(tagged) => {
                self.walk_expr(&tagged.tag);
                for expr in &tagged.tpl.exprs {
                    self.walk_expr(expr);
                }
            }
            Expr::Class(_) => {}
        }
    }

    fn walk_prop(&mut self, prop: &swc_ecma_ast::Prop) {
        match prop {
            swc_ecma_ast::Prop::Shorthand(ident) => {
                if ident.sym.as_ref() == "arguments" {
                    self.push_error(
                        "no-arguments",
                        "Use rest parameters (...args) instead of 'arguments'.",
                        ident.span.lo.0,
                    );
                }
            }
            swc_ecma_ast::Prop::KeyValue(kv) => {
                self.walk_expr(&kv.value);
            }
            swc_ecma_ast::Prop::Assign(assign) => {
                self.walk_expr(&assign.value);
            }
            swc_ecma_ast::Prop::Getter(getter) => {
                if let Some(body) = &getter.body {
                    for stmt in &body.stmts {
                        self.walk_stmt(stmt);
                    }
                }
            }
            swc_ecma_ast::Prop::Setter(setter) => {
                self.walk_pat(&setter.param);
                if let Some(body) = &setter.body {
                    for stmt in &body.stmts {
                        self.walk_stmt(stmt);
                    }
                }
            }
            swc_ecma_ast::Prop::Method(method) => {
                if let Some(body) = &method.function.body {
                    for stmt in &body.stmts {
                        self.walk_stmt(stmt);
                    }
                }
            }
        }
    }

    fn walk_jsx_element(&mut self, elem: &swc_ecma_ast::JSXElement) {
        for attr in &elem.opening.attrs {
            if let swc_ecma_ast::JSXAttrOrSpread::JSXAttr(attr) = attr {
                if let Some(value) = &attr.value {
                    match value {
                        swc_ecma_ast::JSXAttrValue::Str(_) => {}
                        swc_ecma_ast::JSXAttrValue::JSXExprContainer(cont) => {
                            if let swc_ecma_ast::JSXExpr::Expr(expr) = &cont.expr {
                                self.walk_expr(expr);
                            }
                        }
                        swc_ecma_ast::JSXAttrValue::JSXElement(child) => {
                            self.walk_jsx_element(child);
                        }
                        swc_ecma_ast::JSXAttrValue::JSXFragment(frag) => {
                            for child in &frag.children {
                                self.walk_jsx_element_child(child);
                            }
                        }
                    }
                }
            }
        }
        for child in &elem.children {
            self.walk_jsx_element_child(child);
        }
    }

    fn walk_jsx_element_child(&mut self, child: &swc_ecma_ast::JSXElementChild) {
        match child {
            swc_ecma_ast::JSXElementChild::JSXText(_) => {}
            swc_ecma_ast::JSXElementChild::JSXExprContainer(cont) => {
                if let swc_ecma_ast::JSXExpr::Expr(expr) = &cont.expr {
                    self.walk_expr(expr);
                }
            }
            swc_ecma_ast::JSXElementChild::JSXElement(elem) => self.walk_jsx_element(elem),
            swc_ecma_ast::JSXElementChild::JSXFragment(frag) => {
                for c in &frag.children {
                    self.walk_jsx_element_child(c);
                }
            }
            swc_ecma_ast::JSXElementChild::JSXSpreadChild(spread) => {
                self.walk_expr(&spread.expr);
            }
        }
    }

    // ------------------------------------------------------------------
    // TypeScript type walkers
    // ------------------------------------------------------------------

    fn walk_ts_type(&mut self, ty: &swc_ecma_ast::TsType) {
        match ty {
            swc_ecma_ast::TsType::TsKeywordType(kw) => {
                match kw.kind {
                    TsKeywordTypeKind::TsAnyKeyword => {
                        self.push_error(
                            "no-any",
                            "Type 'any' requires dynamic dispatch. Use concrete types.",
                            kw.span.lo.0,
                        );
                    }
                    TsKeywordTypeKind::TsUnknownKeyword => {
                        self.push_error(
                            "no-unknown",
                            "Type 'unknown' requires dynamic dispatch. Use concrete types.",
                            kw.span.lo.0,
                        );
                    }
                    _ => {}
                }
            }
            swc_ecma_ast::TsType::TsTypeRef(type_ref) => {
                if let Some(params) = &type_ref.type_params {
                    for param in &params.params {
                        self.walk_ts_type(param);
                    }
                }
            }
            swc_ecma_ast::TsType::TsArrayType(arr) => {
                self.walk_ts_type(&arr.elem_type);
            }
            swc_ecma_ast::TsType::TsTupleType(tuple) => {
                for elem in &tuple.elem_types {
                    self.walk_ts_type(&elem.ty);
                }
            }
            swc_ecma_ast::TsType::TsUnionOrIntersectionType(union) => {
                match union {
                    swc_ecma_ast::TsUnionOrIntersectionType::TsUnionType(u) => {
                        for t in &u.types {
                            self.walk_ts_type(t);
                        }
                    }
                    swc_ecma_ast::TsUnionOrIntersectionType::TsIntersectionType(i) => {
                        for t in &i.types {
                            self.walk_ts_type(t);
                        }
                    }
                }
            }
            swc_ecma_ast::TsType::TsTypeLit(lit) => {
                for member in &lit.members {
                    self.walk_ts_type_element(member);
                }
            }
            swc_ecma_ast::TsType::TsParenthesizedType(paren) => {
                self.walk_ts_type(&paren.type_ann);
            }
            swc_ecma_ast::TsType::TsFnOrConstructorType(fn_type) => {
                match fn_type {
                    swc_ecma_ast::TsFnOrConstructorType::TsFnType(t) => {
                        for param in &t.params {
                            self.walk_ts_fn_param(param);
                        }
                        self.walk_ts_type(&t.type_ann.type_ann);
                    }
                    swc_ecma_ast::TsFnOrConstructorType::TsConstructorType(t) => {
                        for param in &t.params {
                            self.walk_ts_fn_param(param);
                        }
                        self.walk_ts_type(&t.type_ann.type_ann);
                    }
                }
            }
            swc_ecma_ast::TsType::TsConditionalType(cond) => {
                self.walk_ts_type(&cond.check_type);
                self.walk_ts_type(&cond.extends_type);
                self.walk_ts_type(&cond.true_type);
                self.walk_ts_type(&cond.false_type);
            }
            swc_ecma_ast::TsType::TsMappedType(map) => {
                if let Some(constraint) = &map.type_param.constraint {
                    self.walk_ts_type(constraint);
                }
                if let Some(type_ann) = &map.type_ann {
                    self.walk_ts_type(type_ann);
                }
            }
            swc_ecma_ast::TsType::TsTypeQuery(query) => {
                self.walk_ts_type_query_expr(&query.expr_name);
            }
            swc_ecma_ast::TsType::TsTypeOperator(op) => {
                self.walk_ts_type(&op.type_ann);
            }
            swc_ecma_ast::TsType::TsIndexedAccessType(idx) => {
                self.walk_ts_type(&idx.obj_type);
                self.walk_ts_type(&idx.index_type);
            }
            swc_ecma_ast::TsType::TsInferType(infer) => {
                if let Some(constraint) = &infer.type_param.constraint {
                    self.walk_ts_type(constraint);
                }
                if let Some(default_type) = &infer.type_param.default {
                    self.walk_ts_type(default_type);
                }
            }
            swc_ecma_ast::TsType::TsImportType(import) => {
                if let Some(type_params) = &import.type_args {
                    for param in &type_params.params {
                        self.walk_ts_type(param);
                    }
                }
            }
            swc_ecma_ast::TsType::TsLitType(_) => {}
            swc_ecma_ast::TsType::TsThisType(_) => {
                self.push_error(
                    "no-this",
                    "'this' keyword is forbidden. Use explicit parameters.",
                    ty.span().lo.0,
                );
            }
            _ => {}
        }
    }

    fn walk_ts_type_element(&mut self, member: &swc_ecma_ast::TsTypeElement) {
        match member {
            swc_ecma_ast::TsTypeElement::TsPropertySignature(prop) => {
                if let Some(type_ann) = &prop.type_ann {
                    self.walk_ts_type(&type_ann.type_ann);
                }
            }
            swc_ecma_ast::TsTypeElement::TsMethodSignature(method) => {
                for param in &method.params {
                    self.walk_ts_fn_param(param);
                }
                if let Some(type_ann) = &method.type_ann {
                    self.walk_ts_type(&type_ann.type_ann);
                }
            }
            swc_ecma_ast::TsTypeElement::TsCallSignatureDecl(call) => {
                for param in &call.params {
                    self.walk_ts_fn_param(param);
                }
                if let Some(type_ann) = &call.type_ann {
                    self.walk_ts_type(&type_ann.type_ann);
                }
            }
            swc_ecma_ast::TsTypeElement::TsConstructSignatureDecl(construct) => {
                for param in &construct.params {
                    self.walk_ts_fn_param(param);
                }
                if let Some(type_ann) = &construct.type_ann {
                    self.walk_ts_type(&type_ann.type_ann);
                }
            }
            swc_ecma_ast::TsTypeElement::TsIndexSignature(idx) => {
                for param in &idx.params {
                    self.walk_ts_fn_param(param);
                }
                if let Some(type_ann) = &idx.type_ann {
                    self.walk_ts_type(&type_ann.type_ann);
                }
            }
            swc_ecma_ast::TsTypeElement::TsGetterSignature(getter) => {
                if let Some(type_ann) = &getter.type_ann {
                    self.walk_ts_type(&type_ann.type_ann);
                }
            }
            swc_ecma_ast::TsTypeElement::TsSetterSignature(setter) => {
                self.walk_ts_fn_param(&setter.param);
            }
        }
    }

    fn walk_ts_fn_param(&mut self, param: &swc_ecma_ast::TsFnParam) {
        match param {
            swc_ecma_ast::TsFnParam::Ident(ident) => {
                if let Some(type_ann) = &ident.type_ann {
                    self.walk_ts_type(&type_ann.type_ann);
                }
            }
            swc_ecma_ast::TsFnParam::Array(arr) => self.walk_pat(&swc_ecma_ast::Pat::Array(arr.clone())),
            swc_ecma_ast::TsFnParam::Object(obj) => self.walk_pat(&swc_ecma_ast::Pat::Object(obj.clone())),
            swc_ecma_ast::TsFnParam::Rest(rest) => {
                self.walk_pat(&swc_ecma_ast::Pat::Rest(rest.clone()));
            }
        }
    }

    fn walk_simple_assign_target(&mut self, target: &swc_ecma_ast::SimpleAssignTarget) {
        match target {
            swc_ecma_ast::SimpleAssignTarget::Ident(ident) => {
                if let Some(type_ann) = &ident.type_ann {
                    self.walk_ts_type(&type_ann.type_ann);
                }
            }
            swc_ecma_ast::SimpleAssignTarget::Member(member) => {
                self.walk_expr(&member.obj);
                if let MemberProp::Computed(comp) = &member.prop {
                    self.walk_expr(&comp.expr);
                }
            }
            swc_ecma_ast::SimpleAssignTarget::SuperProp(sup) => {
                if let swc_ecma_ast::SuperProp::Computed(comp) = &sup.prop {
                    self.walk_expr(&comp.expr);
                }
            }
            swc_ecma_ast::SimpleAssignTarget::Paren(paren) => {
                self.walk_expr(&paren.expr);
            }
            swc_ecma_ast::SimpleAssignTarget::OptChain(opt) => {
                self.walk_expr(&Expr::OptChain(opt.clone()));
            }
            swc_ecma_ast::SimpleAssignTarget::TsAs(as_expr) => {
                self.walk_ts_type(&as_expr.type_ann);
                self.walk_expr(&as_expr.expr);
            }
            swc_ecma_ast::SimpleAssignTarget::TsSatisfies(sat) => {
                self.walk_ts_type(&sat.type_ann);
                self.walk_expr(&sat.expr);
            }
            swc_ecma_ast::SimpleAssignTarget::TsNonNull(non_null) => {
                self.walk_expr(&non_null.expr);
            }
            swc_ecma_ast::SimpleAssignTarget::TsTypeAssertion(assertion) => {
                self.walk_ts_type(&assertion.type_ann);
                self.walk_expr(&assertion.expr);
            }
            swc_ecma_ast::SimpleAssignTarget::TsInstantiation(inst) => {
                self.walk_expr(&inst.expr);
            }
            swc_ecma_ast::SimpleAssignTarget::Invalid(_) => {}
        }
    }

    fn walk_assign_target_pat(&mut self, pat: &swc_ecma_ast::AssignTargetPat) {
        match pat {
            swc_ecma_ast::AssignTargetPat::Array(arr) => {
                for _elem in arr.elems.iter().flatten() {
                    self.walk_pat(&swc_ecma_ast::Pat::Array(arr.clone()));
                }
            }
            swc_ecma_ast::AssignTargetPat::Object(obj) => {
                for prop in &obj.props {
                    match prop {
                        swc_ecma_ast::ObjectPatProp::KeyValue(kv) => {
                            self.walk_pat(&kv.value);
                        }
                        swc_ecma_ast::ObjectPatProp::Assign(assign) => {
                            if let Some(value) = &assign.value {
                                self.walk_expr(value);
                            }
                        }
                        swc_ecma_ast::ObjectPatProp::Rest(rest) => {
                            self.walk_pat(&rest.arg);
                        }
                    }
                }
            }
            swc_ecma_ast::AssignTargetPat::Invalid(_) => {}
        }
    }

    fn walk_ts_type_query_expr(&mut self, expr: &swc_ecma_ast::TsTypeQueryExpr) {
        match expr {
            swc_ecma_ast::TsTypeQueryExpr::TsEntityName(_) => {}
            swc_ecma_ast::TsTypeQueryExpr::Import(import) => {
                if let Some(type_args) = &import.type_args {
                    for param in &type_args.params {
                        self.walk_ts_type(param);
                    }
                }
            }
        }
    }

    fn walk_ts_namespace_body(&mut self, body: &swc_ecma_ast::TsNamespaceBody) {
        match body {
            swc_ecma_ast::TsNamespaceBody::TsModuleBlock(block) => {
                for item in &block.body {
                    self.walk_module_item(item);
                }
            }
            swc_ecma_ast::TsNamespaceBody::TsNamespaceDecl(decl) => {
                self.walk_ts_namespace_body(&decl.body);
            }
        }
    }

    // ------------------------------------------------------------------
    // Helpers
    // ------------------------------------------------------------------

    /// Check if a computed member expression is allowed array-like indexing.
    /// Allows numeric literals and simple identifiers on array-like objects.
    fn is_allowed_indexing(&self, obj: &Expr, index: &Expr) -> bool {
        // Numeric literal indexing is always allowed (e.g., arr[0])
        if matches!(index, Expr::Lit(Lit::Num(_))) {
            return true;
        }

        // Simple identifier indexing on array-like objects is allowed
        // (e.g., arr[i], items[idx], values[mid])
        if let Expr::Ident(ident) = index {
            let name = ident.sym.as_ref();
            if matches!(name, "i" | "j" | "k" | "idx" | "index" | "mid" | "pos") {
                return true;
            }
        }

        // Allow any indexing on objects that look like arrays
        if let Expr::Ident(ident) = obj {
            let name = ident.sym.as_ref();
            if name.ends_with('s')
                || name.ends_with("arr")
                || name.ends_with("list")
                || name.ends_with("items")
                || name.ends_with("values")
                || name.ends_with("array")
                || name.ends_with("keys")
            {
                return true;
            }
        }

        false
    }

    /// Check if an expression is an implicit coercion pattern.
    fn check_implicit_coercion(&mut self, expr: &Expr) {
        let is_falsy_literal = match expr {
            Expr::Lit(Lit::Null(_)) => true,
            Expr::Lit(Lit::Num(n)) => n.value == 0.0,
            Expr::Lit(Lit::Str(s)) => s.value.is_empty(),
            Expr::Lit(Lit::Bool(_)) => true,
            _ => false,
        };
        if is_falsy_literal {
            self.push_error(
                "no-implicit-coercion",
                "Implicit boolean coercion forbidden. Use explicit comparison (e.g., s.is_empty())",
                expr.span().lo.0,
            );
        }
    }

    /// Push a validation error.
    fn push_error(&mut self, code: &'static str, message: &str, byte_offset: u32) {
        let (line, column) = self
            .source
            .as_ref()
            .map_or((1, 0), |s| s.location_from_offset(byte_offset));
        self.errors.push(ValidationError {
            code,
            message: message.to_string(),
            line,
            column,
        });
    }

    /// Get all validation errors.
    #[must_use]
    pub fn errors(&self) -> &[ValidationError] {
        &self.errors
    }
}

impl Default for SubsetValidator {
    fn default() -> Self {
        Self::new()
    }
}
