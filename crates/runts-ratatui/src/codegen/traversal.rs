//! JSX traversal helpers - finding JSX in HIR statements

/// Find JSX expression in a body.
/// Handles both direct array format and Block-wrapped format:
/// - [stmt1, stmt2, ...] (function declaration body)
/// - {"Block": {"stmts": [stmt1, stmt2, ...]}} (hand-rolled fixture format)
pub(crate) fn find_jsx_in_body(body: &serde_json::Value) -> Option<serde_json::Value> {
    // Unwrap Block if present to get the statements array
    let stmts = if let Some(block) = body.get("Block") {
        block.get("stmts")?.as_array()?
    } else if let Some(arr) = body.as_array() {
        arr
    } else {
        // Not a Block or array, check if it's directly a JSX expression
        return if is_jsx_expr(body) { Some(body.clone()) } else { None };
    };
    find_jsx_in_stmts(stmts)
}

pub(crate) fn find_jsx_in_stmts(stmts: &[serde_json::Value]) -> Option<serde_json::Value> {
    for stmt in stmts {
        if let Some(jsx) = find_jsx_in_stmt(stmt) {
            return Some(jsx);
        }
    }
    None
}

pub(crate) fn find_jsx_in_stmt(stmt: &serde_json::Value) -> Option<serde_json::Value> {
    let kind = stmt.get("kind")?.as_str()?;
    match kind {
        "Return" => find_jsx_in_return(stmt),
        "Expr" => find_jsx_in_expr_stmt(stmt),
        "Block" => find_jsx_in_block_stmt(stmt),
        "If" => find_jsx_in_if_stmt(stmt),
        _ => None,
    }
}

fn find_jsx_in_return(stmt: &serde_json::Value) -> Option<serde_json::Value> {
    let arg = stmt.get("arg")?;
    let unwrapped = unwrap_jsx(arg);
    if is_jsx_expr(&unwrapped) {
        return Some(unwrapped);
    }
    find_jsx_in_expr(&unwrapped)
}

fn find_jsx_in_expr_stmt(stmt: &serde_json::Value) -> Option<serde_json::Value> {
    let expr = stmt.get("expr")?;
    let unwrapped = unwrap_jsx(expr);
    if is_jsx_expr(&unwrapped) {
        return Some(unwrapped);
    }
    find_jsx_in_expr(&unwrapped)
}

fn find_jsx_in_block_stmt(stmt: &serde_json::Value) -> Option<serde_json::Value> {
    let stmts = stmt.get("stmts")?.as_array()?;
    find_jsx_in_stmts(stmts)
}

fn find_jsx_in_if_stmt(stmt: &serde_json::Value) -> Option<serde_json::Value> {
    if let Some(cons) = stmt.get("consequent") {
        if let Some(jsx) = find_jsx_in_stmt(cons) {
            return Some(jsx);
        }
    }
    if let Some(alt) = stmt.get("alternate") {
        return find_jsx_in_stmt(alt);
    }
    None
}

fn find_jsx_in_expr(expr: &serde_json::Value) -> Option<serde_json::Value> {
    let kind = expr.get("kind")?.as_str()?;
    match kind {
        "JSX" => Some(expr.clone()),
        "Cond" => find_jsx_in_cond(expr),
        _ => None,
    }
}

fn find_jsx_in_cond(expr: &serde_json::Value) -> Option<serde_json::Value> {
    let cond = expr.get("Cond")?;
    if let Some(jsx) = find_jsx_in_expr(cond.get("consequent")?) {
        return Some(jsx);
    }
    find_jsx_in_expr(cond.get("alternate")?)
}

fn is_jsx_expr(expr: &serde_json::Value) -> bool {
    expr.get("JSX").is_some() || expr.get("opening").is_some()
}

fn unwrap_jsx(expr: &serde_json::Value) -> serde_json::Value {
    if let Some(inner) = expr.get("JSX") {
        return inner.clone();
    }
    expr.clone()
}

pub(crate) fn extract_jsx_children(children_val: &serde_json::Value) -> Option<Vec<serde_json::Value>> {
    let arr = children_val.as_array()?;
    let mut out = Vec::with_capacity(arr.len());
    for v in arr {
        if let Some(inner) = v.get("JSX") {
            out.push(inner.clone());
        } else {
            out.push(v.clone());
        }
    }
    Some(out)
}

pub(crate) fn extract_jsx_attrs(attrs_val: &serde_json::Value) -> Option<Vec<(String, serde_json::Value)>> {
    let arr = attrs_val.as_array()?;
    let mut out = Vec::with_capacity(arr.len());
    for item in arr {
        let attr = item.get("Attr")?;
        let name = attr.get("name")?.as_str()?.to_string();
        let value = attr.get("value")?.clone();
        out.push((name, value));
    }
    Some(out)
}
