//! Object member access evaluation

use crate::context::CURRENT_CONTEXT;
use crate::env::Environment;
use crate::eval::object::call_getter;
use crate::value::object::as_array_index;
use crate::value::{create_js_error_with_type, JsError, Object, ObjectKind, Value};
use std::cell::RefCell;
use std::rc::Rc;

/// Evaluate member access on an object. If `env` is provided, global object
/// lookups fall back to the environment's globalThis binding for properties
/// stored as built-in globals (e.g. `isFinite`, `parseInt`, etc.).
pub fn eval_object_member(
    o: &Rc<RefCell<Object>>,
    prop_name: &str,
    env: Option<&Rc<RefCell<Environment>>>,
) -> Result<Value, JsError> {
    if crate::builtins::function::is_function_prototype(o)
        && (prop_name == "arguments" || prop_name == "caller")
    {
        return Err(JsError(
            crate::builtins::function::get_restricted_prop_error(),
        ));
    }
    if crate::value::is_private_name_key(prop_name) {
        return eval_private_name_get(o, prop_name);
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
                if let Some(idx) = as_array_index(prop_name) {
                    if idx < obj.elements.len() && !obj.holes.contains(&idx) {
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
        // For global object: built-in globals (isFinite, parseInt, etc.) are stored
        // in the environment's bindings. Fall back to globalThis bindings if the
        // property wasn't found on the object itself or its prototype chain.
        if obj.kind == ObjectKind::Global {
            // Try the provided env first, then fall back to CURRENT_CONTEXT thread-local.
            let fallback_env = env.or_else(|| {
                CURRENT_CONTEXT.with(|cell| cell.borrow().map(|ptr| unsafe { &*ptr }.env()))
            });
            if let Some(e) = fallback_env {
                if let Some(Value::Object(global_rc)) = e.borrow().get("globalThis").as_ref() {
                    let global = global_rc.borrow();
                    if let Some(found) = global.properties.get(prop_name) {
                        return Ok(found.clone());
                    }
                }
            }
        }
    }
    Ok(Value::Undefined)
}

fn eval_private_name_get(o: &Rc<RefCell<Object>>, prop_name: &str) -> Result<Value, JsError> {
    let obj = o.borrow();
    if let Some(getter_storage) = obj.get_getter(prop_name) {
        let getter_clone = getter_storage.clone();
        drop(obj);
        return call_getter(o, &getter_clone, &Rc::new(RefCell::new(Environment::new())));
    }
    if let Some(val) = obj.properties.get(prop_name) {
        return Ok(val.clone());
    }
    let (_, js_err) = create_js_error_with_type(
        "Cannot read private member from an object whose class did not declare it",
        "TypeError",
    );
    Err(js_err)
}

#[cfg(test)]
mod tests {
    use crate::value::Value;
    use crate::Context;

    #[test]
    fn non_canonical_numeric_member_does_not_alias_element() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("var a = [10, 20]; a['01']").unwrap();
        assert_eq!(result, Value::Undefined);
    }

    #[test]
    fn private_field_brand_check_throws_for_foreign_object() {
        let mut ctx = Context::new().unwrap();
        let err = ctx
            .eval(
                "class C { #m = 44; getWithEval() { return eval(\"this.#m\"); } } \
                 class D { #m = 44; } \
                 C.prototype.getWithEval.call(new D())",
            )
            .unwrap_err();
        assert!(err.0.contains("TypeError"), "got {}", err.0);
    }
}
