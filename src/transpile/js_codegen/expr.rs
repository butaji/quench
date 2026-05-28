use super::super::hir::*;
use super::jsx::jsx_to_js;
use super::stmt::stmt_to_js;

pub fn expr_to_js(expr: &Expr) -> String {
    match expr {
        Expr::Undefined => "undefined".to_string(),
        Expr::Null => "null".to_string(),
        Expr::Boolean(b) => b.to_string(),
        Expr::Number(n) => if n.fract() == 0.0 { format!("{:.0}", n) } else { n.to_string() },
        Expr::String(s) => format!("'{}'", s.replace('\\', "\\\\").replace('\'', "\\'")),
        Expr::Template { parts, exprs } => template_to_js(parts, exprs),
        Expr::Ident { name } => name.clone(),
        Expr::Array { elems } => {
            let items: Vec<String> = elems.iter().map(|e| e.as_ref().map_or("undefined".to_string(), expr_to_js)).collect();
            format!("[{}]", items.join(", "))
        }
        Expr::Object { props } => object_to_js(props),
        Expr::Member { object, property, computed, .. } => {
            let obj = expr_to_js(object);
            let prop = expr_to_js(property);
            if *computed { format!("{}[{}]", obj, prop) } else if let Expr::Ident { name } = property.as_ref() { format!("{}.{}", obj, name) } else { format!("{}[{}]", obj, prop) }
        }
        Expr::Bin { op, left, right } => bin_op_to_js(op, left, right),
        Expr::Unary { op, arg, .. } => unary_to_js(op, arg),
        Expr::Logical { op, left, right } => logical_to_js(op, left, right),
        Expr::Cond { test, consequent, alternate } => format!("({} ? {} : {})", expr_to_js(test), expr_to_js(consequent), expr_to_js(alternate)),
        Expr::Call { callee, args, .. } => format!("{}({})", expr_to_js(callee), args.iter().map(expr_to_js).collect::<Vec<_>>().join(", ")),
        Expr::Assign { op, left, right } => assign_to_js(op, left, right),
        Expr::Arrow { params, body, is_async } => arrow_to_js(params, body, *is_async),
        Expr::Function { decl } => function_to_js(decl),
        Expr::New { callee, args, .. } => format!("new {}({})", expr_to_js(callee), args.iter().map(expr_to_js).collect::<Vec<_>>().join(", ")),
        Expr::Await { arg } => format!("(await {})", expr_to_js(arg)),
        Expr::Spread { arg } => format!("...{}", expr_to_js(arg)),
        Expr::JSX(jsx) => jsx_to_js(jsx),
        Expr::Update { op, arg, .. } => match op { UpdateOp::Increment => format!("++{}", expr_to_js(arg)), UpdateOp::Decrement => format!("--{}", expr_to_js(arg)) },
        _ => String::new(),
    }
}

fn template_to_js(parts: &[TemplatePart], exprs: &[Expr]) -> String {
    let mut result = String::from("`");
    for (i, part) in parts.iter().enumerate() {
        if let TemplatePart::String(s) = part { result.push_str(&s.replace('`', "\\`").replace('$', "\\$")); }
        if i < exprs.len() { result.push_str(&format!("${{{}}}", expr_to_js(&exprs[i]))); }
    }
    result.push('`');
    result
}

fn object_to_js(props: &[ObjectProp]) -> String {
    let fields: Vec<String> = props.iter().filter_map(|prop| match prop {
        ObjectProp::Init { key, value } => Some(format!("{}: {}", prop_key_to_js(key), expr_to_js(value))),
        ObjectProp::Shorthand { name } => Some(name.clone()),
        ObjectProp::Spread { value } => Some(format!("...{}", expr_to_js(value))),
        ObjectProp::Method { key, value } => Some(format!("{}({}) {}", prop_key_to_js(key), params_to_js(&value.params), block_to_js(value.body.as_ref().map(|b| &b.0[..]).unwrap_or(&[])))),
        _ => None,
    }).collect();
    format!("{{{}}}", fields.join(", "))
}

fn bin_op_to_js(op: &BinaryOp, left: &Expr, right: &Expr) -> String {
    let l = expr_to_js(left);
    let r = expr_to_js(right);
    let op_str: &str = match op {
        BinaryOp::Add => "+", BinaryOp::Sub => "-", BinaryOp::Mul => "*", BinaryOp::Div => "/",
        BinaryOp::Mod => "%", BinaryOp::Eq | BinaryOp::EqStrict => "===",
        BinaryOp::Ne | BinaryOp::NeStrict => "!==", BinaryOp::Lt => "<", BinaryOp::Le => "<=",
        BinaryOp::Gt => ">", BinaryOp::Ge => ">=", BinaryOp::BitOr => "|",
        BinaryOp::BitXor => "^", BinaryOp::BitAnd => "&", BinaryOp::LeftShift => "<<",
        BinaryOp::RightShift => ">>", BinaryOp::RightShiftAll => ">>>", _ => "+",
    };
    format!("({} {} {})", l, op_str, r)
}

fn unary_to_js(op: &UnaryOp, arg: &Expr) -> String {
    let a = expr_to_js(arg);
    let op_str = match op { UnaryOp::Minus => "-", UnaryOp::Plus => "+", UnaryOp::Not => "!", UnaryOp::BitNot => "~", UnaryOp::TypeOf => "typeof ", UnaryOp::Void => "void " };
    format!("{}{}", op_str, a)
}

fn logical_to_js(op: &LogicalOp, left: &Expr, right: &Expr) -> String {
    let l = expr_to_js(left);
    let r = expr_to_js(right);
    let op_str = match op { LogicalOp::And => "&&", LogicalOp::Or => "||", LogicalOp::NullishCoalesce => "??" };
    format!("({} {} {})", l, op_str, r)
}

fn assign_to_js(op: &AssignOp, left: &Expr, right: &Expr) -> String {
    let l = expr_to_js(left);
    let r = expr_to_js(right);
    let op_str = match op { AssignOp::Assign => "=", AssignOp::AddAssign => "+=", AssignOp::SubAssign => "-=", AssignOp::MulAssign => "*=", AssignOp::DivAssign => "/=", AssignOp::ModAssign => "%=", _ => "=" };
    format!("{} {} {}", l, op_str, r)
}

fn arrow_to_js(params: &[Param], body: &Stmt, is_async: bool) -> String {
    let p = params_to_js(params);
    let prefix = if is_async { "async " } else { "" };
    match body {
        Stmt::Block(stmts) => format!("{}({}) => {}", prefix, p, block_to_js(stmts)),
        _ => { let b = stmt_to_js(body); if b.starts_with("return ") { format!("{}({}) => {}", prefix, p, &b[7..].trim_end_matches(';')) } else { format!("{}({}) => {}", prefix, p, b) } }
    }
}

fn function_to_js(decl: &FunctionDecl) -> String {
    let name = if decl.name.is_empty() { String::new() } else { format!(" {}", decl.name) };
    let prefix = if decl.is_async { "async " } else { "" };
    format!("{}function{}({}) {}", prefix, name, params_to_js(&decl.params), block_to_js(decl.body.as_ref().map(|b| &b.0[..]).unwrap_or(&[])))
}

pub fn prop_key_to_js(key: &PropKey) -> String {
    match key { PropKey::Ident(s) => s.name.clone(), PropKey::String(s) => s.clone(), PropKey::Number(n) => n.to_string(), PropKey::Computed(e) => expr_to_js(e) }
}

pub fn params_to_js(params: &[Param]) -> String {
    params.iter().map(|p| { let t = p.type_.as_ref().map(|ty| format!(": {}", type_hint_to_js(ty))); format!("{}{}", p.name, t.unwrap_or_default()) }).collect::<Vec<_>>().join(", ")
}

pub fn block_to_js(stmts: &[Stmt]) -> String {
    let inner = stmts.iter().map(stmt_to_js).filter(|s| !s.is_empty()).collect::<Vec<_>>().join("\n");
    format!("{{\n{}\n}}", inner.lines().map(|l| format!("  {}", l)).collect::<Vec<_>>().join("\n"))
}

fn type_hint_to_js(ty: &Type) -> &'static str {
    match ty { Type::String | Type::Number | Type::Boolean => "any", _ => "any" }
}
