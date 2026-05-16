//! # Code Emitter
//!
//! Emits Rust code from AST nodes.

use super::types::{StructFields, EnumDefinition, RustType, to_snake_case, to_pascal_case};
use swc_ecma_ast::{Stmt, Expr};

/// Emits Rust code for types and functions.
pub struct CodeEmitter {
    /// Output buffer
    output: String,
    /// Current indentation level
    indent: usize,
}

impl CodeEmitter {
    /// Create a new code emitter.
    #[must_use]
    pub fn new() -> Self {
        Self {
            output: String::new(),
            indent: 0,
        }
    }

    /// Emit a struct definition.
    pub fn emit_struct(&mut self, name: &str, fields: &StructFields) {
        let struct_name = if name.starts_with("__") {
            name.to_string()
        } else {
            to_pascal_case(name)
        };

        self.push_line("#[derive(Debug, Clone)]");
        self.push_line(&format!("pub struct {struct_name} {{"));
        self.indent += 1;

        for (field_name, field_type) in fields {
            let rust_field = to_snake_case(field_name);
            self.push_line(&format!("pub {rust_field}: {field_type},"));
        }

        self.indent -= 1;
        self.push_line("}");
        self.push_line("");
    }

    /// Emit an enum definition.
    pub fn emit_enum(&mut self, ed: &EnumDefinition) {
        let pascal_name = to_pascal_case(&ed.name);
        self.push_line("#[derive(Debug, Clone, Copy, PartialEq)]");
        self.push_line(&format!("pub enum {pascal_name} {{"));
        self.indent += 1;

        for variant in &ed.variants {
            if variant.fields.is_empty() {
                self.push_line(&format!("{},", to_pascal_case(&variant.name)));
            } else {
                let field_strs: Vec<String> = variant
                    .fields
                    .iter()
                    .map(|(n, t)| format!("{}: {t}", to_snake_case(n)))
                    .collect();
                self.push_line(&format!(
                    "{} {{ {} }},",
                    to_pascal_case(&variant.name),
                    field_strs.join(", ")
                ));
            }
        }

        self.indent -= 1;
        self.push_line("}");
        self.push_line("");
    }

    /// Emit a function with a body.
    pub fn emit_function_with_body(
        &mut self,
        rust_name: &str,
        params: &[(String, RustType)],
        return_type: &RustType,
        is_async: bool,
        body: Option<&Stmt>,
    ) {
        let async_prefix = if is_async { "async " } else { "" };
        let params_str: Vec<String> = params
            .iter()
            .map(|(n, t)| format!("{}: {t}", to_snake_case(n)))
            .collect();

        self.push_indent();
        self.push_str(&format!(
            "pub {async_prefix}fn {rust_name}({}) -> {return_type} {{\n",
            params_str.join(", ")
        ));
        self.indent += 1;

        if let Some(body_stmt) = body {
            self.emit_body_stmt(body_stmt);
        } else {
            self.push_indent();
            self.push_line("()");
        }

        self.indent -= 1;
        self.push_line("}");
        self.push_line("");
    }

    /// Emit a function (no body - returns unit).
    pub fn emit_function(
        &mut self,
        rust_name: &str,
        params: &[(String, RustType)],
        return_type: &RustType,
        is_async: bool,
    ) {
        self.emit_function_with_body(rust_name, params, return_type, is_async, None);
    }

    /// Emit a function body statement.
    fn emit_body_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Block(block) => {
                for s in &block.stmts {
                    self.emit_single_stmt(s);
                }
            }
            Stmt::Expr(expr_stmt) => {
                self.push_indent();
                self.emit_expr(&expr_stmt.expr);
                self.push_str(";\n");
            }
            Stmt::Return(ret) => {
                self.push_indent();
                if let Some(arg) = &ret.arg {
                    self.push_str("return ");
                    self.emit_expr(arg);
                    self.push_str(";\n");
                } else {
                    self.push_str("return ();\n");
                }
            }
            Stmt::If(if_stmt) => {
                self.emit_if(if_stmt);
            }
            Stmt::While(while_stmt) => {
                self.emit_while(while_stmt);
            }
            Stmt::For(for_stmt) => {
                self.emit_for(for_stmt);
            }
            _ => {
                self.push_indent();
                self.push_str("// unsupported statement\n");
            }
        }
    }

    /// Emit a single statement.
    fn emit_single_stmt(&mut self, stmt: &Stmt) {
        self.push_indent();
        match stmt {
            Stmt::Expr(expr_stmt) => {
                self.emit_expr(&expr_stmt.expr);
                self.push_str(";\n");
            }
            Stmt::Decl(decl) => {
                self.emit_var_decl(decl);
            }
            Stmt::If(if_stmt) => {
                self.emit_if(if_stmt);
            }
            Stmt::While(while_stmt) => {
                self.emit_while(while_stmt);
            }
            Stmt::For(for_stmt) => {
                self.emit_for(for_stmt);
            }
            Stmt::Block(block) => {
                self.push_str("{\n");
                self.indent += 1;
                for s in &block.stmts {
                    self.emit_single_stmt(s);
                }
                self.indent -= 1;
                self.push_indent();
                self.push_str("}\n");
            }
            Stmt::Return(ret) => {
                if let Some(arg) = &ret.arg {
                    self.push_str("return ");
                    self.emit_expr(arg);
                    self.push_str(";\n");
                } else {
                    self.push_str("return ();\n");
                }
            }
            _ => {
                self.push_str("// unsupported\n");
            }
        }
    }

    /// Emit a variable declaration.
    fn emit_var_decl(&mut self, decl: &swc_ecma_ast::Decl) {
        if let swc_ecma_ast::Decl::Var(var_decl) = decl {
            for vdecl in &var_decl.decls {
                let name = match &vdecl.name {
                    swc_ecma_ast::Pat::Ident(ident) => {
                        to_snake_case(ident.id.sym.as_ref())
                    }
                    _ => "unknown".to_string(),
                };
                let ty = if let Some(init) = &vdecl.init {
                    self.infer_type(init)
                } else {
                    "()".to_string()
                };

                if let Some(init) = &vdecl.init {
                    self.push_str(&format!("let {}: {} = ", name, ty));
                    self.emit_expr(init);
                    self.push_str(";\n");
                } else {
                    self.push_str(&format!("let {}: {};\n", name, ty));
                }
            }
        }
    }

    /// Emit an if statement.
    fn emit_if(&mut self, stmt: &swc_ecma_ast::IfStmt) {
        self.push_str("if ");
        self.emit_expr(&stmt.test);
        self.push_str(" {\n");
        self.indent += 1;
        self.emit_single_stmt(&stmt.cons);
        self.indent -= 1;
        if let Some(alt) = &stmt.alt {
            self.push_indent();
            self.push_str("} else {\n");
            self.indent += 1;
            self.emit_single_stmt(alt);
            self.indent -= 1;
        }
        self.push_indent();
        self.push_str("}\n");
    }

    /// Emit a while statement.
    fn emit_while(&mut self, stmt: &swc_ecma_ast::WhileStmt) {
        self.push_str("while ");
        self.emit_expr(&stmt.test);
        self.push_str(" {\n");
        self.indent += 1;
        self.emit_single_stmt(&stmt.body);
        self.indent -= 1;
        self.push_indent();
        self.push_str("}\n");
    }

    /// Emit a for statement.
    #[allow(clippy::too_many_lines)]
    fn emit_for(&mut self, stmt: &swc_ecma_ast::ForStmt) {
        self.push_str("for ");
        // Simple for loop - just emit as statements
        if let Some(init) = &stmt.init {
            match init {
                swc_ecma_ast::VarDeclOrExpr::Expr(e) => {
                    self.emit_expr(e);
                }
                swc_ecma_ast::VarDeclOrExpr::VarDecl(d) => {
                    self.emit_var_decl(&swc_ecma_ast::Decl::Var(d.clone()));
                }
            }
        }
        self.push_str("; ");
        if let Some(test) = &stmt.test {
            self.emit_expr(test);
        }
        self.push_str("; ");
        if let Some(update) = &stmt.update {
            self.emit_expr(update);
        }
        self.push_str(" {\n");
        self.indent += 1;
        self.emit_single_stmt(&stmt.body);
        self.indent -= 1;
        self.push_indent();
        self.push_str("}\n");
    }

    /// Emit an expression.
    #[allow(clippy::too_many_lines)]
    fn emit_expr(&mut self, expr: &Expr) {
        match expr {
            Expr::Lit(lit) => self.emit_lit(lit),
            Expr::Ident(ident) => {
                self.push_str(&to_snake_case(ident.sym.as_ref()));
            }
            Expr::Bin(bin_expr) => {
                self.emit_expr(&bin_expr.left);
                self.push_str(&format!(" {} ", self.bin_op_str(&bin_expr.op)));
                self.emit_expr(&bin_expr.right);
            }
            Expr::Unary(unary_expr) => {
                self.push_str(match unary_expr.op {
                    swc_ecma_ast::UnaryOp::Minus => "-",
                    swc_ecma_ast::UnaryOp::Plus => "+",
                    swc_ecma_ast::UnaryOp::Bang => "!",
                    _ => "!",
                });
                self.emit_expr(&unary_expr.arg);
            }
            Expr::Call(call_expr) => {
                self.emit_callee(&call_expr.callee);
                self.push_str("(");
                for (i, arg) in call_expr.args.iter().enumerate() {
                    if i > 0 {
                        self.push_str(", ");
                    }
                    self.emit_expr(&arg.expr);
                }
                self.push_str(")");
            }
            Expr::Member(member_expr) => {
                self.emit_expr(&member_expr.obj);
                self.push_str(".");
                match &member_expr.prop {
                    swc_ecma_ast::MemberProp::Ident(ident) => {
                        self.push_str(&to_snake_case(ident.sym.as_ref()));
                    }
                    swc_ecma_ast::MemberProp::PrivateName(_) => {
                        self.push_str("prop");
                    }
                    swc_ecma_ast::MemberProp::Computed(_) => {
                        self.push_str("prop");
                    }
                }
            }
            Expr::Cond(cond_expr) => {
                self.emit_expr(&cond_expr.test);
                self.push_str(".then(|| ");
                self.emit_expr(&cond_expr.cons);
                self.push_str(").else(|| ");
                self.emit_expr(&cond_expr.alt);
                self.push_str(")");
            }
            Expr::Array(arr) => {
                self.push_str("vec![");
                for (i, elem) in arr.elems.iter().enumerate() {
                    if i > 0 {
                        self.push_str(", ");
                    }
                    if let Some(elem) = elem {
                        self.emit_expr(&elem.expr);
                    }
                }
                self.push_str("]");
            }
            Expr::Object(obj) => {
                self.push_str("{");
                for (i, prop) in obj.props.iter().enumerate() {
                    if i > 0 {
                        self.push_str(", ");
                    }
                    match prop {
                        swc_ecma_ast::PropOrSpread::Prop(prop) => {
                            if let swc_ecma_ast::Prop::KeyValue(kv) = &**prop {
                                let key = match &kv.key {
                                    swc_ecma_ast::PropName::Ident(ident) => {
                                        to_snake_case(ident.sym.as_ref())
                                    }
                                    swc_ecma_ast::PropName::Str(s) => {
                                        to_snake_case(&format!("{:?}", s.value))
                                    }
                                    swc_ecma_ast::PropName::Num(n) => {
                                        n.value.to_string()
                                    }
                                    _ => "unknown".to_string(),
                                };
                                self.push_str(&format!("{}: ", key));
                                self.emit_expr(&kv.value);
                            }
                        }
                        swc_ecma_ast::PropOrSpread::Spread(_) => {}
                    }
                }
                self.push_str("}");
            }
            _ => self.push_str("()"),
        }
    }

    /// Emit a callee (handles different callee types).
    fn emit_callee(&mut self, callee: &swc_ecma_ast::Callee) {
        match callee {
            swc_ecma_ast::Callee::Expr(expr) => {
                self.emit_expr(expr);
            }
            swc_ecma_ast::Callee::Import(_) => {
                self.push_str("import");
            }
            swc_ecma_ast::Callee::Super(_) => {
                self.push_str("super");
            }
        }
    }

    /// Emit a literal.
    fn emit_lit(&mut self, lit: &swc_ecma_ast::Lit) {
        match lit {
            swc_ecma_ast::Lit::Str(s) => {
                self.push_str(&format!("{:?}", s.value));
            }
            swc_ecma_ast::Lit::Num(n) => {
                self.push_str(&n.value.to_string());
            }
            swc_ecma_ast::Lit::Bool(b) => {
                self.push_str(if b.value { "true" } else { "false" });
            }
            swc_ecma_ast::Lit::Null(_) => {
                self.push_str("None");
            }
            _ => self.push_str("()"),
        }
    }

    /// Get binary operator string.
    #[allow(clippy::trivially_copy_pass_by_ref)]
    fn bin_op_str(&self, op: &swc_ecma_ast::BinaryOp) -> &'static str {
        match op {
            swc_ecma_ast::BinaryOp::Add => "+",
            swc_ecma_ast::BinaryOp::Sub => "-",
            swc_ecma_ast::BinaryOp::Mul => "*",
            swc_ecma_ast::BinaryOp::Div => "/",
            swc_ecma_ast::BinaryOp::Mod => "%",
            swc_ecma_ast::BinaryOp::EqEqEq => "==",
            swc_ecma_ast::BinaryOp::NotEqEq => "!=",
            swc_ecma_ast::BinaryOp::Lt => "<",
            swc_ecma_ast::BinaryOp::LtEq => "<=",
            swc_ecma_ast::BinaryOp::Gt => ">",
            swc_ecma_ast::BinaryOp::GtEq => ">=",
            swc_ecma_ast::BinaryOp::LogicalAnd => "&&",
            swc_ecma_ast::BinaryOp::LogicalOr => "||",
            swc_ecma_ast::BinaryOp::BitAnd => "&",
            swc_ecma_ast::BinaryOp::BitOr => "|",
            swc_ecma_ast::BinaryOp::BitXor => "^",
            swc_ecma_ast::BinaryOp::LShift => "<<",
            swc_ecma_ast::BinaryOp::RShift => ">>",
            _ => "??",
        }
    }

    /// Infer type from expression.
    fn infer_type(&self, expr: &Expr) -> String {
        match expr {
            Expr::Lit(lit) => match lit {
                swc_ecma_ast::Lit::Num(_) => "f64".to_string(),
                swc_ecma_ast::Lit::Str(_) => "String".to_string(),
                swc_ecma_ast::Lit::Bool(_) => "bool".to_string(),
                _ => "()".to_string(),
            },
            Expr::Array(_) => "Vec<()>".to_string(),
            _ => "()".to_string(),
        }
    }

    /// Push a line with newline.
    pub fn push_line(&mut self, s: &str) {
        self.output.push_str(s);
        self.output.push('\n');
    }

    /// Push a string without newline.
    pub fn push_str(&mut self, s: &str) {
        self.output.push_str(s);
    }

    /// Push indentation.
    pub fn push_indent(&mut self) {
        for _ in 0..self.indent {
            self.output.push_str("    ");
        }
    }

    /// Get the output.
    #[must_use]
    pub fn output(&self) -> &str {
        &self.output
    }

    /// Consume emitter and return output.
    #[must_use]
    pub fn into_output(self) -> String {
        self.output
    }
}

impl Default for CodeEmitter {
    fn default() -> Self {
        Self::new()
    }
}
