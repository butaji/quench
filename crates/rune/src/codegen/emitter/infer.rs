//! # Type Inference
//!
//! Infers Rust types from TypeScript expressions.

use swc_ecma_ast::{Callee, Expr, Lit};

/// Infer the type of an expression as a Rust type string.
#[allow(clippy::too_many_lines)]
pub fn infer_type(expr: &Expr) -> String {
    match expr {
        Expr::Lit(lit) => infer_literal_type(lit),
        Expr::Array(arr) => infer_array_type(arr),
        Expr::Bin(bin_expr) => infer_bin_type(bin_expr),
        Expr::Unary(unary_expr) => infer_unary_type(unary_expr),
        Expr::Call(call_expr) => infer_call_type(call_expr),
        Expr::Member(member_expr) => infer_member_type(member_expr),
        Expr::Cond(cond_expr) => infer_cond_type(cond_expr),
        Expr::Object(_) => "()".to_string(),
        Expr::Tpl(_) => "String".to_string(),
        Expr::Arrow(_) => "()".to_string(),
        Expr::Paren(paren) => infer_type(&paren.expr),
        Expr::Await(await_expr) => infer_type(&await_expr.arg),
        _ => "()".to_string(),
    }
}

fn infer_literal_type(lit: &Lit) -> String {
    match lit {
        Lit::Num(n) => infer_number_type(n),
        Lit::Str(_) => "String".to_string(),
        Lit::Bool(_) => "bool".to_string(),
        Lit::BigInt(_) => "i64".to_string(),
        Lit::Null(_) => "Option<()>".to_string(),
        _ => "()".to_string(),
    }
}

fn infer_number_type(n: &swc_ecma_ast::Number) -> String {
    if n.value.fract() == 0.0 && n.value.abs() < f64::from(i32::MAX) {
        "i32".to_string()
    } else {
        "f64".to_string()
    }
}

fn infer_array_type(arr: &swc_ecma_ast::ArrayLit) -> String {
    if arr.elems.is_empty() {
        return String::from("Vec<()>");
    }
    arr.elems.first().and_then(|e| e.as_ref()).map_or(
        String::from("Vec<()>"),
        |elem| format!("Vec<{}>", infer_type(&elem.expr)),
    )
}

fn infer_unary_type(unary_expr: &swc_ecma_ast::UnaryExpr) -> String {
    match unary_expr.op {
        swc_ecma_ast::UnaryOp::Bang | swc_ecma_ast::UnaryOp::TypeOf => "bool".to_string(),
        _ => infer_type(&unary_expr.arg),
    }
}

fn infer_cond_type(cond_expr: &swc_ecma_ast::CondExpr) -> String {
    let cons_type = infer_type(&cond_expr.cons);
    let alt_type = infer_type(&cond_expr.alt);
    resolve_common_type(&cons_type, &alt_type)
}

fn resolve_common_type(cons_type: &str, alt_type: &str) -> String {
    if cons_type == alt_type || alt_type == "()" {
        cons_type.to_string()
    } else {
        alt_type.to_string()
    }
}

fn infer_bin_type(bin_expr: &swc_ecma_ast::BinExpr) -> String {
    let left = infer_type(&bin_expr.left);
    let right = infer_type(&bin_expr.right);
    infer_bin_op_type(&left, &right, bin_expr.op)
}

fn infer_bin_op_type(left: &str, right: &str, op: swc_ecma_ast::BinaryOp) -> String {
    if op == swc_ecma_ast::BinaryOp::Add && (left == "String" || right == "String") {
        return "String".to_string();
    }

    if is_comparison_op(op) || is_logical_op(op) {
        return "bool".to_string();
    }

    if is_bitwise_op(op) {
        return "i32".to_string();
    }

    if left == "i32" || right == "i32" {
        "i32".to_string()
    } else {
        "f64".to_string()
    }
}

fn is_comparison_op(op: swc_ecma_ast::BinaryOp) -> bool {
    matches!(
        op,
        swc_ecma_ast::BinaryOp::EqEqEq
            | swc_ecma_ast::BinaryOp::NotEqEq
            | swc_ecma_ast::BinaryOp::Lt
            | swc_ecma_ast::BinaryOp::LtEq
            | swc_ecma_ast::BinaryOp::Gt
            | swc_ecma_ast::BinaryOp::GtEq
    )
}

fn is_logical_op(op: swc_ecma_ast::BinaryOp) -> bool {
    matches!(op, swc_ecma_ast::BinaryOp::LogicalAnd | swc_ecma_ast::BinaryOp::LogicalOr)
}

fn is_bitwise_op(op: swc_ecma_ast::BinaryOp) -> bool {
    matches!(
        op,
        swc_ecma_ast::BinaryOp::BitAnd
            | swc_ecma_ast::BinaryOp::BitOr
            | swc_ecma_ast::BinaryOp::BitXor
            | swc_ecma_ast::BinaryOp::LShift
            | swc_ecma_ast::BinaryOp::RShift
    )
}

fn infer_call_type(call_expr: &swc_ecma_ast::CallExpr) -> String {
    let Callee::Expr(callee) = &call_expr.callee else {
        return "()".to_string();
    };

    match &**callee {
        Expr::Ident(ident) => infer_direct_call_type(ident.sym.as_ref()),
        Expr::Member(member) => infer_method_call_type(member, call_expr),
        _ => "()".to_string(),
    }
}

fn infer_direct_call_type(fn_name: &str) -> String {
    match fn_name {
        "filter_tasks" => "Vec<Task>".to_string(),
        "create_task" => "Task".to_string(),
        "toggle_task" => "Task".to_string(),
        "validate_title" => "Result<String, String>".to_string(),
        "validate_task" => "Result<Task, String>".to_string(),
        "parse_json" => "Result<JsonValue, String>".to_string(),
        "serialize_tasks" => "String".to_string(),
        "deserialize_tasks" => "Result<Vec<Task>, String>".to_string(),
        "merge_tasks" => "Vec<Task>".to_string(),
        "find_task" => "Option<Task>".to_string(),
        "sort_tasks" => "Vec<Task>".to_string(),
        "get_stats" => "Stats".to_string(),
        "is_number" | "is_string" | "is_boolean" | "is_object" => "bool".to_string(),
        "fast_sqrt" => "f64".to_string(),
        "batch_add" => "Vec<f64>".to_string(),
        "mean" | "variance" | "std_dev" => "f64".to_string(),
        _ => "()".to_string(),
    }
}

fn infer_method_call_type(
    member: &swc_ecma_ast::MemberExpr,
    call_expr: &swc_ecma_ast::CallExpr,
) -> String {
    let obj_type = infer_type(&member.obj);
    let method = extract_method_name(&member.prop);
    infer_array_or_string_method(&obj_type, method, call_expr)
}

fn extract_method_name(prop: &swc_ecma_ast::MemberProp) -> &str {
    if let swc_ecma_ast::MemberProp::Ident(ident) = prop {
        ident.sym.as_ref()
    } else {
        ""
    }
}

fn infer_array_or_string_method(
    obj_type: &str,
    method: &str,
    call_expr: &swc_ecma_ast::CallExpr,
) -> String {
    match method {
        "filter" | "map" | "concat" | "slice" | "flat" | "flatMap" => obj_type.to_string(),
        "find" | "findIndex" => unwrap_vec_element(obj_type),
        "some" | "every" | "includes" | "startsWith" | "endsWith" => "bool".to_string(),
        "push" => "usize".to_string(),
        "pop" | "shift" => "Option<()>".to_string(),
        "reduce" => infer_reduce_return_type(call_expr),
        "trim" | "toLowerCase" | "toUpperCase" | "trimStart" | "trimEnd"
        | "substring" | "substr" | "toString" => "String".to_string(),
        "indexOf" | "lastIndexOf" => "Option<usize>".to_string(),
        "charAt" => "Option<char>".to_string(),
        "join" => "String".to_string(),
        "split" => "Vec<String>".to_string(),
        "length" | "len" => "usize".to_string(),
        "forEach" => "()".to_string(),
        _ => "()".to_string(),
    }
}

fn unwrap_vec_element(obj_type: &str) -> String {
    if obj_type.starts_with("Vec") && obj_type.ends_with('>') {
        let inner = &obj_type[4..obj_type.len() - 1];
        format!("Option<{}>", inner)
    } else {
        "Option<()>".to_string()
    }
}

fn infer_reduce_return_type(call_expr: &swc_ecma_ast::CallExpr) -> String {
    call_expr
        .args
        .get(1)
        .map_or_else(|| "()".to_string(), |arg| infer_type(&arg.expr))
}

fn infer_member_type(member_expr: &swc_ecma_ast::MemberExpr) -> String {
    let obj_type = infer_type(&member_expr.obj);
    let prop_name = extract_method_name(&member_expr.prop);
    infer_property_type(&obj_type, prop_name)
}

fn infer_property_type(obj_type: &str, prop_name: &str) -> String {
    // Task properties
    if let Some(t) = try_task_property(obj_type, prop_name) {
        return t;
    }

    // AppState properties
    if let Some(t) = try_appstate_property(obj_type, prop_name) {
        return t;
    }

    // Result pattern
    if let Some(t) = try_result_property(prop_name) {
        return t;
    }

    // Common string/array methods
    if let Some(t) = try_common_method_property(prop_name) {
        return t;
    }

    "()".to_string()
}

fn try_task_property(obj_type: &str, prop: &str) -> Option<String> {
    match (obj_type, prop) {
        ("Task", "id") => Some("i32".to_string()),
        ("Task", "title") => Some("String".to_string()),
        ("Task", "done") => Some("bool".to_string()),
        _ => None,
    }
}

fn try_appstate_property(obj_type: &str, prop: &str) -> Option<String> {
    match (obj_type, prop) {
        ("AppState", "tasks") => Some("Vec<Task>".to_string()),
        ("AppState", "selected") => Some("usize".to_string()),
        ("AppState", "filter") => Some("Filter".to_string()),
        ("AppState", "shouldExit") => Some("bool".to_string()),
        _ => None,
    }
}

fn try_result_property(prop: &str) -> Option<String> {
    match prop {
        "ok" => Some("bool".to_string()),
        "value" => Some("()".to_string()),
        "error" => Some("String".to_string()),
        _ => None,
    }
}

fn try_common_method_property(prop: &str) -> Option<String> {
    match prop {
        "length" | "len" => Some("usize".to_string()),
        "push" => Some("usize".to_string()),
        "pop" | "shift" => Some("Option<()>".to_string()),
        "filter" | "map" | "concat" | "slice" => Some("()".to_string()),
        "find" | "findIndex" => Some("Option<()>".to_string()),
        "some" | "every" | "includes" | "startsWith" | "endsWith" => Some("bool".to_string()),
        "trim" | "toLowerCase" | "toUpperCase" | "trimStart" | "trimEnd"
        | "substring" | "substr" | "toString" => Some("String".to_string()),
        _ => None,
    }
}
