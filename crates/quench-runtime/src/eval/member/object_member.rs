//! Object member access evaluation

use crate::env::Environment;
use crate::eval::object::call_getter;
use crate::value::{JsError, Object, ObjectKind, Value};
use std::cell::RefCell;
use std::rc::Rc;

/// Evaluate member access on an object
pub fn eval_object_member(o: &Rc<RefCell<Object>>, prop_name: &str) -> Result<Value, JsError> {
    if crate::builtins::function::is_function_prototype(o)
        && (prop_name == "arguments" || prop_name == "caller")
    {
        return Err(JsError(
            crate::builtins::function::get_restricted_prop_error(),
        ));
    }
    {
        let mut current: Option<Rc<RefCell<Object>>> = Some(Rc::clone(o));
        while let Some(obj_rc) = current {
            {
                let obj = obj_rc.borrow();
                if let Some(getter_storage) = obj.get_getter(prop_name) {
                    let getter_clone = getter_storage.clone();
                    drop(obj);
                    return call_getter(
                        o,
                        &getter_clone,
                        &Rc::new(RefCell::new(Environment::new())),
                    );
                }
                if let Some(val) = obj.properties.get(prop_name) {
                    return Ok(val.clone());
                }
                // Check symbol properties (key stored as raw "Symbol():N" format)
                if let Some(val) = obj.symbol_properties.get(prop_name) {
                    return Ok(val.clone());
                }
                if let Ok(idx) = prop_name.parse::<usize>() {
                    if idx < obj.elements.len() {
                        return Ok(obj.elements[idx].clone());
                    }
                }
                current = obj.prototype.as_ref().map(Rc::clone);
            }
        }
    }
    {
        let obj = o.borrow();
        if obj.kind == ObjectKind::Date && prop_name == "prototype" {
            let mut proto = Object::new(ObjectKind::Ordinary);
            proto.set("constructor", Value::Object(Rc::clone(o)));
            return Ok(Value::Object(Rc::new(RefCell::new(proto))));
        }
        // Handle __proto__ as a getter for the internal prototype
        if prop_name == "__proto__" {
            if let Some(ref proto_obj) = obj.prototype {
                return Ok(Value::Object(Rc::clone(proto_obj)));
            }
            return Ok(Value::Undefined);
        }
    }
    Ok(Value::Undefined)
}
