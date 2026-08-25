use std::rc::Rc;

use crate::{execute::VmError, ops::Builtin, promise::promise_resolve, value::Value};

pub(super) fn resolve(constructor: &Value, arguments: &[Value]) -> Result<Value, VmError> {
    let result = if matches!(constructor, Value::Builtin(Builtin::Promise)) {
        promise_resolve(arguments)
    } else {
        let capability = Rc::new(crate::value::PromiseData::default());
        *capability.capability_executor.borrow_mut() =
            Some(crate::value::PromiseCapabilityExecutor {
                resolve: std::cell::RefCell::new(None),
                reject: std::cell::RefCell::new(None),
                called: std::cell::Cell::new(false),
            });
        let executor = super::capability_executor_function(&capability);
        let mut result = crate::construct::construct_value(constructor, &[executor])?;
        if !super::capability_callbacks_callable(&capability) {
            return Err(crate::value::error::throw_type_error(
                "Promise capability callbacks must be callable",
            ));
        }
        let resolve = capability
            .capability_executor
            .borrow()
            .as_ref()
            .and_then(|state| state.resolve.borrow().clone())
            .ok_or(VmError::NotCallable)?;
        crate::functions::execute_target(
            &resolve,
            &Value::Undefined,
            &[arguments.first().cloned().unwrap_or(Value::Undefined)],
        )?;
        result = super::attach_promise_data(result, Rc::clone(&capability));
        result
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
    let capability = Rc::new(crate::value::PromiseData::default());
    *capability.capability_executor.borrow_mut() = Some(crate::value::PromiseCapabilityExecutor {
        resolve: std::cell::RefCell::new(None),
        reject: std::cell::RefCell::new(None),
        called: std::cell::Cell::new(false),
    });
    let executor = super::capability_executor_function(&capability);
    let mut result = crate::construct::construct_value(constructor, &[executor])?;
    if !super::capability_callbacks_callable(&capability) {
        return Err(crate::value::error::throw_type_error(
            "Promise capability callbacks must be callable",
        ));
    }
    let reject = capability
        .capability_executor
        .borrow()
        .as_ref()
        .and_then(|state| state.reject.borrow().clone())
        .ok_or(VmError::NotCallable)?;
    crate::functions::execute_target(
        &reject,
        &Value::Undefined,
        &[arguments.first().cloned().unwrap_or(Value::Undefined)],
    )?;
    let prototype = crate::execute::get_property_result(constructor, "prototype")?;
    if crate::value::is_object(&prototype) {
        if let Value::Promise(promise) = &result {
            promise.set_prototype(prototype);
        }
    }
    result = super::attach_promise_data(result, Rc::clone(&capability));
    Ok(result)
}
