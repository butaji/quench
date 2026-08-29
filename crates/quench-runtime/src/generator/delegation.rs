pub(crate) fn execute_yield_star(
    registers: &mut crate::register_file::RegisterFile,
    op: &Op,
    resume: crate::completion::Completion,
) -> Result<Option<crate::completion::Completion>, VmError> {
    let Op::YieldStar { dst, source, iterator } = op else {
        return Err(VmError::MissingReturn);
    };
    let record = delegation_record(registers, *source, *iterator)?;
    let input = crate::execute::read_register(registers, *dst)?;
    let returning = matches!(resume, crate::completion::Completion::Return(_));
    let result = delegate(&record, input, resume)?;
    let crate::collections::iterator::DelegationResult::Done(value) = result else {
        return ongoing_delegation(registers, *dst, result).map(Some);
    };
    if returning {
        return Ok(Some(crate::completion::Completion::Return(value)));
    }
    crate::execute::write_value(registers, *dst, value);
    Ok(None)
}

fn ongoing_delegation(
    registers: &mut crate::register_file::RegisterFile,
    dst: u16,
    result: crate::collections::iterator::DelegationResult,
) -> Result<crate::completion::Completion, VmError> {
    let crate::collections::iterator::DelegationResult::Ongoing { value, passthrough } = result else {
        return Err(VmError::MissingReturn);
    };
    let output = if passthrough { value } else { iterator_result(value, false) };
    crate::execute::write_value(registers, dst, output);
    Ok(crate::completion::Completion::Yield(Value::Undefined))
}

fn delegation_record(registers: &mut crate::register_file::RegisterFile, source: u16, slot: u16) -> Result<Value, VmError> {
    let current = crate::execute::read_register(registers, slot)?;
    if !matches!(current, Value::Undefined) {
        return Ok(current);
    }
    let source = crate::execute::read_register(registers, source)?;
    let iterator = crate::collections::iterator::delegate_start(source)?;
    crate::execute::write_value(registers, slot, iterator.clone());
    Ok(iterator)
}

fn delegate(
    iterator: &Value,
    input: Value,
    resume: crate::completion::Completion,
) -> Result<crate::collections::iterator::DelegationResult, VmError> {
    use crate::completion::Completion;
    match resume {
        Completion::Return(value) => crate::collections::iterator::delegate_return(iterator, value),
        Completion::Throw(value) => crate::collections::iterator::delegate_throw(iterator, value),
        _ => crate::collections::iterator::delegate_next(iterator, input),
    }
}

fn install_delegate_frame_input(generator: &GeneratorData, input: &Value) -> bool {
    let frame = generator.machine.borrow().frames.frames.last().cloned();
    let Some(crate::machine::Frame::Delegate { destination, .. }) = frame else {
        return false;
    };
    crate::execute::write_value(&mut registers_mut(generator), destination, input.clone());
    true
}

fn resume_delegate_frame(
    generator: &GeneratorData,
    resume: &crate::completion::Completion,
) -> Result<Option<crate::completion::Completion>, VmError> {
    let frame = generator.machine.borrow().frames.frames.last().cloned();
    let Some(crate::machine::Frame::Delegate {
        iterator,
        destination,
        ..
    }) = frame
    else {
        return Ok(None);
    };
    let input = crate::execute::read_register(&registers(generator), destination)?;
    let result = delegate(&iterator, input, resume.clone())?;
    match result {
        crate::collections::iterator::DelegationResult::Ongoing { value, passthrough } => {
            let output = if passthrough { value } else { iterator_result(value, false) };
            crate::execute::write_value(&mut registers_mut(generator), destination, output);
            Ok(Some(crate::completion::Completion::Yield(Value::Undefined)))
        }
        crate::collections::iterator::DelegationResult::Done(value) => {
            crate::execute::write_value(&mut registers_mut(generator), destination, value);
            generator.machine.borrow_mut().pop_frame();
            Ok(Some(crate::completion::Completion::Normal))
        }
    }
}
