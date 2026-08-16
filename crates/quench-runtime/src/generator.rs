use crate::{
    execute::VmError,
    facts::ProgramDb,
    ops::Op,
    value::{GeneratorData, GeneratorState, Value},
};
use std::collections::{HashMap, VecDeque};
use std::{
    cell::{Ref, RefCell, RefMut},
    rc::Rc,
};
type InitialGeneratorState = (
    Option<GeneratorState>,
    Vec<Value>,
    u32,
    Option<Rc<crate::environment::Environment>>,
);
include!("generator_private_scope.rs");
include!("generator_private_frame.rs");
include!("generator_branch.rs");
include!("generator_async.rs");
include!("generator_try.rs");
include!("generator_try_frame.rs");
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
    let register_count = function.ops().len().clamp(32, usize::from(u16::MAX)) as u16;
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
        machine: RefCell::new(machine),
        receiver: receiver.clone(),
        arguments: deferred_arguments,
        done: RefCell::new(false),
        state: RefCell::new(state),
        pending_yield: RefCell::new(false),
        executing: RefCell::new(false),
        async_next_queue: RefCell::new(VecDeque::new()),
    })))
}

fn initialize_parameters(
    function: &Rc<crate::value::FunctionValue>,
    receiver: &Value,
    arguments: &[Value],
) -> Result<InitialGeneratorState, VmError> {
    let Some(marker) = function
        .ops()
        .iter()
        .position(|op| matches!(op, Op::ParameterEnd))
    else {
        return Ok((None, Vec::new(), 0, None));
    };
    let (mut registers, environment) =
        crate::functions::build_registers(function, receiver, arguments);
    let _private_environment = crate::private_environment::Guard::install_environment(
        function.private_environment.clone(),
    );
    let _home = crate::super_scope::Guard::install(function, receiver);
    let _with_scope = crate::with_scope::FunctionGuard::isolate();
    let step = crate::vm::execute_generator_step(
        &function.ops()[..marker],
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
        }),
        registers,
        marker.saturating_add(1) as u32,
        Some(environment),
    ))
}

fn registers(generator: &GeneratorData) -> Ref<'_, Vec<Value>> {
    Ref::map(generator.machine.borrow(), |machine| {
        &machine.registers.values
    })
}

fn registers_mut(generator: &GeneratorData) -> RefMut<'_, Vec<Value>> {
    RefMut::map(generator.machine.borrow_mut(), |machine| {
        &mut machine.registers.values
    })
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
        let promise = Rc::new(crate::value::PromiseData::default());
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
    crate::promise::settle_async_generator_completion(completion, Rc::clone(generator), promise);
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
    let mut state = current_state(generator)?;
    if !is_suspended(generator, &state) {
        match resume {
            Resume::Return(value) => return finish(generator, value),
            Resume::Throw(value) => return throw_and_finish(generator, value),
            Resume::Next(_) => {}
        }
    }
    let completion = resume.completion();
    let direct_suspension = state.suspension.is_some();
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
    update_machine_frame(generator, &state)?;
    update_await_frame(generator, &state, &step.completion)?;
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

fn update_machine_frame(generator: &GeneratorData, state: &GeneratorState) -> Result<(), VmError> {
    if push_nested_frame(generator, state)? {
        return Ok(());
    }
    let Some(crate::continuation::SuspensionPoint::YieldStar { dst, iterator, .. }) =
        state.suspension
    else {
        return Ok(());
    };
    let Ok(iterator) = crate::execute::read_register(&registers(generator), iterator) else {
        return Ok(());
    };
    try_push_frame(
        &mut generator.machine.borrow_mut(),
        crate::machine::Frame::Delegate {
            phase: 0,
            iterator,
            destination: dst,
        },
    )
}

fn push_nested_frame(generator: &GeneratorData, state: &GeneratorState) -> Result<bool, VmError> {
    if suspended_try(generator, state).is_some() {
        push_try_frame(generator, state)?;
        return Ok(true);
    }
    if push_iterator_frame(generator, state)? {
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
    Ok(false)
}

fn resume_suspended_contexts(
    generator: &GeneratorData,
    state: &mut GeneratorState,
    completion: &crate::completion::Completion,
) -> Result<Option<Value>, VmError> {
    if let Some(completion) = resume_private_frame(generator, state, completion.clone())? {
        return resume_machine_frame(generator, state, completion).map(Some);
    }
    if let Some(completion) = resume_try_frame(generator, state, completion.clone())? {
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
    });
}
