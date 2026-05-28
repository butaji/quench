//! Expression generation

use crate::transpile::codegen::CodeGenerator;
use crate::transpile::hir::*;

pub struct CodeGenExpr;

impl CodeGenExpr {
    pub fn expr_to_rust(cg: &CodeGenerator, expr: &Expr) -> String {
        match expr {
            Expr::Null => "serde_json::Value::Null".to_string(),
            Expr::Undefined => "serde_json::Value::Null".to_string(),
            Expr::Boolean(b) => format!("serde_json::json!({})", b),
            Expr::Number(n) => format!("serde_json::json!({})", n),
            Expr::String(s) => format!("serde_json::json!({})", s),
            Expr::BigInt(n) => format!("serde_json::json!({})", n),
            Expr::Ident { name } => cg.to_snake_case(name),
            Expr::RegExp { .. } => "serde_json::Value::Null".to_string(),
            Expr::Array { elems } => Self::array_to_json(cg, elems),
            Expr::Object { props } => Self::object_to_json(cg, props),
            Expr::Bin { op, left, right } => Self::binary_to_rust(cg, op, left, right),
            Expr::Unary { op, arg, prefix } => Self::unary_to_rust(cg, op, arg, *prefix),
            Expr::Logical { op, left, right } => Self::logical_to_rust(cg, op, left, right),
            Expr::Assign { op, left, right } => Self::assign_to_rust(cg, op, left, right),
            Expr::Update { op, arg, prefix } => Self::update_to_rust(cg, op, arg, *prefix),
            Expr::Cond { test, consequent, alternate } => Self::cond_to_rust(cg, test, consequent, alternate),
            Expr::Call { callee, args, .. } => Self::call_to_rust(cg, callee, args),
            Expr::New { callee, args, .. } => Self::new_to_rust(cg, callee, args),
            Expr::Member { object, property, computed, optional } => Self::member_to_rust(cg, object, property, *computed, *optional),
            Expr::Function { decl } => Self::function_to_rust(cg, decl),
            Expr::Arrow { params, body, is_async } => Self::arrow_to_rust(cg, params, body, *is_async),
            Expr::Await { arg } => format!("runts_lib::runtime::helpers::await_value({}).await", cg.expr_to_rust(arg)),
            Expr::Yield { arg, .. } => arg.as_ref().map(|a| format!("runts_lib::runtime::helpers::yield_value({})", cg.expr_to_rust(a))).unwrap_or_default(),
            Expr::Spread { arg } => format!("...{}", cg.expr_to_rust(arg)),
            Expr::Template { parts, exprs } => Self::template_to_rust(cg, parts, exprs),
            Expr::TaggedTemplate { tag, template } => format!("{}({})", cg.expr_to_rust(tag), cg.expr_to_rust(template)),
            Expr::Seq { exprs } => Self::seq_to_rust(cg, exprs),
            Expr::JSX(x) => cg.jsx_to_rust(x),
            Expr::Class { .. } => "serde_json::Value::Null".to_string(),
            Expr::TSAs { expr, .. } => cg.expr_to_rust(expr),
            Expr::MetaProp { .. } => "serde_json::Value::Null".to_string(),
        }
    }

    fn array_to_json(cg: &CodeGenerator, elems: &[Option<Expr>]) -> String {
        let items: Vec<String> = elems.iter().map(|e| {
            e.as_ref().map(|e| cg.expr_to_rust(e)).unwrap_or_else(|| "serde_json::Value::Null".to_string())
        }).collect();
        format!("serde_json::json!([{}])", items.join(", "))
    }

    fn object_to_json(cg: &CodeGenerator, props: &[ObjectProp]) -> String {
        let items: Vec<String> = props.iter().filter_map(|p| {
            match p {
                ObjectProp::Init { key, value } => {
                    let k = match key {
                        PropKey::Ident(s) => format!("\"{}\"", s),
                        PropKey::String(s) => format!("\"{}\"", s),
                        _ => return None,
                    };
                    let v = cg.expr_to_rust(value);
                    Some(format!("{}: {}", k, v))
                }
                ObjectProp::Spread { value } => Some(cg.expr_to_rust(value)),
                ObjectProp::Method { key, value } => {
                    let name = match key {
                        PropKey::Ident(s) => s.clone(),
                        _ => return None,
                    };
                    let fn_code = Self::function_to_rust(cg, value);
                    Some(format!("\"{}\": {}", name, fn_code))
                }
                _ => None,
            }
        }).collect();
        format!("serde_json::json!({{{}}})", items.join(", "))
    }

    fn binary_to_rust(cg: &CodeGenerator, op: &BinaryOp, left: &Expr, right: &Expr) -> String {
        let l = cg.expr_to_rust(left);
        let r = cg.expr_to_rust(right);
        let rust_op = match op {
            BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Mod => {
                match op {
                    BinaryOp::Add => "+",
                    BinaryOp::Sub => "-",
                    BinaryOp::Mul => "*",
                    BinaryOp::Div => "/",
                    BinaryOp::Mod => "%",
                    _ => "+",
                }
            }
            BinaryOp::Exp => ".powf(",
            _ => "+",
        };
        if matches!(op, BinaryOp::Exp) {
            format!("({}).powf({})", l, r)
        } else {
            format!("({} {} {})", l, rust_op, r)
        }
    }

    fn unary_to_rust(cg: &CodeGenerator, op: &UnaryOp, arg: &Expr, prefix: bool) -> String {
        let a = cg.expr_to_rust(arg);
        let rust_op = match op {
            UnaryOp::Minus => "-",
            UnaryOp::Plus => "+",
            UnaryOp::Not => "!",
            UnaryOp::BitNot => "~",
            UnaryOp::TypeOf => "runts_lib::runtime::helpers::type_of(",
            UnaryOp::Void => "runts_lib::runtime::helpers::void_of(",
        };
        if matches!(op, UnaryOp::TypeOf | UnaryOp::Void) {
            format!("{}{})", rust_op, a)
        } else if prefix {
            format!("({}{})", rust_op, a)
        } else {
            format!("({}{})", a, rust_op)
        }
    }

    fn logical_to_rust(cg: &CodeGenerator, op: &LogicalOp, left: &Expr, right: &Expr) -> String {
        let l = cg.expr_to_rust(left);
        let r = cg.expr_to_rust(right);
        match op {
            LogicalOp::And => {
                format!("if {} {{ Some({}) }} else {{ None }}", l, r)
            }
            LogicalOp::Or => {
                format!("({}.unwrap_or({}))", l, r)
            }
            LogicalOp::NullishCoalesce => {
                format!("({}.unwrap_or({}))", l, r)
            }
        }
    }

    fn assign_to_rust(cg: &CodeGenerator, op: &AssignOp, left: &Expr, right: &Expr) -> String {
        let l = cg.expr_to_rust(left);
        let r = cg.expr_to_rust(right);
        if *op == AssignOp::Assign {
            format!("{} = {}", l, r)
        } else {
            let rust_op = match op {
                AssignOp::AddAssign => "+",
                AssignOp::SubAssign => "-",
                AssignOp::MulAssign => "*",
                AssignOp::DivAssign => "/",
                AssignOp::ModAssign => "%",
                _ => "+",
            };
            format!("{} = {} {} {}", l, l, rust_op, r)
        }
    }

    fn update_to_rust(cg: &CodeGenerator, op: &UpdateOp, arg: &Expr, prefix: bool) -> String {
        let a = cg.expr_to_rust(arg);
        let rust_op = match op { UpdateOp::Increment => "+= 1", UpdateOp::Decrement => "-= 1" };
        if prefix { format!("{{ {}; {} }}", format!("{} {}", a, rust_op), a) } else { format!("{{ {}; {} }}", a, format!("{} {}", a, rust_op)) }
    }

    fn cond_to_rust(cg: &CodeGenerator, test: &Expr, consequent: &Expr, alternate: &Expr) -> String {
        let t = cg.expr_to_rust(test);
        let c = cg.expr_to_rust(consequent);
        let a = cg.expr_to_rust(alternate);
        format!("if {} {{ {} }} else {{ {} }}", t, c, a)
    }

    fn call_to_rust(cg: &CodeGenerator, callee: &Expr, args: &[Expr]) -> String {
        let c = cg.expr_to_rust(callee);
        let a: Vec<String> = args.iter().map(|e| cg.expr_to_rust(e)).collect();
        format!("{}({})", c, a.join(", "))
    }

    fn new_to_rust(cg: &CodeGenerator, callee: &Expr, args: &[Expr]) -> String {
        let c = cg.expr_to_rust(callee);
        let a: Vec<String> = args.iter().map(|e| cg.expr_to_rust(e)).collect();
        format!("{}({})", c, a.join(", "))
    }

    fn member_to_rust(cg: &CodeGenerator, object: &Expr, property: &Expr, computed: bool, _optional: bool) -> String {
        let o = cg.expr_to_rust(object);
        if computed {
            let p = cg.expr_to_rust(property);
            format!("{}[{}]", o, p)
        } else {
            let p = match property {
                Expr::Ident { name } => name.clone(),
                _ => cg.expr_to_rust(property),
            };
            format!("{}.{}", o, p)
        }
    }

    fn function_to_rust(cg: &CodeGenerator, decl: &FunctionDecl) -> String {
        let params: Vec<String> = decl.params.iter().map(|p| {
            let name = cg.to_snake_case(&p.name);
            p.type_.as_ref().map(|t| format!("{}: {}", name, cg.type_to_rust(t))).unwrap_or(name)
        }).collect();
        let async_kw = if decl.is_async { "async " } else { "" };
        let body = decl.body.as_ref().map(|_b| "{}").unwrap_or_default();
        format!("{}fn({}) {{ {} }}", async_kw, params.join(", "), body)
    }

    fn arrow_to_rust(cg: &CodeGenerator, params: &[Param], body: &Stmt, is_async: bool) -> String {
        let _cg = cg; // suppress unused warning
        let async_kw = if is_async { "async " } else { "" };
        let p: Vec<String> = params.iter().map(|pm| cg.to_snake_case(&pm.name)).collect();
        let body_str = match body {
            Stmt::Return { arg } => arg.as_ref().map(|e| cg.expr_to_rust(e)).unwrap_or_default(),
            _ => "{}".to_string(),
        };
        format!("{}|{}| {}", async_kw, p.join(", "), body_str)
    }

    fn template_to_rust(cg: &CodeGenerator, parts: &[TemplatePart], exprs: &[Expr]) -> String {
        let mut result = String::from("format!(\"");
        for (i, part) in parts.iter().enumerate() {
            match part {
                TemplatePart::String(s) => result.push_str(&s.replace('{', "{{").replace('}', "}}")),
                TemplatePart::Type(_) => if let Some(expr) = exprs.get(i) { result.push_str(&format!("{{{}}}", cg.expr_to_rust(expr))); }
            }
        }
        result.push_str("\", ");
        for expr in exprs { result.push_str(&format!("{}, ", cg.expr_to_rust(expr))); }
        result.push(')');
        result
    }

    fn seq_to_rust(cg: &CodeGenerator, exprs: &[Expr]) -> String {
        let e: Vec<String> = exprs.iter().map(|x| cg.expr_to_rust(x)).collect();
        format!("{{ {}; {} }}", e[..e.len()-1].join("; "), e.last().unwrap())
    }
}
