//! Property access and assignment for the stack machine.

use std::rc::Rc;
use std::cell::RefCell;

use crate::ast::*;
use crate::value::{Value, JsError, Object, ObjectKind, GetterStorage, ValueFunction, to_js_string, to_number};
use crate::env::Environment;

use crate::eval::operators::eval_binary_op;
use super::{Machine, AssignmentTarget, ObjectPropertyKind};

/// Get a static property key as a string slice.
pub fn property_key_static(key: &crate::ast::PropertyKey) -> Result<&str, crate::JsError> {
    match key {
        crate::ast::PropertyKey::Ident(s) => Ok(s),
        crate::ast::PropertyKey::String(s) => Ok(s),
        crate::ast::PropertyKey::Number(_n) => Err(crate::JsError("expected static property key".to_string())),
        crate::ast::PropertyKey::Computed(_) => Err(crate::JsError("expected static property key".to_string())),
    }
}

/// Read a property from a value.
pub fn read_property(
    obj_val: &Value,
    prop_name: &str,
    env: &Rc<RefCell<Environment>>,
) -> Result<Value, JsError> {
    match obj_val {
        Value::Object(o) => read_object_property(o, prop_name, env),
        Value::String(s) => read_string_property(s, prop_name),
        Value::Function(f) => read_function_property(f, prop_name),
        Value::NativeFunction(nf) => read_native_function_property(nf, prop_name),
        Value::NativeConstructor(nc) => read_native_constructor_property(nc, prop_name),
        Value::Number(_) => read_number_property(prop_name, env),
        _ => Ok(Value::Undefined),
    }
}

/// Read a property from an object value.
fn read_object_property(
    o: &Rc<RefCell<Object>>,
    prop_name: &str,
    env: &Rc<RefCell<Environment>>,
) -> Result<Value, JsError> {
    // Check for getter first
    {
        let obj = o.borrow();
        if let Some(getter_storage) = obj.get_getter(prop_name) {
            let getter_clone = getter_storage.clone();
            drop(obj);
            return call_getter(o, &getter_clone, env);
        }
    }
    // Check regular property
    {
        let obj = o.borrow();
        if let Some(val) = obj.get(prop_name) {
            return Ok(val);
        }
    }
    // Check global object for globals
    {
        let obj = o.borrow();
        if obj.kind == ObjectKind::Global {
            drop(obj);
            if let Some(val) = env.borrow().get(prop_name) {
                return Ok(val);
            }
            return Ok(Value::Undefined);
        }
    }
    // Handle Date.prototype specially
    {
        let obj = o.borrow();
        if obj.kind == ObjectKind::Date && prop_name == "prototype" {
            let mut proto = Object::new(ObjectKind::Ordinary);
            let date_constructor = Value::Object(Rc::clone(o));
            proto.set("constructor", date_constructor);
            return Ok(Value::Object(Rc::new(RefCell::new(proto))));
        }
    }
    Ok(Value::Undefined)
}

/// Read a property from a function value.
fn read_function_property(f: &ValueFunction, prop_name: &str) -> Result<Value, JsError> {
    if prop_name == "name" {
        Ok(Value::String(f.name.clone().unwrap_or_default()))
    } else if prop_name == "prototype" {
        Ok(Value::Object(f.get_prototype()))
    } else {
        let proto = f.get_prototype();
        let result = proto.borrow().get(prop_name)
            .unwrap_or(Value::Undefined);
        Ok(result)
    }
}

/// Read a property from a native function value.
fn read_native_function_property(nf: &Rc<crate::value::NativeFunction>, prop_name: &str) -> Result<Value, JsError> {
    match prop_name {
        "name" => Ok(Value::String("anonymous".to_string())),
        "prototype" => {
            let mut proto = Object::new(ObjectKind::Ordinary);
            proto.set("constructor", Value::NativeFunction(Rc::clone(nf)));
            Ok(Value::Object(Rc::new(RefCell::new(proto))))
        }
        "length" => Ok(Value::Number(0.0)),
        "call" | "apply" => Ok(Value::NativeFunction(Rc::clone(nf))),
        _ => Ok(Value::Undefined),
    }
}

/// Read a property from a native constructor value.
fn read_native_constructor_property(nc: &crate::value::NativeConstructor, prop_name: &str) -> Result<Value, JsError> {
    match prop_name {
        "prototype" => Ok(Value::Object(Rc::clone(&nc.prototype))),
        "length" => Ok(Value::Number(0.0)),
        "name" => Ok(Value::String("anonymous".to_string())),
        _ => Ok(Value::Undefined),
    }
}

/// Read a property from a number value.
fn read_number_property(prop_name: &str, env: &Rc<RefCell<Environment>>) -> Result<Value, JsError> {
    if let Some(Value::Object(ref num_obj)) = env.borrow().get("Number") {
        let num_obj = num_obj.borrow();
        if let Some(Value::Object(ref proto)) = num_obj.get("prototype") {
            let proto_obj = proto.borrow();
            if let Some(val) = proto_obj.get(prop_name) {
                return Ok(val);
            }
        }
    }
    Ok(Value::Undefined)
}

/// Read a property from a string value.
pub fn read_string_property(s: &str, prop_name: &str) -> Result<Value, JsError> {
    use crate::value::{NativeFunction, to_js_string};

    match prop_name {
        "length" => Ok(Value::Number(s.len() as f64)),
        "charAt" | "charCodeAt" | "indexOf" | "substring" | "slice"
        | "toUpperCase" | "toLowerCase" | "trim" | "split"
        | "includes" | "startsWith" | "endsWith" | "replace" | "match"
        | "search" | "concat" => {
            let s_clone = s.to_string();
            let prop_name_clone = prop_name.to_string();
            Ok(Value::NativeFunction(Rc::new(NativeFunction::new(move |args| {
                let s = s_clone.clone();
                match prop_name_clone.as_str() {
                    "length" => Ok(Value::Number(s.len() as f64)),
                    "charAt" => {
                        let idx = args.first().map(|v| to_number(v) as usize).unwrap_or(0);
                        Ok(Value::String(s.chars().nth(idx).map(|c| c.to_string()).unwrap_or_default()))
                    }
                    "indexOf" => {
                        let needle = args.first().map(to_js_string).unwrap_or_default();
                        Ok(Value::Number(s.find(&needle).map(|i| i as f64).unwrap_or(-1.0)))
                    }
                    "toUpperCase" => Ok(Value::String(s.to_uppercase())),
                    "toLowerCase" => Ok(Value::String(s.to_lowercase())),
                    "trim" => Ok(Value::String(s.trim().to_string())),
                    "includes" => {
                        let needle = args.first().map(to_js_string).unwrap_or_default();
                        Ok(Value::Boolean(s.contains(&needle)))
                    }
                    "startsWith" => {
                        let needle = args.first().map(to_js_string).unwrap_or_default();
                        Ok(Value::Boolean(s.starts_with(&needle)))
                    }
                    "endsWith" => {
                        let needle = args.first().map(to_js_string).unwrap_or_default();
                        Ok(Value::Boolean(s.ends_with(&needle)))
                    }
                    "concat" => {
                        let sep = args.iter().map(to_js_string).collect::<Vec<_>>().join("");
                        Ok(Value::String(format!("{}{}", s, sep)))
                    }
                    "split" => {
                        let sep = args.first().map(to_js_string).unwrap_or_default();
                        let parts: Vec<Value> = if sep.is_empty() {
                            s.chars().map(|c| Value::String(c.to_string())).collect()
                        } else {
                            s.split(&sep).map(|p| Value::String(p.to_string())).collect()
                        };
                        Ok(Value::Object(Rc::new(RefCell::new(Object::new_array(parts.len())))))
                    }
                    "substring" => {
                        let start = args.first().map(|v| to_number(v) as usize).unwrap_or(0);
                        let end = args.get(1).map(|v| to_number(v) as usize).unwrap_or(s.len());
                        let start = start.min(s.len());
                        let end = end.min(s.len());
                        let start = start.min(end);
                        Ok(Value::String(s.chars().skip(start).take(end - start).collect()))
                    }
                    "slice" => {
                        let start = args.first().map(|v| to_number(v) as i64).unwrap_or(0) as isize;
                        let end = args.get(1).map(|v| to_number(v) as i64).unwrap_or(s.len() as i64) as isize;
                        let len = s.len() as isize;
                        let start = if start < 0 { (len + start).max(0) as usize } else { start as usize }.min(len as usize);
                        let end = if end < 0 { (len + end).max(0) as usize } else { end as usize }.min(len as usize);
                        let end = end.max(start);
                        Ok(Value::String(s.chars().skip(start).take(end - start).collect()))
                    }
                    "match" => {
                        let pattern = args.first().map(to_js_string).unwrap_or_default();
                        Ok(Value::Boolean(s.contains(&pattern)))
                    }
                    "search" => {
                        let pattern = args.first().map(to_js_string).unwrap_or_default();
                        Ok(Value::Number(s.find(&pattern).map(|i| i as f64).unwrap_or(-1.0)))
                    }
                    _ => Ok(Value::Undefined),
                }
            }))))
        }
        _ => Ok(Value::Undefined),
    }
}

/// Call a getter on an object.
pub fn call_getter(
    obj: &Rc<RefCell<Object>>,
    getter_storage: &GetterStorage,
    env: &Rc<RefCell<Environment>>,
) -> Result<Value, JsError> {
    let closure = Rc::clone(env);

    let mut call_env = Environment::with_parent(closure);
    call_env.current_scope_mut().set_this(Value::Object(Rc::clone(obj)));

    let call_env = Rc::new(RefCell::new(call_env));

    let machine = Machine::new(call_env);
    if getter_storage.body.is_empty() {
        Ok(Value::Undefined)
    } else {
        machine.run_statements(&getter_storage.body)
    }
}

/// Apply an object property assignment.
pub fn apply_object_property(
    machine: &mut Machine,
    key: String,
    kind: ObjectPropertyKind,
    obj_rc: Rc<RefCell<Object>>,
) -> Result<(), JsError> {
    let value = machine.pop_value();
    let frame_env = Rc::clone(&machine.current_frame().env);
    let mut obj = obj_rc.borrow_mut();
    match kind {
        ObjectPropertyKind::Value => {
            obj.set(&key, value);
        }
        ObjectPropertyKind::Getter => {
            if let Value::Function(f) = value {
                obj.set_getter(&key, Rc::clone(&f.body));
            }
        }
        ObjectPropertyKind::Setter => {
            if let Value::Function(f) = value {
                obj.set_setter(&key, f.params.first().cloned().unwrap_or_default(), Rc::clone(&f.body), frame_env);
            }
        }
    }
    Ok(())
}

/// Pop value, object, and key from stack; assign value to object[key].
pub fn apply_member_assign(machine: &mut Machine) -> Result<(), JsError> {
    let key = machine.pop_value();
    let obj_val = machine.pop_value();
    let value = machine.pop_value();
    let key_str = to_js_string(&key);
    match obj_val {
        Value::Object(obj_rc) => {
            let has_setter = {
                let obj = obj_rc.borrow();
                obj.get_setter(&key_str).is_some()
            };
            if has_setter {
                return Err(JsError("Setter not supported in member assignment".to_string()));
            }
            obj_rc.borrow_mut().set(&key_str, value);
            machine.current_frame().values.push(Value::Undefined);
            Ok(())
        }
        Value::String(_) => {
            Err(JsError("Cannot assign to property of a string".to_string()))
        }
        _ => Err(JsError("Cannot set property on non-object".to_string())),
    }
}

/// Apply a binary operation.
pub fn apply_binary(machine: &mut Machine, op: BinaryOp) -> Result<(), JsError> {
    use crate::eval::operators::eval_binary_op;

    let right = machine.pop_value();
    let left = machine.pop_value();
    let result = eval_binary_op(op, &left, &right)?;
    machine.current_frame().values.push(result);
    Ok(())
}

/// Apply a unary operation.
pub fn apply_unary(machine: &mut Machine, op: UnaryOp) -> Result<(), JsError> {
    use crate::eval::operators::eval_unary_op;

    let val = machine.pop_value();
    let result = eval_unary_op(op, &val)?;
    machine.current_frame().values.push(result);
    Ok(())
}

/// Apply an assignment to a target.
pub fn apply_assign(machine: &mut Machine, target: AssignmentTarget) -> Result<(), JsError> {
    let value = machine.pop_value();
    let frame = machine.current_frame();
    match target {
        AssignmentTarget::Identifier(name) => {
            let env = Rc::clone(&frame.env);
            if env.borrow().has(&name) {
                if let Some(kind) = env.borrow().get_kind(&name) {
                    if kind == VarKind::Const {
                        return Err(JsError("TypeError: Assignment to constant variable".to_string()));
                    }
                }
                env.borrow_mut().set(&name, value);
            } else {
                env.borrow_mut().define(name, value);
            }
            frame.values.push(Value::Undefined);
            Ok(())
        }
        AssignmentTarget::Member { obj: obj_rc, key } => {
            let has_setter = {
                let obj = obj_rc.borrow();
                obj.get_setter(&key).is_some()
            };
            if has_setter {
                let setter_storage = {
                    let obj = obj_rc.borrow();
                    obj.get_setter(&key).cloned()
                };
                if let Some(storage) = setter_storage {
                    return crate::stack_machine::calls::call_setter(machine, &obj_rc, &storage, value);
                }
            }
            obj_rc.borrow_mut().set(&key, value);
            frame.values.push(Value::Undefined);
            Ok(())
        }
    }
}

/// Apply a compound assignment (e.g., +=, -=, etc.).
pub fn apply_compound_assign(machine: &mut Machine, op: BinaryOp, _target: AssignmentTarget) -> Result<(), JsError> {
    let right = machine.pop_value();
    let left_val = machine.pop_value();
    let result = eval_binary_op(op, &left_val, &right)?;
    machine.current_frame().values.push(result);
    Ok(())
}
