//! Variable extraction and codegen

use super::ink_widget::tag_to_ink;
use super::stmt_collect::collect_stmt;
use super::traversal::{extract_jsx_attrs, extract_jsx_children, find_jsx_in_body};
use once_cell::sync::Lazy;
use serde_json::Value;
use std::collections::HashSet;
use std::sync::Mutex;

static STATE_VARS: Lazy<Mutex<HashSet<String>>>=
    Lazy::new(|| Mutex::new(HashSet::new()));

pub(crate) fn clear_state_vars() {
    STATE_VARS.lock().unwrap().clear();
}

pub(crate) fn add_state_var(var: &str) {
    STATE_VARS.lock().unwrap().insert(var.to_string());
}

pub(crate) fn get_state_vars() -> Vec<String> {
    STATE_VARS.lock().unwrap().clone().into_iter().collect()
}

// ---------------------------------------------------------------------------
// Main variable extraction
// ---------------------------------------------------------------------------

pub(crate) fn extract_var_declarations(body: &Value) -> Vec<(String, String)> {
    let mut decls = Vec::new();
    let stmts = if let Some(arr) = body.as_array() {
        arr.clone()
    } else if let Some(arr) = body.get("Block").and_then(|b| b.get("stmts")).and_then(|s| s.as_array()) {
        arr.clone()
    } else {
        return decls;
    };
    for s in &stmts {
        collect_stmt(s, &mut decls);
    }
    decls
}

// ---------------------------------------------------------------------------
// Hook destructuring (from stmt_collect re-export)
// ---------------------------------------------------------------------------

pub(crate) fn simple_var_decl(assign: &Value) -> Option<(String, String)> {
    let name = left_ident_name(assign.get("left")?)?;
    let value = assign.get("right")?;
    let rust_val = expr_value_to_rust(value)?;
    let kw = if assign.get("kind").and_then(|k| k.as_str()) == Some("Decl") {
        "const"
    } else {
        "let"
    };
    Some((format!("{} {} = {};", kw, name, rust_val), name))
}

fn left_ident_name(left: &Value) -> Option<String> {
    left.get("Ident")?.get("name")?.as_str().map(String::from)
}

pub(crate) fn try_hook_destructuring(assign: &Value) -> Option<(String, String)> {
    let state_name = extract_first_ident(assign.get("left")?)?;
    let init = try_use_state_init(assign.get("right")?)?;
    add_state_var(&state_name);
    Some((format!("let {} = std::cell::Cell::new({});", state_name, init), state_name))
}

fn extract_first_ident(left: &Value) -> Option<String> {
    left.get("Array")?
        .get("elems")?
        .as_array()?
        .first()?
        .get("Ident")?
        .get("name")?
        .as_str()
        .map(String::from)
}

fn try_use_state_init(call: &Value) -> Option<String> {
    let call_expr = call.get("Call")?;
    let callee = call_expr.get("callee")?;
    if !is_use_state_callee(callee) {
        return None;
    }
    let args = call_expr.get("args")?.as_array()?;
    let first = args.first()?.as_object()?;
    extract_init_value(first)
}

fn is_use_state_callee(callee: &Value) -> bool {
    if let Some(member) = callee.get("Member") {
        if let Some(prop) = member.get("property") {
            if let Some(name) = prop.get("name") {
                return name.as_str() == Some("useState");
            }
        }
    }
    if let Some(ident) = callee.get("Ident") {
        if let Some(name) = ident.get("name") {
            return name.as_str() == Some("useState");
        }
    }
    false
}

fn extract_init_value(obj: &serde_json::Map<String, serde_json::Value>) -> Option<String> {
    if let Some(n) = obj.get("Number").and_then(|n| n.as_f64()) {
        return Some(num_to_rust(n));
    }
    if let Some(s) = obj.get("String").and_then(|s| s.as_str()) {
        return Some(escape_str(s));
    }
    if let Some(b) = obj.get("Bool").and_then(|b| b.as_bool()) {
        return Some(b.to_string());
    }
    if let Some(expr) = obj.get("Expr") {
        if let Some(n) = expr.get("Number").and_then(|n| n.as_f64()) {
            return Some(num_to_rust(n));
        }
    }
    Some("0i32".to_string())
}

fn num_to_rust(n: f64) -> String {
    if n.fract() == 0.0 {
        format!("{}i32", n as i64)
    } else {
        format!("{}f64", n)
    }
}

fn escape_str(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

// ---------------------------------------------------------------------------
// Expression to Rust
// ---------------------------------------------------------------------------

pub(crate) fn expr_value_to_rust(value: &Value) -> Option<String> {
    if let Some(n) = value.as_f64() {
        return Some(num_to_rust(n));
    }
    let map = value.as_object()?;
    if let Some(r) = try_simple_literal(map) {
        return Some(r);
    }
    if let Some(inner) = map.get("Expr") {
        return expr_value_to_rust(inner);
    }
    if let Some(cond) = map.get("Cond") {
        return try_cond_to_rust(cond);
    }
    if let Some(arr) = map.get("Array") {
        return try_array_to_rust(arr);
    }
    if map.contains_key("Object") {
        return serde_json::to_string(value).ok().map(|j| format!("serde_json::json!({})", j));
    }
    // Handle Call expressions (function calls)
    if let Some(call) = map.get("Call") {
        return try_call_to_rust(call);
    }
    None
}

/// Convert a function call expression to Rust
fn try_call_to_rust(call: &Value) -> Option<String> {
    let callee = call.get("callee")?;
    let args = call.get("arguments")?.as_array()?;
    
    // Handle method calls: obj.method(args...)
    if let Some(static_member) = callee.get("StaticMember") {
        return try_method_call_to_rust(static_member, args);
    }
    
    // Direct function call: fn(arg1, arg2)
    if let Some(fn_name) = extract_ident_name(callee) {
        let rust_name = map_js_global_fn(fn_name);
        let is_global = rust_name.starts_with("runts_ink::");
        return Some(format_fn_call(rust_name, args.iter().filter_map(|a| {
            if is_global && is_ident_expr(a) {
                Some(format!("&{}", expr_value_to_rust(a)?))
            } else {
                expr_value_to_rust(a)
            }
        })));
    }
    
    None
}

/// Handle method calls like fn.call, fn.apply, fn.bind
fn try_method_call_to_rust(static_member: &Value, args: &[Value]) -> Option<String> {
    let obj = static_member.get("obj")?;
    let property = static_member.get("property")?.as_str()?;
    let fn_name = extract_ident_name(obj)?;
    
    if property == "call" {
        return Some(format_fn_call(fn_name, args.iter().skip(1).filter_map(|a| expr_value_to_rust(a))));
    }
    if property == "apply" {
        return try_apply_call(fn_name, args);
    }
    if property == "bind" {
        return Some(format_bind_call(fn_name, args.iter().skip(1).filter_map(|a| expr_value_to_rust(a))));
    }
    try_regular_method_call(obj, property, args)
}

/// Extract the name from an identifier expression
fn extract_ident_name(value: &Value) -> Option<&str> {
    value.get("Ident")?.get("name")?.as_str()
}

/// Check whether a JSON expression value is a simple identifier.
fn is_ident_expr(value: &Value) -> bool {
    value.get("Ident").is_some()
        || value.as_object().map_or(false, |m| m.get("kind").and_then(|k| k.as_str()) == Some("Ident"))
}

/// Map known JavaScript global functions to their `runts_ink` equivalents.
fn map_js_global_fn(name: &str) -> &str {
    match name {
        "encodeURI" => "runts_ink::encodeURI",
        "encodeURIComponent" => "runts_ink::encodeURIComponent",
        "decodeURI" => "runts_ink::decodeURI",
        "decodeURIComponent" => "runts_ink::decodeURIComponent",
        _ => name,
    }
}

/// Format a function call: fn(arg1, arg2, ...)
fn format_fn_call<'a, I>(fn_name: &str, args: I) -> String 
where I: Iterator<Item = String> {
    let args_str: Vec<String> = args.collect();
    if args_str.is_empty() {
        fn_name.to_string()
    } else {
        format!("{}({})", fn_name, args_str.join(", "))
    }
}

/// Format a bound function: move || fn(arg1, arg2, ...)
fn format_bind_call<'a, I>(fn_name: &str, bound_args: I) -> String 
where I: Iterator<Item = String> {
    let args_str: Vec<String> = bound_args.collect();
    if args_str.is_empty() {
        format!("move || {}", fn_name)
    } else {
        format!("move || {}({})", fn_name, args_str.join(", "))
    }
}

/// Handle fn.apply(null, [arg1, arg2])
fn try_apply_call(fn_name: &str, args: &[Value]) -> Option<String> {
    if args.len() >= 2 {
        let arr = args[1].get("Array")?;
        let elems = arr.get("elems")?.as_array()?;
        let rust_args: Vec<String> = elems.iter().filter_map(|a| expr_value_to_rust(a)).collect();
        return Some(format_fn_call(fn_name, rust_args.into_iter()));
    }
    Some(format_fn_call(fn_name, args.iter().skip(1).filter_map(|a| expr_value_to_rust(a))))
}

/// Handle regular method calls: obj.method(args)
/// Only emits calls for known Rust-compatible methods; returns None for
/// unknown methods so the variable falls back to a safe default.
fn try_regular_method_call(obj: &Value, property: &str, args: &[Value]) -> Option<String> {
    let obj_name = extract_ident_name(obj)?;
    // Whitelist of methods that have direct Rust equivalents on the generated types.
    match property {
        "toString" | "to_string" => {
            return Some(format!("{}.to_string()", obj_name));
        }
        _ => {}
    }
    // Unknown method: skip to avoid invalid Rust codegen.
    let _ = (args, property);
    None
}

fn try_simple_literal(map: &serde_json::Map<String, serde_json::Value>) -> Option<String> {
    map.get("String")
        .and_then(|v| v.as_str())
        .map(escape_str)
        .or_else(|| {
            map.get("Number")
                .and_then(|v| v.as_f64())
                .map(num_to_rust)
        })
        .or_else(|| map.get("Boolean").and_then(|v| v.as_bool()).map(|b| b.to_string()))
        .or_else(|| map.get("Ident")?.get("name")?.as_str().map(String::from))
}

fn try_cond_to_rust(cond: &Value) -> Option<String> {
    let test = expr_value_to_rust(cond.get("test")?)?;
    let cons = expr_value_to_rust(cond.get("consequent")?)?;
    let alt = expr_value_to_rust(cond.get("alternate")?)?;
    Some(format!("if {} {{ {} }} else {{ {} }}", test, cons, alt))
}

fn try_array_to_rust(arr: &Value) -> Option<String> {
    let elems = arr.get("elems")?.as_array()?;
    let has_nested = elems.iter().any(|e| e.get("Array").is_some() || e.get("Object").is_some());
    if has_nested {
        let json = serde_json::to_string(arr).ok()?;
        return Some(format!("serde_json::json!({})", json));
    }
    let parts: Vec<String> = elems.iter().filter_map(|e| {
        expr_value_to_rust(e).map(|v| format!("{}.into()", v))
    }).collect();
    if parts.is_empty() {
        Some("vec![]".to_string())
    } else {
        Some(format!("vec![{}]", parts.join(", ")))
    }
}

// ---------------------------------------------------------------------------
// Main codegen
// ---------------------------------------------------------------------------

pub(crate) fn try_codegen_jsx(items: &Value) -> Option<String> {
    let arr = items.as_array()?;
    for item in arr {
        if let Some((jsx, decls, console)) = extract_jsx_from_function_with_vars(item) {
            let code = generate_widget_for_jsx(jsx)?;
            return Some(wrap_ink_main(&code, &decls, &console));
        }
    }
    None
}

pub(crate) fn extract_jsx_from_function_with_vars(
    item: &Value,
) -> Option<(Value, Vec<(String, String)>, Vec<String>)> {
    let func = item.get("Decl").and_then(|d| d.get("Function"))
        .or_else(|| {
            let stmt = item.get("Stmt")?;
            if stmt.get("kind")?.as_str()? == "ExportDefault" {
                stmt.get("expr")?.get("Function")
            } else {
                None
            }
        })
        .or_else(|| {
            let stmt = item.get("Stmt")?;
            if stmt.get("kind")?.as_str()? == "Return" {
                stmt.get("arg")?.get("Function")
            } else {
                None
            }
        })?;
    let body = func.get("body")?;
    clear_state_vars();
    let decls = extract_var_declarations(body);
    let mut console = Vec::new();
    let stmts = if let Some(arr) = body.as_array() {
        arr.clone()
    } else if let Some(arr) = body.get("Block").and_then(|b| b.as_array()) {
        arr.clone()
    } else {
        vec![body.clone()]
    };
    for stmt in &stmts {
        crate::codegen::stmt_collect::collect_console_calls(stmt, &mut console);
    }
    let jsx = find_jsx_in_body(body)?;
    Some((jsx, decls, console))
}

fn generate_widget_for_jsx(jsx: Value) -> Option<String> {
    let tag = jsx
        .get("opening")?
        .get("name")?
        .get("Ident")?
        .as_str()
        .unwrap_or("Box");
    let attrs = extract_jsx_attrs(jsx.get("opening")?.get("attrs")?).unwrap_or_default();
    let children = extract_jsx_children(jsx.get("children").unwrap_or(&Value::Null))
        .unwrap_or_default();
    Some(tag_to_ink(tag, attrs, children).to_string())
}

fn wrap_ink_main(vnode_expr: &str, decls: &[(String, String)], console: &[String]) -> String {
    let use_section = build_use_section(decls);
    let vars_section = build_code_section(decls.iter().map(|(c, _)| c.as_str()));
    let console_section = build_code_section(console.iter().map(|s| s.as_str()));
    format!(
        "//! Ink app entry: generated by runts-ratatui 0.1\n\
        use runts_ink;\n\
        fn main() -> anyhow::Result<()> {{\n{}\n{}\n{}\
        let root: runts_ink::VNode = {}.into();\n\
        let rendered = runts_ink::render_to_string(root, runts_ink::RenderOptions::default())?;\n\
        print!(\"{{}}\", rendered);\n\
        Ok(())\n\
        }}\n",
        use_section, vars_section, console_section, vnode_expr
    )
}

fn build_use_section(decls: &[(String, String)]) -> String {
    let mut items = Vec::new();
    if !get_state_vars().is_empty() || !decls.is_empty() {
        items.push("use std::cell::Cell;");
    }
    if decls.iter().any(|(c, _)| c.contains("Vec<Value>")) {
        items.push("use serde_json::Value;");
    }
    if items.is_empty() {
        String::new()
    } else {
        items.iter().map(|u| format!("    {}\n", u)).collect()
    }
}

fn build_code_section<'a>(codes: impl Iterator<Item = &'a str>) -> String {
    let lines: Vec<String> = codes.map(|c| format!("    {}", c)).collect();
    if lines.is_empty() {
        String::new()
    } else {
        lines.join("\n") + "\n"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_var() {
        clear_state_vars();
        let body = serde_json::json!([
            {"kind": "Expr", "expr": {"Assign": {"left": {"Ident": {"name": "count"}}, "right": {"Number": 0.0}, "kind": "Decl"}}}
        ]);
        let decls = extract_var_declarations(&body);
        assert_eq!(decls.len(), 1);
        assert_eq!(decls[0].1, "count");
        assert!(decls[0].0.contains("0i32"));
    }

    #[test]
    fn test_use_state() {
        clear_state_vars();
        let body = serde_json::json!([
            {"kind": "Expr", "expr": {"Assign": {"left": {"Array": {"elems": [{"Ident": {"name": "count"}}, {"Ident": {"name": "setCount"}}]}}, "right": {"Call": {"callee": {"Ident": {"name": "useState"}}, "args": [{"Number": 0.0}]}}, "kind": "Decl"}}}
        ]);
        let decls = extract_var_declarations(&body);
        assert_eq!(decls.len(), 1);
        assert_eq!(decls[0].1, "count");
        assert!(decls[0].0.contains("Cell::new"));
        assert!(get_state_vars().contains(&"count".to_string()));
    }

    #[test]
    fn test_react_use_state() {
        clear_state_vars();
        let body = serde_json::json!([
            {"kind": "Expr", "expr": {"Assign": {"left": {"Array": {"elems": [{"Ident": {"name": "enabled"}}, {"Ident": {"name": "setEnabled"}}]}}, "right": {"Call": {"callee": {"Member": {"property": {"name": "useState"}}}, "args": [{"Bool": false}]}}, "kind": "Decl"}}}
        ]);
        let decls = extract_var_declarations(&body);
        assert_eq!(decls.len(), 1);
        assert_eq!(decls[0].1, "enabled");
        assert!(decls[0].0.contains("false"));
    }
}
