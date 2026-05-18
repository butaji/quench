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

    match &**callee {
        Expr::Member(member) => handle_member_call(emitter, member, call_expr),
        Expr::Ident(ident) => emit_direct_call(emitter, ident, call_expr),
        _ => emit_generic_call(emitter, callee, call_expr),
    }
}

fn handle_member_call(
    emitter: &mut CodeEmitter,
    member: &swc_ecma_ast::MemberExpr,
    call_expr: &swc_ecma_ast::CallExpr,
) {
    let Expr::Ident(ident) = &*member.obj else {
        emit_method_or_generic_call(emitter, member, call_expr);
        return;
    };

    let obj_name = ident.sym.as_ref();
    let MemberProp::Ident(prop) = &member.prop else {
        emit_method_or_generic_call(emitter, member, call_expr);
        return;
    };

    let method = prop.sym.as_ref();
    match obj_name {
        "Date" => emit_date_now(emitter),
        "JSON" => emit_json_method(emitter, method, call_expr),
        "Math" => emit_math_call(emitter, method, call_expr),
        _ => emit_method_or_generic_call(emitter, member, call_expr),
    }
}

fn emit_date_now(emitter: &mut CodeEmitter) {
    emitter.push_str(
        "(std::time::SystemTime::now()\
        .duration_since(std::time::UNIX_EPOCH)\
        .unwrap()\
        .as_millis() / 1000) as i32",
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
    match method {
        "floor" => emit_math_unary(emitter, "f64::floor", call_expr),
        "ceil" => emit_math_unary(emitter, "f64::ceil", call_expr),
        "round" => emit_math_unary(emitter, "f64::round", call_expr),
        "abs" => emit_math_unary(emitter, "f64::abs", call_expr),
        "sqrt" => emit_math_unary(emitter, "f64::sqrt", call_expr),
        "pow" => emit_math_binary_pow(emitter, call_expr),
        "max" => emit_math_binary(emitter, "std::cmp::max", call_expr),
        "min" => emit_math_binary(emitter, "std::cmp::min", call_expr),
        "random" => emitter.push_str("rand::random::<f64>()"),
        "parseFloat" => emit_parse_float(emitter, call_expr),
        "parseInt" => emit_parse_int(emitter, call_expr),
        _ => {
            emitter.push_str(method);
            emitter.push_str("(");
            emit_call_args(emitter, call_expr);
            emitter.push_str(")");
        }
    }
}

fn emit_math_unary(emitter: &mut CodeEmitter, fn_path: &str, call_expr: &swc_ecma_ast::CallExpr) {
    emitter.push_str(fn_path);
    emitter.push_str("(");
    emit_call_args(emitter, call_expr);
    emitter.push_str(")");
}

fn emit_math_binary(emitter: &mut CodeEmitter, fn_path: &str, call_expr: &swc_ecma_ast::CallExpr) {
    emitter.push_str(fn_path);
    emitter.push_str("(");
    emit_call_args(emitter, call_expr);
    emitter.push_str(")");
}

fn emit_math_binary_pow(emitter: &mut CodeEmitter, call_expr: &swc_ecma_ast::CallExpr) {
    emitter.push_str("f64::powf(");
    emit_call_args(emitter, call_expr);
    emitter.push_str(")");
}

/// Emit parseFloat: converts string to Option<f64>
fn emit_parse_float(emitter: &mut CodeEmitter, call_expr: &swc_ecma_ast::CallExpr) {
    if let Some(arg) = call_expr.args.first() {
        emitter.push_str("{");
        emitter.push_str(" let s = ");
        emit_expr(emitter, &arg.expr);
        emitter.push_str("; s.trim().parse::<f64>().ok() ");
        emitter.push_str("}");
    } else {
        emitter.push_str("None");
    }
}

/// Emit parseInt: converts string to Option<i32>
fn emit_parse_int(emitter: &mut CodeEmitter, call_expr: &swc_ecma_ast::CallExpr) {
    if let Some(arg) = call_expr.args.first() {
        emitter.push_str("{");
        emitter.push_str(" let s = ");
        emit_expr(emitter, &arg.expr);
        emitter.push_str("; s.trim().parse::<i32>().ok() ");
        emitter.push_str("}");
    } else {
        emitter.push_str("None");
    }
}

fn emit_method_or_generic_call(
    emitter: &mut CodeEmitter,
    member: &swc_ecma_ast::MemberExpr,
    call_expr: &swc_ecma_ast::CallExpr,
) {
    let MemberProp::Ident(prop) = &member.prop else {
        emit_generic_member_call(emitter, member, call_expr);
        return;
    };

    let method = prop.sym.as_ref();
    if is_array_method(method) {
        emit_array_call(emitter, member, method, call_expr);
    } else if is_string_method(method) {
        emit_string_call(emitter, member, method, call_expr);
    } else {
        emit_generic_member_call(emitter, member, call_expr);
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
    match method {
        "slice" => {
            emit_array_slice(emitter, member, call_expr);
        }
        "splice" => {
            emit_expr(emitter, &member.obj);
            emit_array_splice(emitter, call_expr);
        }
        "get" => {
            emit_expr(emitter, &member.obj);
            emit_array_get(emitter, call_expr);
        }
        _ => {
            emit_expr(emitter, &member.obj);
            emit_array_iter_method(emitter, method, call_expr);
        }
    }
}

fn emit_array_slice(
    emitter: &mut CodeEmitter,
    member: &swc_ecma_ast::MemberExpr,
    call_expr: &swc_ecma_ast::CallExpr,
) {
    emit_expr(emitter, &member.obj);
    emitter.push_str(".as_slice()[");

    if let Some(start_arg) = call_expr.args.first() {
        emit_expr(emitter, &start_arg.expr);
        emitter.push_str(" as usize..");

        if call_expr.args.len() >= 2 {
            let end_arg = &call_expr.args[1].expr;
            // Check if the end argument is a negative number (slice with negative end)
            if let swc_ecma_ast::Expr::Unary(unary) = &**end_arg {
                if unary.op == swc_ecma_ast::UnaryOp::Minus {
                    if let swc_ecma_ast::Expr::Lit(swc_ecma_ast::Lit::Num(n)) = &*unary.arg {
                        // Negative end: slice(0, -n) means slice(0, len - n)
                        // For len - n, we use the object's length
                        emitter.push_str("(");
                        emit_expr(emitter, &member.obj);
                        emitter.push_str(".len() - ");
                        emitter.push_str(&format!("{}", n.value as i32));
                        emitter.push_str(" as usize)");
                        emitter.push_str("]");
                        return;
                    }
                }
            }
            // Normal case: positive end index
            emit_expr(emitter, end_arg);
            emitter.push_str(" as usize");
        } else {
            // Single arg: slice from start to end of array
            emitter.push_str("..");
            emit_expr(emitter, &member.obj);
            emitter.push_str(".len() as usize]");
            return;
        }
    }
    emitter.push_str("]");
}

fn emit_array_splice(emitter: &mut CodeEmitter, call_expr: &swc_ecma_ast::CallExpr) {
    // JavaScript: splice(start, deleteCount?) - removes elements starting at start
    // If deleteCount omitted, removes from start to end of array
    // Note: Rust's splice requires explicit range end, so we compute at runtime
    emitter.push_str(".splice(");
    if let Some(start_arg) = call_expr.args.first() {
        emit_expr(emitter, &start_arg.expr);
        emitter.push_str("..");
        if call_expr.args.len() >= 2 {
            // deleteCount provided: start + deleteCount
            emitter.push_str("(");
            emit_expr(emitter, &start_arg.expr);
            emitter.push_str(" + ");
            emit_expr(emitter, &call_expr.args[1].expr);
            emitter.push_str(")");
        } else {
            // No deleteCount: remove to end - use large value, Rust will clamp
            emitter.push_str("usize::MAX");
        }
    }
    emitter.push_str(", vec![])");
}

/// Emit array index access.
///
/// In JavaScript, `arr.get(idx)` is syntactic sugar for `arr[idx]`.
/// Unlike Rust's `.get()`, JavaScript's array access always returns T, not Option<T>.
/// We emit `arr[idx]` (direct indexing) to match JavaScript semantics.
fn emit_array_get(emitter: &mut CodeEmitter, call_expr: &swc_ecma_ast::CallExpr) {
    emitter.push_str("[");
    if let Some(arg) = call_expr.args.first() {
        emit_expr(emitter, &arg.expr);
        emitter.push_str(" as usize");
    }
    emitter.push_str("]");
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
        "filter" => {
            emitter.push_str("filter(");
            emit_call_args(emitter, call_expr);
            emitter.push_str(").cloned().collect::<Vec<_>>()");
            return;
        }
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
