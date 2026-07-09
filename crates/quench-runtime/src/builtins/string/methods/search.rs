//! String search methods (indexOf, lastIndexOf, includes, startsWith, endsWith)

use std::cell::RefCell;
use std::rc::Rc;

use crate::value::{to_js_string, to_number, NativeFunction, Object, Value};

/// Install indexOf and lastIndexOf methods
fn install_index_methods(proto: &Rc<RefCell<Object>>) {
    let proto_clone = Rc::clone(proto);

    proto_clone.borrow_mut().set("indexOf", Value::NativeFunction(Rc::new(NativeFunction::new(move |args| {
        match crate::builtins::get_native_this() {
            Some(Value::String(s)) => {
                let needle = args.first().map(to_js_string).unwrap_or_default();
                let start = args.get(1).map(|v| to_number(v) as usize).unwrap_or(0);
                Ok(Value::Number(s[start..].find(&needle).map(|i| (start + i) as f64).unwrap_or(-1.0)))
            }
            _ => Ok(Value::Undefined),
        }
    }))));

    proto_clone.borrow_mut().set("lastIndexOf", Value::NativeFunction(Rc::new(NativeFunction::new(move |args| {
        match crate::builtins::get_native_this() {
            Some(Value::String(s)) => {
                let needle = args.first().map(to_js_string).unwrap_or_default();
                let pos = args.get(1).map(|v| to_number(v) as usize).unwrap_or(usize::MAX);
                let pos = pos.min(s.len());
                let result = s[..pos].rfind(&needle).map(|i| i as f64).unwrap_or(-1.0);
                Ok(Value::Number(result))
            }
            _ => Ok(Value::Undefined),
        }
    }))));
}

/// Install includes, startsWith, endsWith methods
fn install_prefix_suffix_methods(proto: &Rc<RefCell<Object>>) {
    let proto_clone = Rc::clone(proto);

    proto_clone.borrow_mut().set("includes", Value::NativeFunction(Rc::new(NativeFunction::new(move |args| {
        match crate::builtins::get_native_this() {
            Some(Value::String(s)) => {
                let needle = args.first().map(to_js_string).unwrap_or_default();
                let start = args.get(1).map(|v| to_number(v) as usize).unwrap_or(0);
                Ok(Value::Boolean(s[start..].contains(&needle)))
            }
            _ => Ok(Value::Undefined),
        }
    }))));

    proto_clone.borrow_mut().set("startsWith", Value::NativeFunction(Rc::new(NativeFunction::new(move |args| {
        match crate::builtins::get_native_this() {
            Some(Value::String(s)) => {
                let needle = args.first().map(to_js_string).unwrap_or_default();
                let start = args.get(1).map(|v| to_number(v) as usize).unwrap_or(0);
                Ok(Value::Boolean(s[start..].starts_with(&needle)))
            }
            _ => Ok(Value::Undefined),
        }
    }))));

    proto_clone.borrow_mut().set("endsWith", Value::NativeFunction(Rc::new(NativeFunction::new(move |args| {
        match crate::builtins::get_native_this() {
            Some(Value::String(s)) => {
                let needle = args.first().map(to_js_string).unwrap_or_default();
                let end_pos = args.get(1).map(|v| to_number(v) as usize);
                let matches = if let Some(pos) = end_pos {
                    s[..pos.min(s.len())].ends_with(&needle)
                } else {
                    s.ends_with(&needle)
                };
                Ok(Value::Boolean(matches))
            }
            _ => Ok(Value::Undefined),
        }
    }))));
}

/// Install all search methods
pub fn install_search_methods(proto: &Rc<RefCell<Object>>) {
    install_index_methods(proto);
    install_prefix_suffix_methods(proto);
}
