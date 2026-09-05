use crate::{
    execute::VmError,
    facts::ProgramDb,
    ops::Op,
    value::{GeneratorData, GeneratorState, Value},
};
use std::collections::{HashMap, VecDeque};
use std::{cell::RefCell, rc::Rc};
type InitialGeneratorState = (
    Option<GeneratorState>,
    crate::register_file::RegisterFile,
    u32,
    Option<Rc<crate::environment::Environment>>,
);
include!("generator_private_scope.rs");
include!("generator_private_frame.rs");
include!("generator_branch.rs");
include!("generator_async.rs");
include!("generator_try.rs");
include!("generator_try_frame.rs");
include!("generator_loop.rs");
include!("generator_iterator_binding.rs");
include!("generator_suspension.rs");
include!("generator_reduce.rs");
include!("generator_machine.rs");
include!("generator_result.rs");
include!("generator_completion.rs");
include!("generator_resume_input.rs");

pub(crate) fn create(
    function: &Rc<crate::value::FunctionValue>,
    receiver: &Value,
    arguments: &[Value],
) -> Result<Value, VmError> {
    let (state, registers, pc, environment) = initialize_parameters(function, receiver, arguments)?;
    let deferred_arguments = if state.is_none() {
        arguments.to_vec()
    } else {
        Vec::new()
    };
    let register_count = registers.len().clamp(4, usize::from(u16::MAX)) as u16;
    let mut machine = crate::machine::Machine::with_function(
        &function.code,
        crate::machine::EnvironmentRef(0),
        register_count,
    );
    machine.registers.values = registers;
    machine.pc = pc;
    if let Some(environment) = environment {
        machine.install_environment(environment);
    }
    Ok(Value::Generator(Rc::new(GeneratorData {
        function: Rc::clone(function),
        machine: crate::value::ExecutionCell::new(machine),
        receiver: receiver.clone(),
        arguments: deferred_arguments,
        done: RefCell::new(false),
        state: RefCell::new(state),
        pending_yield: RefCell::new(false),
        executing: RefCell::new(false),
        running: RefCell::new(false),
        async_next_queue: RefCell::new(VecDeque::new()),
    })))
}

fn initialize_parameters(
    function: &Rc<crate::value::FunctionValue>,
    receiver: &Value,
    arguments: &[Value],
) -> Result<InitialGeneratorState, VmError> {
    let code = function.code.code().ok_or(VmError::MissingReturn)?;
    let Some(marker) = code.parameter_end() else {
        return Ok((None, crate::register_file::RegisterFile::new(), 0, None));
    };
    let (mut registers, environment) =
        crate::functions::build_registers(function, receiver, arguments);
    let _private_environment = crate::private_environment::Guard::install_environment(
        function.private_environment.clone(),
    );
    let _home = crate::super_scope::Guard::install(function, receiver);
    let _with_scope = crate::with_scope::FunctionGuard::isolate();
    let step = crate::vm::execute_generator_code_step(
        code.slice(0, marker).ok_or(VmError::MissingReturn)?,
        &mut registers,
        Rc::clone(&environment),
        0,
        crate::completion::Completion::Normal,
    )?;
    require_normal_parameter_completion(step.completion)?;
    Ok((
        Some(GeneratorState {
            nested: 0,
            private_environment: None,
            suspension: None,
            async_for_of: None,
            pending_completion: None,
        }),
        registers,
        marker as u32,
        Some(environment),
    ))
}

fn registers(generator: &GeneratorData) -> &crate::register_file::RegisterFile {
    &generator.machine.borrow().registers.values
}

fn registers_mut(generator: &GeneratorData) -> &mut crate::register_file::RegisterFile {
    &mut generator.machine.borrow_mut().registers.values
}

fn machine_pc(generator: &GeneratorData) -> usize {
    generator.machine.borrow().pc as usize
}

fn set_machine_pc(generator: &GeneratorData, pc: usize) {
    generator.machine.borrow_mut().pc = pc as u32;
}

fn machine_environment(
    generator: &GeneratorData,
) -> Result<Rc<crate::environment::Environment>, VmError> {
    generator
        .machine
        .borrow()
        .environment()
        .ok_or(VmError::MissingReturn)
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
    let generator = generator_handle(receiver, "next")?;
    let completion = resume(&generator, Resume::Next(first_argument(arguments)));
    if generator.function.is_async {
        return Ok(crate::promise::from_async_generator_completion(
            completion, generator,
        ));
    }
    completion
}

pub(crate) fn async_next(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    let Some(Value::Generator(generator)) = receiver else {
        return async_next_error();
    };
    if !generator.function.is_async {
        return async_next_error();
    }
    if *generator.executing.borrow() {
        let promise = crate::value::PromiseData::allocate(crate::value::PromiseState::Pending);
        generator
            .async_next_queue
            .borrow_mut()
            .push_back((first_argument(arguments), Rc::clone(&promise)));
        return Ok(Value::Promise(promise));
    }
    *generator.executing.borrow_mut() = true;
    let result = next(receiver, arguments);
    *generator.executing.borrow_mut() = false;
    drain_async_next_queue(generator);
    result
}

fn async_next_error() -> Result<Value, VmError> {
    let error = crate::value::error::throw_type_error(
        "AsyncGenerator.next called on incompatible receiver",
    );
    Ok(crate::promise::from_async_completion(Err(error)))
}

fn drain_async_next_queue(generator: &Rc<GeneratorData>) {
    let Some((value, promise)) = generator.async_next_queue.borrow_mut().pop_front() else {
        return;
    };
    *generator.executing.borrow_mut() = true;
    let completion = resume(generator, Resume::Next(value));
    *generator.executing.borrow_mut() = false;
    crate::promise::settle_async_generator_completion(
        completion,
        Rc::clone(generator),
        promise,
        false,
    );
}
pub(crate) fn return_(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    let generator = generator_receiver(receiver, "return")?;
    let completion = resume(generator, Resume::Return(first_argument(arguments)));
    if generator.function.is_async {
        let generator = match receiver {
            Some(Value::Generator(generator)) => Rc::clone(generator),
            _ => return completion,
        };
        return Ok(crate::promise::from_async_generator_completion(
            completion, generator,
        ));
    }
    completion
}

pub(crate) fn async_return(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, VmError> {
    if matches!(receiver, Some(Value::Generator(generator)) if generator.function.is_async) {
        return return_(receiver, arguments);
    }
    let error = crate::value::error::throw_type_error(
        "AsyncGenerator.return called on incompatible receiver",
    );
    Ok(crate::promise::from_async_completion(Err(error)))
}
pub(crate) fn throw(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    let generator = generator_receiver(receiver, "throw")?;
    let completion = resume(generator, Resume::Throw(first_argument(arguments)));
    if generator.function.is_async {
        let generator = match receiver {
            Some(Value::Generator(generator)) => Rc::clone(generator),
            _ => return completion,
        };
        return Ok(crate::promise::from_async_generator_completion(
            completion, generator,
        ));
    }
    completion
}

pub(crate) fn async_throw(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    if matches!(receiver, Some(Value::Generator(generator)) if generator.function.is_async) {
        return throw(receiver, arguments);
    }
    let error = crate::value::error::throw_type_error(
        "AsyncGenerator.throw called on incompatible receiver",
    );
    Ok(crate::promise::from_async_completion(Err(error)))
}

pub(crate) fn async_dispose(receiver: Option<&Value>) -> Result<Value, VmError> {
    let receiver = receiver.ok_or_else(|| {
        crate::value::error::throw_type_error("AsyncIterator.prototype[@@asyncDispose]")
    })?;
    let method = match crate::execute::get_property_result(receiver, "return") {
        Ok(method) => method,
        Err(error) => return Ok(crate::promise::from_async_completion(Err(error))),
    };
    if matches!(method, Value::Undefined | Value::Null) {
        return Ok(crate::promise::promise_resolve(&[Value::Undefined]));
    }
    if !crate::conversion::is_callable(&method) {
        let error = crate::value::error::throw_type_error("AsyncIterator return is not callable");
        return Ok(crate::promise::from_async_completion(Err(error)));
    }
    let result = match crate::functions::execute_target(&method, receiver, &[Value::Undefined]) {
        Ok(result) => result,
        Err(error) => return Ok(crate::promise::from_async_completion(Err(error))),
    };
    let promise = crate::promise::promise_resolve(&[result]);
    crate::promise::promise_then(
        Some(&promise),
        &[Value::Builtin(
            crate::ops::Builtin::AsyncIteratorDisposeFulfilled,
        )],
    )
}
#[derive(Clone)]
pub(crate) enum Resume {
    Next(Value),
    Return(Value),
    Throw(Value),
}

pub(crate) fn resume(generator: &GeneratorData, resume: Resume) -> Result<Value, VmError> {
    if *generator.running.borrow() {
        *generator.done.borrow_mut() = true;
        return Err(crate::value::error::throw_type_error(
            "Generator is already executing",
        ));
    }
    *generator.running.borrow_mut() = true;
    let realm = crate::construct::function_realm_id(&generator.function);
    let result = if realm != crate::vm::current_context_or_default().realm() {
        crate::vm::with_realm(realm, || resume_inner(generator, resume.clone()))
            .unwrap_or_else(|| resume_inner(generator, resume))
    } else {
        resume_inner(generator, resume)
    };
    *generator.running.borrow_mut() = false;
    result
}

fn resume_inner(generator: &GeneratorData, resume: Resume) -> Result<Value, VmError> {
    if *generator.done.borrow() {
        return completed_resume(resume);
    }
    initialize_state(generator);
    let mut state = current_state(generator)?;
    if !is_suspended(generator, &state) {
        match resume {
            Resume::Return(value) => return finish(generator, value),
            Resume::Throw(value) => return throw_and_finish(generator, value),
            Resume::Next(_) => {}
        }
    }
    let completion = resume.completion();
    let direct_suspension = matches!(
        state.suspension,
        Some(
            crate::continuation::SuspensionPoint::Yield { .. }
                | crate::continuation::SuspensionPoint::YieldStar { .. }
        )
    );
    if let Resume::Next(input) = resume {
        install_resume_input(generator, &mut state, input);
    }
    if !direct_suspension {
        if let Some(result) = resume_suspended_contexts(generator, &mut state, &completion)? {
            generator.state.replace(Some(state));
            return Ok(result);
        }
    }
    let step = execute_generator_step(generator, &mut state, completion)?;
    set_machine_pc(generator, step.pc);
    state.suspension = step.suspension;
    capture_suspended_private_environment(generator, &mut state, &step.completion);
    update_machine_frame(generator, &state, &step.completion)?;
    update_await_frame(generator, &mut state, &step.completion)?;
    let result = complete_step(generator, &state, step.completion);
    generator.state.replace(Some(state));
    result
}

fn current_state(generator: &GeneratorData) -> Result<GeneratorState, VmError> {
    generator
        .state
        .borrow()
        .as_ref()
        .cloned()
        .ok_or(VmError::MissingReturn)
}

fn update_machine_frame(
    generator: &GeneratorData,
    state: &GeneratorState,
    completion: &crate::completion::Completion,
) -> Result<(), VmError> {
    if let Some(crate::continuation::SuspensionPoint::Loop {
        label,
        body,
        test,
        update,
        body_resume,
        dst,
        yield_dst,
        post_test,
        ..
    }) = state.suspension.clone()
    {
        let resume = parent_resume_range(generator, state);
        try_push_frame(
            &mut generator.machine.borrow_mut(),
            crate::machine::Frame::Loop {
                label,
                body,
                test,
                update,
                body_resume,
                resume,
                dst,
                yield_dst,
                post_test,
            },
        )?;
        return Ok(());
    }
    if completion.is_suspension() && state.suspension.is_none() {
        if push_initial_try_frames(generator)? {
            return Ok(());
        }
        if push_initial_loop_frame(generator, state)? {
            return Ok(());
        }
    }
    if push_nested_frame(generator, state)? {
        return Ok(());
    }
    let Some(crate::continuation::SuspensionPoint::YieldStar { dst, iterator, .. }) =
        state.suspension.as_ref()
    else {
        return Ok(());
    };
    let Ok(iterator) = crate::execute::read_register(&registers(generator), *iterator) else {
        return Ok(());
    };
    try_push_frame(
        &mut generator.machine.borrow_mut(),
        crate::machine::Frame::Delegate {
            phase: 0,
            iterator,
            destination: *dst,
        },
    )
}

fn push_initial_loop_frame(
    generator: &GeneratorData,
    state: &GeneratorState,
) -> Result<bool, VmError> {
    let index = machine_pc(generator)
        .checked_sub(1)
        .ok_or(VmError::MissingReturn)?;
    let Some(Op::Loop {
        label,
        body,
        test,
        update,
        post_test,
        dst,
        ..
    }) = generator
        .function
        .code
        .code()
        .and_then(|code| code.cold_at(index))
    else {
        return Ok(false);
    };
    let Some((body_resume, yield_dst)) = find_yield_in_function(body) else {
        return Ok(false);
    };
    try_push_frame(
        &mut generator.machine.borrow_mut(),
        crate::machine::Frame::Loop {
            label: label.clone(),
            body: body.range,
            test: test.range,
            update: update.range,
            body_resume,
            resume: parent_resume_range(generator, state),
            dst: *dst,
            yield_dst,
            post_test: *post_test,
        },
    )?;
    Ok(true)
}

fn find_yield_in_function(
    function: &crate::machine::FunctionCode,
) -> Option<(crate::machine::CodeRange, u16)> {
    let range = function.range;
    function.code().and_then(|code| {
        for (index, op) in code.cold_ops() {
            if let Op::Yield { src } = op {
                return Some((
                    crate::machine::CodeRange {
                        code: range.code,
                        start: range.start.saturating_add(index as u32 + 1),
                        end: range.end,
                    },
                    *src,
                ));
            }
            let nested = match op {
                Op::Try { body, .. }
                | Op::Loop { body, .. }
                | Op::ForOf { body, .. }
                | Op::ForIn { body, .. } => Some(body),
                Op::Conditional {
                    consequent,
                    alternate,
                    ..
                } => {
                    if let Some(found) = find_yield_in_function(consequent) {
                        return Some(found);
                    }
                    Some(alternate)
                }
                _ => None,
            };
            if let Some(found) = nested.and_then(find_yield_in_function) {
                return Some(found);
            }
        }
        None
    })
}

fn push_nested_frame(generator: &GeneratorData, state: &GeneratorState) -> Result<bool, VmError> {
    let iterator = push_iterator_frame(generator, state)?;
    if suspended_try(generator, state).is_some()
        && !matches!(
            generator.machine.borrow().frames.frames.last(),
            Some(crate::machine::Frame::Try { .. })
        )
    {
        push_try_frame(generator, state)?;
        return Ok(true);
    }
    if iterator {
        return Ok(true);
    }
    if suspended_conditional(generator, state).is_some() {
        push_branch_frame(generator, state)?;
        return Ok(true);
    }
    if suspended_private_scope(generator, state).is_some() {
        push_private_frame(generator, state)?;
        return Ok(true);
    }
    if push_dispose_frame(generator, state)? {
        return Ok(true);
    }
    Ok(false)
}

fn push_dispose_frame(generator: &GeneratorData, state: &GeneratorState) -> Result<bool, VmError> {
    let index = machine_pc(generator)
        .checked_sub(1)
        .ok_or(VmError::MissingReturn)?;
    let Some(Op::WithDispose {
        body,
        stack,
        await_using,
    }) = generator
        .function
        .code
        .code()
        .and_then(|code| code.cold_at(index))
    else {
        return Ok(false);
    };
    let Some(body_code) = body.code() else {
        return Err(VmError::MissingReturn);
    };
    let Some((yield_index, Op::Yield { src })) =
        body_code.find_cold(|op| matches!(op, Op::Yield { .. }))
    else {
        return Ok(false);
    };
    let body_resume = crate::machine::CodeRange {
        code: body.range.code,
        start: body.range.start.saturating_add(yield_index as u32 + 1),
        end: body.range.end,
    };
    let resume = parent_resume_range(generator, state);
    try_push_frame(
        &mut generator.machine.borrow_mut(),
        crate::machine::Frame::Dispose {
            body_resume,
            resume,
            stack: *stack,
            await_using: *await_using,
            yield_dst: *src,
        },
    )?;
    Ok(true)
}

fn resume_dispose_frame(
    generator: &GeneratorData,
    state: &mut GeneratorState,
    input: crate::completion::Completion,
) -> Result<Option<crate::completion::Completion>, VmError> {
    let frame = generator.machine.borrow().frames.frames.last().cloned();
    let Some(crate::machine::Frame::Dispose {
        body_resume,
        resume,
        stack,
        await_using,
        ..
    }) = frame
    else {
        return Ok(None);
    };
    let _private = crate::private_environment::Guard::install_environment(
        generator.function.private_environment.clone(),
    );
    let _home = crate::super_scope::Guard::install(&generator.function, &generator.receiver);
    let _with = crate::with_scope::FunctionGuard::install(&generator.function.with_captures);
    let _locals = crate::locals::EnvironmentGuard::install(machine_environment(generator)?);
    let (completion, next) = if matches!(input, crate::completion::Completion::Normal) {
        let step = execute_frame_step(generator, body_resume)?;
        (step.completion, Some(step.next))
    } else {
        (input, None)
    };
    if completion.is_suspension() {
        advance_frame_after_yield(generator, body_resume, next.ok_or(VmError::MissingReturn)?)?;
        return Ok(Some(completion));
    }
    let completion = execute_with_generator_registers(generator, |registers| {
        crate::disposable_stack::dispose_completion(registers, stack, completion, await_using)
    })?;
    generator.machine.borrow_mut().pop_frame();
    resume_generator_range(generator, state, resume, completion).map(Some)
}

fn install_dispose_frame_input(generator: &GeneratorData, input: &Value) -> bool {
    let frame = generator.machine.borrow().frames.frames.last().cloned();
    let Some(crate::machine::Frame::Dispose { yield_dst, .. }) = frame else {
        return false;
    };
    crate::execute::write_value(&mut registers_mut(generator), yield_dst, input.clone());
    true
}

fn resume_suspended_contexts(
    generator: &GeneratorData,
    state: &mut GeneratorState,
    completion: &crate::completion::Completion,
) -> Result<Option<Value>, VmError> {
    let mut completion = completion.clone();
    if state.async_for_of.is_some() {
        let spec = state.async_for_of.take().ok_or(VmError::MissingReturn)?;
        let _private = crate::private_environment::Guard::install_environment(
            generator.function.private_environment.clone(),
        );
        let _home = crate::super_scope::Guard::install(&generator.function, &generator.receiver);
        let _with = crate::with_scope::FunctionGuard::install(&generator.function.with_captures);
        let _locals = crate::locals::EnvironmentGuard::install(machine_environment(generator)?);
        let input = crate::execute::read_register(&registers(generator), spec.await_dst)?;
        let (next, pending) =
            crate::loops::resume_async_for_of(&mut registers_mut(generator), &spec, input)?;
        state.async_for_of = pending;
        if matches!(next, crate::completion::Completion::Normal) {
            return Ok(None);
        }
        return resume_machine_frame(generator, state, next).map(Some);
    }
    if let Some(resumed) = resume_delegate_frame(generator, &completion)? {
        if resumed.is_suspension() {
            return resume_machine_frame(generator, state, resumed).map(Some);
        }
        completion = resumed;
    }
    if let Some(completion) = resume_dispose_frame(generator, state, completion.clone())? {
        if completion.is_suspension() {
            return resume_machine_frame(generator, state, completion).map(Some);
        }
        return resume_machine_frame(generator, state, completion).map(Some);
    }
    if let Some(completion) = resume_loop_frame(generator, state, completion.clone())? {
        return resume_machine_frame(generator, state, completion).map(Some);
    }
    if let Some(completion) = resume_private_frame(generator, state, completion.clone())? {
        return resume_machine_frame(generator, state, completion).map(Some);
    }
    if let Some(completion) = resume_try_frame(generator, state, completion.clone())? {
        if completion.is_suspension() {
            return resume_machine_frame(generator, state, completion).map(Some);
        }
        if let Some(completion) = resume_iterator_frame(generator, state, completion.clone())? {
            return resume_machine_frame(generator, state, completion).map(Some);
        }
        return resume_machine_frame(generator, state, completion).map(Some);
    }
    if let Some(completion) = resume_iterator_frame(generator, state, completion.clone())? {
        return resume_machine_frame(generator, state, completion).map(Some);
    }
    if let Some(completion) = resume_branch_frame(generator, state, completion.clone())? {
        return resume_machine_frame(generator, state, completion).map(Some);
    }
    let resumed = match suspended_context(generator, state) {
        Some(SuspendedContext::Try) => resume_suspended_try(generator, state, completion.clone())?,
        Some(SuspendedContext::Conditional) => {
            resume_suspended_conditional(generator, state, completion.clone())?
        }
        Some(SuspendedContext::PrivateScope) => {
            resume_suspended_private_scope(generator, state, completion.clone())?
        }
        Some(SuspendedContext::IteratorBinding) => {
            resume_suspended_iterator_binding(generator, state, completion.clone())?
        }
        Some(SuspendedContext::Yield | SuspendedContext::YieldStar) | None => None,
    };
    resumed
        .map(|completion| complete_step(generator, state, completion))
        .transpose()
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

fn generator_handle(receiver: Option<&Value>, method: &str) -> Result<Rc<GeneratorData>, VmError> {
    generator_receiver(receiver, method)?;
    match receiver {
        Some(Value::Generator(value)) => Ok(Rc::clone(value)),
        _ => Err(crate::value::error::throw_type_error(
            "invalid generator receiver",
        )),
    }
}

pub(crate) fn resume_async_after_await(
    generator: &GeneratorData,
    rejected: bool,
    value: Value,
) -> Result<Value, VmError> {
    let input = if rejected {
        Resume::Throw(value)
    } else {
        Resume::Next(value)
    };
    resume(generator, input)
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

include!("generator_suspended_try.rs");

include!("generator_delegation.rs");

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
    generator.machine.borrow_mut().registers.values = registers;
    generator.machine.borrow_mut().pc = 0;
    generator
        .machine
        .borrow_mut()
        .install_environment(environment);
    *state = Some(GeneratorState {
        nested: 0,
        private_environment: None,
        suspension: None,
        async_for_of: None,
        pending_completion: None,
    });
}
