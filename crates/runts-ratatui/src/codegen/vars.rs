//! Variable extraction and codegen

use super::traversal::{extract_jsx_attrs, extract_jsx_children, find_jsx_in_body};

pub(crate) fn extract_var_declarations(body: &serde_json::Value) -> Vec<String> {
    let mut declarations = Vec::new();
    if let Some(stmt_arr) = body.as_array() {
        for stmt in stmt_arr { extract_decl_from_stmt(&mut declarations, stmt); }
    } else if let Some(stmts) = body.get("Block").and_then(|b| b.get("stmts")).and_then(|s| s.as_array()) {
        for stmt in stmts { extract_decl_from_stmt(&mut declarations, stmt); }
    }
    declarations
}

fn extract_decl_from_stmt(decls: &mut Vec<String>, stmt: &serde_json::Value) {
    let kind = match stmt.get("kind").and_then(|k| k.as_str()) { Some(k) => k, None => return };
    if kind == "Block" {
        if let Some(stmts) = stmt.get("stmts").and_then(|s| s.as_array()) { for s in stmts { extract_decl_from_stmt(decls, s); } }
    } else if kind == "Expr" {
        if let Some(expr) = stmt.get("expr").and_then(|e| extract_assign_expr(e)) { decls.push(expr); }
    }
}

fn extract_assign_expr(expr: &serde_json::Value) -> Option<String> {
    let assign = expr.get("Assign")?;
    let name = assign.get("left")?.get("Ident")?.get("name")?.as_str()?;
    let value = assign.get("right")?;
    let rust_value = expr_value_to_rust(value)?;
    let keyword = if expr.get("kind").and_then(|k| k.as_str()) == Some("Decl") { "const" } else { "let" };
    Some(format!("{} {} = {};", keyword, name, rust_value))
}

pub(crate) fn expr_value_to_rust(value: &serde_json::Value) -> Option<String> {
    if let Some(n) = value.as_f64() { return Some(num_to_rust(n)); }
    let map = value.as_object()?;
    if let Some(r) = simple_literal_to_rust(map) { return Some(r); }
    if let Some(inner) = map.get("Expr") { return expr_value_to_rust(inner); }
    if let Some(cond) = map.get("Cond") { return expr_cond_to_rust(cond); }
    if let Some(arr) = map.get("Array") { return expr_array_to_rust(arr); }
    if map.contains_key("Object") { return expr_object_to_rust(value); }
    None
}

fn simple_literal_to_rust(map: &serde_json::Map<String, serde_json::Value>) -> Option<String> {
    if let Some(s) = map.get("String").and_then(|v| v.as_str()) { return Some(escape_string(s)); }
    if let Some(n) = get_json_number(map) { return Some(num_to_rust(n)); }
    if let Some(b) = map.get("Bool").and_then(|v| v.as_bool()) { return Some(b.to_string()); }
    if let Some(name) = get_ident_name(map) { return Some(name); }
    None
}

fn num_to_rust(n: f64) -> String {
    if n.fract() == 0.0 { format!("{}i32", n as i64) } else { format!("{}f64", n) }
}

fn escape_string(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

fn get_json_number(map: &serde_json::Map<String, serde_json::Value>) -> Option<f64> {
    map.get("Number").and_then(|v| v.as_f64())
        .or_else(|| map.get("Number").and_then(|v| v.as_object()).and_then(|o| o.get("0")).and_then(|v| v.as_f64()))
}

fn get_ident_name(map: &serde_json::Map<String, serde_json::Value>) -> Option<String> {
    map.get("Ident").and_then(|v| v.as_object()).and_then(|o| o.get("name")).and_then(|v| v.as_str()).map(|s| s.to_string())
}

fn expr_cond_to_rust(cond: &serde_json::Value) -> Option<String> {
    let test = expr_value_to_rust(cond.get("test")?)?;
    let consequent = expr_value_to_rust(cond.get("consequent")?)?;
    let alternate = expr_value_to_rust(cond.get("alternate")?)?;
    Some(format!("({} ? {} : {})", test, consequent, alternate))
}

fn expr_array_to_rust(arr: &serde_json::Value) -> Option<String> {
    let elems = arr.get("elems")?.as_array()?;
    let parts: Vec<String> = elems.iter().filter_map(expr_value_to_rust).collect();
    Some(format!("[{}]", parts.join(", ")))
}

fn expr_object_to_rust(value: &serde_json::Value) -> Option<String> {
    let json = serde_json::to_string(value).ok()?;
    Some(format!("serde_json::json!({})", json))
}

pub(crate) fn try_codegen_jsx(items: &serde_json::Value) -> Option<String> {
    let items_arr = items.as_array()?;
    for item in items_arr {
        if let Some((jsx_expr, var_decls)) = extract_jsx_from_function_with_vars(item) {
            let widget_code = generate_widget_for_jsx(jsx_expr)?;
            let code = wrap_ink_main(&widget_code.to_string(), &var_decls);
            return Some(code);
        }
    }
    None
}

pub(crate) fn extract_jsx_from_function_with_vars(item: &serde_json::Value) -> Option<(serde_json::Value, Vec<String>)> {
    let decl = item.get("Decl")?;
    let func = decl.get("Function")?;
    let body = func.get("body")?;
    let var_decls = extract_var_declarations(body);
    let jsx = find_jsx_in_body(body)?;
    Some((jsx, var_decls))
}

fn generate_widget_for_jsx(jsx: serde_json::Value) -> Option<String> {
    let tag = jsx.get("opening")?.get("name")?.get("Ident")?.as_str().unwrap_or("Box");
    let attrs = extract_jsx_attrs(jsx.get("opening")?.get("attrs")?).unwrap_or_default();
    let children = extract_jsx_children(jsx.get("children").unwrap_or(&serde_json::Value::Null)).unwrap_or_default();
    let tokens = super::ink_widget::tag_to_ink(tag, attrs, children);
    Some(tokens.to_string())
}

fn wrap_ink_main(vnode_expr: &str, var_decls: &[String]) -> String {
    let vars_section = if var_decls.is_empty() {
        String::new()
    } else {
        let indent = "    ";
        var_decls.iter().map(|d| format!("{}{}", indent, d)).collect::<Vec<_>>().join("\n") + "\n"
    };
    format!(
        "//! Ink app entry: generated by runts-ratatui 0.1\n\
        use runts_ink;\n\
        fn main() -> anyhow::Result<()> {{\n{}\n\
        let root: runts_ink::VNode = {}.into();\n\
        let rendered = runts_ink::render_to_string(root, runts_ink::RenderOptions::default())?;\n\
        print!(\"{{}}\", rendered);\n\
        Ok(())\n\
        }}\n",
        vars_section, vnode_expr
    )
}
