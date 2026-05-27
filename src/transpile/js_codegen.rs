//! JavaScript code generator for island client bundles
//!
//! Generates vanilla JS from HIR that runs in the browser using
//! the global Runts client runtime (signals, effects, hydration).

use super::hir::*;

/// Generate a self-contained island JS bundle from HIR.
///
/// The output is vanilla JS that:
/// 1. Defines the component function
/// 2. Uses Runts.signal() for useState/useSignal
/// 3. Returns a VNode tree compatible with Runts.renderVNode()
/// 4. Registers itself via Runts.registerIsland()
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

    js.push_str(&format!("// Island: {}\n", name));
    js.push_str("(function() {\n");
    js.push_str("'use strict';\n\n");

    // Generate hook helpers
    js.push_str(
        r#"
// Helper: create signal-backed state (useState shim)
function useState(initial) {
  const sig = Runts.signal(initial);
  const set = function(v) { sig.value = v; };
  return [sig, set];
}

// Helper: create ref
function useRef(initial) {
  return { current: initial };
}

// Helper: create memo
function useMemo(fn, deps) {
  return Runts.computed(fn);
}

// Helper: create callback
function useCallback(fn, deps) {
  return fn;
}

// Helper: run effect
function useEffect(fn, deps) {
  Runts.effect(fn);
}
"#,
    );

    // Generate the component function
    js.push_str(&format!("function {}Component(props) {{\n", name));

    // Destructure props with defaults
    if let Some(first_param) = component.params.first() {
        if let Some(Pat::Object { props: pat_props, .. }) = &first_param.pattern {
            let destructured: Vec<String> = pat_props
                .iter()
                .map(|prop| match prop {
                    ObjectPatProp::Init { key, value } => {
                        match value {
                            Pat::Default { arg: _, default } => {
                                format!("{} = {}", key, expr_to_js(default))
                            }
                            Pat::Ident { name: pat_name, .. } => {
                                if key == pat_name {
                                    key.clone()
                                } else {
                                    format!("{}: {}", key, pat_name)
                                }
                            }
                            _ => {
                                format!("{}: {}", key, pat_to_js(value))
                            }
                        }
                    }
                    ObjectPatProp::Rest { .. } => "...rest".to_string(),
                })
                .collect();
            js.push_str(&format!("  const {{{}}} = props || {{}};\n", destructured.join(", ")));
        } else {
            js.push_str(&format!(
                "  const _props = props || {{}};\n  const {} = _props;\n",
                first_param.name
            ));
        }
    }

    // Generate body statements
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

    // Register with Runts
    js.push_str(&format!(
        "Runts.registerIsland('{}', {}Component);\n",
        name, name
    ));

    js.push_str("})();\n");

    js
}

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
            if e.is_empty() {
                String::new()
            } else {
                format!("{};", e)
            }
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

fn jsx_to_js(jsx: &JSXExpr) -> String {
    let tag = match &jsx.opening.name {
        JSXName::Ident(s) => s.clone(),
        JSXName::Member { object, property } => format!("{}.{}", object, property),
        _ => "div".to_string(),
    };

    // Check if component (PascalCase)
    let is_component = tag.chars().next().map(|c| c.is_uppercase()).unwrap_or(false);

    if is_component {
        // For components in islands, we inline or call the component
        let props = jsx_attrs_to_js(&jsx.opening.attrs);
        let children = jsx_children_to_js(&jsx.children);
        let all_props = if children.is_empty() {
            props
        } else {
            if props.is_empty() {
                format!("{{ children: {} }}", children)
            } else {
                format!("{{ {}, children: {} }}", props, children)
            }
        };
        return format!("{}({})", tag, all_props);
    }

    // HTML element → VNode object
    let mut props = Vec::new();
    for attr in &jsx.opening.attrs {
        match attr {
            JSXAttr::Attr { name, value } => {
                let v = match value {
                    Some(JSXAttrValue::String(s)) => format!("'{}'", s.replace('\'', "\\'")),
                    Some(JSXAttrValue::Expr(e)) => expr_to_js(e),
                    None => "true".to_string(),
                };
                let key = if name == "className" { "className" } else { name };
                props.push(format!("{}: {}", key, v));
            }
            JSXAttr::Spread { expr } => {
                props.push(format!("...{}", expr_to_js(expr)));
            }
            _ => {}
        }
    }

    let children = jsx_children_to_js(&jsx.children);
    if !children.is_empty() && children != "null" {
        props.push(format!("children: {}", children));
    }

    format!(
        "{{ type: '{}', props: {{{}}} }}",
        tag,
        props.join(", ")
    )
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
