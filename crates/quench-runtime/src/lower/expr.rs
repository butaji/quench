//! Expression lowering - convert SWC expressions to runtime AST expressions

use swc_ecma_ast as swc;
use crate::ast::{
    ArrowBody, BinaryOp, Class, ClassMember, Expression, PropertyKey, PropertyValue, UpdateOp,
};
use super::helpers::{
    lower_bin_op, lower_unary_op, assign_op_to_bin, lower_prop_name,
};
use super::helpers::{atom_to_string, LowerError};
use super::jsx::{lower_jsx_element, lower_jsx_fragment, lower_jsx_member, lower_jsx_namespaced};
use super::literals::{lower_getter_prop, lower_literal, lower_method_prop, lower_setter_prop, lower_template_literal};

/// Lower a swc Expr to our Expression
pub fn lower_expr(expr: &swc::Expr) -> Result<Expression, LowerError> {
    match expr {
        swc::Expr::Ident(ident) => Ok(Expression::Identifier(atom_to_string(&ident.sym))),
        swc::Expr::This(_) => Ok(Expression::Identifier("this".to_string())),
        swc::Expr::Array(arr) => lower_array_expr(arr),
        swc::Expr::Object(obj) => lower_object_expr(obj),
        swc::Expr::Fn(func) => lower_fn_expr(func),
        swc::Expr::Arrow(arrow) => lower_arrow_expr(arrow),
        swc::Expr::Yield(yield_expr) => lower_yield_expr(yield_expr),
        swc::Expr::MetaProp(_) => Ok(Expression::Undefined),
        swc::Expr::Await(await_expr) => lower_expr(&await_expr.arg),
        swc::Expr::Paren(paren) => lower_expr(&paren.expr),
        swc::Expr::Bin(bin) => lower_bin_expr(bin),
        swc::Expr::Unary(unary) => lower_unary_expr(unary),
        swc::Expr::Update(update) => lower_update_expr(update),
        swc::Expr::Assign(assign) => lower_assign_expr(assign),
        swc::Expr::Member(member) => lower_member_expr(member),
        swc::Expr::SuperProp(_) => Ok(Expression::Undefined),
        swc::Expr::Call(call) => lower_call_expr(call),
        swc::Expr::New(new_expr) => lower_new_expr(new_expr),
        swc::Expr::Seq(seq) => lower_seq_expr(seq),
        swc::Expr::Cond(cond) => lower_cond_expr(cond),
        swc::Expr::OptChain(opt_chain) => lower_opt_chain(opt_chain),
        swc::Expr::Lit(lit) => lower_literal(lit),
        swc::Expr::TaggedTpl(_) => Err(LowerError::new("Tagged templates not supported")),
        swc::Expr::Tpl(tpl) => lower_template_literal(tpl),
        swc::Expr::Class(class_expr) => lower_class_expr(class_expr),
        swc::Expr::Invalid(_) => Err(LowerError::new("Invalid expression")),
        swc::Expr::PrivateName(_) => Ok(Expression::Undefined),
        swc::Expr::JSXMember(member) => lower_jsx_member(member),
        swc::Expr::JSXNamespacedName(ns) => lower_jsx_namespaced(ns),
        swc::Expr::JSXEmpty(_) => Ok(Expression::Null),
        swc::Expr::JSXElement(elem) => lower_jsx_element(elem),
        swc::Expr::JSXFragment(frag) => lower_jsx_fragment(frag),
        swc::Expr::TsTypeAssertion(e) => lower_expr(&e.expr),
        swc::Expr::TsAs(e) => lower_expr(&e.expr),
        swc::Expr::TsSatisfies(e) => lower_expr(&e.expr),
        swc::Expr::TsNonNull(e) => lower_expr(&e.expr),
        swc::Expr::TsConstAssertion(e) => lower_expr(&e.expr),
        swc::Expr::TsInstantiation(e) => lower_expr(&e.expr),
    }
}

fn lower_array_expr(arr: &swc::ArrayLit) -> Result<Expression, LowerError> {
    let mut elements: Vec<Expression> = Vec::new();
    for elem in &arr.elems {
        let e = match elem {
            Some(swc::ExprOrSpread { spread: Some(_), expr }) => {
                // Spread element: [...expr]
                Expression::Spread(Box::new(lower_expr(expr)?))
            }
            Some(swc::ExprOrSpread { spread: None, expr }) => {
                // Regular element
                lower_expr(expr)?
            }
            None => Expression::Undefined, // holes like [1,,3]
        };
        elements.push(e);
    }
    Ok(Expression::Array(elements))
}

fn lower_object_expr(obj: &swc::ObjectLit) -> Result<Expression, LowerError> {
    let props: Vec<(PropertyKey, PropertyValue)> = obj.props.iter()
        .filter_map(|prop| lower_prop_or_spread(prop).ok())
        .collect();
    Ok(Expression::Object(props))
}

fn lower_fn_expr(func: &swc::FnExpr) -> Result<Expression, LowerError> {
    let name = func.ident.as_ref().map(|i| atom_to_string(&i.sym));
    let params = func.function.params.iter().map(|p| {
        match &p.pat {
            swc::Pat::Ident(ident) => atom_to_string(&ident.id.sym),
            _ => "arg".to_string(),
        }
    }).collect();
    let body = func.function.body.as_ref()
        .map(|b| b.stmts.iter().filter_map(super::stmt::lower_stmt).collect())
        .unwrap_or_default();
    Ok(Expression::FunctionExpression { name, params, body })
}

fn lower_arrow_expr(arrow: &swc::ArrowExpr) -> Result<Expression, LowerError> {
    let params: Vec<String> = arrow.params.iter().map(|p| {
        match p {
            swc::Pat::Ident(ident) => atom_to_string(&ident.id.sym),
            _ => "arg".to_string(),
        }
    }).collect();
    let body = match arrow.body.as_ref() {
        swc::BlockStmtOrExpr::BlockStmt(block) => {
            ArrowBody::Block(std::rc::Rc::new(
                block.stmts.iter().filter_map(super::stmt::lower_stmt).collect()
            ))
        }
        swc::BlockStmtOrExpr::Expr(expr) => {
            ArrowBody::Expression(lower_expr(expr)?)
        }
    };
    Ok(Expression::ArrowFunction { params, body: Box::new(body) })
}

fn lower_yield_expr(yield_expr: &swc::YieldExpr) -> Result<Expression, LowerError> {
    if yield_expr.delegate {
        return Err(LowerError::new("Yield delegate not supported"));
    }
    let arg = yield_expr.arg.as_ref().map(|e| lower_expr(e)).transpose()?;
    Ok(arg.unwrap_or(Expression::Undefined))
}

fn lower_bin_expr(bin: &swc::BinExpr) -> Result<Expression, LowerError> {
    let left = lower_expr(&bin.left)?;
    let right = lower_expr(&bin.right)?;
    let op = lower_bin_op(&bin.op)?;
    Ok(Expression::Binary { op, left: Box::new(left), right: Box::new(right) })
}

fn lower_unary_expr(unary: &swc::UnaryExpr) -> Result<Expression, LowerError> {
    let arg = lower_expr(&unary.arg)?;
    let op = lower_unary_op(&unary.op)?;
    Ok(Expression::Unary { op, argument: Box::new(arg) })
}

fn lower_update_expr(update: &swc::UpdateExpr) -> Result<Expression, LowerError> {
    let arg = lower_expr(&update.arg)?;
    let op = if update.op == swc::op!("++") {
        UpdateOp::Increment
    } else {
        UpdateOp::Decrement
    };
    Ok(Expression::Update { op, argument: Box::new(arg), prefix: update.prefix })
}

fn lower_assign_expr(assign: &swc::AssignExpr) -> Result<Expression, LowerError> {
    let left = lower_assign_target(&assign.left)?;
    let right = lower_expr(&assign.right)?;
    if assign.op == swc::AssignOp::Assign {
        Ok(Expression::Assignment { left: Box::new(left), right: Box::new(right) })
    } else {
        let bin_op = assign_op_to_bin(&assign.op)?;
        Ok(Expression::CompoundAssignment {
            op: bin_op,
            left: Box::new(left),
            right: Box::new(right),
        })
    }
}

fn lower_member_expr(member: &swc::MemberExpr) -> Result<Expression, LowerError> {
    let obj = lower_expr(&member.obj)?;
    let (property, computed) = lower_member_prop(&member.prop)?;
    Ok(Expression::Member { object: Box::new(obj), property, computed })
}

fn lower_call_expr(call: &swc::CallExpr) -> Result<Expression, LowerError> {
    let callee = match &call.callee {
        swc::Callee::Expr(expr) => lower_expr(expr)?,
        swc::Callee::Super(_) => {
            // super() call - lower to Identifier so eval_call can recognize it
            Expression::Identifier("super".to_string())
        }
        swc::Callee::Import(_) => {
            return Err(LowerError::new("import callee not supported"));
        }
    };
    let args: Vec<Expression> = call.args.iter()
        .filter_map(|arg| lower_expr(&arg.expr).ok())
        .collect();
    Ok(Expression::Call { callee: Box::new(callee), arguments: args })
}

fn lower_new_expr(new_expr: &swc::NewExpr) -> Result<Expression, LowerError> {
    let constructor = lower_expr(&new_expr.callee)?;
    let args: Vec<Expression> = new_expr.args.as_ref()
        .map(|args| args.iter().filter_map(|arg| lower_expr(&arg.expr).ok()).collect())
        .unwrap_or_default();
    Ok(Expression::New { constructor: Box::new(constructor), arguments: args })
}

fn lower_seq_expr(seq: &swc::SeqExpr) -> Result<Expression, LowerError> {
    let exprs: Vec<Expression> = seq.exprs.iter()
        .filter_map(|e| lower_expr(e).ok())
        .collect();
    Ok(Expression::Sequence(exprs))
}

fn lower_cond_expr(cond: &swc::CondExpr) -> Result<Expression, LowerError> {
    let test = lower_expr(&cond.test)?;
    let consequent = lower_expr(&cond.cons)?;
    let alternate = lower_expr(&cond.alt)?;
    Ok(Expression::Conditional {
        condition: Box::new(test),
        consequent: Box::new(consequent),
        alternate: Box::new(alternate),
    })
}

fn lower_opt_chain(opt_chain: &swc::OptChainExpr) -> Result<Expression, LowerError> {
    let base_expr = match &*opt_chain.base {
        swc::OptChainBase::Member(member) => lower_expr(&member.obj)?,
        swc::OptChainBase::Call(opt_call) => {
            match &*opt_call.callee {
                swc::Expr::Member(member) => lower_expr(&member.obj)?,
                swc::Expr::Ident(ident) => Expression::Identifier(atom_to_string(&ident.sym)),
                _ => return Err(LowerError::new("Unsupported optional call base")),
            }
        }
    };
    process_opt_chain_expr(opt_chain, base_expr)
}

fn process_opt_chain_expr(expr: &swc::OptChainExpr, base_expr: Expression) -> Result<Expression, LowerError> {
    match &*expr.base {
        swc::OptChainBase::Member(member) => {
            process_opt_chain_member(member, base_expr)
        }
        swc::OptChainBase::Call(opt_call) => {
            process_opt_chain_call(opt_call, base_expr)
        }
    }
}

fn process_opt_chain_member(
    member: &swc::MemberExpr,
    base_expr: Expression,
) -> Result<Expression, LowerError> {
    let (property, computed) = lower_member_prop(&member.prop)?;
    let member_expr = Expression::Member {
        object: Box::new(base_expr.clone()),
        property,
        computed,
    };
    make_optional_check(base_expr, member_expr)
}

fn process_opt_chain_call(
    opt_call: &swc::OptCall,
    base_expr: Expression,
) -> Result<Expression, LowerError> {
    match &*opt_call.callee {
        swc::Expr::OptChain(nested) => {
            let inner = process_opt_chain_expr(nested, base_expr)?;
            let args = lower_call_args(opt_call);
            let call_expr = Expression::Call {
                callee: Box::new(inner),
                arguments: args,
            };
            Ok(call_expr)
        }
        swc::Expr::Member(member) => {
            process_opt_chain_member_call(member, opt_call, base_expr)
        }
        swc::Expr::Ident(ident) => {
            let args = lower_call_args(opt_call);
            let callee = Expression::Identifier(atom_to_string(&ident.sym));
            let call_expr = Expression::Call {
                callee: Box::new(callee),
                arguments: args,
            };
            make_optional_check(base_expr, call_expr)
        }
        _ => Err(LowerError::new("Unsupported optional call callee")),
    }
}

fn process_opt_chain_member_call(
    member: &swc::MemberExpr,
    opt_call: &swc::OptCall,
    base_expr: Expression,
) -> Result<Expression, LowerError> {
    let inner_obj = lower_expr(&member.obj)?;
    let (property, computed) = lower_member_prop(&member.prop)?;
    let inner_checked = make_optional_check(
        inner_obj,
        Expression::Member {
            object: Box::new(base_expr.clone()),
            property,
            computed,
        },
    )?;
    let args = lower_call_args(opt_call);
    let call_expr = Expression::Call {
        callee: Box::new(inner_checked),
        arguments: args,
    };
    make_optional_check(base_expr, call_expr)
}

fn lower_call_args(opt_call: &swc::OptCall) -> Vec<Expression> {
    opt_call.args.iter()
        .filter_map(|arg| lower_expr(&arg.expr).ok())
        .collect()
}

fn make_optional_check(obj: Expression, expr: Expression) -> Result<Expression, LowerError> {
    let null_check = Expression::Binary {
        op: BinaryOp::Or,
        left: Box::new(Expression::Binary {
            op: BinaryOp::StrictEq,
            left: Box::new(obj.clone()),
            right: Box::new(Expression::Null),
        }),
        right: Box::new(Expression::Binary {
            op: BinaryOp::StrictEq,
            left: Box::new(obj),
            right: Box::new(Expression::Undefined),
        }),
    };
    Ok(Expression::Conditional {
        condition: Box::new(null_check),
        consequent: Box::new(Expression::Undefined),
        alternate: Box::new(expr),
    })
}

fn lower_member_prop(prop: &swc::MemberProp) -> Result<(PropertyKey, bool), LowerError> {
    match prop {
        swc::MemberProp::Ident(ident) => Ok((PropertyKey::Ident(atom_to_string(&ident.sym)), false)),
        swc::MemberProp::PrivateName(_) => Err(LowerError::new("Private names not supported")),
        swc::MemberProp::Computed(expr) => {
            let expr = lower_expr(&expr.expr)?;
            Ok((PropertyKey::Computed(Box::new(expr)), true))
        }
    }
}

fn lower_assign_target(target: &swc::AssignTarget) -> Result<Expression, LowerError> {
    match target {
        swc::AssignTarget::Simple(simple) => lower_simple_assign_target(simple),
        swc::AssignTarget::Pat(_) => Err(LowerError::new("Destructuring assignment not supported")),
    }
}

fn lower_simple_assign_target(target: &swc::SimpleAssignTarget) -> Result<Expression, LowerError> {
    match target {
        swc::SimpleAssignTarget::Ident(ident) => {
            Ok(Expression::Identifier(atom_to_string(&ident.id.sym)))
        }
        swc::SimpleAssignTarget::Member(member) => {
            let obj = lower_expr(&member.obj)?;
            let (property, computed) = lower_member_prop(&member.prop)?;
            Ok(Expression::Member { object: Box::new(obj), property, computed })
        }
        _ => Err(LowerError::new("Complex assignment target not supported")),
    }
}

fn lower_prop_or_spread(prop: &swc::PropOrSpread) -> Result<(PropertyKey, PropertyValue), LowerError> {
    match prop {
        swc::PropOrSpread::Prop(prop) => lower_prop(prop),
        swc::PropOrSpread::Spread(_) => Err(LowerError::new("Spread not supported")),
    }
}

fn lower_prop(prop: &swc::Prop) -> Result<(PropertyKey, PropertyValue), LowerError> {
    match prop {
        swc::Prop::Shorthand(ident) => {
            let name = atom_to_string(&ident.sym);
            Ok((PropertyKey::Ident(name.clone()), PropertyValue::Value(Expression::Identifier(name))))
        }
        swc::Prop::KeyValue(kv) => {
            let key = lower_prop_name(&kv.key)?;
            let value = lower_expr(&kv.value)?;
            Ok((key, PropertyValue::Value(value)))
        }
        swc::Prop::Getter(getter) => lower_getter_prop(getter),
        swc::Prop::Setter(setter) => lower_setter_prop(setter),
        swc::Prop::Method(method) => lower_method_prop(method),
        swc::Prop::Assign(_) => Err(LowerError::new("Assignment property not supported")),
    }
}

fn lower_class_expr(class_expr: &swc::ClassExpr) -> Result<Expression, LowerError> {
    // ClassExpr has a nested class field
    let class = &class_expr.class;
    let name = class_expr.ident.as_ref().map(|i| atom_to_string(&i.sym));
    let super_class = class.super_class.as_ref().map(|e| lower_expr(e)).transpose()?;
    let body = class.body.iter().map(lower_class_member).collect::<Result<Vec<_>, _>>()?;
    Ok(Expression::Class(Class {
        name,
        super_class: super_class.map(Box::new),
        body,
    }))
}

fn lower_class_member(member: &swc::ClassMember) -> Result<ClassMember, LowerError> {
    use swc::ClassMember::*;
    match member {
        Constructor(params) => {
            let ps: Vec<String> = params.params.iter().filter_map(|p| {
                match p {
                    swc::ParamOrTsParamProp::Param(param) => {
                        match &param.pat {
                            swc::Pat::Ident(ident) => Some(atom_to_string(&ident.id.sym)),
                            _ => None,
                        }
                    }
                    swc::ParamOrTsParamProp::TsParamProp(_) => None,
                }
            }).collect();
            let body = params.body.as_ref()
                .map(|b| b.stmts.iter().filter_map(super::stmt::lower_stmt).collect())
                .unwrap_or_default();
            Ok(ClassMember::Constructor { params: ps, body })
        }
        Method(method) => {
            let name = lower_prop_name(&method.key)?;
            let is_static = method.is_static;
            let ps: Vec<String> = method.function.params.iter().filter_map(|p| {
                match &p.pat {
                    swc::Pat::Ident(ident) => Some(atom_to_string(&ident.id.sym)),
                    _ => None,
                }
            }).collect();
            let body = method.function.body.as_ref()
                .map(|b| b.stmts.iter().filter_map(super::stmt::lower_stmt).collect())
                .unwrap_or_default();
            match method.kind {
                swc::MethodKind::Getter => {
                    Ok(ClassMember::Getter { name, body })
                }
                swc::MethodKind::Setter => {
                    let param = ps.first().cloned().unwrap_or_default();
                    Ok(ClassMember::Setter { name, param, body })
                }
                swc::MethodKind::Method => {
                    if is_static {
                        Ok(ClassMember::StaticMethod { name, params: ps, body })
                    } else {
                        Ok(ClassMember::Method { name, params: ps, body })
                    }
                }
            }
        }
        PrivateMethod(_) => Err(LowerError::new("Private methods not supported")),
        ClassProp(_) => Err(LowerError::new("Class fields not supported")),
        PrivateProp(_) => Err(LowerError::new("Private fields not supported")),
        Empty(_) => Err(LowerError::new("Empty class members not supported")),
        StaticBlock(_) => Err(LowerError::new("Static blocks not supported")),
        TsIndexSignature(_) => Err(LowerError::new("TypeScript index signatures not supported")),
        AutoAccessor(_) => Err(LowerError::new("Auto accessors not supported")),
    }
}


