//! # Rust Emitter
//!
//! Core transpilation from TypeScript AST to Rust source.

use std::collections::HashMap;
use swc_ecma_ast::*;
use crate::{parser::SourceFile, analyzer::{AnalysisResult, TypeInfo, BorrowMode}};
use super::{GeneratedModule, Import, ImportedName, CodegenOptions};

/// Options for code emission.
#[derive(Debug, Clone)]
pub struct EmitOptions {
    /// Generate source maps
    pub source_map: bool,
    /// Pretty print output
    pub pretty: bool,
}

impl Default for EmitOptions {
    fn default() -> Self {
        Self {
            source_map: false,
            pretty: true,
        }
    }
}

/// Emits Rust code from TypeScript AST.
pub struct RustEmitter<'a> {
    source: &'a SourceFile,
    analysis: &'a AnalysisResult,
    imports: Vec<Import>,
    output: String,
    indent: usize,
    options: EmitOptions,
}

impl<'a> RustEmitter<'a> {
    /// Create a new emitter.
    pub fn new(source: &'a SourceFile, analysis: &'a AnalysisResult) -> Self {
        Self {
            source,
            analysis,
            imports: Vec::new(),
            output: String::new(),
            indent: 0,
            options: EmitOptions::default(),
        }
    }

    /// Emit the complete module.
    pub fn emit(mut self) -> crate::Result<GeneratedModule> {
        self.write_header()?;
        self.emit_module_items()?;
        self.write_footer()?;

        let name = self.source.path.file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("module")
            .to_string();

        Ok(GeneratedModule {
            name,
            source: self.output,
            imports: self.imports,
        })
    }

    /// Write module header.
    fn write_header(&mut self) -> crate::Result<()> {
        self.push_line("use std::collections::HashMap;");
        self.push_line("use std::fmt::{self, Write};");
        self.push_line("");
        Ok(())
    }

    /// Write module footer.
    fn write_footer(&mut self) -> crate::Result<()> {
        // Footer if needed
        Ok(())
    }

    /// Emit all module items.
    fn emit_module_items(&mut self) -> crate::Result<()> {
        for item in &self.source.module.body {
            self.emit_module_item(item)?;
            self.push_line("");
        }
        Ok(())
    }

    /// Emit a single module item.
    fn emit_module_item(&mut self, item: &ModuleItem) -> crate::Result<()> {
        match item {
            ModuleItem::Stmt(Stmt::Decl(decl)) => self.emit_decl(decl),
            ModuleItem::ModuleDecl(decl) => self.emit_module_decl(decl),
            ModuleItem::Stmt(stmt) => self.emit_stmt(stmt),
            _ => Ok(()),
        }
    }

    /// Emit a declaration.
    fn emit_decl(&mut self, decl: &Decl) -> crate::Result<()> {
        match decl {
            Decl::Fn(f) => self.emit_fn(f),
            Decl::Var(v) => self.emit_var_decl(v),
            Decl::TsTypeAlias(t) => self.emit_type_alias(t),
            Decl::TsEnum(e) => self.emit_enum(e),
            Decl::TsInterface(i) => self.emit_interface(i),
            _ => Ok(()),
        }
    }

    /// Emit a function.
    fn emit_fn(&mut self, f: &FnDecl) -> crate::Result<()> {
        let name = self.mangle(&f.ident.sym.to_string());
        
        // Determine async
        if f.function.is_async {
            self.push("pub async ");
        } else {
            self.push("pub ");
        }

        // Function name
        self.push(format!("fn {name}("));

        // Parameters
        for (i, param) in f.function.params.iter().enumerate() {
            if i > 0 {
                self.push(", ");
            }
            self.emit_fn_param(param)?;
        }
        self.push(")");

        // Return type
        if let Some(ret) = &f.function.return_type {
            self.push(" -> ");
            self.emit_type(&ret.type_ann)?;
        }

        // Body
        if let Some(body) = &f.function.body {
            self.push(" ");
            self.emit_block_stmt(body)?;
        } else {
            self.push(";")?;
        }

        Ok(())
    }

    /// Emit function parameters.
    fn emit_fn_param(&mut self, param: &Param) -> crate::Result<()> {
        let name = param.pat.as_ident()
            .map(|i| self.mangle(&i.id.sym.to_string()))
            .unwrap_or_else(|| "_".to_string());

        // Determine borrow mode
        let mode = self.analysis.ownership.get(&name).copied().unwrap_or(BorrowMode::Unknown);
        let prefix = match mode {
            BorrowMode::Mut => "&mut ",
            BorrowMode::Shared => "&",
            BorrowMode::Owned => "",
            BorrowMode::Unknown => "",
        };

        // Type annotation
        if let Some(type_ann) = param.pat.as_ident().and_then(|i| i.type_ann.as_ref()) {
            self.push(format!("{prefix}{name}: "));
            self.emit_type(&type_ann.type_ann)?;
        } else {
            self.push(format!("{prefix}{name}"));
        }

        Ok(())
    }

    /// Emit a block statement.
    fn emit_block_stmt(&mut self, block: &BlockStmt) -> crate::Result<()> {
        self.push_line("{")?;
        self.indent += 1;
        for stmt in &block.stmts {
            self.emit_stmt(stmt)?;
            if !matches!(stmt, Stmt::Empty(_)) {
                self.push_line(";")?;
            }
        }
        self.indent -= 1;
        self.push("}")?;
        Ok(())
    }

    /// Emit a statement.
    fn emit_stmt(&mut self, stmt: &Stmt) -> crate::Result<()> {
        self.push_indent();
        match stmt {
            Stmt::Expr(e) => self.emit_expr(&e.expr)?,
            Stmt::If(i) => self.emit_if(i)?,
            Stmt::While(w) => self.emit_while(w)?,
            Stmt::For(f) => self.emit_for(f)?,
            Stmt::ForOf(f) => self.emit_for_of(f)?,
            Stmt::DoWhile(d) => self.emit_do_while(d)?,
            Stmt::Switch(s) => self.emit_switch(s)?,
            Stmt::Block(b) => self.emit_block_stmt(b)?,
            Stmt::Return(r) => self.emit_return(r)?,
            Stmt::Break(_) => self.push("break")?,
            Stmt::Continue(_) => self.push("continue")?,
            Stmt::Empty(_) => self.push("/* empty */")?,
            Stmt::Debugger(_) => self.push("unimplemented!()")?,
            Stmt::Labeled(l) => {
                self.push(format!("{}: ", l.label.sym));
                self.emit_stmt(&l.body)?;
            }
            Stmt::Decl(d) => self.emit_decl(d)?,
            Stmt::Try(_) | Stmt::Throw(_) | Stmt::With(_) => {
                self.push("unreachable!()")?;
            }
        }
        Ok(())
    }

    /// Emit a variable declaration.
    fn emit_var_decl(&mut self, v: &VarDecl) -> crate::Result<()> {
        let keyword = match v.kind {
            VarDeclKind::Const => "let",
            VarDeclKind::Let => "let mut",
            VarDeclKind::Var => "let mut",
        };

        for (i, decl) in v.decls.iter().enumerate() {
            if i > 0 {
                self.push_line(";")?;
                self.push_indent();
            }
            self.push(format!("{} ", keyword));
            self.emit_pat(&decl.name)?;
            if let Some(init) = &decl.init {
                self.push(" = ");
                self.emit_expr(init)?;
            }
        }
        Ok(())
    }

    /// Emit a pattern.
    fn emit_pat(&mut self, pat: &Pat) -> crate::Result<()> {
        match pat {
            Pat::Ident(i) => {
                let name = self.mangle(&i.id.sym.to_string());
                self.push(name);
                if let Some(type_ann) = &i.type_ann {
                    self.push(": ");
                    self.emit_type(&type_ann.type_ann)?;
                }
            }
            Pat::Array(a) => {
                self.push("[");
                for (i, elem) in a.elems.iter().enumerate() {
                    if i > 0 { self.push(", "); }
                    if let Some(p) = elem {
                        self.emit_pat(p)?;
                    }
                }
                self.push("]");
            }
            Pat::Object(o) => {
                self.push("{");
                for (i, prop) in o.props.iter().enumerate() {
                    if i > 0 { self.push(", "); }
                    match prop {
                        ObjectPatProp::Assign(a) => {
                            self.push(&a.key.sym.to_string());
                            if let Some(v) = &a.value {
                                self.push(" = ");
                                self.emit_expr(&Expr::Paren(ParenExpr {
                                    span: Default::default(),
                                    expr: Box::new(v.clone()),
                                }))?;
                            }
                        }
                        ObjectPatProp::KeyValue(kv) => {
                            self.emit_expr(&Expr::Ident(kv.key.clone().into()))?;
                            self.push(": ");
                            self.emit_pat(&kv.value)?;
                        }
                        ObjectPatProp::Rest(r) => {
                            self.push("..");
                            self.emit_pat(&r.arg)?;
                        }
                    }
                }
                self.push("}");
            }
            Pat::Rest(r) => {
                self.push("..");
                self.emit_pat(&r.arg)?;
            }
            Pat::Assign(a) => {
                self.emit_pat(&a.left)?;
                self.push(" = ");
                self.emit_expr(&a.right)?;
            }
            _ => self.push("_"),
        }
        Ok(())
    }

    /// Emit an expression.
    fn emit_expr(&mut self, expr: &Expr) -> crate::Result<()> {
        match expr {
            Expr::Lit(l) => self.emit_lit(l)?,
            Expr::Ident(i) => {
                let name = self.mangle(&i.sym.to_string());
                self.push(name);
            }
            Expr::Bin(b) => self.emit_bin_expr(b)?,
            Expr::Unary(u) => self.emit_unary(u)?,
            Expr::Update(u) => self.emit_update(u)?,
            Expr::Assign(a) => self.emit_assign(a)?,
            Expr::Member(m) => self.emit_member(m)?,
            Expr::Call(c) => self.emit_call(c)?,
            Expr::Arrow(a) => self.emit_arrow(a)?,
            Expr::Fn(f) => self.emit_fn_expr(f)?,
            Expr::Cond(c) => self.emit_cond(c)?,
            Expr::Array(a) => self.emit_array(a)?,
            Expr::Object(o) => self.emit_object(o)?,
            Expr::Paren(p) => {
                self.push("(");
                self.emit_expr(&p.expr)?;
                self.push(")");
            }
            Expr::Seq(s) => {
                for (i, e) in s.exprs.iter().enumerate() {
                    if i > 0 { self.push(", "); }
                    self.emit_expr(e)?;
                }
            }
            Expr::Tpl(t) => self.emit_tpl(t)?,
            Expr::TaggedTpl(t) => {
                self.emit_expr(&t.tag)?;
                self.emit_tpl(&t.tpl)?;
            }
            Expr::Await(a) => {
                self.push("(");
                self.push("async { ");
                self.emit_expr(&a.arg)?;
                self.push(".await");
                self.push(" })()");
            }
            Expr::TsTypeAssertion(t) => self.emit_expr(&t.expr)?,
            Expr::TsAs(t) => {
                // Type assertion - emit expression, type is for reference
                self.emit_expr(&t.expr)?;
            }
            Expr::TsNonNull(t) => self.emit_expr(&t.expr)?,
            Expr::TsSatisfies(t) => self.emit_expr(&t.expr)?,
            Expr::This(_) => self.push("/* this */ unimplemented!()")?,
            Expr::Super(_) => self.push("unimplemented!()")?,
            Expr::New(n) => self.emit_call(&CallExpr {
                span: n.span,
                callee: Box::new(Expr::Ident(Ident {
                    span: Default::default(),
                    sym: n.callee.value.as_str().into(),
                    optional: false,
                })),
                args: n.args.clone(),
                type_args: None,
            })?,
            Expr::Class(_) => self.push("unimplemented!() /* class */")?,
            Expr::Yield(_) => self.push("unimplemented!() /* yield */")?,
            Expr::MetaProp(_) => self.push("unimplemented!()")?,
            Expr::Invalid(_) => self.push("unreachable!()")?,
            Expr::Jsx(j) => self.emit_jsx(j)?,
        }
        Ok(())
    }

    /// Emit a literal.
    fn emit_lit(&mut self, lit: &Lit) -> crate::Result<()> {
        match lit {
            Lit::Null(_) => self.push("()"),
            Lit::Bool(b) => self.push(if b.value { "true" } else { "false" }),
            Lit::Num(n) => {
                if n.value.fract() == 0.0 {
                    self.push(format!("{}", n.value as i32));
                } else {
                    self.push(format!("{}", n.value));
                }
            }
            Lit::Str(s) => self.push(format!("{:?}", s.value)),
            Lit::BigInt(b) => self.push(format!("{}i64", b.value)),
            Lit::Regex(_) => self.push("Regex::new(\"...\").unwrap()")?,
            Lit::JSXText(t) => self.push(format!("{:?}", t.value)),
        }
        Ok(())
    }

    /// Emit a binary expression.
    fn emit_bin_expr(&mut self, bin: &BinExpr) -> crate::Result<()> {
        let rust_op = match bin.op {
            BinaryOp::Add => "+",
            BinaryOp::Sub => "-",
            BinaryOp::Mul => "*",
            BinaryOp::Div => "/",
            BinaryOp::Mod => "%",
            BinaryOp::EqEq | BinaryOp::EqEqEq => "==",
            BinaryOp::NotEq | BinaryOp::NotEqEq => "!=",
            BinaryOp::Lt => "<",
            BinaryOp::Le => "<=",
            BinaryOp::Gt => ">",
            BinaryOp::Ge => ">=",
            BinaryOp::LogicalAnd => "&&",
            BinaryOp::LogicalOr => "||",
            BinaryOp::BinAnd => "&",
            BinaryOp::BinOr => "|",
            BinaryOp::BinXor => "^",
            BinaryOp::LShift => "<<",
            BinaryOp::RShift => ">>",
            BinaryOp::ZeroFillRShift => ">>",
            BinaryOp::Exp => "f64::powf",
            BinaryOp::NullishCoalescing => "unwrap_or",
        };

        if bin.op == BinaryOp::Exp {
            self.push("f64::powf(")?;
            self.emit_expr(&bin.left)?;
            self.push(", ")?;
            self.emit_expr(&bin.right)?;
            self.push(")")?;
            return Ok(());
        }

        if bin.op == BinaryOp::NullishCoalescing {
            self.emit_expr(&bin.left)?;
            self.push(".unwrap_or(")?;
            self.emit_expr(&bin.right)?;
            self.push(")")?;
            return Ok(());
        }

        self.push("(")?;
        self.emit_expr(&bin.left)?;
        self.push(format!(" {} ", rust_op))?;
        self.emit_expr(&bin.right)?;
        self.push(")")?;
        Ok(())
    }

    /// Emit a unary expression.
    fn emit_unary(&mut self, u: &UnaryExpr) -> crate::Result<()> {
        let rust_op = match u.op {
            UnaryOp::Minus => "-",
            UnaryOp::Plus => "",
            UnaryOp::Bang => "!",
            UnaryOp::Tilde => "!",
            UnaryOp::TypeOf => "/* typeof */ unimplemented!()",
            UnaryOp::Void => "()",
        };

        if u.op == UnaryOp::TypeOf {
            self.push(rust_op)?;
            return Ok(());
        }

        if u.op == UnaryOp::Void {
            self.push("()")?;
            return Ok(());
        }

        self.push(rust_op)?;
        self.emit_expr(&u.arg)?;
        Ok(())
    }

    /// Emit an update expression.
    fn emit_update(&mut self, u: &UpdateExpr) -> crate::Result<()> {
        let op = if u.is_prefix { 
            match u.op {
                UpdateOp::PlusPlus => "++",
                UpdateOp::MinusMinus => "--",
            }
        } else {
            match u.op {
                UpdateOp::PlusPlus => "++",
                UpdateOp::MinusMinus => "--",
            }
        };

        if u.is_prefix {
            self.push(op)?;
            self.emit_expr(&u.arg)?;
        } else {
            self.emit_expr(&u.arg)?;
            self.push(op)?;
        }
        Ok(())
    }

    /// Emit an assignment expression.
    fn emit_assign(&mut self, a: &AssignExpr) -> crate::Result<()> {
        let rust_op = match a.op {
            AssignOp::Assign => "=",
            AssignOp::AddAssign => "+=",
            AssignOp::SubAssign => "-=",
            AssignOp::MulAssign => "*=",
            AssignOp::DivAssign => "/=",
            AssignOp::ModAssign => "%=",
            _ => "=",
        };

        self.emit_expr(&a.left)?;
        self.push(format!(" {} ", rust_op))?;
        self.emit_expr(&a.value)?;
        Ok(())
    }

    /// Emit a member expression.
    fn emit_member(&mut self, m: &MemberExpr) -> crate::Result<()> {
        self.emit_expr(&m.obj)?;
        
        if m.computed {
            self.push("[")?;
            self.emit_expr(&m.prop)?;
            self.push("]")?;
        } else {
            self.push(".")?;
            match &m.prop {
                Expr::Ident(i) => self.push(&i.sym.to_string()),
                _ => self.emit_expr(&m.prop)?,
            }
        }
        Ok(())
    }

    /// Emit a call expression.
    fn emit_call(&mut self, c: &CallExpr) -> crate::Result<()> {
        self.emit_expr(&c.callee)?;
        self.push("(")?;
        for (i, arg) in c.args.iter().enumerate() {
            if i > 0 { self.push(", "); }
            self.emit_expr(&arg.expr)?;
        }
        self.push(")")?;
        Ok(())
    }

    /// Emit an arrow function.
    fn emit_arrow(&mut self, a: &ArrowExpr) -> crate::Result<()> {
        // Collect parameter names
        let params: Vec<String> = a.params.iter().filter_map(|p| {
            p.pat.as_ident().map(|i| self.mangle(&i.id.sym.to_string()))
        }).collect();

        if a.is_async {
            self.push("async { |")?;
        } else {
            self.push("|| {")?;
        }
        self.push(params.join(", "));
        self.push("| ")?;

        match &a.body {
            BlockStmtOrExpr::BlockStmt(b) => {
                for stmt in &b.stmts {
                    self.emit_stmt(stmt)?;
                    self.push_line(";")?;
                }
            }
            BlockStmtOrExpr::Expr(e) => {
                self.push("(")?;
                self.emit_expr(e)?;
                self.push(")")?;
            }
        }

        self.push("}")?;
        Ok(())
    }

    /// Emit a function expression.
    fn emit_fn_expr(&mut self, f: &FnExpr) -> crate::Result<()> {
        self.push("|")?;
        let params: Vec<String> = f.function.params.iter().filter_map(|p| {
            p.pat.as_ident().map(|i| self.mangle(&i.id.sym.to_string()))
        }).collect();
        self.push(params.join(", "));
        self.push("| ")?;

        if let Some(body) = &f.function.body {
            self.emit_block_stmt(body)?;
        }
        Ok(())
    }

    /// Emit a conditional expression.
    fn emit_cond(&mut self, c: &CondExpr) -> crate::Result<()> {
        self.emit_expr(&c.test)?;
        self.push(" ? ")?;
        self.emit_expr(&c.cons)?;
        self.push(" : ")?;
        self.emit_expr(&c.alt)?;
        Ok(())
    }

    /// Emit an array literal.
    fn emit_array(&mut self, a: &ArrayExpr) -> crate::Result<()> {
        self.push("vec![")?;
        for (i, elem) in a.elems.iter().enumerate() {
            if i > 0 { self.push(", "); }
            if let Some(e) = elem {
                self.emit_expr(&e.expr)?;
            }
        }
        self.push("]")?;
        Ok(())
    }

    /// Emit an object literal.
    fn emit_object(&mut self, o: &ObjectExpr) -> crate::Result<()> {
        self.push("__rune_obj!({")?;
        for (i, prop) in o.props.iter().enumerate() {
            if i > 0 { self.push(", "); }
            match prop {
                PropOrSpread::Prop(p) => self.emit_prop(p)?,
                PropOrSpread::Spread(s) => {
                    self.push("..")?;
                    self.emit_expr(&s.expr)?;
                }
            }
        }
        self.push("})")?;
        Ok(())
    }

    /// Emit a property.
    fn emit_prop(&mut self, prop: &Prop) -> crate::Result<()> {
        match prop {
            Prop::Shorthand(i) => self.push(&self.mangle(&i.sym.to_string())),
            Prop::KeyValue(kv) => {
                // Key
                match &kv.key {
                    PropName::Str(s) => self.push(format!("{:?}: ", s.value)),
                    PropName::Ident(i) => self.push(format!("{}: ", i.sym)),
                    PropName::Num(n) => self.push(format!("{}: ", n.value)),
                    PropName::Computed(c) => {
                        self.push("[")?;
                        self.emit_expr(&c.expr)?;
                        self.push("]: ")?;
                    }
                    _ => {}
                }
                // Value
                self.emit_expr(&kv.value)?;
            }
            Prop::Assign(a) => {
                self.push(&self.mangle(&a.key.sym.to_string()))?;
                self.push(" = ")?;
                self.emit_expr(&a.value)?;
            }
            Prop::Getter(g) => {
                self.push("get ")?;
                if let PropName::Ident(i) = &g.key {
                    self.push(&i.sym.to_string());
                }
                self.push("() ")?;
                if let Some(body) = &g.body {
                    self.emit_block_stmt(body)?;
                }
            }
            Prop::Setter(s) => {
                self.push("set ")?;
                if let PropName::Ident(i) = &s.key {
                    self.push(&i.sym.to_string());
                }
                self.push("(")?;
                if let Some(param) = &s.param {
                    self.emit_pat(param)?;
                }
                self.push(") ")?;
                if let Some(body) = &s.body {
                    self.emit_block_stmt(body)?;
                }
            }
            Prop::Method(m) => {
                if let PropName::Ident(i) = &m.key {
                    self.push(&i.sym.to_string());
                }
                self.push("(")?;
                // Parameters
                self.push(") ")?;
                if let Some(body) = &m.function.body {
                    self.emit_block_stmt(body)?;
                }
            }
        }
        Ok(())
    }

    /// Emit a template literal.
    fn emit_tpl(&mut self, t: &TplExpr) -> crate::Result<()> {
        self.push("format!(\"")?;
        for (i, part) in t.quasis.iter().enumerate() {
            self.push(&part.raw.value.to_string().replace("{", "{{").replace("}", "}}"));
            if i < t.exprs.len() {
                self.push("{}")?;
            }
        }
        self.push("\", ")?;
        for (i, expr) in t.exprs.iter().enumerate() {
            if i > 0 { self.push(", "); }
            self.emit_expr(expr)?;
        }
        self.push(")")?;
        Ok(())
    }

    /// Emit an if statement.
    fn emit_if(&mut self, i: &IfStmt) -> crate::Result<()> {
        self.push("if ")?;
        self.emit_expr(&i.test)?;
        self.push(" ")?;
        self.emit_stmt(&i.cons)?;
        if let Some(alt) = &i.alt {
            self.push(" else ")?;
            self.emit_stmt(alt)?;
        }
        Ok(())
    }

    /// Emit a while loop.
    fn emit_while(&mut self, w: &WhileStmt) -> crate::Result<()> {
        self.push("while ")?;
        self.emit_expr(&w.test)?;
        self.push(" ")?;
        self.emit_stmt(&w.body)?;
        Ok(())
    }

    /// Emit a for loop.
    fn emit_for(&mut self, f: &ForStmt) -> crate::Result<()> {
        self.push("for ")?;
        if let Some(init) = &f.init {
            match init {
                VarDeclOrExpr::VarDecl(v) => {
                    self.emit_var_decl(v)?;
                }
                VarDeclOrExpr::Expr(e) => {
                    self.emit_expr(e)?;
                }
            }
        }
        if let Some(test) = &f.test {
            self.emit_expr(test)?;
        }
        self.push("; ")?;
        if let Some(update) = &f.update {
            self.emit_expr(update)?;
        }
        self.push(" ")?;
        self.emit_stmt(&f.body)?;
        Ok(())
    }

    /// Emit a for-of loop.
    fn emit_for_of(&mut self, f: &ForOfStmt) -> crate::Result<()> {
        self.push("for ")?;
        self.emit_expr(&f.left)?;
        self.push(" in ")?;
        self.emit_expr(&f.right)?;
        self.push(" ")?;
        self.emit_stmt(&f.body)?;
        Ok(())
    }

    /// Emit a do-while loop.
    fn emit_do_while(&mut self, d: &DoWhileStmt) -> crate::Result<()> {
        self.push("loop { ")?;
        self.emit_stmt(&d.body)?;
        self.push("if !(")?;
        self.emit_expr(&d.test)?;
        self.push(") { break; } }")?;
        Ok(())
    }

    /// Emit a switch statement.
    fn emit_switch(&mut self, s: &SwitchStmt) -> crate::Result<()> {
        self.push("match ")?;
        self.emit_expr(&s.discriminant)?;
        self.push_line(" {")?;
        for case in &s.cases {
            for item in &case.cons {
                if let Stmt::Expr(e) = item {
                    if let Expr::Member(m) = &*e.expr {
                        // Pattern: msg.tag === "Move"
                        if let Expr::Ident(tag_ident) = &m.obj {
                            if let Expr::Lit(Lit::Str(s)) = &*m.prop {
                                self.push_indent();
                                self.push(format!("{}::{}{} => ", 
                                    self.mangle(&tag_ident.sym.to_string()).to_uppercase(),
                                    self.mangle(&tag_ident.sym.to_string()),
                                    self.pascal_case(&s.value)
                                ))?;
                                // Find the body statements (everything after the comparison)
                                self.emit_switch_body(&case.cons)?;
                                break;
                            }
                        }
                    }
                }
            }
        }
        self.push_line("}")?;
        Ok(())
    }

    /// Emit switch case body.
    fn emit_switch_body(&mut self, cons: &[Stmt]) -> crate::Result<()> {
        // Skip the first statement (the comparison), emit the rest
        for (i, stmt) in cons.iter().enumerate().skip(1) {
            if i > 1 { self.push_line(";")?; }
            self.emit_stmt(stmt)?;
        }
        Ok(())
    }

    /// Emit a return statement.
    fn emit_return(&mut self, r: &ReturnStmt) -> crate::Result<()> {
        self.push("return ")?;
        if let Some(value) = &r.value {
            self.emit_expr(value)?;
        }
        Ok(())
    }

    /// Emit a type alias.
    fn emit_type_alias(&mut self, t: &TsTypeAliasDecl) -> crate::Result<()> {
        self.push(format!("pub type {} = ", self.mangle(&t.id.sym.to_string())))?;
        self.emit_ts_type(&t.type_ann)?;
        self.push(";")?;
        Ok(())
    }

    /// Emit a TypeScript enum.
    fn emit_enum(&mut self, e: &TsEnumDecl) -> crate::Result<()> {
        self.push(format!("pub enum {}", self.mangle(&e.id.sym.to_string())))?;
        self.push_line(" {")?;
        for member in &e.members {
            let tag = match &member.id {
                TsEnumMemberId::Str(s) => s.value.to_string(),
                TsEnumMemberId::Computed(_) => continue,
            };
            self.push_indent();
            self.push(format!("{},", self.pascal_case(&tag)))?;
            self.push_line("")?;
        }
        self.push("}")?;
        Ok(())
    }

    /// Emit an interface.
    fn emit_interface(&mut self, i: &TsInterfaceDecl) -> crate::Result<()> {
        self.push(format!("pub struct {} ", self.mangle(&i.id.sym.to_string())))?;
        self.emit_block_stmt(&BlockStmt {
            span: Default::default(),
            stmts: i.body.body.iter().map(|m| {
                Stmt::Decl(Decl::Var(VarDecl {
                    span: Default::default(),
                    kind: VarDeclKind::Const,
                    decls: vec![VarDeclarator {
                        span: Default::default(),
                        name: match &m {
                            TsClassMember::Property(p) => {
                                Pat::Ident(BindingIdent {
                                    id: Ident {
                                        span: Default::default(),
                                        sym: p.key.as_str().unwrap_or_default().into(),
                                        optional: false,
                                    },
                                    type_ann: None,
                                })
                            }
                            _ => Pat::Invalid(Invalid { span: Default::default() }),
                        },
                        init: None,
                        definite: false,
                    }],
                    init: None,
                }))
            }).collect(),
        })?;
        Ok(())
    }

    /// Emit a module declaration.
    fn emit_module_decl(&mut self, decl: &ModuleDecl) -> crate::Result<()> {
        match decl {
            ModuleDecl::Import(i) => self.emit_import(i),
            ModuleDecl::Export(e) => self.emit_export(e),
            _ => Ok(()),
        }
    }

    /// Emit an import.
    fn emit_import(&mut self, import: &ImportDecl) -> crate::Result<()> {
        let module_name = import.src.value.to_string();
        
        if module_name.starts_with("native:") {
            // Native import: native:bar -> crate::native::bar
            let path = module_name.strip_prefix("native:").unwrap_or(&module_name);
            let import = Import {
                path: format!("crate::native::{}", path.replace(".", "::")),
                names: import.specifiers.iter().filter_map(|s| {
                    match s {
                        ImportSpecifier::Named(n) => Some(ImportedName {
                            original: n.local.sym.to_string(),
                            rust_name: self.snake_case(&n.local.sym.to_string()),
                        }),
                        ImportSpecifier::Default(n) => Some(ImportedName {
                            original: "default".to_string(),
                            rust_name: n.sym.to_string(),
                        }),
                        ImportSpecifier::Namespace(n) => Some(ImportedName {
                            original: "*".to_string(),
                            rust_name: n.sym.to_string(),
                        }),
                    }
                }).collect(),
                is_native: true,
            };
            self.imports.push(import);
        } else if module_name.starts_with("./") || module_name.starts_with("../") {
            // Relative import
            let import = Import {
                path: format!("crate::generated::{}", module_name.replace("./", "").replace(".r.ts", "").replace(".r.tsx", "")),
                names: import.specifiers.iter().filter_map(|s| {
                    match s {
                        ImportSpecifier::Named(n) => Some(ImportedName {
                            original: n.local.sym.to_string(),
                            rust_name: self.snake_case(&n.local.sym.to_string()),
                        }),
                        _ => None,
                    }
                }).collect(),
                is_native: false,
            };
            self.imports.push(import);
        }

        Ok(())
    }

    /// Emit an export.
    fn emit_export(&mut self, export: &ExportDecl) -> crate::Result<()> {
        match export {
            ExportDecl::Decl(d) => self.emit_decl(d),
            ExportDecl::Named(n) => {
                for spec in &n.specifiers {
                    if let ExportSpecifier::Named(ns) = spec {
                        self.push(format!("pub use {};", self.mangle(&ns.orig.sym.to_string())))?;
                    }
                }
                Ok(())
            }
        }
    }

    /// Emit a type annotation.
    fn emit_type(&mut self, ts_type: &TsType) -> crate::Result<()> {
        self.emit_ts_type(ts_type)
    }

    /// Emit a TypeScript type.
    fn emit_ts_type(&mut self, ts_type: &TsType) -> crate::Result<()> {
        match ts_type {
            TsType::TsKeywordType(k) => {
                match k.kind {
                    TsKeywordTypeKind::TsNumberKeyword => self.push("f64"),
                    TsKeywordTypeKind::TsStringKeyword => self.push("String"),
                    TsKeywordTypeKind::TsBooleanKeyword => self.push("bool"),
                    TsKeywordTypeKind::TsNullKeyword => self.push("()"),
                    TsKeywordTypeKind::TsUndefinedKeyword => self.push("()"),
                    TsKeywordTypeKind::TsVoidKeyword => self.push("()"),
                    TsKeywordTypeKind::TsAnyType | TsKeywordTypeKind::TsUnknownType => {
                        self.push("/* unknown */ unimplemented!()")
                    }
                    _ => self.push("()"),
                }
            }
            TsType::TsArrayType(a) => {
                self.emit_ts_type(&a.elem_type)?;
                self.push("Vec<")?;
                self.emit_ts_type(&a.elem_type)?;
                self.push(">")?;
            }
            TsType::TsUnionOrIntersectionType(t) => {
                if t.ts_type_union.is_some() {
                    self.push("/* union */ unimplemented!()")?;
                } else {
                    self.push("/* intersection */ unimplemented!()")?;
                }
            }
            TsType::TsTypeRef(t) => {
                let name = t.type_name.as_str();
                match name {
                    "Array" | "Vec" => {
                        self.push("Vec<")?;
                        if let Some(params) = &t.type_params {
                            if !params.params.is_empty() {
                                self.emit_ts_type(&params.params[0])?;
                            }
                        }
                        self.push(">")?;
                    }
                    "Option" => {
                        self.push("Option<")?;
                        if let Some(params) = &t.type_params {
                            if !params.params.is_empty() {
                                self.emit_ts_type(&params.params[0])?;
                            }
                        }
                        self.push(">")?;
                    }
                    "Result" => {
                        self.push("Result<")?;
                        if let Some(params) = &t.type_params {
                            if params.params.len() >= 2 {
                                self.emit_ts_type(&params.params[0])?;
                                self.push(", ")?;
                                self.emit_ts_type(&params.params[1])?;
                            }
                        }
                        self.push(">")?;
                    }
                    "string" => self.push("String"),
                    "number" => self.push("f64"),
                    "boolean" => self.push("bool"),
                    "void" => self.push("()"),
                    _ => self.push(&self.mangle(name)),
                }
            }
            TsType::TsLiteralType(l) => {
                match &l.lit {
                    TsLit::Str(s) => self.push(format!("&{:?}", s.value)),
                    TsLit::Num(n) => {
                        if n.value.fract() == 0.0 {
                            self.push(format!("{}", n.value as i32));
                        } else {
                            self.push(format!("{}", n.value));
                        }
                    }
                    TsLit::BigInt(b) => self.push(format!("{}i64", b.value)),
                    TsLit::Boolean(b) => self.push(if b.value { "true" } else { "false" }),
                }
            }
            TsType::TsTupleType(t) => {
                self.push("(")?;
                for (i, elem) in t.elem_types.iter().enumerate() {
                    if i > 0 { self.push(", "); }
                    self.emit_ts_type(&elem.ty)?;
                }
                self.push(")")?;
            }
            TsType::TsParenthesizedType(p) => {
                self.emit_ts_type(&p.type_ann)?;
            }
            _ => self.push("()"),
        }
        Ok(())
    }

    /// Emit JSX.
    fn emit_jsx(&mut self, _jsx: &JsxExpr) -> crate::Result<()> {
        self.push("/* JSX */ unimplemented!()")?;
        Ok(())
    }

    // Utility methods

    fn push(&mut self, s: &str) {
        self.output.push_str(s);
    }

    fn push_line(&mut self, s: &str) {
        self.output.push_str(s);
        self.output.push('\n');
    }

    fn push_indent(&mut self) {
        for _ in 0..self.indent {
            self.output.push_str("    ");
        }
    }

    fn mangle(&self, name: &str) -> String {
        if matches!(
            name,
            "as" | "async" | "await" | "break" | "const" | "continue" | "crate" | "dyn"
            | "else" | "enum" | "extern" | "false" | "fn" | "for" | "if" | "impl"
            | "in" | "let" | "loop" | "match" | "mod" | "move" | "mut" | "pub"
            | "ref" | "return" | "self" | "Self" | "static" | "struct" | "super"
            | "trait" | "true" | "type" | "unsafe" | "use" | "where" | "while"
        ) {
            format!("{}_", name)
        } else {
            self.snake_case(name)
        }
    }

    fn snake_case(&self, s: &str) -> String {
        let mut result = String::new();
        for (i, c) in s.chars().enumerate() {
            if c.is_uppercase() && i > 0 {
                result.push('_');
            }
            result.push(c.to_lowercase().next().unwrap_or(c));
        }
        result
    }

    fn pascal_case(&self, s: &str) -> String {
        let mut result = String::new();
        let mut capitalize_next = true;
        for c in s.chars() {
            if c == '_' || c == '-' || c == ' ' {
                capitalize_next = true;
            } else if capitalize_next {
                result.push(c.to_uppercase().next().unwrap_or(c));
                capitalize_next = false;
            } else {
                result.push(c);
            }
        }
        result
    }
}
