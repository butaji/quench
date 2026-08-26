use crate::{execute::VmError, value::Value};

pub(super) fn resolve(constructor: &Value, arguments: &[Value]) -> Result<Value, VmError> {
    let (result, resolve, _) = crate::promise::new_promise_capability(constructor)?;
    let value = arguments.first().cloned().unwrap_or(Value::Undefined);
    crate::functions::execute_target(&resolve, &Value::Undefined, &[value])?;
    let Value::Promise(promise) = &result else {
        return Ok(result);
    };
    let prototype = crate::execute::get_property_result(constructor, "prototype")?;
    if crate::value::is_object(&prototype) {
        promise.set_prototype(prototype);
    }
    Ok(result)
}

pub(super) fn reject(constructor: &Value, arguments: &[Value]) -> Result<Value, VmError> {
    let (result, _, reject) = crate::promise::new_promise_capability(constructor)?;
    let reason = arguments.first().cloned().unwrap_or(Value::Undefined);
    crate::functions::execute_target(&reject, &Value::Undefined, &[reason])?;
    let Value::Promise(promise) = &result else {
        return Ok(result);
    };
    let prototype = crate::execute::get_property_result(constructor, "prototype")?;
    if crate::value::is_object(&prototype) {
        promise.set_prototype(prototype);
    }
    Ok(result)
}
