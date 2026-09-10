use crate::machine::Frame as OpFrame;

fn yielded_result(
    generator: &GeneratorData,
    state: &GeneratorState,
    value: Value,
) -> Result<Value, VmError> {
    let op = generator
        .function
        .code
        .code()
        .and_then(|code| code.cold_at(machine_pc(generator)))
        .or_else(|| suspended_try(generator, state).map(|(_, yield_op, _)| yield_op));
    // A delegated yield nested inside a `for..of` is resumed through the
    // iterator frame. In that layout the machine PC points at the enclosing
    // loop rather than the original `YieldStar` opcode, while the Delegate
    // frame still owns the already-normalized iterator result. Return that
    // value directly; wrapping the completion's placeholder would turn a
    // real chunk into `undefined`. Direct `yield*` keeps the opcode path
    // below, because its destination register has the canonical result.
    if !matches!(op, Some(Op::YieldStar { .. })) {
        let delegated = generator.machine.borrow().frames.frames.last().and_then(|frame| {
            let OpFrame::Delegate { destination, .. } = frame else {
                return None;
            };
            // Any enclosing control-flow frame means the completion came
            // through a nested range (conditional, loop, try, ...). A lone
            // Delegate is the ordinary direct `yield*` path and is handled
            // by the opcode-specific logic below.
            let nested = generator.machine.borrow().frames.frames.len() > 1;
            nested.then(|| *destination)
        });
        if let Some(destination) = delegated {
            return crate::execute::read_register(&registers(generator), destination);
        }
    }
    if generator.function.is_async {
        let value = match op {
            Some(Op::YieldStar { dst, .. }) => {
                let result = crate::execute::read_register(&registers(generator), *dst)?;
                if let Value::Promise(_) = result {
                    return async_yield_star_result(generator, result);
                }
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


fn async_yield_star_result(generator: &GeneratorData, value: Value) -> Result<Value, VmError> {
    let Value::Promise(promise) = value else {
        return Ok(iterator_result(value, false));
    };
    let state = promise.state.borrow().clone();
    match state {
        crate::value::PromiseState::Fulfilled(result) => {
            let value = crate::execute::get_property_result(&result, "value")?;
            Ok(iterator_result(value, false))
        }
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
