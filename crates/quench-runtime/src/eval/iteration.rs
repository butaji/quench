//! Iteration support for for-of/for-in loops

use std::cell::RefCell;
use std::rc::Rc;

use crate::ast::{Expression, Statement, VarKind};
use crate::env::Environment;
use crate::eval::expression::eval_expression;
use crate::eval::object::{
    assign_to, call_iterator_return, declare_pattern_bindings_with_kind, obtain_iterator,
    take_iterator_step,
};
use crate::eval::statement::eval_statement;
use crate::interpreter::{
    loop_handles_break, loop_handles_continue, set_control_flow, take_control_flow, ControlFlow,
};
use crate::value::{JsError, Object, ObjectKind, Value};

/// Get an iterator for for-of/for-in loops (materialized; spread/destructuring).
pub fn get_iterator(value: &Value) -> Result<Vec<Value>, JsError> {
    match value {
        Value::Object(o) => get_object_iterator(o),
        Value::String(s) => get_string_iterator(s),
        Value::Generator(gen) => get_generator_values(gen),
        _ => Err(JsError("TypeError: Value is not iterable".to_string())),
    }
}

fn get_generator_values(
    gen: &Rc<RefCell<crate::value::GeneratorObject>>,
) -> Result<Vec<Value>, JsError> {
    let mut values = Vec::new();
    let mut g = gen.borrow_mut();
    loop {
        let result = g.next(Value::Undefined)?;
        if result.done {
            break;
        }
        values.push(result.value);
    }
    Ok(values)
}

fn get_object_iterator(o: &Rc<RefCell<Object>>) -> Result<Vec<Value>, JsError> {
    if o.borrow().kind == ObjectKind::Array {
        return Ok(o.borrow().elements.clone());
    }
    let env = Rc::new(RefCell::new(Environment::new()));
    let iterator = obtain_iterator(o)?;
    let mut index = 0usize;
    let mut items = Vec::new();
    loop {
        let (item, done) = take_iterator_step(&iterator, &mut index, &env)?;
        if done {
            break;
        }
        items.push(item);
    }
    Ok(items)
}

fn get_string_iterator(s: &str) -> Result<Vec<Value>, JsError> {
    Ok(s.chars().map(|c| Value::String(c.to_string())).collect())
}

/// Get enumerable property keys for for-in loop
pub fn get_enumerable_keys(value: &Value) -> Result<Vec<String>, JsError> {
    match value {
        Value::Object(o) => get_object_keys(o),
        Value::String(s) => Ok((0..s.len()).map(|i| i.to_string()).collect()),
        _ => Ok(vec![]),
    }
}

fn get_object_keys(o: &Rc<RefCell<Object>>) -> Result<Vec<String>, JsError> {
    // EnumerateObjectProperties: walk the prototype chain, collecting each
    // object's own enumerable keys — integer indices ascending, then string
    // keys in creation (descriptor insertion) order — skipping keys already
    // seen (shadowed).
    let mut keys = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut current: Option<Rc<RefCell<Object>>> = Some(Rc::clone(o));
    while let Some(obj_rc) = current {
        let obj = obj_rc.borrow();
        // Integer indices ascending (from both the elements vector for arrays
        // and numeric string keys in properties for ordinary objects).
        let mut numeric: Vec<(usize, String)> = Vec::new();
        for (k, _) in obj.properties.iter() {
            if let Some(i) = crate::value::object::as_array_index(k) {
                numeric.push((i, k.clone()));
            }
        }
        for i in 0..obj.elements.len() {
            if !obj.holes.contains(&i) {
                numeric.push((i, i.to_string()));
            }
        }
        numeric.sort_by_key(|(i, _)| *i);
        numeric.dedup_by_key(|(i, _)| *i);
        for (_, key) in numeric {
            if !seen.contains(&key) {
                seen.insert(key.clone());
                if obj.is_enumerable(&key) {
                    keys.push(key);
                }
            }
        }
        for key in obj.descriptors.keys() {
            if crate::value::object::as_array_index(key).is_some() {
                continue;
            }
            if !seen.contains(key) {
                seen.insert(key.clone());
                if obj.is_enumerable(key) {
                    keys.push(key.clone());
                }
            }
        }
        current = obj.prototype.clone();
    }
    Ok(keys)
}

fn abrupt_close(
    iterator: &Rc<RefCell<Object>>,
    completion: Result<Value, JsError>,
) -> Result<Value, JsError> {
    if let Some(close_err) = call_iterator_return(iterator, completion.is_err()) {
        return Err(close_err);
    }
    completion
}

fn eval_for_of_iterator(
    iterator: Rc<RefCell<Object>>,
    variable: &Expression,
    body: &Statement,
    loop_binding: Option<VarKind>,
    env: &Rc<RefCell<Environment>>,
    in_arrow_function: bool,
) -> Result<Value, JsError> {
    let per_iteration = loop_binding.is_some_and(|k| matches!(k, VarKind::Let | VarKind::Const));
    let mut index = 0usize;
    let mut v = Value::Undefined;
    loop {
        let (item, done) = take_iterator_step(&iterator, &mut index, env)?;
        if done {
            break;
        }
        let (flow, body_val) = match run_for_of_iteration(
            variable,
            &item,
            body,
            loop_binding,
            per_iteration,
            env,
            in_arrow_function,
        ) {
            Ok(pair) => pair,
            Err(e) => return abrupt_close(&iterator, Err(e)),
        };
        if let Some(flow) = flow {
            match flow {
                ControlFlow::Break(_) => {
                    if loop_handles_break(&flow, &[]) {
                        return abrupt_close(&iterator, Ok(body_val));
                    }
                    set_control_flow(flow);
                    return abrupt_close(&iterator, Ok(body_val));
                }
                ControlFlow::Continue(_) => {
                    if loop_handles_continue(&flow, &[]) {
                        v = body_val;
                        continue;
                    }
                    set_control_flow(flow);
                    return abrupt_close(&iterator, Ok(body_val));
                }
                ControlFlow::Return(val)
                | ControlFlow::Yield(val)
                | ControlFlow::YieldDelegate(val) => {
                    let val = val.clone();
                    return abrupt_close(&iterator, Ok(val));
                }
            }
        }
        v = body_val;
    }
    if let Some(ControlFlow::Return(val))
    | Some(ControlFlow::Yield(val))
    | Some(ControlFlow::YieldDelegate(val)) = take_control_flow()
    {
        Ok(val)
    } else {
        Ok(v)
    }
}

/// Collect the identifier names assigned by a for-of LHS expression, including
/// identifiers wrapped in `AssignmentTarget` (as produced for assignment LHS).
fn collect_lhs_identifiers(variable: &Expression) -> Vec<String> {
    fn collect_binding(binding: &crate::ast::BindingElement) -> Vec<String> {
        match binding {
            crate::ast::BindingElement::Identifier(name) => {
                if name != "__hole" {
                    vec![name.clone()]
                } else {
                    vec![]
                }
            }
            crate::ast::BindingElement::ArrayPattern(elements) => {
                elements.iter().flat_map(collect_binding).collect()
            }
            crate::ast::BindingElement::ObjectPattern(props) => props
                .iter()
                .flat_map(|(_, b)| collect_binding(b))
                .collect(),
            crate::ast::BindingElement::Default(b, _) => collect_binding(b),
            crate::ast::BindingElement::Rest(b) => collect_binding(b),
            crate::ast::BindingElement::AssignmentTarget(e) => match e {
                Expression::Identifier(name) => vec![name.clone()],
                _ => vec![],
            },
        }
    }
    match variable {
        Expression::Identifier(name) => vec![name.clone()],
        Expression::ArrayPattern(bindings) => bindings.iter().flat_map(collect_binding).collect(),
        Expression::ObjectPattern(props) => props
            .iter()
            .flat_map(|(_, b)| collect_binding(b))
            .collect(),
        _ => Vec::new(),
    }
}

fn run_for_of_iteration(
    variable: &Expression,
    item: &Value,
    body: &Statement,
    loop_binding: Option<VarKind>,
    per_iteration: bool,
    env: &Rc<RefCell<Environment>>,
    in_arrow_function: bool,
) -> Result<(Option<ControlFlow>, Value), JsError> {
    if per_iteration {
        env.borrow_mut().push_scope();
    }
    let result = (|| {
        if let Some(kind) = loop_binding {
            declare_for_of_binding(variable, kind, env)?;
        } else {
            // Assignment LHS: assigning to a binding in its TDZ must throw
            // ReferenceError (e.g. a `let` declared later in the same scope).
            for name in collect_lhs_identifiers(variable) {
                if env.borrow().is_tdz(&name) {
                    let (_, js_err) = crate::value::error::create_js_error_with_type(
                        &format!("Cannot access '{}' before initialization", name),
                        "ReferenceError",
                    );
                    return Err(js_err);
                }
            }
        }
        assign_to(variable, item, env)?;
        eval_statement(body, env, false, in_arrow_function)
    })();
    let body_val = match &result {
        Ok(val) => val.clone(),
        Err(_) => Value::Undefined,
    };
    if per_iteration {
        env.borrow_mut().pop_scope();
    }
    match result {
        Ok(_) => match take_control_flow() {
            Some(cf @ ControlFlow::Break(_)) => Ok((Some(cf), body_val)),
            Some(cf @ ControlFlow::Continue(_)) => Ok((Some(cf), body_val)),
            Some(ControlFlow::Return(val))
            | Some(ControlFlow::Yield(val))
            | Some(ControlFlow::YieldDelegate(val)) => {
                set_control_flow(ControlFlow::Return(val.clone()));
                Ok((Some(ControlFlow::Return(val)), body_val))
            }
            None => Ok((None, body_val)),
        },
        Err(e) => Err(e),
    }
}

fn declare_for_of_binding(
    variable: &Expression,
    kind: VarKind,
    env: &Rc<RefCell<Environment>>,
) -> Result<(), JsError> {
    match variable {
        Expression::Identifier(name) => {
            env.borrow_mut().declare_var(name.clone(), kind);
            Ok(())
        }
        Expression::ArrayPattern(bindings) => {
            for binding in bindings {
                declare_pattern_bindings_with_kind(binding, kind, env);
            }
            Ok(())
        }
        Expression::ObjectPattern(props) => {
            for (_, binding) in props {
                declare_pattern_bindings_with_kind(binding, kind, env);
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

/// Declare a for-in let/const binding in TDZ (before the object expression is
/// evaluated), so referencing it throws ReferenceError.
fn declare_for_in_binding_tdz(
    variable: &Expression,
    env: &Rc<RefCell<Environment>>,
) -> Result<(), JsError> {
    let names: Vec<String> = match variable {
        Expression::Identifier(name) => vec![name.clone()],
        Expression::ArrayPattern(bindings) => bindings
            .iter()
            .flat_map(crate::lower::pattern::collect_pattern_identifiers)
            .collect(),
        Expression::ObjectPattern(props) => props
            .iter()
            .flat_map(|(_, b)| crate::lower::pattern::collect_pattern_identifiers(b))
            .collect(),
        _ => Vec::new(),
    };
    for name in names {
        if !env.borrow().current_scope().borrow().has(&name) {
            env.borrow_mut().declare_var(name.clone(), VarKind::Let);
        }
        env.borrow_mut().current_scope().borrow_mut().mark_tdz(name);
    }
    Ok(())
}

/// Evaluate a for-of loop
pub fn eval_for_of(
    variable: &Expression,
    iterable: &Expression,
    body: &Statement,
    loop_binding: Option<crate::ast::VarKind>,
    env: &Rc<RefCell<Environment>>,
    in_arrow_function: bool,
) -> Result<Value, JsError> {
    // Declare the let/const binding in TDZ before evaluating the iterable, so
    // the iterable expression observes the TDZ (reference → ReferenceError).
    let tdz_scope = loop_binding.is_some_and(|k| matches!(k, VarKind::Let | VarKind::Const));
    if tdz_scope {
        env.borrow_mut().push_scope();
        let _ = declare_for_in_binding_tdz(variable, env);
    }
    let result = (|| {
        let iter_value = eval_expression(iterable, env, in_arrow_function)?;
        let iterator = match &iter_value {
            Value::String(s) => {
                let items: Vec<Value> = s
                    .chars()
                    .map(|c| Value::String(c.to_string()))
                    .collect();
                let arr = Object::new_array_from(items);
                Rc::new(RefCell::new(arr))
            }
            Value::Generator(gen) => {
                crate::value::generator::generator_as_iterator_object(Rc::clone(gen))
            }
            Value::Object(o) => obtain_iterator(o)?,
            _ => return Err(JsError("TypeError: Value is not iterable".to_string())),
        };
        eval_for_of_iterator(iterator, variable, body, loop_binding, env, in_arrow_function)
    })();
    if tdz_scope {
        env.borrow_mut().pop_scope();
    }
    result
}

/// Evaluate a for-in loop
pub fn eval_for_in(
    variable: &Expression,
    object: &Expression,
    body: &Statement,
    loop_binding: Option<VarKind>,
    env: &Rc<RefCell<Environment>>,
    in_arrow_function: bool,
) -> Result<Value, JsError> {
    // Declare the let/const binding in TDZ before evaluating the object, so
    // the object expression can observe the TDZ (reference → ReferenceError).
    // The binding lives in a scope confined to the for-in statement.
    let tdz_scope = loop_binding.is_some();
    if tdz_scope {
        env.borrow_mut().push_scope();
        let _ = declare_for_in_binding_tdz(variable, env);
    }
    let result = (|| -> Result<Value, JsError> {
        let obj_value = eval_expression(object, env, in_arrow_function)?;
        let keys = get_enumerable_keys(&obj_value)?;
        let per_iteration =
            loop_binding.is_some_and(|k| matches!(k, VarKind::Let | VarKind::Const));
        let mut v = Value::Undefined;
        for key in keys {
            // Live enumeration: skip keys deleted (or made non-enumerable)
            // during a previous iteration.
            if !is_key_enumerable(&obj_value, &key) {
                continue;
            }
            let (flow, body_val) = run_for_in_iteration(
                variable,
                &Value::String(key),
                body,
                loop_binding,
                per_iteration,
                env,
                in_arrow_function,
            )?;
            if let Some(flow) = flow {
                match flow {
                    ControlFlow::Break(_) => {
                        // A handled break returns the current body value.
                        return Ok(body_val);
                    }
                    ControlFlow::Continue(_) => {
                        v = body_val;
                        continue;
                    }
                    ControlFlow::Return(val)
                    | ControlFlow::Yield(val)
                    | ControlFlow::YieldDelegate(val) => {
                        return Ok(val);
                    }
                }
            }
            v = body_val;
        }
        if let Some(ControlFlow::Return(val))
        | Some(ControlFlow::Yield(val))
        | Some(ControlFlow::YieldDelegate(val)) = take_control_flow()
        {
            Ok(val)
        } else {
            Ok(v)
        }
    })();
    if tdz_scope {
        env.borrow_mut().pop_scope();
    }
    result
}

/// Whether `key` is still an enumerable own property (own object or a
/// prototype), i.e. it was not deleted or made non-enumerable during for-in.
fn is_key_enumerable(obj_value: &Value, key: &str) -> bool {
    let Value::Object(rc) = obj_value else {
        return false;
    };
    let mut current: Option<Rc<RefCell<Object>>> = Some(Rc::clone(rc));
    while let Some(obj_rc) = current {
        let obj = obj_rc.borrow();
        if obj.has_own(key) {
            return obj.is_enumerable(key);
        }
        current = obj.prototype.clone();
    }
    false
}

/// Run one for-in iteration: declare the per-iteration binding (if any) and
/// assign the key to the LHS pattern/identifier.
fn run_for_in_iteration(
    variable: &Expression,
    key: &Value,
    body: &Statement,
    loop_binding: Option<VarKind>,
    per_iteration: bool,
    env: &Rc<RefCell<Environment>>,
    in_arrow_function: bool,
) -> Result<(Option<ControlFlow>, Value), JsError> {
    if per_iteration {
        env.borrow_mut().push_scope();
    }
    let result = (|| {
        if let Some(kind) = loop_binding {
            declare_for_of_binding(variable, kind, env)?;
        }
        assign_to(variable, key, env)?;
        eval_statement(body, env, false, in_arrow_function)
    })();
    let body_val = match &result {
        Ok(val) => val.clone(),
        Err(_) => Value::Undefined,
    };
    if per_iteration {
        env.borrow_mut().pop_scope();
    }
    match result {
        Ok(_) => match take_control_flow() {
            Some(cf @ ControlFlow::Break(_)) => Ok((Some(cf), body_val)),
            Some(cf @ ControlFlow::Continue(_)) => Ok((Some(cf), body_val)),
            Some(ControlFlow::Return(val))
            | Some(ControlFlow::Yield(val))
            | Some(ControlFlow::YieldDelegate(val)) => {
                set_control_flow(ControlFlow::Return(val.clone()));
                Ok((Some(ControlFlow::Return(val)), body_val))
            }
            None => Ok((None, body_val)),
        },
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtins;
    use crate::context::Context;
    use crate::value::Value;

    fn new_ctx() -> Context {
        let mut ctx = Context::new().unwrap();
        builtins::register_builtins(&mut ctx);
        ctx
    }

    #[test]
    fn test_get_iterator_array() {
        let mut ctx = new_ctx();
        let arr = ctx.eval("[10, 20, 30]").unwrap();
        let items = get_iterator(&arr).unwrap();
        assert_eq!(items.len(), 3);
        assert_eq!(items[0], Value::Number(10.0));
    }

    #[test]
    fn test_for_of_array_sum() {
        let mut ctx = new_ctx();
        let result = ctx
            .eval("let s = 0; for (let x of [1, 2, 3]) { s += x; } s")
            .unwrap();
        assert_eq!(result, Value::Number(6.0));
    }

    #[test]
    fn test_for_of_return_closes_iterator() {
        let mut ctx = new_ctx();
        let result = ctx.eval(
            "class E extends Error {} \
             var error = new E(); \
             var iter = { \
               [Symbol.iterator]() { return this; }, \
               next() { return { done: false }; }, \
               return() { throw error; } \
             }; \
             class C extends class {} { \
               constructor() { \
                 super(); \
                 for (var k of iter) { return 0; } \
               } \
             }; \
             var threw = false; \
             try { new C(); } catch (e) { threw = (e instanceof E); } \
             threw",
        );
        assert_eq!(result.unwrap(), Value::Boolean(true));
    }

    #[test]
    fn test_for_of_non_iterable_throws() {
        let mut ctx = new_ctx();
        let result = ctx.eval("let s = 0; for (let x of 42) { s += x; }");
        assert!(result.is_err());
    }
}
