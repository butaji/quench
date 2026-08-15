use std::rc::Rc;

use crate::{execute::VmError, ops::Builtin, promise::promise_resolve, value::Value};

pub(super) fn resolve(constructor: &Value, arguments: &[Value]) -> Result<Value, VmError> {
    let result = if matches!(constructor, Value::Builtin(Builtin::Promise)) {
        promise_resolve(arguments)
    } else {
        let target = Rc::new(crate::value::PromiseData::default());
        let executor = super::bound_settler(Builtin::PromiseResolve, &target);
        crate::construct::construct_value(constructor, &[executor])?
    };
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
    let target = Rc::new(crate::value::PromiseData::default());
    let executor = super::bound_settler(Builtin::PromiseResolve, &target);
    let result = crate::construct::construct_value(constructor, &[executor])?;
    let Value::Promise(promise) = &result else {
        return Ok(result);
    };
    super::reject_promise(
        promise,
        arguments.first().cloned().unwrap_or(Value::Undefined),
    );
    let prototype = crate::execute::get_property_result(constructor, "prototype")?;
    if crate::value::is_object(&prototype) {
        promise.set_prototype(prototype);
    }
    Ok(result)
}
