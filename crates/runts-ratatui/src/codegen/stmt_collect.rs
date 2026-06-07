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

fn collect_for(stmt: &Value, decls: &mut Vec<(String, String)>) {
    if let Some(body) = stmt.get("body") {
        collect_stmt(body, decls);
    }
}

fn collect_while(stmt: &Value, decls: &mut Vec<(String, String)>) {
    if let Some(body) = stmt.get("body") {
        collect_stmt(body, decls);
    }
}

fn collect_do_while(stmt: &Value, decls: &mut Vec<(String, String)>) {
    if let Some(body) = stmt.get("body") {
        collect_stmt(body, decls);
    }
}

fn collect_if(stmt: &Value, decls: &mut Vec<(String, String)>) {
    if let Some(consequent) = stmt.get("consequent") {
        collect_stmt(consequent, decls);
    }
    if let Some(alternate) = stmt.get("alternate") {
        collect_stmt(alternate, decls);
    }
}

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
        let rust_val = vars::expr_value_to_rust(&serde_json::json!({"Array": arr}))?;
        return Some((rust_val, Some("Vec<Value>".to_string())));
    }
    if init.get("Object").is_some() {
        let rust_val = serde_json::to_string(init).ok()?;
        return Some((format!("serde_json::json!({})", rust_val), Some("serde_json::Value".to_string())));
    }
    vars::expr_value_to_rust(init).map(|v| (v, None))
}
