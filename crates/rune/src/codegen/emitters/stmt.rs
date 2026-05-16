//! # Statement Emitter
//!
//! Emits Rust code from TypeScript statements.

use swc_ecma_ast::*;
use super::expr::ExprEmitter;
use crate::analyzer::AnalysisResult;

/// Emits Rust code for statements.
pub struct StmtEmitter<'a> {
    /// Output buffer
    output: String,
    /// Current indentation
    indent: usize,
    /// Expression emitter reference
    expr_emitter: ExprEmitter<'a>,
}

impl<'a> StmtEmitter<'a> {
    /// Create a new statement emitter.
    pub fn new(analysis: &'a AnalysisResult) -> Self {
        Self {
            output: String::new(),
            indent: 0,
            expr_emitter: ExprEmitter::new(analysis),
        }
    }

    /// Emit a statement.
    pub fn emit_stmt(&mut self, stmt: &Stmt) {
        self.push_indent();
        match stmt {
            Stmt::Expr(e) => self.emit_expr_stmt(&e.expr),
            Stmt::If(i) => self.emit_if(i),
            Stmt::While(w) => self.emit_while(w),
            Stmt::For(f) => self.emit_for(f),
            Stmt::ForOf(f) => self.emit_for_of(f),
            Stmt::DoWhile(d) => self.emit_do_while(d),
            Stmt::Switch(s) => self.emit_switch(s),
            Stmt::Block(b) => self.emit_block(b),
            Stmt::Return(r) => self.emit_return(r),
            Stmt::Break(_) => self.push("break;"),
            Stmt::Continue(_) => self.push("continue;"),
            Stmt::Empty(_) => self.push("/* empty */;"),
            Stmt::Debugger(_) => self.push("unimplemented!();"),
            Stmt::Labeled(l) => self.emit_labeled(l),
            Stmt::Decl(d) => self.emit_decl(d),
            Stmt::Try(_) | Stmt::Throw(_) | Stmt::With(_) => {
                self.push("unreachable!() /* forbidden */;");
            }
        }
    }

    /// Get the output.
    pub fn into_output(self) -> String {
        self.output
    }

    /// Emit an expression statement.
    fn emit_expr_stmt(&mut self, expr: &Expr) {
        let code = self.expr_emitter.emit_expr(expr);
        self.push(&code);
        self.push_line(";");
    }

    /// Emit an if statement.
    fn emit_if(&mut self, i: &IfStmt) {
        let test = self.expr_emitter.emit_expr(&i.test);
        self.push("if ");
        self.push(&test);
        self.push(" ");
        self.emit_stmt_body(&i.cons);

        if let Some(alt) = &i.alt {
            self.push(" else ");
            self.emit_stmt_body(alt);
        }
    }

    /// Emit a while loop.
    fn emit_while(&mut self, w: &WhileStmt) {
        let test = self.expr_emitter.emit_expr(&w.test);
        self.push("while ");
        self.push(&test);
        self.push(" ");
        self.emit_stmt_body(&w.body);
    }

    /// Emit a for loop.
    fn emit_for(&mut self, f: &ForStmt) {
        self.push("for ");

        if let Some(init) = &f.init {
            match init {
                VarDeclOrExpr::VarDecl(v) => self.emit_var_decl(v),
                VarDeclOrExpr::Expr(e) => {
                    let code = self.expr_emitter.emit_expr(e);
                    self.push(&code);
                }
            }
        }

        if let Some(test) = &f.test {
            let code = self.expr_emitter.emit_expr(test);
            self.push(&code);
        }
        self.push("; ");

        if let Some(update) = &f.update {
            let code = self.expr_emitter.emit_expr(update);
            self.push(&code);
        }

        self.push(" ");
        self.emit_stmt_body(&f.body);
    }

    /// Emit a for-of loop.
    fn emit_for_of(&mut self, f: &ForOfStmt) {
        self.push("for ");
        self.emit_pattern(&f.left);
        self.push(" in ");

        let right = self.expr_emitter.emit_expr(&f.right);
        self.push(&right);
        self.push(" ");
        self.emit_stmt_body(&f.body);
    }

    /// Emit a do-while loop.
    fn emit_do_while(&mut self, d: &DoWhileStmt) {
        self.push("loop { ");
        self.emit_stmt(&d.body);
        self.push("if !(");
        let test = self.expr_emitter.emit_expr(&d.test);
        self.push(&test);
        self.push(") { break; } }");
        self.push_line(";");
    }

    /// Emit a switch statement.
    fn emit_switch(&mut self, s: &SwitchStmt) {
        let disc = self.expr_emitter.emit_expr(&s.discriminant);
        self.push("match ");
        self.push(&disc);
        self.push_line(" {");

        for case in &s.cases {
            for item in &case.cons {
                if let Stmt::Expr(e) = item {
                    if let Expr::Member(m) = &*e.expr {
                        if let Expr::Lit(Lit::Str(s)) = &*m.prop {
                            let var = self.expr_emitter.emit_expr(&m.obj);
                            let tag = &s.value;
                            self.push_indent();
                            self.push(&format!(
                                "{} => {{ ",
                                self.pascal_case(tag)
                            ));
                        }
                    }
                }
                self.emit_stmt(item);
                self.push_line(";");
            }
        }

        self.push_line("}");
    }

    /// Emit a block statement.
    fn emit_block(&mut self, block: &BlockStmt) {
        self.push_line("{");
        self.indent += 1;

        for stmt in &block.stmts {
            self.emit_stmt(stmt);
        }

        self.indent -= 1;
        self.push_indent();
        self.push("}");
    }

    /// Emit a return statement.
    fn emit_return(&mut self, r: &ReturnStmt) {
        self.push("return");
        if let Some(value) = &r.value {
            self.push(" ");
            let code = self.expr_emitter.emit_expr(value);
            self.push(&code);
        }
        self.push(";");
    }

    /// Emit a labeled statement.
    fn emit_labeled(&mut self, l: &LabeledStmt) {
        self.push(&format!("{}: ", l.label.sym));
        self.emit_stmt(&l.body);
    }

    /// Emit a declaration.
    fn emit_decl(&mut self, decl: &Decl) {
        match decl {
            Decl::Fn(f) => self.emit_fn_decl(f),
            Decl::Var(v) => self.emit_var_decl(v),
            Decl::TsTypeAlias(t) => self.emit_type_alias(t),
            Decl::TsEnum(e) => self.emit_enum(e),
            _ => {}
        }
    }

    /// Emit a function declaration.
    fn emit_fn_decl(&mut self, f: &FnDecl) {
        if f.function.is_async {
            self.push("pub async ");
        } else {
            self.push("pub ");
        }

        let name = self.expr_emitter.mangle(&f.ident.sym.to_string());
        self.push(&format!("fn {}(", name));

        for (i, param) in f.function.params.iter().enumerate() {
            if i > 0 { self.push(", "); }
            self.emit_fn_param(param);
        }

        self.push(")");

        if let Some(ret) = &f.function.return_type {
            self.push(" -> ");
            self.emit_ts_type(&ret.type_ann);
        }

        if let Some(body) = &f.function.body {
            self.push(" ");
            self.emit_block(body);
        } else {
            self.push(";");
        }
        self.push_line("");
    }

    /// Emit function parameters.
    fn emit_fn_param(&mut self, param: &Param) {
        let name = param.pat.as_ident()
            .map(|i| self.expr_emitter.mangle(&i.id.sym.to_string()))
            .unwrap_or_else(|| "_".to_string());

        if let Some(type_ann) = param.pat.as_ident().and_then(|i| i.type_ann.as_ref()) {
            self.push(&format!("{}: ", name));
            self.emit_ts_type(&type_ann.type_ann);
        } else {
            self.push(&name);
        }
    }

    /// Emit a variable declaration.
    fn emit_var_decl(&mut self, v: &VarDecl) {
        let keyword = match v.kind {
            VarDeclKind::Const => "let",
            VarDeclKind::Let | VarDeclKind::Var => "let mut",
        };

        for (i, decl) in v.decls.iter().enumerate() {
            if i > 0 {
                self.push_line(";");
                self.push_indent();
            }
            self.push(&format!("{} ", keyword));
            self.emit_pattern(&decl.name);

            if let Some(init) = &decl.init {
                self.push(" = ");
                let code = self.expr_emitter.emit_expr(init);
                self.push(&code);
            }
        }
    }

    /// Emit a pattern.
    fn emit_pattern(&mut self, pat: &Pat) {
        match pat {
            Pat::Ident(i) => {
                let name = self.expr_emitter.mangle(&i.id.sym.to_string());
                self.push(&name);
                if let Some(type_ann) = &i.type_ann {
                    self.push(": ");
                    self.emit_ts_type(&type_ann.type_ann);
                }
            }
            Pat::Array(a) => {
                self.push("[");
                for (i, elem) in a.elems.iter().enumerate() {
                    if i > 0 { self.push(", "); }
                    if let Some(p) = elem {
                        self.emit_pattern(p);
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
                                let code = self.expr_emitter.emit_expr(&Expr::Paren(ParenExpr {
                                    span: Default::default(),
                                    expr: Box::new(v.clone()),
                                }));
                                self.push(&code);
                            }
                        }
                        ObjectPatProp::KeyValue(kv) => {
                            let key = self.expr_emitter.emit_expr(&Expr::Ident(kv.key.clone().into()));
                            self.push(&key);
                            self.push(": ");
                            self.emit_pattern(&kv.value);
                        }
                        ObjectPatProp::Rest(r) => {
                            self.push("..");
                            self.emit_pattern(&r.arg);
                        }
                    }
                }
                self.push("}");
            }
            _ => self.push("_"),
        }
    }

    /// Emit a type alias.
    fn emit_type_alias(&mut self, t: &TsTypeAliasDecl) {
        let name = self.expr_emitter.mangle(&t.id.sym.to_string());
        self.push(&format!("pub type {} = ", name));
        self.emit_ts_type(&t.type_ann);
        self.push_line(";");
    }

    /// Emit a TypeScript enum.
    fn emit_enum(&mut self, e: &TsEnumDecl) {
        let name = self.expr_emitter.mangle(&e.id.sym.to_string());
        self.push(&format!("pub enum {}", name));
        self.push_line(" {");

        for member in &e.members {
            let tag = match &member.id {
                TsEnumMemberId::Str(s) => s.value.to_string(),
                TsEnumMemberId::Computed(_) => continue,
            };
            self.push_indent();
            self.push(&format!("{},", self.pascal_case(&tag)));
            self.push_line("");
        }

        self.push_line("}");
    }

    /// Emit a TypeScript type.
    fn emit_ts_type(&mut self, ts_type: &TsType) {
        match ts_type {
            TsType::TsKeywordType(k) => {
                let ty = match k.kind {
                    TsKeywordTypeKind::TsNumberKeyword => "f64",
                    TsKeywordTypeKind::TsStringKeyword => "String",
                    TsKeywordTypeKind::TsBooleanKeyword => "bool",
                    TsKeywordTypeKind::TsNullKeyword => "()",
                    TsKeywordTypeKind::TsUndefinedKeyword => "()",
                    TsKeywordTypeKind::TsVoidKeyword => "()",
                    _ => "()",
                };
                self.push(ty);
            }
            TsType::TsArrayType(a) => {
                self.emit_ts_type(&a.elem_type);
                self.push("Vec<");
                self.emit_ts_type(&a.elem_type);
                self.push(">");
            }
            TsType::TsTypeRef(t) => {
                let name = t.type_name.as_str();
                match name {
                    "Array" | "Vec" => {
                        self.push("Vec<");
                        if let Some(params) = &t.type_params {
                            if !params.params.is_empty() {
                                self.emit_ts_type(&params.params[0]);
                            }
                        }
                        self.push(">");
                    }
                    "Option" => {
                        self.push("Option<");
                        if let Some(params) = &t.type_params {
                            if !params.params.is_empty() {
                                self.emit_ts_type(&params.params[0]);
                            }
                        }
                        self.push(">");
                    }
                    "Result" => {
                        self.push("Result<");
                        if let Some(params) = &t.type_params {
                            if params.params.len() >= 2 {
                                self.emit_ts_type(&params.params[0]);
                                self.push(", ");
                                self.emit_ts_type(&params.params[1]);
                            }
                        }
                        self.push(">");
                    }
                    "string" => self.push("String"),
                    "number" => self.push("f64"),
                    "boolean" => self.push("bool"),
                    "void" => self.push("()"),
                    _ => self.push(&self.expr_emitter.mangle(name)),
                }
            }
            TsType::TsTupleType(t) => {
                self.push("(");
                for (i, elem) in t.elem_types.iter().enumerate() {
                    if i > 0 { self.push(", "); }
                    self.emit_ts_type(&elem.ty);
                }
                self.push(")");
            }
            TsType::TsParenthesizedType(p) => {
                self.emit_ts_type(&p.type_ann);
            }
            _ => self.push("()"),
        }
    }

    /// Emit a statement body (handles single statement without braces).
    fn emit_stmt_body(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Block(b) => self.emit_block(b),
            _ => {
                self.indent += 1;
                self.emit_stmt(stmt);
                self.indent -= 1;
            }
        }
    }

    /// Convert to PascalCase.
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

    /// Push string to output.
    fn push(&mut self, s: &str) {
        self.output.push_str(s);
    }

    /// Push line to output.
    fn push_line(&mut self, s: &str) {
        self.output.push_str(s);
        self.output.push('\n');
    }

    /// Push indentation.
    fn push_indent(&mut self) {
        for _ in 0..self.indent {
            self.output.push_str("    ");
        }
    }
}
