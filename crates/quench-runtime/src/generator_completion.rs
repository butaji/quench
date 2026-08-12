fn complete_step(
    generator: &GeneratorData,
    state: &GeneratorState,
    completion: crate::completion::Completion,
) -> Result<Value, VmError> {
    use crate::completion::Completion;
    match completion {
        Completion::Yield(value) => yielded_result(generator, state, value),
        Completion::Return(value) => finish(generator, value),
        Completion::Normal => finish(generator, Value::Undefined),
        Completion::Throw(value) => throw_and_finish(generator, value),
        _ => Err(VmError::MissingReturn),
    }
}

fn finish(generator: &GeneratorData, value: Value) -> Result<Value, VmError> {
    *generator.done.borrow_mut() = true;
    Ok(iterator_result(value, true))
}
