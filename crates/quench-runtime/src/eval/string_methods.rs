//! Primitive string member access.
//!
//! Resolves properties and methods from the prototype object exposed by the
//! global `String` constructor (the same object that carries the RegExp-based
//! methods installed by `builtins::regex::string_methods`), so that primitive
//! string member access and `String.prototype` stay in sync.

use crate::env::Environment;
use crate::value::{JsError, Value};
use std::cell::RefCell;
use std::rc::Rc;

/// Resolve a property on a primitive string value: `length`, numeric indices,
/// or a method inherited from the global `String` constructor's prototype.
pub fn resolve_string_member(s: &str, prop_name: &str, env: &Rc<RefCell<Environment>>) -> Value {
    if prop_name == "length" {
        return Value::Number(s.len() as f64);
    }
    if let Some(Value::Object(ctor)) = env.borrow().get("String") {
        let proto = ctor.borrow().get("prototype");
        if let Some(Value::Object(proto)) = proto {
            if let Some(val) = proto.borrow().get(prop_name) {
                return val;
            }
        }
    }
    // Numeric property access (e.g., str[0], str[15])
    if let Ok(idx) = prop_name.parse::<usize>() {
        let ch = s
            .chars()
            .nth(idx)
            .map(|c| c.to_string())
            .unwrap_or_default();
        return Value::String(ch);
    }
    Value::Undefined
}

/// Resolve a string primitive property to its value (method or property).
pub fn get_string_method(
    s: &str,
    prop_name: &str,
    env: &Rc<RefCell<Environment>>,
) -> Result<Value, JsError> {
    Ok(resolve_string_member(s, prop_name, env))
}

#[cfg(test)]
mod tests {
    use crate::value::Value;
    use crate::Context;

    #[test]
    fn test_primitive_methods_dispatch_through_prototype() {
        let mut ctx = Context::new().unwrap();
        assert_eq!(ctx.eval("'hello'.length").unwrap(), Value::Number(5.0));
        assert_eq!(
            ctx.eval("'hello'.toUpperCase()").unwrap(),
            Value::String("HELLO".to_string())
        );
        assert_eq!(
            ctx.eval("'hello'.charAt(1)").unwrap(),
            Value::String("e".to_string())
        );
        assert_eq!(
            ctx.eval("'hello'[1]").unwrap(),
            Value::String("e".to_string())
        );
        assert_eq!(
            ctx.eval("'a,b,c'.split(',').length").unwrap(),
            Value::Number(3.0)
        );
        assert_eq!(
            ctx.eval("'a,b,c'.split(',')[1]").unwrap(),
            Value::String("b".to_string())
        );
        assert_eq!(
            ctx.eval("'hello'.slice(1, 3)").unwrap(),
            Value::String("el".to_string())
        );
        assert_eq!(
            ctx.eval("'hello'.includes('ell')").unwrap(),
            Value::Boolean(true)
        );
    }

    #[test]
    fn test_prototype_mutation_visible_on_primitives() {
        let mut ctx = Context::new().unwrap();
        // Methods resolve from the shared String.prototype, so additions are visible
        ctx.eval("String.prototype.shout = function() { return 'hi'; };")
            .unwrap();
        assert_eq!(
            ctx.eval("typeof 'x'.shout").unwrap(),
            Value::String("function".to_string())
        );
    }
}
