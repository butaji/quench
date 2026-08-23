pub(crate) fn resume(generator: &GeneratorData, resume: Resume) -> Result<Value, VmError> {
    if *generator.running.borrow() {
        return Err(crate::value::error::throw_type_error(
            "Generator is already executing",
        ));
    }
    *generator.running.borrow_mut() = true;
    let result = resume_inner(generator, resume);
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
    });
}
