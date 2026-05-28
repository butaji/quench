//! Expression conversion

use crate::transpile::hir as hir;
use crate::transpile::parser::jsx::{convert_jsx_element, convert_jsx_child};
use crate::transpile::parser::stmt::convert_stmt_to_stmt;
use crate::transpile::parser::types::{convert_ts_type, convert_param};

use oxc_ast::ast::*;
use oxc_syntax::operator::{BinaryOperator, UnaryOperator, LogicalOperator};

pub fn convert_expr(expr: &Expression) -> Option<hir::Expr> {
    convert_simple_expr(expr).or_else(|| convert_complex_expr(expr))
}

fn convert_simple_expr(expr: &Expression) -> Option<hir::Expr> {
    match expr {
        Expression::BooleanLiteral(b) => Some(hir::Expr::Boolean(b.value)),
        Expression::NumericLiteral(n) => Some(hir::Expr::Number(n.value)),
        Expression::StringLiteral(s) => Some(hir::Expr::String(s.value.to_string())),
        Expression::NullLiteral(_) => Some(hir::Expr::Null),
        Expression::BigIntLiteral(n) => n.value.parse().ok().map(hir::Expr::BigInt),
        Expression::Identifier(id) => Some(hir::Expr::Ident { name: id.name.to_string() }),
        Expression::ThisExpression(_) | Expression::Super(_) => Some(hir::Expr::Null),
        Expression::ArrayExpression(_arr) => Some(hir::Expr::Array { elems: vec![] }),
        Expression::AssignmentExpression(_assign) => Some(hir::Expr::Null),
        _ => None,
    }
}

fn convert_complex_expr(expr: &Expression) -> Option<hir::Expr> {
    match expr {
        Expression::ObjectExpression(obj) => convert_object(obj),
        Expression::JSXElement(elem) => Some(hir::Expr::JSX(convert_jsx_element(elem))),
        Expression::JSXFragment(frag) => Some(hir::Expr::JSX(convert_jsx_fragment(frag))),
        Expression::BinaryExpression(bin) => convert_binary(bin),
        Expression::UnaryExpression(unary) => convert_unary(unary),
        Expression::LogicalExpression(logical) => convert_logical(logical),
        Expression::ConditionalExpression(cond) => convert_conditional(cond),
        Expression::CallExpression(call) => convert_call(call),
        Expression::StaticMemberExpression(member) => convert_member(member),
        Expression::NewExpression(new) => convert_new(new),
        Expression::ArrowFunctionExpression(arrow) => convert_arrow(arrow),
        Expression::FunctionExpression(func) => convert_function(func),
        Expression::TemplateLiteral(lit) => convert_template(lit),
        Expression::SequenceExpression(seq) => convert_sequence(seq),
        Expression::AwaitExpression(a) => convert_await(a),
        _ => None,
    }
}

fn convert_object(obj: &ObjectExpression) -> Option<hir::Expr> {
    let props: Vec<_> = obj.properties.iter().filter_map(|prop| {
        if let ObjectPropertyKind::ObjectProperty(p) = prop {
            let key_name = p.key.name().map(|n| n.to_string()).unwrap_or_default();
            let key = hir::PropKey::Ident(key_name);
            Some(hir::ObjectProp::Init { key, value: hir::Expr::Null })
        } else { None }
    }).collect();
    Some(hir::Expr::Object { props })
}

fn convert_jsx_fragment(frag: &JSXFragment) -> hir::JSXExpr {
    hir::JSXExpr {
        opening: hir::JSXOpening { name: hir::JSXName::Fragment, attrs: vec![], self_closing: false },
        children: frag.children.iter().filter_map(convert_jsx_child).collect(),
        closing: None,
    }
}

fn convert_sequence(seq: &SequenceExpression) -> Option<hir::Expr> {
    let exprs: Vec<_> = seq.expressions.iter().filter_map(convert_expr).collect();
    Some(hir::Expr::Seq { exprs })
}

fn convert_await(a: &AwaitExpression) -> Option<hir::Expr> {
    convert_expr(&a.argument).map(|arg| hir::Expr::Await { arg: Box::new(arg) })
}

fn convert_binary(bin: &BinaryExpression) -> Option<hir::Expr> {
    use oxc_syntax::operator::BinaryOperator as BO;
    let left = convert_expr(&bin.left)?;
    let right = convert_expr(&bin.right)?;
    let op = match_bin_op(bin.operator);
    Some(hir::Expr::Bin { op, left: Box::new(left), right: Box::new(right) })
}

fn match_bin_op(op: BinaryOperator) -> hir::BinaryOp {
    use oxc_syntax::operator::BinaryOperator as BO;
    match op {
        BO::Addition => hir::BinaryOp::Add,
        BO::Subtraction => hir::BinaryOp::Sub,
        BO::Multiplication => hir::BinaryOp::Mul,
        BO::Division => hir::BinaryOp::Div,
        BO::Remainder => hir::BinaryOp::Mod,
        BO::Equality => hir::BinaryOp::Eq,
        BO::StrictEquality => hir::BinaryOp::EqStrict,
        BO::Inequality => hir::BinaryOp::Ne,
        BO::StrictInequality => hir::BinaryOp::NeStrict,
        BO::LessThan => hir::BinaryOp::Lt,
        BO::LessEqualThan => hir::BinaryOp::Le,
        BO::GreaterThan => hir::BinaryOp::Gt,
        BO::GreaterEqualThan => hir::BinaryOp::Ge,
        BO::ShiftLeft => hir::BinaryOp::LeftShift,
        BO::ShiftRight => hir::BinaryOp::RightShift,
        BO::BitwiseAnd => hir::BinaryOp::BitAnd,
        BO::BitwiseOr => hir::BinaryOp::BitOr,
        BO::BitwiseXor => hir::BinaryOp::BitXor,
        _ => hir::BinaryOp::Add,
    }
}

fn convert_unary(unary: &UnaryExpression) -> Option<hir::Expr> {
    use oxc_syntax::operator::UnaryOperator as UO;
    let arg = convert_expr(&unary.argument)?;
    let op = match unary.operator {
        UO::UnaryPlus => hir::UnaryOp::Plus,
        UO::UnaryNegation => hir::UnaryOp::Minus,
        UO::LogicalNot => hir::UnaryOp::Not,
        UO::BitwiseNot => hir::UnaryOp::BitNot,
        UO::Typeof => hir::UnaryOp::TypeOf,
        UO::Void => hir::UnaryOp::Void,
        _ => hir::UnaryOp::Void,
    };
    Some(hir::Expr::Unary { op, arg: Box::new(arg), prefix: unary.prefix })
}

fn convert_logical(logical: &LogicalExpression) -> Option<hir::Expr> {
    use oxc_syntax::operator::LogicalOperator as LO;
    let left = convert_expr(&logical.left)?;
    let right = convert_expr(&logical.right)?;
    let op = match logical.operator {
        LO::LogicalAnd => hir::LogicalOp::And,
        LO::LogicalOr => hir::LogicalOp::Or,
        LO::Coalesce => hir::LogicalOp::NullishCoalesce,
        _ => hir::LogicalOp::And,
    };
    Some(hir::Expr::Logical { op, left: Box::new(left), right: Box::new(right) })
}

fn convert_conditional(cond: &ConditionalExpression) -> Option<hir::Expr> {
    let test = convert_expr(&cond.test)?;
    let consequent = convert_expr(&cond.consequent)?;
    let alternate = convert_expr(&cond.alternate)?;
    Some(hir::Expr::Cond { test: Box::new(test), consequent: Box::new(consequent), alternate: Box::new(alternate) })
}

fn convert_call(call: &CallExpression) -> Option<hir::Expr> {
    let callee = convert_expr(&call.callee)?;
    let args: Vec<_> = call.arguments.iter().filter_map(|a| convert_arg(a)).collect();
    Some(hir::Expr::Call { callee: Box::new(callee), args, type_args: vec![] })
}

fn convert_arg(arg: &Argument) -> Option<hir::Expr> {
    match arg { Argument::Expression(e) => convert_expr(e), _ => None }
}

fn convert_member(member: &StaticMemberExpression) -> Option<hir::Expr> {
    let object = convert_expr(&member.object)?;
    let property = hir::Expr::Ident { name: member.property.name.to_string() };
    Some(hir::Expr::Member { object: Box::new(object), property: Box::new(property), computed: member.computed, optional: false })
}

fn convert_new(new: &NewExpression) -> Option<hir::Expr> {
    let callee = convert_expr(&new.callee)?;
    let args: Vec<_> = new.arguments.as_ref().map(|a| a.iter().filter_map(|arg| convert_arg(arg))).unwrap_or_default().collect();
    Some(hir::Expr::New { callee: Box::new(callee), args, type_args: vec![] })
}

fn convert_arrow(arrow: &ArrowFunctionExpression) -> Option<hir::Expr> {
    let params: Vec<_> = arrow.params.iter().filter_map(|p| convert_param(p)).collect();
    let body = convert_arrow_body(&arrow.body)?;
    Some(hir::Expr::Arrow { params, body: Box::new(body), is_async: arrow.r#async })
}

fn convert_arrow_body(body: &FunctionBody) -> Option<hir::Stmt> {
    convert_stmt_to_stmt(&body.body[0])
}

fn convert_function(func: &FunctionExpression) -> Option<hir::Expr> {
    let decl = hir::FunctionDecl { name: "".to_string(), params: vec![], ret_type: None };
    Some(hir::Expr::Function { decl })
}

fn convert_template(lit: &TemplateLiteral) -> Option<hir::Expr> {
    let parts: Vec<_> = lit.quasis.iter().map(|q| hir::TemplatePart::String(q.value.raw.to_string())).collect();
    let exprs: Vec<_> = lit.expressions.iter().filter_map(convert_expr).collect();
    Some(hir::Expr::Template { parts, exprs })
}
