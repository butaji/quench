use crate::ast::{Expression, PropertyKey};
use crate::env::Environment;
use crate::value::{to_js_string, JsError, Value};
use std::cell::RefCell;
use std::rc::Rc;

/// Extract a property name from a PropertyKey, evaluating computed keys.
pub fn extract_property_name(
    key: PropertyKey,
    computed: bool,
    env: &Rc<RefCell<Environment>>,
    in_arrow_function: bool,
) -> Result<String, JsError> {
    match key {
        PropertyKey::Ident(name) => {
            if computed {
                let val = crate::eval::expression::eval_expression(
                    &Expression::Identifier(name),
                    env,
                    in_arrow_function,
                )?;
                match &val {
                    Value::Symbol(s) => Ok(s.property_key()),
                    _ => Ok(to_js_string(&val)),
                }
            } else {
                Ok(name)
            }
        }
        PropertyKey::String(s) => Ok(s),
        PropertyKey::Number(n) => Ok(n.to_string()),
        PropertyKey::Computed(expr) => {
            let val = crate::eval::expression::eval_expression(&expr, env, in_arrow_function)?;
            match &val {
                Value::Symbol(s) => Ok(s.property_key()),
                _ => Ok(to_js_string(&val)),
            }
        }
    }
}
