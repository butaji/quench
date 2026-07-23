//! Private instance method/accessor installation and eval early-error checks.

use crate::ast::{ArrowBody, Expression, PropertyKey, Statement};
use crate::env::Environment;
use crate::eval::class::helpers::{
    private_field_add, prop_key_to_string, storage_key_for_property,
};
use crate::eval::expression::capture_env_for_closure;
use crate::value::{ClassValue, JsError, Object, Value, ValueFunction};
use std::cell::RefCell;
use std::rc::Rc;

fn private_element_exists(obj: &Object, key: &str) -> bool {
    obj.properties.contains_key(key)
        || obj.getters.contains_key(key)
        || obj.setters.contains_key(key)
}

fn ensure_private_add(obj: &Rc<RefCell<Object>>, key: &str) -> Result<(), JsError> {
    if !crate::value::is_private_name_key(key) {
        return Ok(());
    }
    let o = obj.borrow();
    if private_element_exists(&o, key) {
        let (_, js_err) = crate::value::error::create_js_error_with_type(
            "Private method or accessor already defined",
            "TypeError",
        );
        return Err(js_err);
    }
    if !o.extensible {
        let (_, js_err) = crate::value::error::create_js_error_with_type(
            "Cannot add private field to non-extensible object",
            "TypeError",
        );
        return Err(js_err);
    }
    Ok(())
}

/// Install private instance methods, getters, and setters on `instance`.
pub fn install_instance_private_elements(
    class: &ClassValue,
    instance: &Rc<RefCell<Object>>,
    env: &Rc<RefCell<Environment>>,
) -> Result<(), JsError> {
    let closure = class.get_class_def_env().unwrap_or_else(|| Rc::clone(env));
    let member_closure = capture_env_for_closure(&closure);

    for (name, params, body, is_async, is_generator) in &class.methods {
        let key_str = prop_key_to_string(name, &closure, false)?;
        if !key_str.starts_with('#') {
            continue;
        }
        let storage_key = storage_key_for_property(name, &key_str);
        ensure_private_add(instance, &storage_key)?;
        let mut func = ValueFunction::new(
            Some(key_str.clone()),
            params.clone(),
            body.clone(),
            Rc::clone(&member_closure),
            *is_async,
            *is_generator,
        );
        func.strict = true;
        func.is_method = true;
        private_field_add(instance, &storage_key, Value::Function(func))?;
    }

    for (name, body) in &class.getters {
        let key_str = prop_key_to_string(name, &closure, false)?;
        if !key_str.starts_with('#') {
            continue;
        }
        let storage_key = storage_key_for_property(name, &key_str);
        ensure_private_add(instance, &storage_key)?;
        instance.borrow_mut().set_getter(
            &storage_key,
            Rc::new(body.clone()),
            Rc::clone(&member_closure),
            true,
        );
    }

    for (name, param, body) in &class.setters {
        let key_str = prop_key_to_string(name, &closure, false)?;
        if !key_str.starts_with('#') {
            continue;
        }
        let storage_key = storage_key_for_property(name, &key_str);
        ensure_private_add(instance, &storage_key)?;
        instance.borrow_mut().set_setter(
            &storage_key,
            param.clone(),
            Rc::new(body.clone()),
            Rc::clone(&member_closure),
            true,
        );
    }
    Ok(())
}

pub fn program_contains_super_call(body: &[Statement]) -> bool {
    body.iter().any(stmt_contains_super_call)
}

pub fn program_contains_super_property(body: &[Statement]) -> bool {
    body.iter().any(stmt_contains_super_property)
}

pub fn program_contains_arguments(body: &[Statement]) -> bool {
    body.iter().any(stmt_contains_arguments)
}

pub fn program_contains_new_target(body: &[Statement]) -> bool {
    body.iter().any(stmt_contains_new_target)
}

/// PerformEval early errors for eval inside a class field initializer.
pub fn reject_class_field_eval_early_errors(
    body: &[Statement],
    is_direct: bool,
) -> Result<(), JsError> {
    if program_contains_super_call(body) {
        return eval_syntax_error("super is not allowed in class field initializer eval");
    }
    if !is_direct {
        if program_contains_super_property(body) {
            return eval_syntax_error("super is not allowed in class field initializer eval");
        }
        if program_contains_new_target(body) {
            return eval_syntax_error("new.target is not allowed in eval");
        }
    }
    if is_direct && program_contains_arguments(body) {
        return eval_syntax_error("arguments is not allowed in eval");
    }
    Ok(())
}

fn eval_syntax_error(msg: &str) -> Result<(), JsError> {
    let (err_val, js_err) = crate::value::error::create_js_error_with_type(msg, "SyntaxError");
    crate::value::set_thrown_value(err_val);
    Err(js_err)
}

fn callee_is_super(callee: &Expression) -> bool {
    match callee {
        Expression::Identifier(id) => id == "super",
        Expression::Member { object, .. } => {
            matches!(object.as_ref(), Expression::Identifier(id) if id == "super")
        }
        _ => false,
    }
}

fn expr_is_super_call(expr: &Expression) -> bool {
    matches!(expr, Expression::Call { callee, .. } if callee_is_super(callee))
}

fn expr_is_super_property(expr: &Expression) -> bool {
    matches!(
        expr,
        Expression::Member { object, .. }
            if matches!(object.as_ref(), Expression::Identifier(id) if id == "super")
    )
}

fn stmt_contains_super_call(stmt: &Statement) -> bool {
    walk_stmt(stmt, expr_contains_super_call, stmt_contains_super_call)
}

fn stmt_contains_super_property(stmt: &Statement) -> bool {
    walk_stmt(
        stmt,
        expr_contains_super_property,
        stmt_contains_super_property,
    )
}

fn stmt_contains_arguments(stmt: &Statement) -> bool {
    walk_stmt(stmt, expr_contains_arguments, stmt_contains_arguments)
}

fn stmt_contains_new_target(stmt: &Statement) -> bool {
    walk_stmt(stmt, expr_contains_new_target, stmt_contains_new_target)
}

fn walk_stmt(
    stmt: &Statement,
    expr_check: fn(&Expression) -> bool,
    stmt_check: fn(&Statement) -> bool,
) -> bool {
    match stmt {
        Statement::Expression(expr) | Statement::Return(Some(expr)) | Statement::Throw(expr) => {
            expr_check(expr)
        }
        Statement::Block(stmts) => stmts.iter().any(stmt_check),
        Statement::If {
            condition,
            consequent,
            alternate,
        } => {
            expr_check(condition)
                || stmt_check(consequent)
                || alternate.as_ref().is_some_and(|a| stmt_check(a))
        }
        Statement::While {
            condition, body, ..
        } => expr_check(condition) || stmt_check(body),
        Statement::For {
            init,
            condition,
            update,
            body,
        } => {
            init.as_ref()
                .is_some_and(|i| for_init_check(i, expr_check, stmt_check))
                || condition.as_ref().is_some_and(|c| expr_check(c))
                || update.as_ref().is_some_and(|u| expr_check(u))
                || stmt_check(body)
        }
        Statement::ForIn {
            variable,
            object,
            body,
        } => expr_check(variable) || expr_check(object) || stmt_check(body),
        Statement::Try {
            body,
            handler,
            finalizer,
            ..
        } => {
            stmt_check(body)
                || handler.as_ref().is_some_and(|h| stmt_check(h))
                || finalizer.as_ref().is_some_and(|f| stmt_check(f))
        }
        Statement::VarDeclaration { init, .. } => init.as_ref().is_some_and(expr_check),
        Statement::Labeled { body, .. } => stmt_check(body),
        Statement::With { object, body } => expr_check(object) || stmt_check(body),
        Statement::DoWhile {
            body, condition, ..
        } => stmt_check(body) || expr_check(condition),
        Statement::SequenceDecls(stmts) => stmts.iter().any(stmt_check),
        _ => false,
    }
}

fn for_init_check(
    init: &crate::ast::ForInit,
    expr_check: fn(&Expression) -> bool,
    _stmt_check: fn(&Statement) -> bool,
) -> bool {
    match init {
        crate::ast::ForInit::Expression(expr) => expr_check(expr),
        crate::ast::ForInit::VarDeclaration { init, .. } => init.as_ref().is_some_and(expr_check),
    }
}

fn expr_contains_super_call(expr: &Expression) -> bool {
    if expr_is_super_call(expr) {
        return true;
    }
    walk_expr(expr, expr_contains_super_call, stmt_contains_super_call)
}

fn expr_contains_super_property(expr: &Expression) -> bool {
    if expr_is_super_property(expr) {
        return true;
    }
    walk_expr(
        expr,
        expr_contains_super_property,
        stmt_contains_super_property,
    )
}

fn expr_contains_arguments(expr: &Expression) -> bool {
    if matches!(expr, Expression::Identifier(id) if id == "arguments") {
        return true;
    }
    walk_expr(expr, expr_contains_arguments, stmt_contains_arguments)
}

fn expr_contains_new_target(expr: &Expression) -> bool {
    if matches!(expr, Expression::Identifier(id) if id == "new.target") {
        return true;
    }
    walk_expr(expr, expr_contains_new_target, stmt_contains_new_target)
}

fn walk_expr(
    expr: &Expression,
    expr_check: fn(&Expression) -> bool,
    stmt_check: fn(&Statement) -> bool,
) -> bool {
    match expr {
        Expression::Call {
            callee, arguments, ..
        } => expr_check(callee) || arguments.iter().any(expr_check),
        Expression::Member {
            object, property, ..
        } => expr_check(object) || property_expr(property).is_some_and(expr_check),
        Expression::ArrowFunction { body, .. } => arrow_body_check(body, expr_check, stmt_check),
        Expression::FunctionExpression { body, .. } => body.iter().any(stmt_check),
        Expression::Assignment { left, right, .. }
        | Expression::CompoundAssignment { left, right, .. }
        | Expression::LogicalCompoundAssignment { left, right, .. } => {
            expr_check(left) || expr_check(right)
        }
        Expression::Binary { left, right, .. } => expr_check(left) || expr_check(right),
        Expression::Unary { argument, .. } | Expression::Update { argument, .. } => {
            expr_check(argument)
        }
        Expression::Conditional {
            condition,
            consequent,
            alternate,
        } => expr_check(condition) || expr_check(consequent) || expr_check(alternate),
        Expression::Sequence(exprs) | Expression::Array(exprs) => exprs.iter().any(expr_check),
        Expression::Object(props) => props
            .iter()
            .any(|(_, val)| property_value_check(val, expr_check, stmt_check)),
        Expression::New {
            constructor,
            arguments,
            ..
        } => expr_check(constructor) || arguments.iter().any(expr_check),
        Expression::Spread(inner) => expr_check(inner),
        Expression::BlockExpr(stmts) => stmts.iter().any(stmt_check),
        Expression::ForOf {
            variable,
            iterable,
            body,
        } => expr_check(variable) || expr_check(iterable) || stmt_check(body),
        Expression::ForIn {
            variable,
            object,
            body,
        } => expr_check(variable) || expr_check(object) || stmt_check(body),
        Expression::Yield(inner) => inner.as_ref().is_some_and(|e| expr_check(e)),
        Expression::YieldDelegate(inner) => expr_check(inner),
        Expression::Class(class) => class
            .body
            .iter()
            .any(|member| class_member_check(member, expr_check, stmt_check)),
        _ => false,
    }
}

fn property_expr(key: &PropertyKey) -> Option<&Expression> {
    match key {
        PropertyKey::Computed(expr) => Some(expr),
        _ => None,
    }
}

fn arrow_body_check(
    body: &ArrowBody,
    expr_check: fn(&Expression) -> bool,
    stmt_check: fn(&Statement) -> bool,
) -> bool {
    match body {
        ArrowBody::Expression(expr) => expr_check(expr),
        ArrowBody::Block(stmts) => stmts.iter().any(stmt_check),
    }
}

fn property_value_check(
    val: &crate::ast::PropertyValue,
    expr_check: fn(&Expression) -> bool,
    stmt_check: fn(&Statement) -> bool,
) -> bool {
    match val {
        crate::ast::PropertyValue::Value(expr) => expr_check(expr),
        crate::ast::PropertyValue::Getter { body, .. }
        | crate::ast::PropertyValue::Setter { body, .. } => body.iter().any(stmt_check),
    }
}

fn class_member_check(
    member: &crate::ast::ClassMember,
    expr_check: fn(&Expression) -> bool,
    stmt_check: fn(&Statement) -> bool,
) -> bool {
    use crate::ast::ClassMember;
    match member {
        ClassMember::Method { body, .. }
        | ClassMember::StaticMethod { body, .. }
        | ClassMember::Constructor { body, .. } => body.iter().any(stmt_check),
        ClassMember::Field { value, .. } | ClassMember::StaticField { value, .. } => {
            expr_check(value)
        }
        ClassMember::StaticBlock { body, .. } => body.iter().any(stmt_check),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::Program;
    use crate::Context;

    fn eval(src: &str) -> Result<crate::value::Value, JsError> {
        Context::new().unwrap().eval(src)
    }

    fn is_syntax_error(err: &JsError) -> bool {
        err.0.contains("SyntaxError")
    }

    #[test]
    fn direct_eval_super_property_allowed_in_derived_field() {
        eval("class A {} class C extends A { x = eval('super.x'); } new C();").unwrap();
    }

    #[test]
    fn direct_eval_super_call_in_class_field_throws_syntax_error() {
        let err =
            eval("class A {} class C extends A { x = eval('super();'); } new C();").unwrap_err();
        assert!(is_syntax_error(&err));
    }

    #[test]
    fn indirect_eval_super_call_in_class_field_throws_syntax_error() {
        let err = eval("class A {} class C extends A { x = (0, eval)('super();'); } new C();")
            .unwrap_err();
        assert!(is_syntax_error(&err));
    }

    #[test]
    fn indirect_eval_super_property_in_class_field_throws_syntax_error() {
        let err = eval("class A {} class C extends A { x = (0, eval)('super.x'); } new C();")
            .unwrap_err();
        assert!(is_syntax_error(&err));
    }

    #[test]
    fn direct_eval_arguments_in_class_field_throws_syntax_error() {
        let err = eval("class C { x = eval('arguments'); } new C();").unwrap_err();
        assert!(is_syntax_error(&err));
    }

    #[test]
    fn indirect_eval_new_target_in_class_field_throws_syntax_error() {
        let err = eval("class C { x = (0, eval)('new.target'); } new C();").unwrap_err();
        assert!(is_syntax_error(&err));
    }

    #[test]
    fn super_property_is_not_super_call() {
        let program = Context::new().unwrap().parse("super.x;").unwrap();
        let Program::Script(body) = program;
        assert!(!program_contains_super_call(&body));
        assert!(program_contains_super_property(&body));
    }

    #[test]
    fn super_call_in_arrow_within_eval_is_detected() {
        let program = Context::new().unwrap().parse("() => super();").unwrap();
        let Program::Script(body) = program;
        assert!(program_contains_super_call(&body));
    }
}
