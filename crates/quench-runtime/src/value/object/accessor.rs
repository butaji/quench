//! Getter/setter methods for Object.

use std::cell::RefCell;
use std::rc::Rc;

use crate::ast::{Param, Statement};
use crate::env::Environment;
use crate::value::function::ValueFunction;

/// Set a getter function for a property
pub fn set_getter(
    obj: &mut crate::value::Object,
    key: &str,
    body: std::rc::Rc<Vec<Statement>>,
    closure: std::rc::Rc<std::cell::RefCell<Environment>>,
    is_method: bool,
) {
    let strict = crate::interpreter::is_strict_mode();
    // Create and cache the ValueFunction immediately so that
    // getOwnPropertyDescriptor returns the same function object.
    let mut func = ValueFunction::new(None, vec![], (*body).clone(), closure.clone(), false, false);
    func.is_method = is_method;
    let func = crate::value::Value::Function(func);
    obj.getters.insert(
        key.to_string(),
        crate::value::object::helpers::GetterStorage {
            body,
            closure,
            func: Some(func),
            strict,
        },
    );
    // Set correct flags: non-enumerable (class getters are not enumerable),
    // configurable (can be deleted/reconfigured).
    let flags = crate::value::object::helpers::PropertyFlags {
        enumerable: false,
        configurable: true,
        ..Default::default()
    };
    obj.descriptors.insert(key.to_string(), flags);
}

/// Install a getter from a function value (Object.defineProperty path)
pub fn set_getter_func(obj: &mut crate::value::Object, key: &str, func: crate::value::Value) {
    obj.getters.insert(
        key.to_string(),
        crate::value::object::helpers::GetterStorage {
            body: std::rc::Rc::new(Vec::new()),
            closure: std::rc::Rc::new(std::cell::RefCell::new(Environment::new())),
            func: Some(func),
            strict: crate::interpreter::is_strict_mode(),
        },
    );
}

/// Set a setter function for a property
pub fn set_setter(
    obj: &mut crate::value::Object,
    key: &str,
    param: String,
    body: std::rc::Rc<Vec<Statement>>,
    closure: std::rc::Rc<std::cell::RefCell<Environment>>,
    is_method: bool,
) {
    let strict = crate::interpreter::is_strict_mode();
    // Create and cache the ValueFunction immediately so that
    // getOwnPropertyDescriptor returns the same function object.
    let mut func = ValueFunction::new(
        None,
        vec![Param::new(&param)],
        (*body).clone(),
        closure.clone(),
        false,
        false,
    );
    func.is_method = is_method;
    let func = crate::value::Value::Function(func);
    obj.setters.insert(
        key.to_string(),
        crate::value::object::helpers::SetterStorage {
            param,
            body,
            closure,
            func: Some(func),
            strict,
        },
    );
    // Set correct flags: non-enumerable (class setters are not enumerable),
    // configurable (can be deleted/reconfigured).
    let flags = crate::value::object::helpers::PropertyFlags {
        enumerable: false,
        configurable: true,
        ..Default::default()
    };
    obj.descriptors.insert(key.to_string(), flags);
}

/// Install a setter from a function value (Object.defineProperty path)
pub fn set_setter_func(obj: &mut crate::value::Object, key: &str, func: crate::value::Value) {
    let _ = matches!(func, crate::value::Value::Function(_));
    obj.setters.insert(
        key.to_string(),
        crate::value::object::helpers::SetterStorage {
            param: String::new(),
            body: std::rc::Rc::new(Vec::new()),
            closure: std::rc::Rc::new(std::cell::RefCell::new(Environment::new())),
            func: Some(func),
            strict: crate::interpreter::is_strict_mode(),
        },
    );
}

/// Define an accessor property (get/set function values + flags).
pub fn define_accessor(
    obj: &mut crate::value::Object,
    key: &str,
    getter: Option<crate::value::Value>,
    setter: Option<crate::value::Value>,
    flags: crate::value::object::helpers::PropertyFlags,
) {
    if let Some(g) = getter {
        set_getter_func(obj, key, g);
    }
    if let Some(s) = setter {
        set_setter_func(obj, key, s);
    }
    obj.descriptors.insert(key.to_string(), flags);
}

/// Check if property has a getter
pub fn has_getter(obj: &crate::value::Object, key: &str) -> bool {
    obj.getters.contains_key(key)
}

/// Check if property has a setter
pub fn has_setter(obj: &crate::value::Object, key: &str) -> bool {
    obj.setters.contains_key(key)
}

/// Get the getter storage for a property
pub fn get_getter<'a>(
    obj: &'a crate::value::Object,
    key: &str,
) -> Option<&'a crate::value::object::helpers::GetterStorage> {
    obj.getters.get(key)
}

/// Get the setter storage for a property
pub fn get_setter<'a>(
    obj: &'a crate::value::Object,
    key: &str,
) -> Option<&'a crate::value::object::helpers::SetterStorage> {
    obj.setters.get(key)
}

/// Get the setter function value (from Object.defineProperty style or {set} shorthand).
pub fn get_setter_func(obj: &crate::value::Object, key: &str) -> Option<crate::value::Value> {
    obj.setters.get(key).and_then(|s| {
        if let Some(ref f) = s.func {
            return Some(f.clone());
        }
        if !s.body.is_empty() {
            let closure = Rc::new(RefCell::new((*s.closure).borrow().clone()));
            let func = crate::value::Value::Function(ValueFunction::new(
                None,
                vec![Param::new(&s.param)],
                (*s.body).clone(),
                closure,
                false,
                false,
            ));
            return Some(func);
        }
        None
    })
}
