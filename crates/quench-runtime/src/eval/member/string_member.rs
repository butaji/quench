//! String member access evaluation

use crate::env::Environment;
use crate::value::{JsError, Value};
use std::cell::RefCell;
use std::rc::Rc;

/// Evaluate member access on a string
pub fn eval_string_member(
    s: &str,
    prop_name: &str,
    env: &Rc<RefCell<Environment>>,
) -> Result<Value, JsError> {
    Ok(crate::eval::string_methods::resolve_string_member(
        s, prop_name, env,
    ))
}
