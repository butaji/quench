//! Expression generation

use crate::transpile::codegen::CodeGenerator;
use crate::transpile::hir::*;

pub struct CodeGenExpr;

impl CodeGenExpr {
    pub fn expr_to_rust(cg: &CodeGenerator, expr: &Expr) -> String {
        use Expr::*;
        match expr {
            Null | Undefined => "serde_json::Value::Null".to_string(),
            Boolean(b) => format!("serde_json::json!({})", b),
            Number(n) => format!("serde_json::json!({})", n),
            String(s) => format!("serde_json::json!({})", s),
            BigInt(n) => format!("serde_json::json!({})", n),
            Ident { name } => cg.to_snake_case(name),
            RegExp { .. } => "serde_json::Value::Null".to_string(),
            Array { elems } => Self::array_to_json(cg, elems),
            Object { props } => Self::object_to_json(cg, props),
            Bin { op, left, right } => Self::binary_to_rust(cg, op, left, right),
            Unary { op, arg, .. } => Self::unary_to_rust(cg, op, arg),
            Logical { op, left, right } => Self::logical_to_rust(cg, op, left, right),
            Assign { op, left, right } => Self::assign_to_rust(cg, op, left, right),
            Update { op, arg, .. } => Self::update_to_rust(cg, op, arg),
            Cond { test, consequent, alternate } => Self::cond_to_rust(cg, test, consequent, alternate),
            Call { callee, args, .. } => Self::call_to_rust(cg, callee, args),
            New { callee, args, .. } => Self::new_to_rust(cg, callee, args),
            Member { object, property, .. } => Self::member_to_rust(cg, object, property),
            Function { decl } => Self::function_to_rust(cg, decl),
            Arrow { params, body, is_async } => Self::arrow_to_rust(cg, params, body, *is_async),
            Await { arg } => format!("runts_lib::runtime::helpers::await_value({}).await", cg.expr_to_rust(arg)),
            Yield { arg, .. } => arg.as_ref().map(|a| format!("runts_lib::runtime::helpers::yield_value({})", cg.expr_to_rust(a))).unwrap_or_default(),
            Spread { arg } => format!("...{}", cg.expr_to_rust(arg)),
            Template { parts, exprs } => Self::template_to_rust(cg, parts, exprs),
            TaggedTemplate { tag, template } => format!("{}({})", cg.expr_to_rust(tag), cg.expr_to_rust(template)),
            Seq { exprs } => Self::seq_to_rust(cg, exprs),
            JSX(x) => cg.jsx_to_rust(x),
            Class { .. } | MetaProp { .. } | RegExp { .. } => "serde_json::Value::Null".to_string(),
            TSAs { expr, .. } => cg.expr_to_rust(expr),
        }
    }

    fn array_to_json(cg: &CodeGenerator, elems: &[Option<Expr>]) -> String {
        let items: Vec<String> = elems.iter().map(|e| e.as_ref().map_or("serde_json::Value::Null".to_string(), |e| cg.expr_to_rust(e))).collect();
        format!("serde_json::json!([{}])", items.join(", "))
    }

    fn object_to_json(cg: &CodeGenerator, props: &[ObjectProp]) -> String {
        let fields: Vec<String> = props.iter().filter_map(|p| match p {
            ObjectProp::Init { key, value } => Some(format!("\"{}\": {}", Self::prop_key_to_str(key), cg.expr_to_rust(value))),
            ObjectProp::Shorthand { name } => Some(format!("\"{}\": {}", name, cg.to_snake_case(name))),
            ObjectProp::Spread { value } => Some(format!("...{}", cg.expr_to_rust(value))),
            _ => None,
        }).collect();
        format!("serde_json::json!({{{}}})", fields.join(", "))
    }

    fn binary_to_rust(cg: &CodeGenerator, op: &BinaryOp, left: &Expr, right: &Expr) -> String {
        let l = cg.expr_to_rust(left);
        let r = cg.expr_to_rust(right);
        let o = match op { BinaryOp::Add => "+", BinaryOp::Sub => "-", BinaryOp::Mul => "*", BinaryOp::Div => "/", BinaryOp::Mod => "%", _ => "+" };
        format!("({} {} {})", l, o, r)
    }

    fn unary_to_rust(cg: &CodeGenerator, op: &UnaryOp, arg: &Expr) -> String {
        let a = cg.expr_to_rust(arg);
        let o = match op { UnaryOp::Minus => "-", UnaryOp::Plus => "+", UnaryOp::Not => "!", _ => "!" };
        format!("{}{}", o, a)
    }

    fn logical_to_rust(cg: &CodeGenerator, op: &LogicalOp, left: &Expr, right: &Expr) -> String {
        let l = cg.expr_to_rust(left);
        let r = cg.expr_to_rust(right);
        let o = match op { LogicalOp::And => "&&", LogicalOp::Or => "||", LogicalOp::NullishCoalesce => "??", };
        format!("({} {} {})", l, o, r)
    }

    fn assign_to_rust(cg: &CodeGenerator, op: &AssignOp, left: &Expr, right: &Expr) -> String {
        let l = cg.expr_to_rust(left);
        let r = cg.expr_to_rust(right);
        let o = match op { AssignOp::AddAssign => "+=", AssignOp::SubAssign => "-=", _ => "=", };
        format!("{} {} {}", l, o, r)
    }

    fn update_to_rust(cg: &CodeGenerator, op: &UpdateOp, arg: &Expr) -> String {
        let a = cg.expr_to_rust(arg);
        match op { UpdateOp::Increment => format!("++{}", a), UpdateOp::Decrement => format!("--{}", a) }
    }

    fn cond_to_rust(cg: &CodeGenerator, test: &Expr, consequent: &Expr, alternate: &Expr) -> String {
        format!("if {} {{ {} }} else {{ {} }}", cg.expr_to_rust(test), cg.expr_to_rust(consequent), cg.expr_to_rust(alternate))
    }

    fn call_to_rust(cg: &CodeGenerator, callee: &Callee, args: &[Expr]) -> String {
        let c = match callee { Callee::Expr(e) => cg.expr_to_rust(e), Callee::Import => "import()".to_string() };
        let a = args.iter().map(cg.expr_to_rust).collect::<Vec<_>>().join(", ");
        format!("{}({})", c, a)
    }

    fn new_to_rust(cg: &CodeGenerator, callee: &Expr, args: &[Expr]) -> String {
        let c = cg.expr_to_rust(callee);
        let a = args.iter().map(cg.expr_to_rust).collect::<Vec<_>>().join(", ");
        format!("{}({})", c, a)
    }

    fn member_to_rust(cg: &CodeGenerator, object: &Expr, property: &Expr) -> String {
        let o = cg.expr_to_rust(object);
        let p = cg.expr_to_rust(property);
        format!("{}.{}", o, p)
    }

    fn function_to_rust(_cg: &CodeGenerator, _decl: &FunctionDecl) -> String { "|| {}".to_string() }
    fn arrow_to_rust(cg: &CodeGenerator, _params: &[Param], body: &Stmt, _is_async: bool) -> String { cg.stmt_to_rust(body) }
    fn template_to_rust(cg: &CodeGenerator, parts: &[TemplatePart], exprs: &[Expr]) -> String { "String::new()".to_string() }
    fn seq_to_rust(cg: &CodeGenerator, exprs: &[Expr]) -> String { exprs.last().map_or(String::new(), |e| cg.expr_to_rust(e)) }

    fn prop_key_to_str(key: &PropKey) -> &str {
        match key { PropKey::Ident(i) => &i.name, PropKey::String(s) => s, _ => "" }
    }
}
