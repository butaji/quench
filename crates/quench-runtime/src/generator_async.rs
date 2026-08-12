fn yielded_result(
    generator: &GeneratorData,
    state: &GeneratorState,
    value: Value,
) -> Result<Value, VmError> {
    let op = generator
        .function
        .ops()
        .get(state.pc)
        .or_else(|| suspended_try(generator, state).map(|(_, yield_op, _)| yield_op));
    if generator.function.is_async {
        let value = match op {
            Some(Op::YieldStar { dst, .. }) => {
                let result = crate::execute::read_register(&registers(generator), *dst)?;
                crate::execute::get_property_result(&result, "value")?
            }
            _ => value,
        };
        return async_yield_result(generator, value);
    }
    let Some(Op::YieldStar { dst, .. }) = op else {
        return Ok(iterator_result(value, false));
    };
    crate::execute::read_register(&registers(generator), *dst)
}

fn async_yield_result(generator: &GeneratorData, value: Value) -> Result<Value, VmError> {
    let Value::Promise(promise) = value else {
        return Ok(iterator_result(value, false));
    };
    let state = promise.state.borrow().clone();
    match state {
        crate::value::PromiseState::Fulfilled(value) => Ok(iterator_result(value, false)),
        crate::value::PromiseState::Rejected(reason) => {
            *generator.done.borrow_mut() = true;
            Err(VmError::Thrown(reason))
        }
        crate::value::PromiseState::Pending => {
            *generator.pending_yield.borrow_mut() = true;
            Err(VmError::Suspended(promise))
        }
    }
}
