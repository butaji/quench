//! Variable extraction and codegen

use super::ink_widget::tag_to_ink;
use super::traversal::{extract_jsx_attrs, extract_jsx_children, find_jsx_in_body};
use once_cell::sync::Lazy;
use std::collections::HashSet;
use std::sync::Mutex;

static STATE_VARS: Lazy<Mutex<HashSet<String>>> =
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

pub(crate) fn extract_var_declarations(body: &serde_json::Value) -> Vec<(String, String)> {
    let mut decls = Vec::new();
    if let Some(arr) = body.as_array() {
        for s in arr {
            collect_decl(s, &mut decls);
        }
    } else if let Some(arr) = body
        .get("Block")
        .and_then(|b| b.get("stmts"))
        .and_then(|s| s.as_array())
    {
        for s in arr {
            collect_decl(s, &mut decls);
        }
    }
    decls
}

fn collect_decl(stmt: &serde_json::Value, decls: &mut Vec<(String, String)>) {
    // Check for Stmt kinds
    if matches_kind(stmt, "Block") {
        collect_from_block(stmt, decls);
    } else if matches_kind(stmt, "Expr") {
        collect_from_expr(stmt, decls);
    }
    // Check for VariableDecl (flat format with kind = VariableKind)
    // The kind field contains "Const", "Let", or "Var"
    else if matches_kind(stmt, "Const") || matches_kind(stmt, "Let") || matches_kind(stmt, "Var") {
        collect_from_var_decl(stmt, decls);
    }
}

fn collect_from_var_decl(stmt: &serde_json::Value, decls: &mut Vec<(String, String)>) {
    // Handle flat VariableDecl format: {kind: "Const"|"Let"|"Var", pattern, init, ...}
    if let Some(pattern) = stmt.get("pattern") {
        let init = stmt.get("init");
        // Destructuring: const [a, b] = ...
        if let Some(elems) = pattern.get("elems").and_then(|e| e.as_array()) {
            collect_array_destructure(elems, init, decls);
        }
        // Simple binding: const x = ...
        else if let Some(name) = pattern.get("name").and_then(|n| n.as_str()) {
            collect_simple_binding(name, init, decls);
        }
    }
}

fn collect_array_destructure(
    elems: &[serde_json::Value],
    init: Option<&serde_json::Value>,
    decls: &mut Vec<(String, String)>,
) {
    for elem in elems {
        if let Some(name) = elem.get("name").and_then(|n| n.as_str()) {
            if !name.is_empty() {
                let default_val = "0i32".to_string();
                let (value, type_hint) = init
                    .and_then(|i| extract_call_arg_value_with_type(i))
                    .unwrap_or_else(|| (default_val, None));
                let decl = format_var_decl(name, &value, type_hint.as_deref());
                decls.push((decl, name.to_string()));
            }
        }
    }
}

fn collect_simple_binding(
    name: &str,
    init: Option<&serde_json::Value>,
    decls: &mut Vec<(String, String)>,
) {
    if name.is_empty() {
        return;
    }
    let default_val = "0i32".to_string();
    let (value, type_hint) = init
        .and_then(|i| extract_call_arg_value_with_type(i))
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

/// Extract the call argument value and return (rust_value, type_hint)
fn extract_call_arg_value_with_type(init: &serde_json::Value) -> Option<(String, Option<String>)> {
    // Check for array initializer
    if let Some(arr) = init.get("Array") {
        let rust_val = try_array_to_rust(arr)?;
        return Some((rust_val, Some("Vec<Value>".to_string())));
    }
    // Check for object initializer
    if init.get("Object").is_some() {
        let rust_val = serde_json::to_string(init).ok()?;
        return Some((format!("serde_json::json!({})", rust_val), Some("serde_json::Value".to_string())));
    }
    // For other expressions, use the existing logic
    extract_call_arg_value(init).map(|v| (v, None))
}

fn extract_call_arg_value(init: &serde_json::Value) -> Option<String> {
    // Extract value from Call expression arguments
    if let Some(call) = init.get("Call") {
        if let Some(args) = call.get("arguments").and_then(|a| a.as_array()) {
            if let Some(first_arg) = args.first() {
                return expr_value_to_rust(first_arg);
            }
        }
    }
    // Fallback for other expression types
    expr_value_to_rust(init)
}

fn matches_kind(stmt: &serde_json::Value, kind: &str) -> bool {
    stmt.get("kind").and_then(|k| k.as_str()) == Some(kind)
}

fn collect_from_block(stmt: &serde_json::Value, decls: &mut Vec<(String, String)>) {
    if let Some(arr) = stmt.get("stmts").and_then(|s| s.as_array()) {
        for s in arr {
            collect_decl(s, decls);
        }
    }
}

fn collect_from_expr(stmt: &serde_json::Value, decls: &mut Vec<(String, String)>) {
    if let Some(expr) = stmt.get("expr") {
        if let Some(d) = try_extract_assign(expr) {
            decls.push(d);
        }
    }
}

// ---------------------------------------------------------------------------
// Assignment extraction
// ---------------------------------------------------------------------------

fn try_extract_assign(expr: &serde_json::Value) -> Option<(String, String)> {
    let assign = expr.get("Assign")?;
    let left = assign.get("left")?;
    if left.get("Array").is_some() {
        return try_hook_destructuring(assign);
    }
    simple_var_decl(assign)
}

fn simple_var_decl(assign: &serde_json::Value) -> Option<(String, String)> {
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

fn left_ident_name(left: &serde_json::Value) -> Option<String> {
    left.get("Ident")?.get("name")?.as_str().map(String::from)
}

// ---------------------------------------------------------------------------
// Hook destructuring
// ---------------------------------------------------------------------------

fn try_hook_destructuring(assign: &serde_json::Value) -> Option<(String, String)> {
    let state_name = extract_first_ident(assign.get("left")?)?;
    let init = try_use_state_init(assign.get("right")?)?;
    add_state_var(&state_name);
    Some((format!("let {} = std::cell::Cell::new({});", state_name, init), state_name))
}

fn extract_first_ident(left: &serde_json::Value) -> Option<String> {
    left.get("Array")?
        .get("elems")?
        .as_array()?
        .first()?
        .get("Ident")?
        .get("name")?
        .as_str()
        .map(String::from)
}

fn try_use_state_init(call: &serde_json::Value) -> Option<String> {
    let call_expr = call.get("Call")?;
    let callee = call_expr.get("callee")?;
    if !is_use_state_callee(callee) {
        return None;
    }
    let args = call_expr.get("args")?.as_array()?;
    let first = args.first()?.as_object()?;
    extract_init_value(first)
}

fn is_use_state_callee(callee: &serde_json::Value) -> bool {
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

pub(crate) fn expr_value_to_rust(value: &serde_json::Value) -> Option<String> {
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

fn try_cond_to_rust(cond: &serde_json::Value) -> Option<String> {
    let test = expr_value_to_rust(cond.get("test")?)?;
    let cons = expr_value_to_rust(cond.get("consequent")?)?;
    let alt = expr_value_to_rust(cond.get("alternate")?)?;
    // Rust requires if/else, not ternary operator
    Some(format!("if {} {{ {} }} else {{ {} }}", test, cons, alt))
}

fn try_array_to_rust(arr: &serde_json::Value) -> Option<String> {
    let elems = arr.get("elems")?.as_array()?;
    let parts: Vec<String> = elems.iter().filter_map(expr_value_to_rust).collect();
    // Use vec! macro to avoid type inference issues with empty arrays
    if parts.is_empty() {
        Some("vec![]".to_string())
    } else {
        Some(format!("vec![{}]", parts.join(", ")))
    }
}

// ---------------------------------------------------------------------------
// Main codegen
// ---------------------------------------------------------------------------

pub(crate) fn try_codegen_jsx(items: &serde_json::Value) -> Option<String> {
    let arr = items.as_array()?;
    for item in arr {
        if let Some((jsx, decls)) = extract_jsx_from_function_with_vars(item) {
            let code = generate_widget_for_jsx(jsx)?;
            return Some(wrap_ink_main(&code, &decls));
        }
    }
    None
}

pub(crate) fn extract_jsx_from_function_with_vars(
    item: &serde_json::Value,
) -> Option<(serde_json::Value, Vec<(String, String)>)> {
    // Try multiple patterns for finding a function:
    // 1. Decl.Function (direct function declaration)
    // 2. Stmt with kind=ExportDefault containing expr.Function
    // 3. Stmt with kind=Return containing arg.Function
    let func = item.get("Decl").and_then(|d| d.get("Function"))
        .or_else(|| {
            // export default function Name() {...}
            // The Stmt contains: {kind: "ExportDefault", expr: Function}
            let stmt = item.get("Stmt")?;
            if stmt.get("kind")?.as_str()? == "ExportDefault" {
                stmt.get("expr")?.get("Function")
            } else {
                None
            }
        })
        .or_else(|| {
            // Function as return value
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
    let jsx = find_jsx_in_body(body)?;
    Some((jsx, decls))
}

fn generate_widget_for_jsx(jsx: serde_json::Value) -> Option<String> {
    let tag = jsx
        .get("opening")?
        .get("name")?
        .get("Ident")?
        .as_str()
        .unwrap_or("Box");
    let attrs = extract_jsx_attrs(jsx.get("opening")?.get("attrs")?).unwrap_or_default();
    let children = extract_jsx_children(jsx.get("children").unwrap_or(&serde_json::Value::Null))
        .unwrap_or_default();
    Some(tag_to_ink(tag, attrs, children).to_string())
}

fn wrap_ink_main(vnode_expr: &str, decls: &[(String, String)]) -> String {
    let state_vars = get_state_vars();
    let vars_section = if decls.is_empty() {
        String::new()
    } else {
        decls
            .iter()
            .map(|(code, _)| format!("    {}", code))
            .collect::<Vec<_>>()
            .join("\n")
            + "\n"
    };
    // Build use section with necessary imports
    let mut use_items = Vec::new();
    if !state_vars.is_empty() || !decls.is_empty() {
        use_items.push("use std::cell::Cell;");
    }
    // Check if any decl uses Vec<Value>
    let has_vec_value = decls.iter().any(|(code, _)| code.contains("Vec<Value>"));
    if has_vec_value {
        use_items.push("use serde_json::Value;");
    }
    let use_section = if use_items.is_empty() {
        String::new()
    } else {
        use_items.iter().map(|u| format!("    {}\n", u)).collect::<String>()
    };
    format!(
        "//! Ink app entry: generated by runts-ratatui 0.1\n\
        use runts_ink;\n\
        fn main() -> anyhow::Result<()> {{\n{}\n{}\n\
        let root: runts_ink::VNode = {}.into();\n\
        let rendered = runts_ink::render_to_string(root, runts_ink::RenderOptions::default())?;\n\
        print!(\"{{}}\", rendered);\n\
        Ok(())\n\
        }}\n",
        use_section, vars_section, vnode_expr
    )
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
