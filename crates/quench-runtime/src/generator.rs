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
    let Some(Value::Generator(generator)) = receiver else {
        return Err(crate::value::error::throw_type_error(
            "Generator.next called on incompatible receiver",
        ));
    };
    let completion = resume(
        generator,
        arguments.first().cloned().unwrap_or(Value::Undefined),
    );
    if generator.function.is_async {
        return Ok(crate::promise::from_async_completion(completion));
    }
    completion
}

fn resume(generator: &GeneratorData, input: Value) -> Result<Value, VmError> {
    if *generator.done.borrow() {
        return Ok(iterator_result(Value::Undefined, true));
    }
    initialize_state(generator);
    let mut state = generator.state.borrow_mut();
    let state = state.as_mut().ok_or(VmError::MissingReturn)?;
    install_resume_input(generator, state, input);
    let _home = crate::super_scope::Guard::install(&generator.function, &generator.receiver);
    let _with_scope = crate::with_scope::FunctionGuard::isolate();
    let (completion, pc) = crate::vm::execute_generator_step(
        &generator.function.body,
        &mut state.registers,
        state.environment.clone(),
        state.pc,
    )?;
    state.pc = pc;
    complete_step(generator, completion)
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
    if !result.done {
        return Ok(Some(crate::completion::Completion::Yield(result.value)));
    }
    if returning {
        return Ok(Some(crate::completion::Completion::Return(result.value)));
    }
    crate::execute::write_value(registers, *dst, result.value);
    Ok(None)
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
    completion: crate::completion::Completion,
) -> Result<Value, VmError> {
    use crate::completion::Completion;
    match completion {
        Completion::Yield(value) => Ok(iterator_result(value, false)),
        Completion::Return(value) => finish(generator, value),
        Completion::Normal => finish(generator, Value::Undefined),
        Completion::Throw(value) => Err(VmError::Thrown(value)),
        _ => Err(VmError::MissingReturn),
    }
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
