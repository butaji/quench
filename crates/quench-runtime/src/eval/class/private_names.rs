//! Class-scoped private name keys (unique per class id).

use std::cell::Cell;
use std::collections::HashSet;
use std::rc::Rc;

use crate::ast::{
    ArrowBody, Class, ClassMember, Expression, ForInit, PropertyKey, PropertyValue, Statement,
};
use crate::value::JsError;

thread_local! {
    static PARENT_CLASS_ID: Cell<Option<usize>> = const { Cell::new(None) };
    static DECLARED_PRIVATE: std::cell::RefCell<HashSet<String>> =
        std::cell::RefCell::new(HashSet::new());
}

pub fn scope_class_private_names(
    class: &mut Class,
    class_id: usize,
    parent_class_id: Option<usize>,
) {
    let declared = collect_declared_private_names(&class.body);
    PARENT_CLASS_ID.with(|c| c.set(parent_class_id));
    DECLARED_PRIVATE.with(|d| *d.borrow_mut() = declared);
    for member in &mut class.body {
        scope_class_member(member, class_id);
    }
}

pub(crate) fn collect_declared_private_names(members: &[ClassMember]) -> HashSet<String> {
    let mut names = HashSet::new();
    for member in members {
        if let Some(PropertyKey::Ident(s)) = member_private_key(member) {
            if is_private_name(s) {
                names.insert(bare_private_name(s));
            }
        }
    }
    names
}

fn is_private_name(s: &str) -> bool {
    is_unscoped_private(s) || crate::value::is_private_name_key(s)
}

fn member_private_key(member: &ClassMember) -> Option<&PropertyKey> {
    match member {
        ClassMember::Field { name, .. }
        | ClassMember::StaticField { name, .. }
        | ClassMember::Method { name, .. }
        | ClassMember::StaticMethod { name, .. }
        | ClassMember::Getter { name, .. }
        | ClassMember::StaticGetter { name, .. }
        | ClassMember::Setter { name, .. }
        | ClassMember::StaticSetter { name, .. } => Some(name),
        ClassMember::Constructor { .. } | ClassMember::StaticBlock { .. } => None,
    }
}

fn bare_private_name(s: &str) -> String {
    s.strip_prefix('#')
        .unwrap_or(s)
        .trim_start_matches("\0private:")
        .rsplit(':')
        .next()
        .unwrap_or(s)
        .trim_end_matches('\0')
        .to_string()
}

pub fn scope_script_private_names(body: &mut [Statement], class_id: usize) {
    scope_statements(body, class_id);
}

/// Direct eval in a class constructor/method: reject references to private names
/// not declared in the enclosing class (and not inherited via `extends`).
pub fn reject_undeclared_private_names_in_eval(
    body: &[Statement],
    declared: &HashSet<String>,
) -> Result<(), JsError> {
    if body
        .iter()
        .any(|s| stmt_references_undeclared_private(s, declared))
    {
        let (err_val, js_err) = crate::value::error::create_js_error_with_type(
            "Private field must be declared in an enclosing class",
            "SyntaxError",
        );
        crate::value::set_thrown_value(err_val);
        return Err(js_err);
    }
    Ok(())
}

fn stmt_references_undeclared_private(stmt: &Statement, declared: &HashSet<String>) -> bool {
    match stmt {
        Statement::Expression(expr) | Statement::Return(Some(expr)) | Statement::Throw(expr) => {
            expr_references_undeclared_private(expr, declared)
        }
        Statement::VarDeclaration {
            init: Some(expr), ..
        } => expr_references_undeclared_private(expr, declared),
        Statement::Block(stmts) | Statement::SequenceDecls(stmts) => stmts
            .iter()
            .any(|s| stmt_references_undeclared_private(s, declared)),
        Statement::If {
            condition,
            consequent,
            alternate,
        } => {
            expr_references_undeclared_private(condition, declared)
                || stmt_references_undeclared_private(consequent, declared)
                || alternate
                    .as_ref()
                    .is_some_and(|s| stmt_references_undeclared_private(s, declared))
        }
        Statement::While { condition, body }
        | Statement::DoWhile {
            condition, body, ..
        } => {
            expr_references_undeclared_private(condition, declared)
                || stmt_references_undeclared_private(body, declared)
        }
        Statement::For {
            init,
            condition,
            update,
            body,
        } => {
            init.as_ref()
                .is_some_and(|i| for_init_references_undeclared_private(i, declared))
                || condition
                    .as_ref()
                    .is_some_and(|e| expr_references_undeclared_private(e, declared))
                || update
                    .as_ref()
                    .is_some_and(|e| expr_references_undeclared_private(e, declared))
                || stmt_references_undeclared_private(body, declared)
        }
        Statement::Return(None) => false,
        _ => false,
    }
}

fn for_init_references_undeclared_private(init: &ForInit, declared: &HashSet<String>) -> bool {
    match init {
        ForInit::Expression(expr) => expr_references_undeclared_private(expr, declared),
        ForInit::VarDeclaration { init, .. } => init
            .as_ref()
            .is_some_and(|e| expr_references_undeclared_private(e, declared)),
    }
}

fn expr_references_undeclared_private(expr: &Expression, declared: &HashSet<String>) -> bool {
    match expr {
        Expression::Member {
            object, property, ..
        } => {
            expr_references_undeclared_private(object, declared)
                || private_key_undeclared(property, declared)
        }
        Expression::Assignment { left, right, .. }
        | Expression::CompoundAssignment { left, right, .. }
        | Expression::LogicalCompoundAssignment { left, right, .. } => {
            expr_references_undeclared_private(left, declared)
                || expr_references_undeclared_private(right, declared)
        }
        Expression::Call { callee, arguments } => {
            expr_references_undeclared_private(callee, declared)
                || arguments
                    .iter()
                    .any(|a| expr_references_undeclared_private(a, declared))
        }
        Expression::Unary { argument, .. } | Expression::Update { argument, .. } => {
            expr_references_undeclared_private(argument, declared)
        }
        Expression::Binary { left, right, .. } => {
            expr_references_undeclared_private(left, declared)
                || expr_references_undeclared_private(right, declared)
        }
        Expression::Conditional {
            condition,
            consequent,
            alternate,
        } => {
            expr_references_undeclared_private(condition, declared)
                || expr_references_undeclared_private(consequent, declared)
                || expr_references_undeclared_private(alternate, declared)
        }
        Expression::Array(exprs) | Expression::Sequence(exprs) => exprs
            .iter()
            .any(|e| expr_references_undeclared_private(e, declared)),
        Expression::Spread(inner) => expr_references_undeclared_private(inner, declared),
        Expression::Yield(Some(inner)) | Expression::YieldDelegate(inner) => {
            expr_references_undeclared_private(inner, declared)
        }
        _ => false,
    }
}

fn private_key_undeclared(key: &PropertyKey, declared: &HashSet<String>) -> bool {
    let PropertyKey::Ident(s) = key else {
        return false;
    };
    if !is_unscoped_private(s) {
        return false;
    }
    !declared.contains(&bare_private_name(s))
}

fn scope_class_member(member: &mut ClassMember, class_id: usize) {
    match member {
        ClassMember::Constructor { body, .. } => scope_statements(body, class_id),
        ClassMember::Method {
            name, body, params, ..
        }
        | ClassMember::StaticMethod {
            name, body, params, ..
        } => {
            scope_property_key(name, class_id);
            scope_params(params, class_id);
            scope_statements(body, class_id);
        }
        ClassMember::Getter { name, body } | ClassMember::StaticGetter { name, body } => {
            scope_property_key(name, class_id);
            scope_statements(body, class_id);
        }
        ClassMember::Setter {
            name, body, param, ..
        }
        | ClassMember::StaticSetter {
            name, body, param, ..
        } => {
            scope_property_key(name, class_id);
            scope_statements(body, class_id);
            let _ = param;
        }
        ClassMember::Field { name, value } | ClassMember::StaticField { name, value } => {
            scope_property_key(name, class_id);
            scope_expression(value, class_id);
        }
        ClassMember::StaticBlock { body } => scope_statements(body, class_id),
    }
}

fn scope_params(params: &mut [crate::ast::Param], class_id: usize) {
    for param in params.iter_mut() {
        if let Some(pattern) = &mut param.pattern {
            scope_binding_element(pattern, class_id);
        }
        if let Some(default) = &mut param.default {
            scope_expression(default, class_id);
        }
    }
}

fn scope_binding_element(elem: &mut crate::ast::BindingElement, class_id: usize) {
    match elem {
        crate::ast::BindingElement::Default(inner, default) => {
            scope_binding_element(inner, class_id);
            scope_expression(default, class_id);
        }
        crate::ast::BindingElement::ArrayPattern(elems) => {
            for e in elems.iter_mut() {
                scope_binding_element(e, class_id);
            }
        }
        crate::ast::BindingElement::ObjectPattern(props) => {
            for (key, e) in props.iter_mut() {
                scope_property_key(key, class_id);
                scope_binding_element(e, class_id);
            }
        }
        crate::ast::BindingElement::Rest(inner) => scope_binding_element(inner, class_id),
        crate::ast::BindingElement::AssignmentTarget(expr) => scope_expression(expr, class_id),
        crate::ast::BindingElement::Identifier(_) => {}
    }
}

fn scope_statements(stmts: &mut [Statement], class_id: usize) {
    for stmt in stmts.iter_mut() {
        scope_statement(stmt, class_id);
    }
}

fn scope_statement(stmt: &mut Statement, class_id: usize) {
    match stmt {
        Statement::VarDeclaration {
            init: Some(expr), ..
        } => scope_expression(expr, class_id),
        Statement::VarDeclaration { .. } => {}
        Statement::FunctionDeclaration { body, params, .. } => {
            scope_params(params, class_id);
            scope_statements(body, class_id);
        }
        Statement::ClassDeclaration { .. } | Statement::Import { .. } | Statement::Export(_) => {}
        Statement::If {
            condition,
            consequent,
            alternate,
        } => {
            scope_expression(condition, class_id);
            scope_statement(consequent, class_id);
            if let Some(alt) = alternate {
                scope_statement(alt, class_id);
            }
        }
        Statement::While { condition, body }
        | Statement::DoWhile {
            condition, body, ..
        } => {
            scope_expression(condition, class_id);
            scope_statement(body, class_id);
        }
        Statement::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(init) = init {
                scope_for_init(init, class_id);
            }
            if let Some(cond) = condition {
                scope_expression(cond, class_id);
            }
            if let Some(upd) = update {
                scope_expression(upd, class_id);
            }
            scope_statement(body, class_id);
        }
        Statement::ForIn {
            variable,
            object,
            body,
        } => {
            scope_expression(variable, class_id);
            scope_expression(object, class_id);
            scope_statement(body, class_id);
        }
        Statement::Block(stmts) | Statement::SequenceDecls(stmts) => {
            scope_statements(stmts, class_id);
        }
        Statement::Return(Some(e)) => scope_expression(e, class_id),
        Statement::Return(None) => {}
        Statement::Throw(expr) => scope_expression(expr, class_id),
        Statement::Expression(expr) => scope_expression(expr, class_id),
        Statement::Labeled { body, .. } => scope_statement(body, class_id),
        Statement::Try {
            body,
            handler,
            finalizer,
            ..
        } => {
            scope_statement(body, class_id);
            if let Some(h) = handler {
                scope_statement(h, class_id);
            }
            if let Some(f) = finalizer {
                scope_statement(f, class_id);
            }
        }
        Statement::With { object, body } => {
            scope_expression(object, class_id);
            scope_statement(body, class_id);
        }
        _ => {}
    }
}

fn scope_for_init(init: &mut ForInit, class_id: usize) {
    match init {
        ForInit::Expression(expr) => scope_expression(expr, class_id),
        ForInit::VarDeclaration { init, .. } => {
            if let Some(expr) = init {
                scope_expression(expr, class_id);
            }
        }
    }
}

fn scope_expression(expr: &mut Expression, class_id: usize) {
    match expr {
        Expression::Member {
            object, property, ..
        } => {
            scope_expression(object, class_id);
            scope_property_key(property, class_id);
        }
        Expression::Call { callee, arguments } => {
            scope_expression(callee, class_id);
            for arg in arguments.iter_mut() {
                scope_expression(arg, class_id);
            }
        }
        Expression::ArrowFunction {
            params,
            body,
            is_async,
            is_generator,
        } => {
            scope_params(params, class_id);
            scope_arrow_body(body, class_id);
            let _ = (is_async, is_generator);
        }
        Expression::FunctionExpression { params, body, .. } => {
            scope_params(params, class_id);
            scope_statements(body, class_id);
        }
        Expression::Assignment { left, right }
        | Expression::CompoundAssignment { left, right, .. }
        | Expression::LogicalCompoundAssignment { left, right, .. } => {
            scope_expression(left, class_id);
            scope_expression(right, class_id);
        }
        Expression::Binary { left, right, .. } => {
            scope_expression(left, class_id);
            scope_expression(right, class_id);
        }
        Expression::Unary { argument, .. } | Expression::Update { argument, .. } => {
            scope_expression(argument, class_id);
        }
        Expression::Conditional {
            condition,
            consequent,
            alternate,
        } => {
            scope_expression(condition, class_id);
            scope_expression(consequent, class_id);
            scope_expression(alternate, class_id);
        }
        Expression::Sequence(exprs) | Expression::Array(exprs) => {
            for e in exprs.iter_mut() {
                scope_expression(e, class_id);
            }
        }
        Expression::Object(props) => {
            for (_, val) in props.iter_mut() {
                scope_property_value(val, class_id);
            }
        }
        Expression::New {
            constructor,
            arguments,
        } => {
            scope_expression(constructor, class_id);
            for arg in arguments.iter_mut() {
                scope_expression(arg, class_id);
            }
        }
        Expression::Spread(inner) => scope_expression(inner, class_id),
        Expression::BlockExpr(stmts) => scope_statements(stmts, class_id),
        Expression::Yield(Some(e)) => scope_expression(e, class_id),
        Expression::Yield(None) => {}
        Expression::YieldDelegate(inner) => scope_expression(inner, class_id),
        _ => {}
    }
}

fn scope_arrow_body(body: &mut ArrowBody, class_id: usize) {
    match body {
        ArrowBody::Expression(expr) => scope_expression(expr, class_id),
        ArrowBody::Block(stmts) => {
            let inner: &mut Vec<Statement> = Rc::make_mut(stmts);
            scope_statements(inner, class_id);
        }
    }
}

fn scope_property_value(val: &mut PropertyValue, class_id: usize) {
    match val {
        PropertyValue::Value(expr) => scope_expression(expr, class_id),
        PropertyValue::Getter { body, .. } | PropertyValue::Setter { body, .. } => {
            scope_statements(body, class_id);
        }
    }
}

fn scope_property_key(key: &mut PropertyKey, class_id: usize) {
    if let PropertyKey::Ident(s) = key {
        if is_unscoped_private(s) {
            let bare = bare_private_name(s);
            let target_id = if DECLARED_PRIVATE.with(|d| d.borrow().contains(&bare)) {
                class_id
            } else {
                PARENT_CLASS_ID.with(|p| p.get().unwrap_or(class_id))
            };
            *s = crate::value::scoped_private_name_key(target_id, s);
        }
    }
}

fn is_unscoped_private(s: &str) -> bool {
    s.starts_with('#') || crate::value::is_unscoped_private_name_key(s)
}

#[cfg(test)]
mod tests {
    use crate::Context;

    #[test]
    fn distinct_classes_with_same_private_ident_do_not_share_storage() {
        let src = "class C { #m = 44; get() { return this.#m; } } \
                   class D { #m = 99; } \
                   C.prototype.get.call(new D())";
        let err = Context::new().unwrap().eval(src).unwrap_err();
        assert!(err.0.contains("TypeError"), "got {}", err.0);
    }

    #[test]
    fn nested_class_accesses_outer_private_field() {
        let src = "class Outer { #x = 42; innerclass() { \
                   return class extends Outer { f() { this.#x = 1; } }; } \
                   value() { return this.#x; } } \
                   var outer = new Outer(); var Inner = outer.innerclass(); var i = new Inner(); \
                   outer.value() === 42 && i.value() === 42";
        let r = Context::new().unwrap().eval(src).unwrap();
        assert_eq!(r, crate::value::Value::Boolean(true));
    }
}
