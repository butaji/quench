//! # Expression Emitter
//!
//! Emits Rust code from TypeScript expressions.

use swc_ecma_ast::*;
use crate::codegen::{CodegenOptions, GeneratedModule, Import, ImportedName};
use crate::analyzer::{AnalysisResult, BorrowMode};

/// Emits Rust code for expressions.
pub struct ExprEmitter<'a> {
    /// Output buffer
    pub output: String,
    /// Current indentation
    pub indent: usize,
    /// Analysis result
    analysis: &'a AnalysisResult,
}

impl<'a> ExprEmitter<'a> {
    /// Create a new expression emitter.
    pub fn new(analysis: &'a AnalysisResult) -> Self {
        Self {
            output: String::new(),
            indent: 0,
            analysis,
        }
    }

    /// Emit an expression.
    pub fn emit_expr(&mut self, expr: &Expr) -> String {
        match expr {
            Expr::Lit(l) => self.emit_lit(l),
            Expr::Ident(i) => self.mangle(&i.sym.to_string()),
            Expr::Bin(b) => self.emit_bin_expr(b),
            Expr::Unary(u) => self.emit_unary(u),
            Expr::Update(u) => self.emit_update(u),
            Expr::Assign(a) => self.emit_assign(a),
            Expr::Member(m) => self.emit_member(m),
            Expr::Call(c) => self.emit_call(c),
            Expr::Arrow(a) => self.emit_arrow(a),
            Expr::Fn(f) => self.emit_fn_expr(f),
            Expr::Cond(c) => self.emit_cond(c),
            Expr::Array(a) => self.emit_array(a),
            Expr::Object(o) => self.emit_object(o),
            Expr::Paren(p) => {
                let inner = self.emit_expr(&p.expr);
                format!("({})", inner)
            }
            Expr::Tpl(t) => self.emit_tpl(t),
            Expr::Await(a) => {
                let inner = self.emit_expr(&a.arg);
                format!("({}.await)", inner)
            }
            Expr::TsAs(t) => self.emit_expr(&t.expr),
            _ => "unimplemented!()".to_string(),
        }
    }

    /// Emit a literal.
    fn emit_lit(&self, lit: &Lit) -> String {
        match lit {
            Lit::Null(_) => "()".to_string(),
            Lit::Bool(b) => (if b.value { "true" } else { "false" }).to_string(),
            Lit::Num(n) => {
                if n.value.fract() == 0.0 {
                    format!("{}", n.value as i32)
                } else {
                    format!("{}", n.value)
                }
            }
            Lit::Str(s) => format!("{:?}", s.value),
            Lit::BigInt(b) => format!("{}i64", b.value),
            _ => "unimplemented!()".to_string(),
        }
    }

    /// Emit a binary expression.
    fn emit_bin_expr(&self, bin: &BinExpr) -> String {
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

        let left = self.emit_expr(&bin.left);
        let right = self.emit_expr(&bin.right);

        if bin.op == BinaryOp::Exp {
            return format!("f64::powf({}, {})", left, right);
        }
        if bin.op == BinaryOp::NullishCoalescing {
            return format!("({}).unwrap_or({})", left, right);
        }

        format!("({} {} {})", left, rust_op, right)
    }

    /// Emit a unary expression.
    fn emit_unary(&self, u: &UnaryExpr) -> String {
        let rust_op = match u.op {
            UnaryOp::Minus => "-",
            UnaryOp::Plus => "",
            UnaryOp::Bang => "!",
            UnaryOp::Tilde => "!",
            UnaryOp::TypeOf => "/* typeof */ unimplemented!()",
            UnaryOp::Void => "()",
        };

        if u.op == UnaryOp::TypeOf || u.op == UnaryOp::Void {
            return rust_op.to_string();
        }

        let arg = self.emit_expr(&u.arg);
        format!("{}{}", rust_op, arg)
    }

    /// Emit an update expression.
    fn emit_update(&self, u: &UpdateExpr) -> String {
        let op = match u.op {
            UpdateOp::PlusPlus => "++",
            UpdateOp::MinusMinus => "--",
        };

        let arg = self.emit_expr(&u.arg);
        if u.is_prefix {
            format!("{}{}", op, arg)
        } else {
            format!("{}{}", arg, op)
        }
    }

    /// Emit an assignment expression.
    fn emit_assign(&self, a: &AssignExpr) -> String {
        let rust_op = match a.op {
            AssignOp::Assign => "=",
            AssignOp::AddAssign => "+=",
            AssignOp::SubAssign => "-=",
            AssignOp::MulAssign => "*=",
            AssignOp::DivAssign => "/=",
            AssignOp::ModAssign => "%=",
            _ => "=",
        };

        let left = self.emit_expr(&a.left);
        let value = self.emit_expr(&a.value);
        format!("({} {} {})", left, rust_op, value)
    }

    /// Emit a member expression.
    fn emit_member(&self, m: &MemberExpr) -> String {
        let obj = self.emit_expr(&m.obj);

        if m.computed {
            let prop = self.emit_expr(&m.prop);
            return format!("{}[{}]", obj, prop);
        }

        match &m.prop {
            Expr::Ident(i) => format!("{}.{}", obj, i.sym),
            _ => format!("{}.{}", obj, self.emit_expr(&m.prop)),
        }
    }

    /// Emit a call expression.
    fn emit_call(&self, c: &CallExpr) -> String {
        let callee = self.emit_expr(&c.callee);
        let args: Vec<String> = c.args.iter()
            .map(|a| self.emit_expr(&a.expr))
            .collect();
        format!("{}({})", callee, args.join(", "))
    }

    /// Emit an arrow function.
    fn emit_arrow(&self, a: &ArrowExpr) -> String {
        let params: Vec<String> = a.params.iter()
            .filter_map(|p| p.pat.as_ident().map(|i| self.mangle(&i.id.sym.to_string())))
            .collect();

        let body = match &a.body {
            BlockStmtOrExpr::BlockStmt(b) => {
                let stmts: Vec<String> = b.stmts.iter()
                    .map(|s| self.emit_expr_stmt(s))
                    .collect();
                format!("{{ {} }}", stmts.join("; "))
            }
            BlockStmtOrExpr::Expr(e) => self.emit_expr(e),
        };

        format!("|| {{ |{}| {} }}", params.join(", "), body)
    }

    /// Emit a function expression.
    fn emit_fn_expr(&self, f: &FnExpr) -> String {
        let params: Vec<String> = f.function.params.iter()
            .filter_map(|p| p.pat.as_ident().map(|i| self.mangle(&i.id.sym.to_string())))
            .collect();

        let body = f.function.body.as_ref()
            .map(|b| self.emit_block(b))
            .unwrap_or_default();

        format!("|{}| {}", params.join(", "), body)
    }

    /// Emit a conditional expression.
    fn emit_cond(&self, c: &CondExpr) -> String {
        let test = self.emit_expr(&c.test);
        let cons = self.emit_expr(&c.cons);
        let alt = self.emit_expr(&c.alt);
        format!("if {} {{ {} }} else {{ {} }}", test, cons, alt)
    }

    /// Emit an array literal.
    fn emit_array(&self, a: &ArrayExpr) -> String {
        let elems: Vec<String> = a.elems.iter()
            .map(|e| e.as_ref().map(|e| self.emit_expr(&e.expr)).unwrap_or_default())
            .collect();
        format!("vec![{}]", elems.join(", "))
    }

    /// Emit an object literal.
    fn emit_object(&self, o: &ObjectExpr) -> String {
        let props: Vec<String> = o.props.iter()
            .filter_map(|p| self.emit_prop(p))
            .collect();
        format!("__rune_obj!({{{}}})", props.join(", "))
    }

    /// Emit a template literal.
    fn emit_tpl(&self, t: &TplExpr) -> String {
        let mut result = String::from("format!(\"");
        let exprs: Vec<String> = t.exprs.iter()
            .map(|e| self.emit_expr(e))
            .collect();

        for (i, quasi) in t.quasis.iter().enumerate() {
            result.push_str(&quasi.raw.value.replace("{", "{{").replace("}", "}}"));
            if i < exprs.len() {
                result.push_str("{}");
            }
        }
        result.push('"');

        if !exprs.is_empty() {
            result.push_str(", ");
            result.push_str(&exprs.join(", "));
        }

        result.push(')');
        result
    }

    /// Emit a property.
    fn emit_prop(&self, prop: &PropOrSpread) -> Option<String> {
        match prop {
            PropOrSpread::Prop(Prop::KeyValue(kv)) => {
                let key = match &kv.key {
                    PropName::Str(s) => format!("{:?}: ", s.value),
                    PropName::Ident(i) => format!("{}: ", i.sym),
                    _ => return None,
                };
                let value = self.emit_expr(&kv.value);
                Some(format!("{}{}", key, value))
            }
            PropOrSpread::Spread(s) => {
                Some(format!("..{}", self.emit_expr(&s.expr)))
            }
            _ => None,
        }
    }

    /// Emit a statement as expression.
    fn emit_expr_stmt(&self, stmt: &Stmt) -> String {
        match stmt {
            Stmt::Expr(e) => self.emit_expr(&e.expr),
            Stmt::Return(r) => {
                r.value.as_ref()
                    .map(|v| format!("return {}", self.emit_expr(v)))
                    .unwrap_or_else(|| "return".to_string())
            }
            Stmt::Break(_) => "break".to_string(),
            Stmt::Continue(_) => "continue".to_string(),
            _ => "unimplemented!()".to_string(),
        }
    }

    /// Emit a block statement.
    fn emit_block(&self, block: &BlockStmt) -> String {
        let stmts: Vec<String> = block.stmts.iter()
            .map(|s| self.emit_expr_stmt(s))
            .collect();
        format!("{{ {} }}", stmts.join("; "))
    }

    /// Mangle a name to avoid keyword conflicts.
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
            name.to_string()
        }
    }
}
