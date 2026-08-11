use crate::{execute::VmError, promise::promise_resolve, value::Value};

pub(super) fn resolve(constructor: &Value, arguments: &[Value]) -> Result<Value, VmError> {
    let result = promise_resolve(arguments);
    let Value::Promise(promise) = &result else {
        return Ok(result);
    };
    let prototype = crate::execute::get_property_result(constructor, "prototype")?;
    if crate::value::is_object(&prototype) {
        promise.set_prototype(prototype);
    }
    Ok(result)
}
