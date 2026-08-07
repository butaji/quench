//! Member access helpers.

use crate::env::Environment;
use crate::eval::string_methods::get_string_method;
use crate::value::{JsError, Object, ObjectKind, Value};
use std::cell::RefCell;
use std::rc::Rc;

/// Get a member function from a value.
pub fn get_member_function(
    obj_val: &Value,
    prop_name: &str,
    env: &Rc<RefCell<Environment>>,
) -> Result<Value, JsError> {
    match obj_val {
        Value::Object(o) => crate::eval::member::eval_object_member(o, prop_name, Some(env)),
        Value::String(s) => get_string_method(s, prop_name, env),
        Value::Number(_) | Value::BigInt(_) => get_number_method(obj_val, prop_name, env),
        Value::Function(f) => crate::eval::member::eval_function_member(f, prop_name),
        Value::NativeFunction(nf) => {
            crate::eval::member::eval_native_function_member(nf, prop_name)
        }
        Value::NativeConstructor(nc) => {
            crate::eval::member::eval_native_constructor_member(nc, prop_name)
        }
        Value::Class(class) => {
            let val = crate::eval::member::eval_member_access(obj_val, prop_name, env)?;
            if !matches!(val, Value::Undefined) {
                return Ok(val);
            }
            let proto = crate::eval::class::get_or_create_class_prototype(class, env)?;
            crate::eval::member::eval_object_member(&proto, prop_name, Some(env))
        }
        Value::Generator(gen) => {
            let is_async = gen.borrow().is_async;
            match prop_name {
                "next" => {
                    if is_async {
                        Ok(crate::value::generator::async_generator_next_fn(
                            gen.clone(),
                        ))
                    } else {
                        Ok(crate::value::generator::generator_next_fn(gen.clone()))
                    }
                }
                "return" => {
                    if is_async {
                        Ok(crate::value::generator::async_generator_return_fn(
                            gen.clone(),
                        ))
                    } else {
                        Ok(crate::value::generator::generator_return_fn(gen.clone()))
                    }
                }
                "throw" => {
                    if is_async {
                        Ok(crate::value::generator::async_generator_throw_fn(
                            gen.clone(),
                        ))
                    } else {
                        Ok(crate::value::generator::generator_throw_fn(gen.clone()))
                    }
                }
                _ => Ok(Value::Undefined),
            }
        }
        _ => Ok(Value::Undefined),
    }
}

/// Check if a property key matches a name.
pub fn name_matches_prop(key: &crate::ast::PropertyKey, name: &str) -> bool {
    match key {
        crate::ast::PropertyKey::Ident(s) => {
            s == name || (s.starts_with('#') && name == crate::value::private_name_key(s))
        }
        crate::ast::PropertyKey::String(s) => s == name,
        crate::ast::PropertyKey::Number(n) => n.to_string() == name,
        crate::ast::PropertyKey::Computed(_) => false,
    }
}

/// Get a method from a boxed Number/Boolean/Symbol/BigInt.
pub fn get_number_method(
    obj_val: &Value,
    prop_name: &str,
    env: &Rc<RefCell<Environment>>,
) -> Result<Value, JsError> {
    if !matches!(
        obj_val,
        Value::Number(_) | Value::Boolean(_) | Value::Symbol(_) | Value::BigInt(_)
    ) {
        return Ok(Value::Undefined);
    }
    let ctor_name = match obj_val {
        Value::Number(_) => "Number",
        Value::Boolean(_) => "Boolean",
        Value::Symbol(_) => "Symbol",
        Value::BigInt(_) => "BigInt",
        _ => return Ok(Value::Undefined),
    };
    let ctor_val = match env.borrow().get(ctor_name) {
        Some(v) => v,
        None => return Ok(Value::Undefined),
    };
    let proto = match &ctor_val {
        Value::Object(o) => o.borrow().get("prototype"),
        Value::NativeFunction(nf) => nf
            .prototype
            .borrow()
            .as_ref()
            .map(|p| Value::Object(Rc::clone(p))),
        Value::NativeConstructor(nc) => Some(Value::Object(Rc::clone(&nc.prototype))),
        _ => None,
    };
    let proto_rc = match proto {
        Some(Value::Object(o)) => o,
        _ => return Ok(Value::Undefined),
    };
    let mut boxed = Object::new(ObjectKind::Ordinary);
    boxed.prototype = Some(Rc::clone(&proto_rc));
    match obj_val {
        Value::Number(n) => {
            boxed.exotic_kind = Some(crate::value::kind::ExoticKind::Number);
            boxed.set("_value", Value::Number(*n));
        }
        Value::Boolean(b) => {
            boxed.exotic_kind = Some(crate::value::kind::ExoticKind::Boolean);
            boxed.set("_value", Value::Boolean(*b));
        }
        Value::Symbol(_) => {}
        _ => {}
    }
    let boxed_rc = Rc::new(RefCell::new(boxed));
    crate::eval::member::eval_object_member(&boxed_rc, prop_name, Some(env))
}

/// Get the `this` value for a getter/setter call.
pub fn getter_this_value(obj: &Rc<RefCell<Object>>) -> Value {
    let obj_borrow = obj.borrow();
    if let Some(exotic) = &obj_borrow.exotic_kind {
        match exotic {
            crate::value::kind::ExoticKind::Number => {
                if let Some(Value::Number(n)) = obj_borrow.properties.get("_value") {
                    return Value::Number(*n);
                }
            }
            crate::value::kind::ExoticKind::Boolean => {
                if let Some(Value::Boolean(b)) = obj_borrow.properties.get("_value") {
                    return Value::Boolean(*b);
                }
            }
            _ => {}
        }
    }
    Value::Object(Rc::clone(obj))
}

/// Check if any prototype in the chain has a non-writable property with this name.
pub fn has_readonly_prototype_property(object: &Rc<RefCell<Object>>, property: &str) -> bool {
    let mut prototype = object.borrow().prototype.as_ref().map(Rc::clone);
    while let Some(current) = prototype {
        let borrowed = current.borrow();
        if let Some(descriptor) = borrowed.get_descriptor(property) {
            return !descriptor.writable;
        }
        prototype = borrowed.prototype.as_ref().map(Rc::clone);
    }
    false
}

/// Check if a constructor has a read-only built-in property.
pub fn is_readonly_constructor_property(constructor: &str, property: &str) -> bool {
    if matches!(property, "length" | "name") {
        return true;
    }
    constructor == "Number"
        && matches!(
            property,
            "MAX_VALUE"
                | "MIN_VALUE"
                | "NaN"
                | "NEGATIVE_INFINITY"
                | "POSITIVE_INFINITY"
                | "MAX_SAFE_INTEGER"
                | "MIN_SAFE_INTEGER"
                | "EPSILON"
        )
}

#[cfg(test)]
mod tests {
    use crate::ast::PropertyKey;

    #[test]
    fn test_name_matches_prop_ident() {
        let key = PropertyKey::Ident("foo".to_string());
        assert!(super::name_matches_prop(&key, "foo"));
        assert!(!super::name_matches_prop(&key, "bar"));
    }

    #[test]
    fn test_name_matches_prop_string() {
        let key = PropertyKey::String("hello".to_string());
        assert!(super::name_matches_prop(&key, "hello"));
        assert!(!super::name_matches_prop(&key, "world"));
    }

    #[test]
    fn test_name_matches_prop_number() {
        let key = PropertyKey::Number(42.0);
        assert!(super::name_matches_prop(&key, "42"));
        assert!(!super::name_matches_prop(&key, "43"));
    }

    #[test]
    fn test_name_matches_prop_computed() {
        let key = PropertyKey::Computed(Box::new(crate::ast::Expression::Identifier(
            "computed".to_string(),
        )));
        assert!(!super::name_matches_prop(&key, "computed"));
    }

    #[test]
    fn test_is_readonly_constructor_property_length() {
        assert!(super::is_readonly_constructor_property("Foo", "length"));
        assert!(super::is_readonly_constructor_property("Bar", "name"));
    }

    #[test]
    fn test_is_readonly_constructor_property_number_static() {
        assert!(super::is_readonly_constructor_property(
            "Number",
            "MAX_VALUE"
        ));
        assert!(super::is_readonly_constructor_property(
            "Number",
            "MIN_VALUE"
        ));
        assert!(super::is_readonly_constructor_property("Number", "NaN"));
        assert!(super::is_readonly_constructor_property(
            "Number",
            "POSITIVE_INFINITY"
        ));
        assert!(super::is_readonly_constructor_property(
            "Number",
            "MAX_SAFE_INTEGER"
        ));
    }

    #[test]
    fn test_is_readonly_constructor_property_not_number() {
        assert!(!super::is_readonly_constructor_property(
            "String",
            "MAX_VALUE"
        ));
        assert!(!super::is_readonly_constructor_property(
            "Array",
            "MAX_VALUE"
        ));
    }

    #[test]
    fn test_is_readonly_constructor_property_other_props() {
        assert!(!super::is_readonly_constructor_property(
            "Number",
            "prototype"
        ));
        assert!(!super::is_readonly_constructor_property("Number", "foo"));
    }
}
