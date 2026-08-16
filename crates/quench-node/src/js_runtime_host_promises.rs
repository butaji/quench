impl QuenchNodeHost {
    fn common_wrapper(&self, arguments: &[Value], succeeds: bool) -> Result<Value, VmError> {
        let callback = arguments.first().cloned().unwrap_or(Value::Undefined);
        let id = self.next_common_wrapper.get();
        self.next_common_wrapper.set(id.saturating_add(1));
        let wrapper = capability_function(HostCapabilityKind::Custom(id));
        let wrapper = quench_runtime::execute::set_property(wrapper, "calls", Value::Number(0.0));
        self.common_wrappers
            .borrow_mut()
            .insert(id, (callback, succeeds, 0, wrapper.clone()));
        Ok(wrapper)
    }

    fn common_wrapper_call(&self, id: u16, arguments: &[Value]) -> Result<Value, VmError> {
        let (callback, succeeds, calls, wrapper) = self
            .common_wrappers
            .borrow()
            .get(&id)
            .cloned()
            .ok_or(VmError::NotCallable)?;
        let calls = calls + 1;
        if let Some(entry) = self.common_wrappers.borrow_mut().get_mut(&id) {
            entry.2 = calls;
        }
        let _ =
            quench_runtime::execute::set_property(wrapper, "calls", Value::Number(calls as f64));
        if !succeeds {
            return Err(VmError::EvalError("unexpected callback call".into()));
        }
        if matches!(callback, Value::Undefined) {
            return Ok(Value::Undefined);
        }
        quench_runtime::execute::call(
            &callback,
            &Value::Undefined,
            if arguments.len() > 1 {
                &arguments[1..]
            } else {
                &[]
            },
        )
    }

    fn util_promisify(&self, arguments: &[Value]) -> Result<Value, VmError> {
        let callback = arguments.first().cloned().ok_or(VmError::NotCallable)?;
        let id = self.next_promisified.get();
        self.next_promisified.set(id.saturating_add(1));
        self.promisified.borrow_mut().insert(id, callback);
        let wrapper = capability_function(HostCapabilityKind::Custom(id));
        let updated =
            quench_runtime::execute::set_prototype_of(&wrapper, arguments.first().unwrap())?;
        quench_runtime::execute::replace_value(&wrapper, &updated);
        Ok(updated)
    }

    fn util_deprecate(&self, arguments: &[Value]) -> Result<Value, VmError> {
        let callback = arguments.first().cloned().ok_or(VmError::NotCallable)?;
        if let Some(code) = arguments.get(2) {
            if !matches!(code, Value::String(_)) {
                return Err(VmError::Thrown(quench_runtime::host_api::object(vec![
                    ("code".into(), Value::String("ERR_INVALID_ARG_TYPE".into())),
                    ("name".into(), Value::String("TypeError".into())),
                    (
                        "message".into(),
                        Value::String("The \"code\" argument must be of type string.".into()),
                    ),
                ])));
            }
        }
        let id = self.next_deprecated.get();
        self.next_deprecated.set(id.saturating_add(1));
        self.deprecated.borrow_mut().insert(id, callback);
        let wrapper = capability_function(HostCapabilityKind::Custom(id));
        if let Ok(length) =
            quench_runtime::execute::get_property_result(arguments.first().unwrap(), "length")
        {
            quench_runtime::execute::set_callable_property(&wrapper, "length", length)?;
        }
        quench_runtime::execute::set_prototype_of(&wrapper, arguments.first().unwrap())?;
        Ok(wrapper)
    }

    fn call_deprecated(&self, id: u16, arguments: &[Value]) -> Result<Value, VmError> {
        let callback = self
            .deprecated
            .borrow()
            .get(&id)
            .cloned()
            .ok_or(VmError::NotCallable)?;
        quench_runtime::execute::call(&callback, &Value::Undefined, arguments)
    }

    fn call_promisified(&self, id: u16, arguments: &[Value]) -> Result<Value, VmError> {
        let callback = self
            .promisified
            .borrow()
            .get(&id)
            .cloned()
            .ok_or(VmError::NotCallable)?;
        let promise_id = self.next_promise.get();
        self.next_promise.set(promise_id.saturating_add(1));
        let promise = Rc::new(quench_runtime::value::PromiseData::new(
            quench_runtime::value::PromiseState::Pending,
        ));
        self.pending_promises
            .borrow_mut()
            .insert(promise_id, promise.clone());
        let mut call_arguments = arguments.to_vec();
        call_arguments.push(capability_function(HostCapabilityKind::Custom(promise_id)));
        quench_runtime::execute::call(&callback, &Value::Undefined, &call_arguments)?;
        Ok(Value::Promise(promise))
    }

    fn resolve_promisified(&self, id: u16, arguments: &[Value]) -> Result<Value, VmError> {
        let promise = self
            .pending_promises
            .borrow_mut()
            .remove(&id)
            .ok_or(VmError::NotCallable)?;
        let error = arguments.first().cloned().unwrap_or(Value::Null);
        let result = if !matches!(error, Value::Null | Value::Undefined) {
            quench_runtime::value::PromiseState::Rejected(error)
        } else if arguments.len() <= 2 {
            quench_runtime::value::PromiseState::Fulfilled(
                arguments.get(1).cloned().unwrap_or(Value::Undefined),
            )
        } else {
            quench_runtime::value::PromiseState::Fulfilled(quench_runtime::host_api::array(
                arguments[1..].to_vec(),
            ))
        };
        promise.state.replace(result.clone());
        promise.result.replace(match result {
            quench_runtime::value::PromiseState::Fulfilled(value)
            | quench_runtime::value::PromiseState::Rejected(value) => Some(value),
            _ => None,
        });
        Ok(Value::Undefined)
    }
}
