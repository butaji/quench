//! # Call Expression Emitter
//!
//! Emits Rust function calls from TypeScript.

use super::{emit_expr, CodeEmitter};
use swc_ecma_ast::{Callee, Expr, MemberProp};

/// Emit a function call with built-in handling.
pub fn emit_call(emitter: &mut CodeEmitter, call_expr: &swc_ecma_ast::CallExpr) {
    let Callee::Expr(callee) = &call_expr.callee else {
        emitter.push_str("/* unknown callee */ ()");
        return;
    };

    if let Expr::Member(member) = &**callee {
        handle_member_call(emitter, member, call_expr);
    } else if let Expr::Ident(ident) = &**callee {
        emit_direct_call(emitter, ident, call_expr);
    } else {
        emit_generic_call(emitter, callee, call_expr);
    }
}

fn handle_member_call(
    emitter: &mut CodeEmitter,
    member: &swc_ecma_ast::MemberExpr,
    call_expr: &swc_ecma_ast::CallExpr,
) {
    if let Expr::Ident(ident) = &*member.obj {
        let obj_name = ident.sym.as_ref();
        if let MemberProp::Ident(prop) = &member.prop {
            let method = prop.sym.as_ref();
            if obj_name == "Date" {
                emit_date_now(emitter);
                return;
            }
            if obj_name == "JSON" {
                emit_json_method(emitter, method, call_expr);
                return;
            }
            if obj_name == "Math" {
                emit_math_call(emitter, method, call_expr);
                return;
            }
        }
    }

    emit_method_or_generic_call(emitter, member, call_expr);
}

fn emit_date_now(emitter: &mut CodeEmitter) {
    emitter.push_str(
        "std::time::SystemTime::now()\
        .duration_since(std::time::UNIX_EPOCH)\
        .unwrap()\
        .as_millis() as i64",
    );
}

fn emit_json_method(emitter: &mut CodeEmitter, method: &str, call_expr: &swc_ecma_ast::CallExpr) {
    match method {
        "stringify" => emit_json_stringify(emitter, call_expr),
        "parse" => emit_json_parse(emitter, call_expr),
        _ => emitter.push_str("/* unknown JSON method */"),
    }
}

fn emit_json_stringify(emitter: &mut CodeEmitter, call_expr: &swc_ecma_ast::CallExpr) {
    if let Some(arg) = call_expr.args.first() {
        emitter.push_str("serde_json::to_string(&");
        emit_expr(emitter, &arg.expr);
        emitter.push_str(").unwrap_or_default()");
    } else {
        emitter.push_str("String::new()");
    }
}

fn emit_json_parse(emitter: &mut CodeEmitter, call_expr: &swc_ecma_ast::CallExpr) {
    if let Some(arg) = call_expr.args.first() {
        emitter.push_str("serde_json::from_str::<serde_json::Value>(&");
        emit_expr(emitter, &arg.expr);
        emitter.push_str(").ok()");
    } else {
        emitter.push_str("None");
    }
}

fn emit_math_call(emitter: &mut CodeEmitter, method: &str, call_expr: &swc_ecma_ast::CallExpr) {
    let fn_name = match method {
        "floor" | "ceil" | "round" | "abs" | "sqrt" | "max" | "min" | "random" => method,
        "pow" => "powf",
        m => m,
    };
    emitter.push_str(&format!("{fn_name}("));
    emit_call_args(emitter, call_expr);
    emitter.push_str(")");
}

fn emit_method_or_generic_call(
    emitter: &mut CodeEmitter,
    member: &swc_ecma_ast::MemberExpr,
    call_expr: &swc_ecma_ast::CallExpr,
) {
    match &member.prop {
        MemberProp::Ident(prop) => {
            let method = prop.sym.as_ref();
            if is_array_method(method) {
                emit_array_call(emitter, member, method, call_expr);
            } else if is_string_method(method) {
                emit_string_call(emitter, member, method, call_expr);
            } else {
                emit_generic_member_call(emitter, member, call_expr);
            }
        }
        _ => emit_generic_member_call(emitter, member, call_expr),
    }
}

fn is_array_method(method: &str) -> bool {
    matches!(
        method,
        "filter"
            | "map"
            | "reduce"
            | "forEach"
            | "some"
            | "every"
            | "find"
            | "findIndex"
            | "concat"
            | "join"
            | "reverse"
            | "sort"
            | "slice"
            | "splice"
            | "get"
    )
}

fn is_string_method(method: &str) -> bool {
    matches!(method, "localeCompare" | "includes" | "indexOf")
}

fn emit_array_call(
    emitter: &mut CodeEmitter,
    member: &swc_ecma_ast::MemberExpr,
    method: &str,
    call_expr: &swc_ecma_ast::CallExpr,
) {
    emit_expr(emitter, &member.obj);

    match method {
        "splice" => emit_array_splice(emitter, call_expr),
        "get" => emit_array_get(emitter, call_expr),
        _ => emit_array_iter_method(emitter, method, call_expr),
    }
}

fn emit_array_splice(emitter: &mut CodeEmitter, call_expr: &swc_ecma_ast::CallExpr) {
    emitter.push_str(".splice(");
    if let Some(start_arg) = call_expr.args.first() {
        emit_expr(emitter, &start_arg.expr);
        emitter.push_str("..");
        emit_expr(emitter, &start_arg.expr);
        emitter.push_str(" + ");
        if call_expr.args.len() >= 2 {
            emit_expr(emitter, &call_expr.args[1].expr);
        } else {
            emitter.push_str("1");
        }
    }
    emitter.push_str(", vec![])");
}

fn emit_array_get(emitter: &mut CodeEmitter, call_expr: &swc_ecma_ast::CallExpr) {
    emitter.push_str(".get(");
    if let Some(arg) = call_expr.args.first() {
        emit_expr(emitter, &arg.expr);
        emitter.push_str(" as usize");
    }
    emitter.push_str(")");
}

fn emit_array_iter_method(
    emitter: &mut CodeEmitter,
    method: &str,
    call_expr: &swc_ecma_ast::CallExpr,
) {
    emitter.push_str(".iter().");
    let rust_method = match method {
        "forEach" => "for_each",
        "findIndex" => "position",
        "some" => "any",
        "every" => "all",
        m => m,
    };
    emitter.push_str(rust_method);
    emitter.push_str("(");
    emit_call_args(emitter, call_expr);
    emitter.push_str(")");
}

fn emit_string_call(
    emitter: &mut CodeEmitter,
    member: &swc_ecma_ast::MemberExpr,
    method: &str,
    call_expr: &swc_ecma_ast::CallExpr,
) {
    match method {
        "localeCompare" => emit_string_locale_compare(emitter, member, call_expr),
        "includes" => emit_string_includes(emitter, member, call_expr),
        "indexOf" => emit_string_index_of(emitter, member, call_expr),
        _ => emit_generic_member_call(emitter, member, call_expr),
    }
}

fn emit_string_locale_compare(
    emitter: &mut CodeEmitter,
    member: &swc_ecma_ast::MemberExpr,
    call_expr: &swc_ecma_ast::CallExpr,
) {
    emitter.push_str("(");
    emit_expr(emitter, &member.obj);
    emitter.push_str(".cmp(");
    if let Some(arg) = call_expr.args.first() {
        emit_expr(emitter, &arg.expr);
    }
    emitter.push_str(") as i32)");
}

fn emit_string_includes(
    emitter: &mut CodeEmitter,
    member: &swc_ecma_ast::MemberExpr,
    call_expr: &swc_ecma_ast::CallExpr,
) {
    emitter.push_str("(");
    emit_expr(emitter, &member.obj);
    emitter.push_str(".contains(");
    if let Some(arg) = call_expr.args.first() {
        emit_expr(emitter, &arg.expr);
    }
    emitter.push_str("))");
}

fn emit_string_index_of(
    emitter: &mut CodeEmitter,
    member: &swc_ecma_ast::MemberExpr,
    call_expr: &swc_ecma_ast::CallExpr,
) {
    emitter.push_str("(");
    emit_expr(emitter, &member.obj);
    emitter.push_str(".find(");
    if let Some(arg) = call_expr.args.first() {
        emit_expr(emitter, &arg.expr);
    }
    emitter.push_str(").is_some() as i32)");
}

fn emit_generic_member_call(
    emitter: &mut CodeEmitter,
    member: &swc_ecma_ast::MemberExpr,
    call_expr: &swc_ecma_ast::CallExpr,
) {
    emit_expr(emitter, &member.obj);
    match &member.prop {
        MemberProp::Ident(prop) => {
            emitter.push_str(&format!(".{}(", prop.sym.as_ref()));
        }
        _ => {
            emitter.push_str(".( /* computed */");
        }
    }
    emit_call_args(emitter, call_expr);
    emitter.push_str(")");
}

fn emit_direct_call(
    emitter: &mut CodeEmitter,
    ident: &swc_ecma_ast::Ident,
    call_expr: &swc_ecma_ast::CallExpr,
) {
    let fn_name = super::to_snake_case(ident.sym.as_ref());
    emitter.push_str(&fn_name);
    emitter.push_str("(");
    emit_call_args(emitter, call_expr);
    emitter.push_str(")");
}

fn emit_generic_call(emitter: &mut CodeEmitter, callee: &Expr, call_expr: &swc_ecma_ast::CallExpr) {
    emit_expr(emitter, callee);
    emitter.push_str("(");
    emit_call_args(emitter, call_expr);
    emitter.push_str(")");
}

fn emit_call_args(emitter: &mut CodeEmitter, call_expr: &swc_ecma_ast::CallExpr) {
    for (i, arg) in call_expr.args.iter().enumerate() {
        if i > 0 {
            emitter.push_str(", ");
        }
        if arg.spread.is_some() {
            emitter.push_str("/* spread */");
        }
        emit_expr(emitter, &arg.expr);
    }
}
