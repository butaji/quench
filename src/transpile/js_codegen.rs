//! JavaScript code generator for island client bundles
//!
//! Generates standard Preact ES modules from HIR.
//! Each island bundle:
//!   1. Imports preact + hooks
//!   2. Defines the component as a Preact function component
//!   3. Auto-hydrates its data-island container

use super::hir::*;

/// Generate a Preact island JS bundle from HIR.
pub fn generate_island_js(name: &str, module: &Module) -> String {
    let mut js = String::new();

    // Find the default export component
    let component = module.items.iter().find_map(|item| {
        if let ModuleItem::Export(Export::Default { expr }) = item {
            if let Expr::Function { decl } = expr {
                return Some(decl.clone());
            }
        }
        None
    });

    let Some(component) = component else {
        return format!(
            "console.error('[runts] No default export found for island: {}');",
            name
        );
    };

    // ── Imports ────────────────────────────────────────────────
    js.push_str("import { h, render } from 'preact';\n");

    // Check which hooks are used and import only those
    let body_stmts = component.body.as_ref().map(|b| b.0.as_slice()).unwrap_or(&[]);
    let mut hooks = Vec::new();
    if stmts_have_call(body_stmts, "useState") { hooks.push("useState"); }
    if stmts_have_call(body_stmts, "useEffect") { hooks.push("useEffect"); }
    if stmts_have_call(body_stmts, "useRef") { hooks.push("useRef"); }
    if stmts_have_call(body_stmts, "useMemo") { hooks.push("useMemo"); }
    if stmts_have_call(body_stmts, "useCallback") { hooks.push("useCallback"); }
    if stmts_have_call(body_stmts, "useReducer") { hooks.push("useReducer"); }
    if stmts_have_call(body_stmts, "useContext") { hooks.push("useContext"); }
    if stmts_have_call(body_stmts, "useId") { hooks.push("useId"); }
    if stmts_have_call(body_stmts, "useSignal") { hooks.push("useSignal"); }
    if stmts_have_call(body_stmts, "useComputed") { hooks.push("useComputed"); }
    if stmts_have_call(body_stmts, "useSignalEffect") { hooks.push("useSignalEffect"); }
    if !hooks.is_empty() {
        js.push_str(&format!(
            "import {{ {} }} from 'preact/hooks';\n",
            hooks.join(", ")
        ));
    }

    // ── Component ──────────────────────────────────────────────
    js.push('\n');
    js.push_str(&format!("// Island: {}\n", name));
    js.push_str("export default function ");
    js.push_str(name);
    js.push_str("Component(");

    // Params
    let params_js = params_to_js(&component.params);
    js.push_str(&params_js);
    js.push_str(") {\n");

    // Body
    if let Some(body) = &component.body {
        for stmt in &body.0 {
            let stmt_js = stmt_to_js(stmt);
            if !stmt_js.is_empty() {
                for line in stmt_js.lines() {
                    js.push_str("  ");
                    js.push_str(line);
                    js.push('\n');
                }
            }
        }
    }

    js.push_str("}\n\n");

    // ── Auto-hydrate ───────────────────────────────────────────
    js.push_str("// Auto-hydrate on client\n");
    js.push_str("const el = document.querySelector('[data-island=\"");
    js.push_str(name);
    js.push_str("\"]');\n");
    js.push_str("if (el && typeof Runts !== 'undefined') {\n");
    js.push_str("  Runts.registerIsland('");
    js.push_str(name);
    js.push_str("', ");
    js.push_str(name);
    js.push_str("Component);\n");
    js.push_str("}\n");

    js
}

// ── Detect hook usage ──────────────────────────────────────────

fn has_hook_calls(stmts: &[Stmt]) -> bool {
    stmts.iter().any(|s| stmt_has_call(s, "useState")
        || stmt_has_call(s, "useEffect")
        || stmt_has_call(s, "useRef")
        || stmt_has_call(s, "useMemo")
        || stmt_has_call(s, "useCallback")
        || stmt_has_call(s, "useReducer")
        || stmt_has_call(s, "useContext")
        || stmt_has_call(s, "useId")
        || stmt_has_call(s, "useSignal")
        || stmt_has_call(s, "useComputed")
        || stmt_has_call(s, "useSignalEffect"))
}

/// Check if any statement in the slice contains a call to `name`.
fn stmts_have_call(stmts: &[Stmt], name: &str) -> bool {
    stmts.iter().any(|s| stmt_has_call(s, name))
}

fn stmt_has_call(stmt: &Stmt, name: &str) -> bool {
    match stmt {
        Stmt::Variable { decl } => {
            decl.init.as_ref().map_or(false, |e| expr_has_call(e, name))
        }
        Stmt::Expr { expr } => expr_has_call(expr, name),
        Stmt::Return { arg: Some(expr) } => expr_has_call(expr, name),
        Stmt::If { test, consequent, alternate } => {
            expr_has_call(test, name)
                || stmt_has_call(consequent, name)
                || alternate.as_ref().map_or(false, |s| stmt_has_call(s, name))
        }
        Stmt::Block(stmts) => has_hook_calls(stmts),
        _ => false,
    }
}

fn expr_has_call(expr: &Expr, name: &str) -> bool {
    match expr {
        Expr::Call { callee, .. } => {
            if let Expr::Ident { name: n } = callee.as_ref() {
                if n == name { return true; }
            }
            false
        }
        Expr::Member { object, .. } => expr_has_call(object, name),
        Expr::Bin { left, right, .. } => expr_has_call(left, name) || expr_has_call(right, name),
        Expr::Unary { arg, .. } => expr_has_call(arg, name),
        Expr::Logical { left, right, .. } => expr_has_call(left, name) || expr_has_call(right, name),
        Expr::Cond { test, consequent, alternate } => {
            expr_has_call(test, name)
                || expr_has_call(consequent, name)
                || expr_has_call(alternate, name)
        }
        Expr::Assign { right, .. } => expr_has_call(right, name),
        Expr::Array { elems } => elems.iter().any(|e| e.as_ref().map_or(false, |e| expr_has_call(e, name))),
        Expr::Object { props } => props.iter().any(|p| match p {
            ObjectProp::Init { value, .. } => expr_has_call(value, name),
            ObjectProp::Method { value, .. } => {
                value.body.as_ref().map_or(false, |b| has_hook_calls(&b.0))
            }
            ObjectProp::Spread { value } => expr_has_call(value, name),
            _ => false,
        }),
        Expr::Arrow { body, .. } => {
            match body.as_ref() {
                Stmt::Block(stmts) => has_hook_calls(stmts),
                stmt => stmt_has_call(stmt, name),
            }
        }
        Expr::Function { decl } => {
            decl.body.as_ref().map_or(false, |b| has_hook_calls(&b.0))
        }
        Expr::JSX(jsx) => jsx_has_call(jsx, name),
        Expr::Template { exprs, .. } => exprs.iter().any(|e| expr_has_call(e, name)),
        _ => false,
    }
}

fn jsx_has_call(jsx: &JSXExpr, name: &str) -> bool {
    jsx.children.iter().any(|child| match child {
        JSXChild::Expr(e) => expr_has_call(e, name),
        JSXChild::JSX(inner) => jsx_has_call(inner, name),
        JSXChild::Fragment { children } => children.iter().any(|c| match c {
            JSXChild::Expr(e) => expr_has_call(e, name),
            JSXChild::JSX(inner) => jsx_has_call(inner, name),
            _ => false,
        }),
        _ => false,
    })
}

// ── JS codegen helpers ─────────────────────────────────────────

fn stmt_to_js(stmt: &Stmt) -> String {
    match stmt {
        Stmt::Variable { decl } => {
            let kind = match decl.kind {
                VariableKind::Const | VariableKind::Let => "const",
                VariableKind::Var => "var",
            };
            if let Some(init) = &decl.init {
                let init_js = expr_to_js(init);
                if let Some(pat) = &decl.pattern {
                    let pat_js = pat_to_js(pat);
                    format!("{} {} = {};", kind, pat_js, init_js)
                } else {
                    format!("{} {} = {};", kind, decl.name, init_js)
                }
            } else {
                format!("{} {};", kind, decl.name)
            }
        }
        Stmt::Expr { expr } => {
            let e = expr_to_js(expr);
            if e.is_empty() { String::new() } else { format!("{};", e) }
        }
        Stmt::Return { arg: Some(expr) } => {
            format!("return {};", expr_to_js(expr))
        }
        Stmt::Return { arg: None } => "return null;".to_string(),
        Stmt::Block(stmts) => {
            let inner = stmts
                .iter()
                .map(stmt_to_js)
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join("\n");
            format!("{{\n{}\n}}", indent(&inner))
        }
        Stmt::If { test, consequent, alternate } => {
            let mut s = format!("if ({}) {{", expr_to_js(test));
            let then_js = stmt_to_js(consequent);
            if !then_js.is_empty() {
                s.push('\n');
                s.push_str(&indent(&then_js));
                s.push('\n');
            }
            s.push('}');
            if let Some(else_stmt) = alternate {
                s.push_str(" else {");
                let else_js = stmt_to_js(else_stmt);
                if !else_js.is_empty() {
                    s.push('\n');
                    s.push_str(&indent(&else_js));
                    s.push('\n');
                }
                s.push('}');
            }
            s
        }
        Stmt::For { init, test, update, body: _body } => {
            let init_js = init.as_ref().map_or(String::new(), |i| match i {
                ForInit::Variable(v) => {
                    if let Some(init_expr) = &v.init {
                        format!("const {} = {}", v.name, expr_to_js(init_expr))
                    } else {
                        format!("const {}", v.name)
                    }
                }
                ForInit::Expr(e) => expr_to_js(e),
            });
            let test_js = test.as_ref().map_or(String::new(), expr_to_js);
            let update_js = update.as_ref().map_or(String::new(), expr_to_js);
            format!("for ({}; {}; {}) {{", init_js, test_js, update_js)
        }
        Stmt::While { test, body: _body } => {
            format!("while ({}) {{", expr_to_js(test))
        }
        Stmt::Switch { discriminant, cases } => {
            let mut s = format!("switch ({}) {{\n", expr_to_js(discriminant));
            for case in cases {
                if let Some(test_expr) = &case.test {
                    s.push_str(&format!("  case {}:\n", expr_to_js(test_expr)));
                } else {
                    s.push_str("  default:\n");
                }
                for stmt in &case.consequent {
                    let stmt_js = stmt_to_js(stmt);
                    if !stmt_js.is_empty() {
                        s.push_str("    ");
                        s.push_str(&stmt_js);
                        s.push('\n');
                    }
                }
            }
            s.push('}');
            s
        }
        Stmt::Try { block, handler, finalizer } => {
            let mut s = "try {\n".to_string();
            if let Stmt::Block(stmts) = block.as_ref() {
                for stmt in stmts {
                    let stmt_js = stmt_to_js(stmt);
                    if !stmt_js.is_empty() {
                        s.push_str("  ");
                        s.push_str(&stmt_js);
                        s.push('\n');
                    }
                }
            }
            s.push('}');
            if let Some(h) = handler {
                s.push_str(" catch (e) {\n");
                if let Stmt::Block(stmts) = h.as_ref() {
                    for stmt in stmts {
                        let stmt_js = stmt_to_js(stmt);
                        if !stmt_js.is_empty() {
                            s.push_str("  ");
                            s.push_str(&stmt_js);
                            s.push('\n');
                        }
                    }
                }
                s.push('}');
            }
            if let Some(finally_block) = finalizer {
                s.push_str(" finally {\n");
                if let Stmt::Block(stmts) = finally_block.as_ref() {
                    for stmt in stmts {
                        let stmt_js = stmt_to_js(stmt);
                        if !stmt_js.is_empty() {
                            s.push_str("  ");
                            s.push_str(&stmt_js);
                            s.push('\n');
                        }
                    }
                }
                s.push('}');
            }
            s
        }
        _ => String::new(),
    }
}

fn expr_to_js(expr: &Expr) -> String {
    match expr {
        Expr::Undefined => "undefined".to_string(),
        Expr::Null => "null".to_string(),
        Expr::Boolean(b) => b.to_string(),
        Expr::Number(n) => {
            if n.fract() == 0.0 {
                format!("{:.0}", n)
            } else {
                n.to_string()
            }
        }
        Expr::String(s) => format!("'{}'", s.replace('\\', "\\\\").replace('\'', "\\'")),
        Expr::Template { parts, exprs } => {
            let mut result = String::from("`");
            for (i, part) in parts.iter().enumerate() {
                if let TemplatePart::String(s) = part {
                    result.push_str(&s.replace('`', "\\`").replace('$', "\\$"));
                }
                if i < exprs.len() {
                    result.push_str("${");
                    result.push_str(&expr_to_js(&exprs[i]));
                    result.push('}');
                }
            }
            result.push('`');
            result
        }
        Expr::Ident { name } => name.clone(),
        Expr::Array { elems } => {
            let items: Vec<String> = elems
                .iter()
                .map(|e| e.as_ref().map_or("undefined".to_string(), expr_to_js))
                .collect();
            format!("[{}]", items.join(", "))
        }
        Expr::Object { props } => {
            let fields: Vec<String> = props
                .iter()
                .map(|prop| match prop {
                    ObjectProp::Init { key, value } => {
                        let k = prop_key_to_js(key);
                        let v = expr_to_js(value);
                        format!("{}: {}", k, v)
                    }
                    ObjectProp::Shorthand { name } => name.clone(),
                    ObjectProp::Spread { value } => format!("...{}", expr_to_js(value)),
                    ObjectProp::Method { key, value } => {
                        let k = prop_key_to_js(key);
                        let params = params_to_js(&value.params);
                        let body = block_to_js(value.body.as_ref().map(|b| &b.0[..]).unwrap_or(&[]));
                        format!("{}({}) {}", k, params, body)
                    }
                    _ => String::new(),
                })
                .filter(|s| !s.is_empty())
                .collect();
            format!("{{{}}}", fields.join(", "))
        }
        Expr::Member { object, property, computed, .. } => {
            let obj = expr_to_js(object);
            if *computed {
                format!("{}[{}]", obj, expr_to_js(property))
            } else if let Expr::Ident { name } = property.as_ref() {
                format!("{}.{}", obj, name)
            } else {
                format!("{}[{}]", obj, expr_to_js(property))
            }
        }
        Expr::Bin { op, left, right } => {
            let l = expr_to_js(left);
            let r = expr_to_js(right);
            let op_str = match op {
                BinaryOp::Add => "+",
                BinaryOp::Sub => "-",
                BinaryOp::Mul => "*",
                BinaryOp::Div => "/",
                BinaryOp::Mod => "%",
                BinaryOp::Eq | BinaryOp::EqStrict => "===",
                BinaryOp::Ne | BinaryOp::NeStrict => "!==",
                BinaryOp::Lt => "<",
                BinaryOp::Le => "<=",
                BinaryOp::Gt => ">",
                BinaryOp::Ge => ">=",
                BinaryOp::BitOr => "|",
                BinaryOp::BitXor => "^",
                BinaryOp::BitAnd => "&",
                BinaryOp::LeftShift => "<<",
                BinaryOp::RightShift => ">>",
                BinaryOp::RightShiftAll => ">>>",
                _ => "+",
            };
            format!("({} {} {})", l, op_str, r)
        }
        Expr::Unary { op, arg, .. } => {
            let a = expr_to_js(arg);
            let op_str = match op {
                UnaryOp::Minus => "-",
                UnaryOp::Plus => "+",
                UnaryOp::Not => "!",
                UnaryOp::BitNot => "~",
                UnaryOp::TypeOf => "typeof ",
                UnaryOp::Void => "void ",
            };
            format!("{}{}", op_str, a)
        }
        Expr::Logical { op, left, right } => {
            let l = expr_to_js(left);
            let r = expr_to_js(right);
            let op_str = match op {
                LogicalOp::And => "&&",
                LogicalOp::Or => "||",
                LogicalOp::NullishCoalesce => "??",
            };
            format!("({} {} {})", l, op_str, r)
        }
        Expr::Cond { test, consequent, alternate } => {
            format!(
                "({} ? {} : {})",
                expr_to_js(test),
                expr_to_js(consequent),
                expr_to_js(alternate)
            )
        }
        Expr::Call { callee, args, .. } => {
            let c = expr_to_js(callee);
            let a: Vec<String> = args.iter().map(expr_to_js).collect();
            format!("{}({})", c, a.join(", "))
        }
        Expr::Assign { op, left, right } => {
            let l = expr_to_js(left);
            let r = expr_to_js(right);
            let op_str = match op {
                AssignOp::Assign => "=",
                AssignOp::AddAssign => "+=",
                AssignOp::SubAssign => "-=",
                AssignOp::MulAssign => "*=",
                AssignOp::DivAssign => "/=",
                AssignOp::ModAssign => "%=",
                _ => "=",
            };
            format!("{} {} {}", l, op_str, r)
        }
        Expr::Arrow { params, body, is_async } => {
            let p = params_to_js(params);
            let prefix = if *is_async { "async " } else { "" };
            match body.as_ref() {
                Stmt::Block(stmts) => {
                    let b = block_to_js(stmts);
                    format!("{}({}) => {}", prefix, p, b)
                }
                single => {
                    let b = stmt_to_js(single);
                    if b.starts_with("return ") {
                        format!("{}({}) => {}", prefix, p, &b[7..].trim_end_matches(';'))
                    } else {
                        format!("{}({}) => {}", prefix, p, b)
                    }
                }
            }
        }
        Expr::Function { decl } => {
            let name = if decl.name.is_empty() {
                String::new()
            } else {
                format!(" {}", decl.name)
            };
            let p = params_to_js(&decl.params);
            let prefix = if decl.is_async { "async " } else { "" };
            let body = block_to_js(decl.body.as_ref().map(|b| &b.0[..]).unwrap_or(&[]));
            format!("{}function{}({}) {}", prefix, name, p, body)
        }
        Expr::New { callee, args, .. } => {
            let c = expr_to_js(callee);
            let a: Vec<String> = args.iter().map(expr_to_js).collect();
            format!("new {}({})", c, a.join(", "))
        }
        Expr::Await { arg } => {
            format!("(await {})", expr_to_js(arg))
        }
        Expr::Spread { arg } => {
            format!("...{}", expr_to_js(arg))
        }
        Expr::JSX(jsx) => jsx_to_js(jsx),
        Expr::Update { op, arg, .. } => {
            let a = expr_to_js(arg);
            match op {
                UpdateOp::Increment => format!("++{}", a),
                UpdateOp::Decrement => format!("--{}", a),
            }
        }
        _ => String::new(),
    }
}

// ── JSX → Preact h() calls ─────────────────────────────────────

fn jsx_to_js(jsx: &JSXExpr) -> String {
    let tag = match &jsx.opening.name {
        JSXName::Ident(s) => s.clone(),
        JSXName::Member { object, property } => format!("{}.{}", object, property),
        _ => "div".to_string(),
    };

    let is_component = tag.chars().next().map(|c| c.is_uppercase()).unwrap_or(false);

    if is_component {
        let props = jsx_attrs_to_js(&jsx.opening.attrs);
        let children = jsx_children_to_js(&jsx.children);
        let all_props = if children.is_empty() || children == "null" {
            if props.is_empty() { "null".to_string() } else { format!("{{ {} }}", props) }
        } else {
            if props.is_empty() {
                format!("{{ children: {} }}", children)
            } else {
                format!("{{ {}, children: {} }}", props, children)
            }
        };
        return format!("h({}, {})", tag, all_props);
    }

    // HTML element → h('tag', props, children)
    let mut props = Vec::new();
    for attr in &jsx.opening.attrs {
        match attr {
            JSXAttr::Attr { name, value } => {
                let v = match value {
                    Some(JSXAttrValue::String(s)) => format!("'{}'", s.replace('\'', "\\'")),
                    Some(JSXAttrValue::Expr(e)) => expr_to_js(e),
                    None => "true".to_string(),
                };
                let key = if name == "className" { "class" } else { name };
                props.push(format!("'{}': {}", key, v));
            }
            JSXAttr::Spread { expr } => {
                props.push(format!("...{}", expr_to_js(expr)));
            }
            _ => {}
        }
    }

    let children = jsx_children_to_js(&jsx.children);

    let props_str = if props.is_empty() {
        "null".to_string()
    } else {
        format!("{{ {} }}", props.join(", "))
    };

    if children.is_empty() || children == "null" {
        format!("h('{}', {})", tag, props_str)
    } else {
        format!("h('{}', {}, {})", tag, props_str, children)
    }
}

fn jsx_attrs_to_js(attrs: &[JSXAttr]) -> String {
    let fields: Vec<String> = attrs
        .iter()
        .filter_map(|attr| match attr {
            JSXAttr::Attr { name, value } => {
                let v = match value {
                    Some(JSXAttrValue::String(s)) => format!("'{}'", s.replace('\'', "\\'")),
                    Some(JSXAttrValue::Expr(e)) => expr_to_js(e),
                    None => "true".to_string(),
                };
                Some(format!("{}: {}", name, v))
            }
            JSXAttr::Spread { expr } => Some(format!("...{}", expr_to_js(expr))),
            _ => None,
        })
        .collect();
    fields.join(", ")
}

fn jsx_children_to_js(children: &[JSXChild]) -> String {
    let items: Vec<String> = children
        .iter()
        .filter_map(|child| match child {
            JSXChild::Text(s) => {
                let trimmed = s.trim();
                if trimmed.is_empty() {
                    None
                } else {
                    Some(format!("'{}'", trimmed.replace('\'', "\\'")))
                }
            }
            JSXChild::Expr(e) => Some(expr_to_js(e)),
            JSXChild::JSX(jsx) => Some(jsx_to_js(jsx)),
            JSXChild::Fragment { children } => Some(jsx_children_to_js(children)),
            _ => None,
        })
        .collect();

    if items.is_empty() {
        "null".to_string()
    } else if items.len() == 1 {
        items[0].clone()
    } else {
        format!("[{}]", items.join(", "))
    }
}

fn pat_to_js(pat: &Pat) -> String {
    match pat {
        Pat::Ident { name, .. } => name.clone(),
        Pat::Object { props, .. } => {
            let fields: Vec<String> = props
                .iter()
                .map(|p| match p {
                    ObjectPatProp::Init { key, value } => {
                        let v = pat_to_js(value);
                        if key == &v {
                            key.clone()
                        } else {
                            format!("{}: {}", key, v)
                        }
                    }
                    ObjectPatProp::Rest { .. } => "...rest".to_string(),
                })
                .collect();
            format!("{{{}}}", fields.join(", "))
        }
        Pat::Array { elems, .. } => {
            let items: Vec<String> = elems
                .iter()
                .map(|e| e.as_ref().map_or("".to_string(), pat_to_js))
                .collect();
            format!("[{}]", items.join(", "))
        }
        Pat::Assign { left, right, .. } => {
            format!("{} = {}", pat_to_js(left), expr_to_js(right))
        }
        Pat::Rest { arg } => {
            format!("...{}", pat_to_js(arg))
        }
        Pat::Default { arg, default } => {
            format!("{} = {}", pat_to_js(arg), expr_to_js(default))
        }
    }
}

fn prop_key_to_js(key: &PropKey) -> String {
    match key {
        PropKey::Ident(s) => s.clone(),
        PropKey::String(s) => format!("'{}'", s.replace('\'', "\\'")),
        PropKey::Number(n) => n.to_string(),
        PropKey::Computed(e) => format!("[{}]", expr_to_js(e)),
    }
}

fn params_to_js(params: &[Param]) -> String {
    params
        .iter()
        .map(|p| {
            if let Some(pat) = &p.pattern {
                pat_to_js(pat)
            } else {
                p.name.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn block_to_js(stmts: &[Stmt]) -> String {
    let body = stmts
        .iter()
        .map(stmt_to_js)
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    if body.is_empty() {
        "{}".to_string()
    } else {
        format!("{{\n{}\n}}", indent(&body))
    }
}

fn indent(s: &str) -> String {
    s.lines()
        .map(|line| {
            if line.trim().is_empty() {
                line.to_string()
            } else {
                format!("  {}", line)
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}
