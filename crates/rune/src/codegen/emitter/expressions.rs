//! # Expression Emitter
//!
//! Emits Rust expressions from TypeScript AST.

use super::CodeEmitter;
use swc_ecma_ast::{Expr, Lit};

/// Emit an expression.
pub fn emit_expr(emitter: &mut CodeEmitter, expr: &Expr) {
    match expr {
        Expr::Lit(lit) => emit_lit(emitter, lit),
        Expr::Ident(ident) => {
            emitter.push_str(&super::to_snake_case(ident.sym.as_ref()));
        }
        Expr::Bin(bin_expr) => {
            emit_expr(emitter, &bin_expr.left);
            emitter.push_str(&format!(" {} ", bin_op_str(bin_expr.op)));
            emit_expr(emitter, &bin_expr.right);
        }
        Expr::Unary(unary_expr) => {
            emitter.push_str(match unary_expr.op {
                swc_ecma_ast::UnaryOp::Minus => "-",
                swc_ecma_ast::UnaryOp::Plus => "+",
                swc_ecma_ast::UnaryOp::Bang => "!",
                _ => "!",
            });
            emit_expr(emitter, &unary_expr.arg);
        }
        Expr::Call(call_expr) => emit_call(emitter, call_expr),
        Expr::Member(member_expr) => emit_member(emitter, member_expr),
        Expr::Cond(cond_expr) => {
            emit_expr(emitter, &cond_expr.test);
            emitter.push_str(".then(|| ");
            emit_expr(emitter, &cond_expr.cons);
            emitter.push_str(").else(|| ");
            emit_expr(emitter, &cond_expr.alt);
            emitter.push_str(")");
        }
        Expr::Array(arr) => {
            emitter.push_str("vec![");
            for (i, elem) in arr.elems.iter().enumerate() {
                if i > 0 {
                    emitter.push_str(", ");
                }
                if let Some(elem) = elem {
                    emit_expr(emitter, &elem.expr);
                }
            }
            emitter.push_str("]");
        }
        Expr::Object(obj) => emit_object(emitter, obj),
        Expr::Arrow(arrow) => emit_arrow(emitter, arrow),
        Expr::Paren(paren) => {
            emitter.push_str("(");
            emit_expr(emitter, &paren.expr);
            emitter.push_str(")");
        }
        Expr::New(_n) => {
            emitter.push_str("/* new */ ()");
        }
        Expr::Tpl(_) => emitter.push_str("String::new()"),
        Expr::JSXElement(_) | Expr::JSXFragment(_) => emitter.push_str("()"),
        _ => emitter.push_str("()"),
    }
}

/// Emit a function call.
fn emit_call(emitter: &mut CodeEmitter, call_expr: &swc_ecma_ast::CallExpr) {
    if let swc_ecma_ast::Callee::Expr(expr) = &call_expr.callee {
        if let Expr::Member(member) = &**expr {
            if let Expr::Ident(ident) = &*member.obj {
                let obj_name = ident.sym.as_ref();
                if let swc_ecma_ast::MemberProp::Ident(prop) = &member.prop {
                    let method_name = prop.sym.as_ref();
                    if obj_name == "Date" && method_name == "now" {
                        emitter.push_str("std::time::SystemTime::now()");
                        emitter.push_str(".duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as i64");
                        return;
                    }
                }
            }
        }
    }
    emit_callee(emitter, &call_expr.callee);
    emitter.push_str("(");
    for (i, arg) in call_expr.args.iter().enumerate() {
        if i > 0 {
            emitter.push_str(", ");
        }
        emit_expr(emitter, &arg.expr);
    }
    emitter.push_str(")");
}

/// Emit a member expression.
fn emit_member(emitter: &mut CodeEmitter, member_expr: &swc_ecma_ast::MemberExpr) {
    emit_expr(emitter, &member_expr.obj);
    emitter.push_str(".");
    match &member_expr.prop {
        swc_ecma_ast::MemberProp::Ident(ident) => {
            let prop_name = ident.sym.as_ref();
            emit_method_name(emitter, prop_name);
        }
        swc_ecma_ast::MemberProp::PrivateName(_) => emitter.push_str("prop"),
        swc_ecma_ast::MemberProp::Computed(_) => emitter.push_str("prop"),
    }
}

/// Emit a method name mapping.
fn emit_method_name(emitter: &mut CodeEmitter, prop_name: &str) {
    match prop_name {
        "length" => emitter.push_str("len()"),
        "toString" => emitter.push_str("&*"),
        "valueOf" => {}
        "toLowerCase" => emitter.push_str("to_lowercase"),
        "toUpperCase" => emitter.push_str("to_uppercase"),
        "trim" => emitter.push_str("trim"),
        "trimStart" | "trimLeft" => emitter.push_str("trim_start"),
        "trimEnd" | "trimRight" => emitter.push_str("trim_end"),
        "includes" => emitter.push_str("contains"),
        "startsWith" => emitter.push_str("starts_with"),
        "endsWith" => emitter.push_str("ends_with"),
        "indexOf" => emitter.push_str("find"),
        "charAt" => emitter.push_str("chars().nth(0)"),
        "split" => emitter.push_str("split"),
        "toFixed" => emitter.push_str("trunc"),
        "push" => emitter.push_str("push"),
        "pop" => emitter.push_str("pop"),
        "shift" => emitter.push_str("remove"),
        "unshift" => emitter.push_str("insert"),
        "filter" => emitter.push_str("into_iter().filter"),
        "map" => emitter.push_str("into_iter().map"),
        "reduce" => emitter.push_str("into_iter().fold"),
        "forEach" => emitter.push_str("into_iter().for_each"),
        "some" => emitter.push_str("into_iter().any"),
        "every" => emitter.push_str("into_iter().all"),
        "find" => emitter.push_str("into_iter().find"),
        "findIndex" => emitter.push_str("into_iter().position"),
        "concat" => emitter.push_str("push"),
        "join" => emitter.push_str("join"),
        "reverse" => emitter.push_str("reverse"),
        "sort" => emitter.push_str("sort_by"),
        "slice" => emitter.push_str("iter().skip"),
        "splice" => emitter.push_str("splice"),
        "fill" => emitter.push_str("fill"),
        "copyWithin" => emitter.push_str("copy_within"),
        "entries" => emitter.push_str("iter().enumerate"),
        "keys" => emitter.push_str("iter().enumerate"),
        "values" => emitter.push_str("iter"),
        "now" => emitter.push_str("now"),
        "getTime" => emitter.push_str("duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as i64"),
        "id" | "title" | "done" | "tasks" | "selected" => emitter.push_str(prop_name),
        _ => emitter.push_str(&super::to_snake_case(prop_name)),
    }
}

/// Emit an object literal.
fn emit_object(emitter: &mut CodeEmitter, obj: &swc_ecma_ast::ObjectLit) {
    emitter.push_str("{");
    let mut first = true;
    for prop in &obj.props {
        match prop {
            swc_ecma_ast::PropOrSpread::Prop(prop) => {
                if !first {
                    emitter.push_str(", ");
                }
                first = false;
                match &**prop {
                    swc_ecma_ast::Prop::KeyValue(kv) => {
                        emit_prop_key(emitter, &kv.key);
                        emitter.push_str(": ");
                        emit_expr(emitter, &kv.value);
                    }
                    swc_ecma_ast::Prop::Shorthand(ident) => {
                        let name = super::to_snake_case(ident.sym.as_ref());
                        emitter.push_str(&name);
                        emitter.push_str(": ");
                        emitter.push_str(&name);
                    }
                    swc_ecma_ast::Prop::Assign(kv) => {
                        emitter.push_str(&super::to_snake_case(kv.key.sym.as_ref()));
                        emitter.push_str(": ");
                        emit_expr(emitter, &kv.value);
                    }
                    swc_ecma_ast::Prop::Getter(_) | swc_ecma_ast::Prop::Setter(_) | swc_ecma_ast::Prop::Method(_) => {
                        emitter.push_str("/* method */ ()");
                    }
                }
            }
            swc_ecma_ast::PropOrSpread::Spread(spread) => {
                if !first {
                    emitter.push_str(", ");
                }
                first = false;
                emitter.push_str("..");
                emit_expr(emitter, &spread.expr);
            }
        }
    }
    emitter.push_str("}");
}

/// Emit object property key.
fn emit_prop_key(emitter: &mut CodeEmitter, key: &swc_ecma_ast::PropName) {
    match key {
        swc_ecma_ast::PropName::Ident(ident) => {
            emitter.push_str(&super::to_snake_case(ident.sym.as_ref()));
        }
        swc_ecma_ast::PropName::Str(s) => {
            emitter.push_str(&super::to_snake_case(&format!("{:?}", s.value)));
        }
        swc_ecma_ast::PropName::Num(n) => {
            emitter.push_str(&n.value.to_string());
        }
        _ => emitter.push_str("unknown"),
    }
}

/// Emit an arrow function.
fn emit_arrow(emitter: &mut CodeEmitter, arrow: &swc_ecma_ast::ArrowExpr) {
    emitter.push_str("|| ");
    match &*arrow.body {
        swc_ecma_ast::BlockStmtOrExpr::Expr(e) => {
            emit_expr(emitter, e);
        }
        swc_ecma_ast::BlockStmtOrExpr::BlockStmt(block) => {
            emitter.push_str("{ ");
            for s in &block.stmts {
                super::emit_single_stmt(emitter, s);
            }
            emitter.push_str(" }");
        }
    }
}

/// Emit a callee.
fn emit_callee(emitter: &mut CodeEmitter, callee: &swc_ecma_ast::Callee) {
    match callee {
        swc_ecma_ast::Callee::Expr(expr) => emit_expr(emitter, expr),
        swc_ecma_ast::Callee::Import(_) => emitter.push_str("import"),
        swc_ecma_ast::Callee::Super(_) => emitter.push_str("super"),
    }
}

/// Emit a literal.
fn emit_lit(emitter: &mut CodeEmitter, lit: &Lit) {
    match lit {
        Lit::Str(s) => emitter.push_str(&format!("{:?}", s.value)),
        Lit::Num(n) => emitter.push_str(&n.value.to_string()),
        Lit::Bool(b) => emitter.push_str(if b.value { "true" } else { "false" }),
        Lit::Null(_) => emitter.push_str("None"),
        Lit::BigInt(_) => emitter.push_str("0i64"),
        Lit::JSXText(_) => emitter.push_str("String::new()"),
        Lit::Regex(_) => emitter.push_str("String::new()"),
    }
}

/// Get binary operator string.
fn bin_op_str(op: swc_ecma_ast::BinaryOp) -> &'static str {
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
pub fn infer_type(expr: &Expr) -> String {
    match expr {
        Expr::Lit(lit) => match lit {
            Lit::Num(_) => "f64".to_string(),
            Lit::Str(_) => "String".to_string(),
            Lit::Bool(_) => "bool".to_string(),
            Lit::BigInt(_) => "i64".to_string(),
            _ => "()".to_string(),
        },
        Expr::Array(_) => "Vec<()>".to_string(),
        Expr::Call(call_expr) => infer_call_type(call_expr),
        Expr::Member(member_expr) => infer_member_type(member_expr),
        Expr::Object(_) | Expr::Arrow(_) => "()".to_string(),
        _ => "()".to_string(),
    }
}

/// Infer type from call expression.
fn infer_call_type(call_expr: &swc_ecma_ast::CallExpr) -> String {
    if let swc_ecma_ast::Callee::Expr(callee_expr) = &call_expr.callee {
        if let Expr::Member(member) = &**callee_expr {
            if let Expr::Ident(ident) = &*member.obj {
                let obj_name = ident.sym.as_ref();
                if let swc_ecma_ast::MemberProp::Ident(prop) = &member.prop {
                    let method_name = prop.sym.as_ref();
                    if obj_name == "title" || obj_name == "s" || obj_name == "str" || obj_name == "text" {
                        match method_name {
                            "trim" | "trimStart" | "trimEnd" | "toLowerCase" | "toUpperCase"
                            | "substring" | "substr" | "slice" => return "String".to_string(),
                            "charAt" | "charCodeAt" => return "char".to_string(),
                            "indexOf" | "lastIndexOf" => return "Option<usize>".to_string(),
                            "includes" | "startsWith" | "endsWith" => return "bool".to_string(),
                            "length" => return "usize".to_string(),
                            _ => {}
                        }
                    }
                    match method_name {
                        "filter" | "map" | "concat" | "slice" | "splice" => return "Vec<()>".to_string(),
                        "find" | "findIndex" => return "Option<()>".to_string(),
                        "some" | "every" => return "bool".to_string(),
                        "length" => return "usize".to_string(),
                        "push" | "pop" | "shift" => return "Option<()>".to_string(),
                        _ => {}
                    }
                }
            }
        }
    }
    "()".to_string()
}

/// Infer type from member expression.
fn infer_member_type(member_expr: &swc_ecma_ast::MemberExpr) -> String {
    if let swc_ecma_ast::MemberProp::Ident(prop) = &member_expr.prop {
        let prop_name = prop.sym.as_ref();
        match prop_name {
            "length" => "usize".to_string(),
            _ => "()".to_string(),
        }
    } else {
        "()".to_string()
    }
}
