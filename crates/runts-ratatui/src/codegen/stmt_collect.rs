//! Statement traversal for variable extraction.
//!
//! Extracts variable declarations and assignments from various statement types
//! to support the ratatui codegen.

use crate::codegen::vars;
use serde_json::Value;

/// Recursively collect declarations from any statement type.
pub fn collect_stmt(stmt: &Value, decls: &mut Vec<(String, String)>) {
    collect_by_kind(stmt.get("kind").and_then(|k| k.as_str()).unwrap_or(""), stmt, decls);
}

/// Recursively collect console method calls from any statement type.
pub fn collect_console_calls(stmt: &Value, calls: &mut Vec<String>) {
    let kind = stmt.get("kind").and_then(|k| k.as_str()).unwrap_or("");
    match kind {
        "Block" => collect_console_block(stmt, calls),
        "Expr" => collect_console_expr(stmt, calls),
        "Const" | "Let" | "Var" => collect_console_var(stmt, calls),
        _ => {}
    }
}

fn collect_console_block(stmt: &Value, calls: &mut Vec<String>) {
    if let Some(arr) = stmt.get("stmts").and_then(|s| s.as_array()) {
        for s in arr {
            collect_console_calls(s, calls);
        }
    }
}

fn collect_console_expr(stmt: &Value, calls: &mut Vec<String>) {
    if let Some(expr) = stmt.get("expr") {
        try_push_console(expr, calls);
        recurse_expr_for_console(expr, calls);
    }
}

fn recurse_expr_for_console(expr: &Value, calls: &mut Vec<String>) {
    if let Some(call) = expr.get("Call") {
        if let Some(args) = call.get("arguments").and_then(|a| a.as_array()) {
            for arg in args {
                recurse_expr_for_console(arg, calls);
            }
        }
    }
    if let Some(arrow) = expr.get("ArrowFunction") {
        if let Some(body) = arrow.get("body") {
            collect_console_in_body(body, calls);
        }
    }
    if let Some(func) = expr.get("Function") {
        if let Some(body) = func.get("body") {
            collect_console_in_body(body, calls);
        }
    }
}

fn collect_console_in_body(body: &Value, calls: &mut Vec<String>) {
    if let Some(block) = body.get("Block").and_then(|b| b.as_array()) {
        for stmt in block {
            collect_console_calls(stmt, calls);
        }
    } else {
        collect_console_calls(body, calls);
    }
}

fn collect_console_var(stmt: &Value, calls: &mut Vec<String>) {
    if let Some(init) = stmt.get("init") {
        try_push_console(init, calls);
    }
}

fn try_push_console(expr: &Value, calls: &mut Vec<String>) {
    if let Some((prop, args)) = extract_console_call(expr) {
        if let Some(code) = console_call_to_rust(prop, args) {
            calls.push(code);
        }
    }
}

fn extract_console_call(expr: &Value) -> Option<(&str, &[Value])> {
    let call = expr.get("Call")?;
    let member = call.get("callee")?.get("StaticMember")?;
    if !is_console_member(member) {
        return None;
    }
    let prop = member.get("property")?.as_str()?;
    let args = call.get("arguments")?.as_array()?;
    Some((prop, args.as_slice()))
}

fn is_console_member(member: &Value) -> bool {
    member.get("obj")
        .and_then(|o| o.get("Ident"))
        .and_then(|i| i.get("name"))
        .and_then(|n| n.as_str())
        == Some("console")
}

fn console_call_to_rust(method: &str, args: &[Value]) -> Option<String> {
    match method {
        "log" | "info" | "warn" | "error" => Some(fmt_console_print(method, args)),
        "time" => Some("();".to_string()),
        "timeEnd" => {
            let label = args.first().and_then(|a| a.get("String")?.as_str()).unwrap_or("");
            Some(format!("println!(\"{}: 0.000ms\");", label))
        }
        "table" => Some(fmt_console_table(args)),
        _ => None,
    }
}

fn fmt_console_print(method: &str, args: &[Value]) -> String {
    let macro_name = if method == "error" || method == "warn" {
        "eprintln"
    } else {
        "println"
    };
    let rust_args: Vec<String> = args.iter().filter_map(arg_to_rust_str).collect();
    if rust_args.is_empty() {
        return format!("{}!();", macro_name);
    }
    if rust_args.len() == 1 {
        return format!("{}!(\"{}\", {});", macro_name, "{}", rust_args[0]);
    }
    let fmt = "{}".repeat(rust_args.len());
    format!("{}!(\"{}\", {});", macro_name, fmt, rust_args.join(", "))
}

fn fmt_console_table(args: &[Value]) -> String {
    let rust_args: Vec<String> = args.iter().filter_map(arg_to_rust_str).collect();
    if rust_args.len() == 1 {
        format!("println!(\"{}\", {});", "{}", rust_args[0])
    } else {
        "();".to_string()
    }
}

fn arg_to_rust_str(arg: &Value) -> Option<String> {
    if let Some(s) = arg.get("String").and_then(|s| s.as_str()) {
        Some(format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"")))
    } else if let Some(n) = arg.as_f64() {
        Some(format!("{}f64", n))
    } else if let Some(n) = arg.get("Number").and_then(|n| n.as_f64()) {
        Some(format!("{}f64", n))
    } else if let Some(arr) = arg.get("Array") {
        Some(hir_array_to_json_expr(arr))
    } else if let Some(obj) = arg.get("Object") {
        Some(hir_object_to_json_expr(obj))
    } else {
        None
    }
}

fn hir_array_to_json_expr(arr: &Value) -> String {
    let empty = vec![];
    let elems = arr.get("elems").and_then(|e| e.as_array()).unwrap_or(&empty);
    let parts: Vec<String> = elems.iter().filter_map(arg_to_rust_str).collect();
    format!("serde_json::json!([{}])", parts.join(", "))
}

fn hir_object_to_json_expr(obj: &Value) -> String {
    let empty = vec![];
    let members = obj.get("members").and_then(|m| m.as_array()).unwrap_or(&empty);
    let mut pairs = Vec::new();
    for member in members {
        if let Some(prop) = member.get("prop") {
            let (key, val) = extract_prop_pair(prop);
            if let Some(v) = val {
                pairs.push(format!("\"{}\": {}", key, v));
            }
        }
    }
    format!("serde_json::json!({{{}}})", pairs.join(", "))
}

fn extract_prop_pair(prop: &Value) -> (String, Option<String>) {
    if let Some(init) = prop.get("Init") {
        let key = init.get("key")
            .and_then(|k| k.get("Str").or_else(|| k.get("String")))
            .and_then(|k| k.as_str())
            .unwrap_or("");
        let val = init.get("value").and_then(arg_to_rust_str);
        return (key.to_string(), val);
    }
    (String::new(), None)
}

fn collect_by_kind(kind: &str, stmt: &Value, decls: &mut Vec<(String, String)>) {
    match kind {
        "Block" => collect_block(stmt, decls),
        "Expr" => collect_expr(stmt, decls),
        "Const" | "Let" | "Var" => collect_var_decl(stmt, decls),
        // Don't recurse into loops - assignments inside loops can't be extracted
        // as they may reference loop variables not visible at top level
        "For" | "While" | "DoWhile" | "If" | "Switch" => {}
        _ => {}
    }
}

fn collect_block(stmt: &Value, decls: &mut Vec<(String, String)>) {
    if let Some(arr) = stmt.get("stmts").and_then(|s| s.as_array()) {
        for s in arr {
            collect_stmt(s, decls);
        }
    }
}

fn collect_expr(stmt: &Value, decls: &mut Vec<(String, String)>) {
    if let Some(expr) = stmt.get("expr") {
        if let Some(d) = try_extract_assign(expr) {
            decls.push(d);
        }
    }
}

fn collect_var_decl(stmt: &Value, decls: &mut Vec<(String, String)>) {
    if let Some(pattern) = stmt.get("pattern") {
        let init = stmt.get("init");
        if let Some(elems) = pattern.get("elems").and_then(|e| e.as_array()) {
            collect_array_destructure(elems, init, decls);
        } else if let Some(name) = pattern.get("name").and_then(|n| n.as_str()) {
            collect_simple_binding(name, init, decls);
        }
    }
}

fn collect_array_destructure(
    elems: &[Value],
    init: Option<&Value>,
    decls: &mut Vec<(String, String)>,
) {
    for elem in elems {
        if let Some(name) = elem.get("name").and_then(|n| n.as_str()) {
            if !name.is_empty() {
                let default_val = "0i32".to_string();
                let (value, type_hint) = extract_call_arg_value_with_type(init)
                    .unwrap_or_else(|| (default_val, None));
                let decl = format_var_decl(name, &value, type_hint.as_deref());
                decls.push((decl, name.to_string()));
            }
        }
    }
}

fn collect_simple_binding(name: &str, init: Option<&Value>, decls: &mut Vec<(String, String)>) {
    if name.is_empty() {
        return;
    }
    let default_val = "0i32".to_string();
    let (value, type_hint) = extract_call_arg_value_with_type(init)
        .unwrap_or_else(|| (default_val, None));
    let decl = format_var_decl(name, &value, type_hint.as_deref());
    decls.push((decl, name.to_string()));
}

fn format_var_decl(name: &str, value: &str, type_hint: Option<&str>) -> String {
    if let Some(ty) = type_hint {
        format!("let {}: {} = {};", name, ty, value)
    } else {
        format!("let {} = {};", name, value)
    }
}

#[allow(dead_code)]
fn collect_for(stmt: &Value, decls: &mut Vec<(String, String)>) {
    if let Some(body) = stmt.get("body") {
        collect_stmt(body, decls);
    }
}

#[allow(dead_code)]
fn collect_while(stmt: &Value, decls: &mut Vec<(String, String)>) {
    if let Some(body) = stmt.get("body") {
        collect_stmt(body, decls);
    }
}

#[allow(dead_code)]
fn collect_do_while(stmt: &Value, decls: &mut Vec<(String, String)>) {
    if let Some(body) = stmt.get("body") {
        collect_stmt(body, decls);
    }
}

#[allow(dead_code)]
fn collect_if(stmt: &Value, decls: &mut Vec<(String, String)>) {
    if let Some(consequent) = stmt.get("consequent") {
        collect_stmt(consequent, decls);
    }
    if let Some(alternate) = stmt.get("alternate") {
        collect_stmt(alternate, decls);
    }
}

#[allow(dead_code)]
fn collect_switch(stmt: &Value, decls: &mut Vec<(String, String)>) {
    if let Some(cases) = stmt.get("cases").and_then(|c| c.as_array()) {
        for case in cases {
            if let Some(consequent) = case.get("consequent") {
                collect_block(consequent, decls);
            }
        }
    }
}

// Re-export helper functions from vars module
pub fn try_extract_assign(expr: &Value) -> Option<(String, String)> {
    let assign = expr.get("Assign")?;
    let left = assign.get("left")?;
    if left.get("Array").is_some() {
        return vars::try_hook_destructuring(assign);
    }
    vars::simple_var_decl(assign)
}

pub fn extract_call_arg_value_with_type(init: Option<&Value>) -> Option<(String, Option<String>)> {
    let init = init?;
    if let Some(arr) = init.get("Array") {
        let has_nested = arr.get("elems")
            .and_then(|e| e.as_array())
            .map_or(false, |es| es.iter().any(|e| e.get("Array").is_some() || e.get("Object").is_some()));
        if has_nested {
            let rust_val = serde_json::to_string(arr).ok()?;
            return Some((format!("serde_json::json!({})", rust_val), Some("serde_json::Value".to_string())));
        }
        let rust_val = vars::expr_value_to_rust(&serde_json::json!({"Array": arr}))?;
        return Some((rust_val, Some("Vec<Value>".to_string())));
    }
    if let Some(obj) = init.get("Object") {
        let rust_val = hir_object_to_json_expr(obj);
        return Some((rust_val, Some("serde_json::Value".to_string())));
    }
    vars::expr_value_to_rust(init).map(|v| (v, None))
}
