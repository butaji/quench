use std::{cell::RefCell, rc::Rc};

use crate::{
    execute::VmError,
    facts::ProgramDb,
    ops::Op,
    value::{GeneratorData, GeneratorState, Value},
};
use std::collections::HashMap;

pub(crate) fn create(
    function: &Rc<crate::value::FunctionValue>,
    receiver: &Value,
    arguments: &[Value],
) -> Result<Value, VmError> {
    let state = initialize_parameters(function, receiver, arguments)?;
    let deferred_arguments = if state.is_none() {
        arguments.to_vec()
    } else {
        Vec::new()
    };
    Ok(Value::Generator(Rc::new(GeneratorData {
        function: Rc::clone(function),
        receiver: receiver.clone(),
        arguments: deferred_arguments,
        done: RefCell::new(false),
        state: RefCell::new(state),
    })))
}

fn initialize_parameters(
    function: &Rc<crate::value::FunctionValue>,
    receiver: &Value,
    arguments: &[Value],
) -> Result<Option<GeneratorState>, VmError> {
    let Some(marker) = function
        .body
        .iter()
        .position(|op| matches!(op, Op::ParameterEnd))
    else {
        return Ok(None);
    };
    let (mut registers, environment) =
        crate::functions::build_registers(function, receiver, arguments);
    let _home = crate::super_scope::Guard::install(function, receiver);
    let _with_scope = crate::with_scope::FunctionGuard::isolate();
    let (completion, _) = crate::vm::execute_generator_step(
        &function.body[..marker],
        &mut registers,
        Rc::clone(&environment),
        0,
        crate::completion::Completion::Normal,
    )?;
    require_normal_parameter_completion(completion)?;
    Ok(Some(GeneratorState {
        registers,
        environment,
        pc: marker.saturating_add(1),
    }))
}

fn require_normal_parameter_completion(
    completion: crate::completion::Completion,
) -> Result<(), VmError> {
    match completion {
        crate::completion::Completion::Normal => Ok(()),
        crate::completion::Completion::Throw(value) => Err(VmError::Thrown(value)),
        _ => Err(VmError::MissingReturn),
    }
}

pub(crate) fn next(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    let generator = generator_receiver(receiver, "next")?;
    let completion = resume(generator, Resume::Next(first_argument(arguments)));
    if generator.function.is_async {
        return Ok(crate::promise::from_async_completion(completion));
    }
    completion
}

pub(crate) fn return_(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    let generator = generator_receiver(receiver, "return")?;
    resume(generator, Resume::Return(first_argument(arguments)))
}

pub(crate) fn throw(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    let generator = generator_receiver(receiver, "throw")?;
    resume(generator, Resume::Throw(first_argument(arguments)))
}

enum Resume {
    Next(Value),
    Return(Value),
    Throw(Value),
}

fn resume(generator: &GeneratorData, resume: Resume) -> Result<Value, VmError> {
    if *generator.done.borrow() {
        return completed_resume(resume);
    }
    initialize_state(generator);
    let mut state = generator.state.borrow_mut();
    let state = state.as_mut().ok_or(VmError::MissingReturn)?;
    if !is_suspended(generator, state) {
        match resume {
            Resume::Return(value) => return finish(generator, value),
            Resume::Throw(value) => return throw_and_finish(generator, value),
            Resume::Next(_) => {}
        }
    }
    let completion = resume.completion();
    if let Resume::Next(input) = resume {
        install_resume_input(generator, state, input);
    }
    if let Some(completion) = resume_suspended_try(generator, state, completion.clone())? {
        return complete_step(generator, state, completion);
    }
    let _home = crate::super_scope::Guard::install(&generator.function, &generator.receiver);
    let _with_scope = crate::with_scope::FunctionGuard::isolate();
    let (completion, pc) = crate::vm::execute_generator_step(
        &generator.function.body,
        &mut state.registers,
        state.environment.clone(),
        state.pc,
        completion,
    )?;
    state.pc = pc;
    complete_step(generator, state, completion)
}

impl Resume {
    fn completion(&self) -> crate::completion::Completion {
        match self {
            Self::Return(value) => crate::completion::Completion::Return(value.clone()),
            Self::Throw(value) => crate::completion::Completion::Throw(value.clone()),
            Self::Next(_) => crate::completion::Completion::Normal,
        }
    }
}

fn generator_receiver<'a>(
    receiver: Option<&'a Value>,
    method: &str,
) -> Result<&'a GeneratorData, VmError> {
    let Some(Value::Generator(generator)) = receiver else {
        return Err(crate::value::error::throw_type_error(&format!(
            "Generator.{method} called on incompatible receiver"
        )));
    };
    Ok(generator)
}

fn first_argument(arguments: &[Value]) -> Value {
    arguments.first().cloned().unwrap_or(Value::Undefined)
}

fn completed_resume(resume: Resume) -> Result<Value, VmError> {
    match resume {
        Resume::Next(_) => Ok(iterator_result(Value::Undefined, true)),
        Resume::Return(value) => Ok(iterator_result(value, true)),
        Resume::Throw(value) => Err(VmError::Thrown(value)),
    }
}

fn is_suspended(generator: &GeneratorData, state: &GeneratorState) -> bool {
    if matches!(
        generator.function.body.get(state.pc.wrapping_sub(1)),
        Some(Op::Yield { .. })
    ) {
        return true;
    }
    let Some(Op::YieldStar { iterator, .. }) = generator.function.body.get(state.pc) else {
        return suspended_try(generator, state).is_some();
    };
    crate::execute::read_register(&state.registers, *iterator)
        .is_ok_and(|value| !matches!(value, Value::Undefined))
}

fn suspended_try<'a>(
    generator: &'a GeneratorData,
    state: &GeneratorState,
) -> Option<(&'a Op, &'a Op, &'a [Op])> {
    let index = state.pc.checked_sub(1)?;
    let op @ Op::Try { body, .. } = generator.function.body.get(index)? else {
        return None;
    };
    let (yield_index, yield_op) = body.iter().enumerate().find(|(_, candidate)| {
        let Op::YieldStar { iterator, .. } = candidate else {
            return false;
        };
        crate::execute::read_register(&state.registers, *iterator)
            .is_ok_and(|value| !matches!(value, Value::Undefined))
    })?;
    Some((op, yield_op, &body[yield_index + 1..]))
}

fn resume_suspended_try(
    generator: &GeneratorData,
    state: &mut GeneratorState,
    resume: crate::completion::Completion,
) -> Result<Option<crate::completion::Completion>, VmError> {
    let Some((try_op, yield_op, suffix)) = suspended_try(generator, state) else {
        return Ok(None);
    };
    let completion = match execute_yield_star(&mut state.registers, yield_op, resume) {
        Ok(Some(crate::completion::Completion::Yield(value))) => {
            return Ok(Some(crate::completion::Completion::Yield(value)));
        }
        Ok(Some(completion)) => completion,
        Ok(None) => crate::execute::execute_completion_in_place(suffix, &mut state.registers)?,
        Err(VmError::Thrown(value)) => crate::completion::Completion::Throw(value),
        Err(error) => return Err(error),
    };
    complete_suspended_try(try_op, &mut state.registers, completion).map(Some)
}

fn complete_suspended_try(
    op: &Op,
    registers: &mut Vec<Value>,
    completion: crate::completion::Completion,
) -> Result<crate::completion::Completion, VmError> {
    let Op::Try {
        handler,
        finalizer,
        catch_slot,
        ..
    } = op
    else {
        return Err(VmError::MissingReturn);
    };
    let completion = handle_suspended_throw(registers, handler, *catch_slot, completion)?;
    let Some(finalizer) = finalizer else {
        return Ok(completion);
    };
    match crate::execute::execute_completion_in_place(finalizer, registers)? {
        crate::completion::Completion::Normal => Ok(completion),
        abrupt => Ok(abrupt),
    }
}

fn handle_suspended_throw(
    registers: &mut Vec<Value>,
    handler: &Option<Vec<Op>>,
    catch_slot: Option<u16>,
    completion: crate::completion::Completion,
) -> Result<crate::completion::Completion, VmError> {
    let crate::completion::Completion::Throw(value) = completion else {
        return Ok(completion);
    };
    let Some(handler) = handler else {
        return Ok(crate::completion::Completion::Throw(value));
    };
    if let Some(slot) = catch_slot {
        crate::locals::write(slot, value);
    }
    crate::execute::execute_completion_in_place(handler, registers)
}

fn throw_and_finish(generator: &GeneratorData, value: Value) -> Result<Value, VmError> {
    *generator.done.borrow_mut() = true;
    Err(VmError::Thrown(value))
}

fn install_resume_input(generator: &GeneratorData, state: &mut GeneratorState, input: Value) {
    if let Some(Op::YieldStar { dst, .. }) = generator.function.body.get(state.pc) {
        crate::execute::write_value(&mut state.registers, *dst, input);
        return;
    }
    let Some(index) = state.pc.checked_sub(1) else {
        return;
    };
    let Some(Op::Yield { src }) = generator.function.body.get(index) else {
        return;
    };
    crate::execute::write_value(&mut state.registers, *src, input);
}

pub(crate) fn execute_yield_star(
    registers: &mut Vec<Value>,
    op: &Op,
    resume: crate::completion::Completion,
) -> Result<Option<crate::completion::Completion>, VmError> {
    let Op::YieldStar {
        dst,
        source,
        iterator,
    } = op
    else {
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
    registers: &mut Vec<Value>,
    dst: u16,
    result: crate::collections::iterator::DelegationResult,
) -> Result<crate::completion::Completion, VmError> {
    let crate::collections::iterator::DelegationResult::Ongoing { value, passthrough } = result
    else {
        return Err(VmError::MissingReturn);
    };
    let output = if passthrough {
        value
    } else {
        iterator_result(value, false)
    };
    crate::execute::write_value(registers, dst, output);
    Ok(crate::completion::Completion::Yield(Value::Undefined))
}

fn delegation_record(registers: &mut Vec<Value>, source: u16, slot: u16) -> Result<Value, VmError> {
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

fn initialize_state(generator: &GeneratorData) {
    let mut state = generator.state.borrow_mut();
    if state.is_some() {
        return;
    }
    let (registers, environment) = crate::functions::build_registers(
        &generator.function,
        &generator.receiver,
        &generator.arguments,
    );
    *state = Some(GeneratorState {
        registers,
        environment,
        pc: 0,
    });
}

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
        Completion::Throw(value) => Err(VmError::Thrown(value)),
        _ => Err(VmError::MissingReturn),
    }
}

fn yielded_result(
    generator: &GeneratorData,
    state: &GeneratorState,
    value: Value,
) -> Result<Value, VmError> {
    let op = generator
        .function
        .body
        .get(state.pc)
        .or_else(|| suspended_try(generator, state).map(|(_, yield_op, _)| yield_op));
    let Some(Op::YieldStar { dst, .. }) = op else {
        return Ok(iterator_result(value, false));
    };
    crate::execute::read_register(&state.registers, *dst)
}

fn finish(generator: &GeneratorData, value: Value) -> Result<Value, VmError> {
    *generator.done.borrow_mut() = true;
    Ok(iterator_result(value, true))
}

fn iterator_result(value: Value, done: bool) -> Value {
    Value::Object(Rc::new(vec![
        ("value".to_string(), value),
        ("done".to_string(), Value::Boolean(done)),
    ]))
}

pub(crate) fn reduce_yield(
    yield_expression: &oxc::ast::ast::YieldExpression<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    if yield_expression.delegate {
        return reduce_yield_star(yield_expression, ops, facts, next, locals);
    }
    let src = match yield_expression.argument.as_ref() {
        Some(argument) => crate::reduce::reduce_expression(argument, ops, facts, next, locals)?,
        None => crate::reduce_support::emit_undefined(ops, next),
    };
    ops.push(Op::Yield { src });
    Some(src)
}

fn reduce_yield_star(
    expression: &oxc::ast::ast::YieldExpression<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    let source =
        crate::reduce::reduce_expression(expression.argument.as_ref()?, ops, facts, next, locals)?;
    let dst = *next;
    let iterator = next.saturating_add(1);
    *next = next.saturating_add(2);
    ops.push(Op::YieldStar {
        dst,
        source,
        iterator,
    });
    Some(dst)
}
