//! Expression conversion - Module re-exports
//!
//! These functions are consolidated from the parser into this single file
//! to avoid module declaration issues.

use crate::transpile::hir::{self, Expr, ObjectMemberExpr};
use oxc_ast::ast::*;
use std::vec::Vec;

// Re-export from expr_ops
pub use super::expr_ops::{assign_op, binary_op, unary_op};

fn binding_id_name(pat: &BindingPattern) -> Option<String> {
    if let BindingPattern::BindingIdentifier(id)=pat{Some(id.name.to_string())}else{None}
}

fn convert_simple_assignment_target(target: &SimpleAssignmentTarget) -> Option<Expr> {
    match target {
        SimpleAssignmentTarget::AssignmentTargetIdentifier(id)=>Some(hir::Expr::Ident{name:id.name.to_string()}),
        SimpleAssignmentTarget::ComputedMemberExpression(m)=>conv_computed_member(m),
        SimpleAssignmentTarget::StaticMemberExpression(m)=>conv_static_member(m),
        SimpleAssignmentTarget::PrivateFieldExpression(_)=>None,
        _=>None,
    }
}

fn convert_assignment_target(target: &AssignmentTarget) -> Option<Expr> {
    target.as_simple_assignment_target()
        .and_then(convert_simple_assignment_target)
}

fn pat_ident(name: &str) -> hir::Pat {
    hir::Pat::Ident { name: name.to_string(), type_: None, optional: false }
}

fn obj_pat_key(key: &PropertyKey) -> Option<String> {
    if let PropertyKey::StaticIdentifier(id)=key{Some(id.name.to_string())}
    else if let PropertyKey::StringLiteral(s)=key{Some(s.value.to_string())}
    else if let PropertyKey::NumericLiteral(n)=key{Some(n.value.to_string())}
    else{None}
}

/// Convert a binding pattern (for destructuring)
pub fn convert_binding_pattern(pat: &BindingPattern) -> Option<hir::Pat> {
    convert_binding_pattern_inner(pat)
}

fn convert_binding_pattern_inner(pat: &BindingPattern) -> Option<hir::Pat> {
    match pat {
        BindingPattern::BindingIdentifier(id)=>Some(pat_ident(&id.name)),
        BindingPattern::ArrayPattern(arr)=>{
            let elements:Vec<Option<hir::Pat>>=arr.elements.iter().map(|e|{
                e.as_ref().and_then(|e|convert_binding_pattern_inner(e))
            }).collect();
            let rest = arr.rest.as_ref().and_then(|r| convert_binding_pattern_inner(&r.argument));
            Some(hir::Pat::Array{elems:elements, rest:rest.map(Box::new)})
        }
        BindingPattern::ObjectPattern(obj)=>{
            let mut props:Vec<hir::ObjectPatProp>=Vec::new();
            let mut rest: Option<hir::Pat> = None;
            for p in &obj.properties {
                // Check if this is a rest element
                if let BindingPattern::BindingIdentifier(id) = &p.value {
                    if let Some(key) = obj_pat_key(&p.key) {
                        props.push(hir::ObjectPatProp::Init{key, value: pat_ident(&id.name)});
                    }
                }
            }
            // Object rest pattern: const { x, ...rest } = obj
            // This is handled separately as it's part of ObjectPattern structure
            if obj.rest.is_some() {
                if let Some(r) = &obj.rest {
                    rest = convert_binding_pattern_inner(&r.argument);
                }
            }
            Some(hir::Pat::Object{props, rest: rest.map(Box::new)})
        }
        BindingPattern::AssignmentPattern(assign)=>{
            let left = convert_binding_pattern_inner(&assign.left)?;
            let right = convert_expr(&assign.right).ok()?;
            Some(hir::Pat::Assign{left: Box::new(left), right: Box::new(right)})
        }
    }
}

/// Convert an array expression element list
pub fn arr_elems(arr: &ArrayExpression) -> Vec<Option<Expr>> {
    arr.elements.iter().map(|e|{
        if matches!(e, ArrayExpressionElement::Elision(_)|ArrayExpressionElement::SpreadElement(_)){None}
        else{e.as_expression().and_then(|expr|convert_expr(expr).ok())}
    }).collect()
}

/// Convert template literal
pub fn conv_template(t: &TemplateLiteral) -> Result<Expr, ()> {
    let mut parts = Vec::new();
    let mut exprs = Vec::new();

    for (i, quasi) in t.quasis.iter().enumerate() {
        let val = quasi.value.cooked.as_ref()
            .map(|s| s.to_string())
            .unwrap_or_else(|| quasi.value.raw.to_string());
        parts.push(hir::TemplatePart::String { value: val });
        if i < t.expressions.len() {
            exprs.push(convert_expr(&t.expressions[i])?);
        }
    }

    Ok(Expr::Template { parts, exprs })
}

fn prop_key(key: &PropertyKey) -> Option<hir::PropKey> {
    if let PropertyKey::StaticIdentifier(id)=key{Some(hir::PropKey::Str(id.name.to_string()))}
    else if let PropertyKey::StringLiteral(s)=key{Some(hir::PropKey::Str(s.value.to_string()))}
    else if let PropertyKey::NumericLiteral(n)=key{Some(hir::PropKey::Num(n.value))}
    else{None}
}

/// Convert an object expression
pub fn conv_object(o: &ObjectExpression) -> Result<Expr, ()> {
    let members:Vec<ObjectMemberExpr>=o.properties.iter().filter_map(|p|{
        let prop=match p {
            ObjectPropertyKind::ObjectProperty(p)=>{
                let key=prop_key(&p.key)?;
                let value=convert_expr(&p.value).ok()?;
                hir::ObjectProp::Init{key, value, computed: p.computed}
            }
            ObjectPropertyKind::SpreadProperty(sp)=>{
                let expr=convert_expr(&sp.argument).ok()?;
                hir::ObjectProp::Spread{arg: expr}
            }
        };
        Some(ObjectMemberExpr{prop})
    }).collect();
    Ok(Expr::Object{members})
}

/// Convert binary expression
pub fn conv_bin(bin: &BinaryExpression) -> Option<hir::Expr> {
    let op = binary_op(&bin.operator)?;
    let left = convert_expr(&bin.left).ok()?;
    let right = convert_expr(&bin.right).ok()?;
    Some(hir::Expr::Bin { op, left: Box::new(left), right: Box::new(right) })
}

/// Convert logical expression
pub fn conv_log(log: &LogicalExpression) -> Option<hir::Expr> {
    let op=if log.operator==LogicalOperator::And{hir::LogicalOp::And}
    else if log.operator==LogicalOperator::Or{hir::LogicalOp::Or}
    else{hir::LogicalOp::NullishCoalescing};
    let left=convert_expr(&log.left).ok()?;
    let right=convert_expr(&log.right).ok()?;
    Some(hir::Expr::Logical{op, left:Box::new(left), right:Box::new(right)})
}

/// Convert conditional expression
pub fn conv_cond(cond: &ConditionalExpression) -> Option<hir::Expr> {
    let test = convert_expr(&cond.test).ok()?;
    let consequent = convert_expr(&cond.consequent).ok()?;
    let alternate = convert_expr(&cond.alternate).ok()?;
    Some(hir::Expr::Cond {
        test: Box::new(test),
        consequent: Box::new(consequent),
        alternate: Box::new(alternate),
    })
}

fn arg_expr(a: &Argument) -> Option<Expr> {
    if matches!(a, Argument::SpreadElement(_)){None}
    else{a.as_expression().and_then(|expr|convert_expr(expr).ok())}
}

/// Convert call expression
pub fn conv_call(call: &CallExpression) -> Option<hir::Expr> {
    let callee=convert_expr(&call.callee).ok()?;
    let arguments:Vec<Expr>=call.arguments.iter().filter_map(arg_expr).collect();
    Some(hir::Expr::Call{callee:Box::new(callee), arguments, optional: call.optional})
}

/// Convert new expression
pub fn conv_new(new: &NewExpression) -> Option<hir::Expr> {
    let callee=convert_expr(&new.callee).ok()?;
    let arguments:Vec<Expr>=new.arguments.iter().filter_map(arg_expr).collect();
    Some(hir::Expr::New{callee:Box::new(callee), arguments, optional: false})
}

/// Convert update expression (++/--)
pub fn conv_update(update: &UpdateExpression) -> Option<hir::Expr> {
    let op=if update.operator==UpdateOperator::Increment{hir::UpdateOp::PlusPlus}else{hir::UpdateOp::MinusMinus};
    let argument=convert_simple_assignment_target(&update.argument)?;
    Some(hir::Expr::Update{op, arg:Box::new(argument), prefix: update.prefix})
}

/// Convert unary expression
pub fn conv_unary(unary: &UnaryExpression) -> Option<hir::Expr> {
    let op = unary_op(&unary.operator);
    let arg = convert_expr(&unary.argument).ok()?;
    Some(hir::Expr::Unary {
        op,
        arg: Box::new(arg),
        prefix: true,
    })
}

/// Convert static member expression (obj.prop)
pub fn conv_static_member(m: &StaticMemberExpression) -> Option<hir::Expr> {
    let obj = convert_expr(&m.object).ok()?;
    Some(hir::Expr::StaticMember {
        obj: Box::new(obj),
        property: m.property.name.to_string(),
        optional: m.optional,
    })
}

/// Convert computed member expression (obj[expr])
pub fn conv_computed_member(m: &ComputedMemberExpression) -> Option<hir::Expr> {
    let obj = convert_expr(&m.object).ok()?;
    let property = convert_expr(&m.expression).ok()?;
    Some(hir::Expr::Member {
        obj: Box::new(obj),
        property: Box::new(property),
        computed: true,
        optional: m.optional,
    })
}

/// Convert private field expression (obj.#field)
pub fn conv_private_field(m: &PrivateFieldExpression) -> Option<hir::Expr> {
    let obj = convert_expr(&m.object).ok()?;
    Some(hir::Expr::PrivateMember {
        obj: Box::new(obj),
        property: m.field.name.to_string(),
        optional: m.optional,
    })
}

/// Convert chain expression (a?.b, a?.b.c, a?.(), etc.)
/// Flattens the chain into nested HIR exprs, preserving optional flags.
pub fn conv_chain(chain: &ChainExpression) -> Option<hir::Expr> {
    conv_chain_element(&chain.expression)
}

fn conv_chain_element(elem: &ChainElement) -> Option<hir::Expr> {
    match elem {
        ChainElement::StaticMemberExpression(m) => conv_chain_static(m),
        ChainElement::ComputedMemberExpression(m) => conv_chain_computed(m),
        ChainElement::PrivateFieldExpression(m) => conv_chain_private(m),
        ChainElement::CallExpression(c) => conv_chain_call(c),
        ChainElement::TSNonNullExpression(inner) => conv_chain_element_inner(&inner.expression),
    }
}

fn conv_chain_static(m: &StaticMemberExpression) -> Option<hir::Expr> {
    let obj = conv_chain_element_inner(&m.object)?;
    Some(hir::Expr::StaticMember {
        obj: Box::new(obj),
        property: m.property.name.to_string(),
        optional: m.optional,
    })
}

fn conv_chain_computed(m: &ComputedMemberExpression) -> Option<hir::Expr> {
    let obj = conv_chain_element_inner(&m.object)?;
    let property = convert_expr(&m.expression).ok()?;
    Some(hir::Expr::Member {
        obj: Box::new(obj),
        property: Box::new(property),
        computed: true,
        optional: m.optional,
    })
}

fn conv_chain_private(m: &PrivateFieldExpression) -> Option<hir::Expr> {
    let obj = conv_chain_element_inner(&m.object)?;
    Some(hir::Expr::PrivateMember {
        obj: Box::new(obj),
        property: m.field.name.to_string(),
        optional: m.optional,
    })
}

fn conv_chain_call(c: &CallExpression) -> Option<hir::Expr> {
    let callee = conv_chain_element_inner(&c.callee)?;
    let arguments: Vec<Expr> = c.arguments.iter().filter_map(arg_expr).collect();
    Some(hir::Expr::Call {
        callee: Box::new(callee),
        arguments,
        optional: c.optional,
    })
}

/// Convert expression inside a chain (may be another chain element or a base expr)
fn conv_chain_element_inner(expr: &Expression) -> Option<hir::Expr> {
    match expr {
        Expression::ChainExpression(chain) => conv_chain(chain),
        Expression::StaticMemberExpression(m) => conv_static_member(m),
        Expression::ComputedMemberExpression(m) => conv_computed_member(m),
        Expression::PrivateFieldExpression(m) => conv_private_field(m),
        Expression::CallExpression(c) => conv_call(c),
        _ => convert_expr(expr).ok(),
    }
}

/// Convert assignment expression
pub fn conv_assign(assign: &AssignmentExpression) -> Option<hir::Expr> {
    let op = assign_op(&assign.operator)?;
    let left = convert_assignment_target(&assign.left)?;
    let right = convert_expr(&assign.right).ok()?;
    Some(hir::Expr::Assign {
        op,
        left: Box::new(left),
        right: Box::new(right),
    })
}

/// Convert arrow function
pub fn conv_arrow(arrow: &ArrowFunctionExpression) -> Option<Expr> {
    let params: Vec<hir::Param> = arrow
        .params
        .items
        .iter()
        .filter_map(|p| {
            binding_id_name(&p.pattern).map(|name| hir::Param {
                name,
                type_: None,
                default: None,
                optional: false,
                pattern: convert_binding_pattern(&p.pattern),
                ownership: hir::Ownership::Owned,
            })
        })
        .collect();

    let body = if arrow.expression {
        if let Some(Statement::ExpressionStatement(es)) = arrow.body.statements.first() {
            convert_expr(&es.expression).ok()?
        } else {
            hir::Expr::Undefined
        }
    } else {
        hir::Expr::Block(
            arrow.body.statements.iter().filter_map(|s| stmt_to_hir_stmt(s).ok()).collect()
        )
    };

    Some(hir::Expr::ArrowFunction {
        params,
        body: Box::new(body),
        is_async: arrow.r#async,
    })
}

/// Convert statement to HIR statement
fn stmt_to_hir_stmt(s: &Statement) -> Result<hir::Stmt, ()> {
    match s {
        Statement::ExpressionStatement(e)=>{
            let expr=convert_expr(&e.expression).map_err(|_|())?;
            Ok(hir::Stmt::Expr{expr})
        }
        Statement::ReturnStatement(r)=>{
            let arg=r.argument.as_ref().and_then(|a|convert_expr(a).ok());
            Ok(hir::Stmt::Return{arg})
        }
        Statement::BlockStatement(b)=>{
            let stmts:Vec<hir::Stmt>=b.body.iter().filter_map(|s|stmt_to_hir_stmt(s).ok()).collect();
            Ok(hir::Stmt::Block{stmts})
        }
        _=>Ok(hir::Stmt::Empty),
    }
}

fn func_expr_params(items: &[oxc_ast::ast::FormalParameter]) -> Vec<hir::Param> {
    items.iter().filter_map(|p|{
        binding_id_name(&p.pattern).map(|name|hir::Param{
            name,
            type_:None,
            default:None,
            optional:false,
            pattern:convert_binding_pattern(&p.pattern),
            ownership:hir::Ownership::Owned,
        })
    }).collect()
}

fn conv_func_expr(f: &oxc_ast::ast::Function) -> Result<Expr, ()> {
    Ok(hir::Expr::Function(hir::FunctionDecl{
        name:f.id.as_ref().map(|id|id.name.to_string()).unwrap_or_default(),
        generics:vec![],
        params:func_expr_params(&f.params.items),
        return_type:None,
        body:Some(hir::Block(vec![])),
        is_async:f.r#async,
        is_generator:f.generator,
        decorators:vec![],
        throws:false,
        error_type:None,
    }))
}

/// Convert expression
pub fn convert_expr(expr: &Expression) -> Result<Expr, ()> {
    match expr {
        Expression::StringLiteral(s)=>Ok(hir::Expr::String(s.value.to_string())),
        Expression::NumericLiteral(n)=>Ok(hir::Expr::Number(n.value)),
        Expression::BooleanLiteral(b)=>Ok(hir::Expr::Boolean(b.value)),
        Expression::NullLiteral(_)=>Ok(hir::Expr::Null),
        Expression::Identifier(id)=>Ok(hir::Expr::Ident{name:id.name.to_string()}),
        Expression::BigIntLiteral(b)=>Ok(hir::Expr::BigInt(b.value.parse::<u64>().unwrap_or(0))),
        Expression::BinaryExpression(b)=>conv_bin(b).ok_or(()),
        Expression::LogicalExpression(l)=>conv_log(l).ok_or(()),
        Expression::ConditionalExpression(c)=>conv_cond(c).ok_or(()),
        Expression::CallExpression(c)=>conv_call(c).ok_or(()),
        Expression::NewExpression(n)=>conv_new(n).ok_or(()),
        Expression::UpdateExpression(u)=>conv_update(u).ok_or(()),
        Expression::UnaryExpression(u)=>conv_unary(u).ok_or(()),
        Expression::StaticMemberExpression(m)=>conv_static_member(m).ok_or(()),
        Expression::ComputedMemberExpression(m)=>conv_computed_member(m).ok_or(()),
        Expression::PrivateFieldExpression(m)=>conv_private_field(m).ok_or(()),
        Expression::ChainExpression(chain)=>conv_chain(chain).ok_or(()),
        Expression::AssignmentExpression(a)=>conv_assign(a).ok_or(()),
        Expression::ArrowFunctionExpression(a)=>conv_arrow(a).ok_or(()),
        Expression::ArrayExpression(a)=>Ok(hir::Expr::Array{elems:arr_elems(a)}),
        Expression::ObjectExpression(o)=>conv_object(o),
        Expression::TemplateLiteral(t)=>conv_template(t),
        Expression::FunctionExpression(f)=>conv_func_expr(f),
        Expression::JSXElement(elem)=>Ok(hir::Expr::JSX(super::jsx::convert_jsx_element(elem))),
        Expression::JSXFragment(frag)=>Ok(hir::Expr::JSX(super::jsx::convert_jsx_fragment(frag))),
        Expression::ParenthesizedExpression(p)=>convert_expr(&p.expression),
        Expression::RegExpLiteral(r)=>Ok(hir::Expr::RegExp{pattern:r.regex.pattern.text.to_string(),flags:r.regex.flags.to_string()}),
        Expression::SequenceExpression(s)=>{
            if let Some(last)=s.expressions.last(){convert_expr(last)}else{Err(())}
        }
        _=>Err(()),
    }
}
