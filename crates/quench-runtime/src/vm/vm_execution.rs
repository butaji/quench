#[cold]
#[inline(never)]
pub(crate) fn not_callable() -> VmError {
    VmError::Thrown(crate::builtins::error(
        Builtin::TypeError,
        &[Value::String("value is not callable".to_string())],
    ))
}

impl VmError {
    pub fn render(&self) -> String {
        match self {
            VmError::Thrown(value) => render_thrown(value),
            VmError::Suspended(_) => "Suspended".to_string(),
            VmError::NotCallable => "TypeError: value is not callable".to_string(),
            other => format!("{other:?}"),
        }
    }
}

pub fn execute(ops: &[Op]) -> Result<Value, VmError> {
    execute_with_context(ops, &VmContext::default())
}

pub fn execute_with_registers(ops: &[Op], registers: Vec<Value>) -> Result<Value, VmError> {
    let context = current_context_or_default();
    execute_with_registers_context(ops, registers, &context)
}

pub fn execute_in_place(ops: &[Op], registers: &mut crate::register_file::RegisterFile) -> Result<Value, VmError> {
    let context = current_context_or_default();
    execute_in_place_context(ops, registers, &context)
}

pub(crate) fn current_context_or_default() -> Rc<VmContext> {
    CURRENT_CONTEXT
        .with(|current| current.borrow().clone())
        .unwrap_or_else(|| Rc::new(VmContext::default()))
}

pub(crate) fn execute_completion_in_place(
    ops: &[Op],
    registers: &mut crate::register_file::RegisterFile,
) -> Result<crate::completion::Completion, VmError> {
    let context = current_context_or_default();
    execute_completion_in_place_context(ops, registers, &context)
}

pub fn execute_with_context(ops: &[Op], context: &VmContext) -> Result<Value, VmError> {
    // Intrinsic overrides are scoped to one top-level evaluation; do not
    // carry mutations such as Math.random assignments into the next one.
    crate::builtins::reset_intrinsic_prototype_state();
    let result = execute_with_registers_context(ops, Vec::new(), context);
    // Drive the promise microtask queue so `.then`/`.catch` reactions and
    // synchronously-settling promise chains run to completion. Reinstall
    // the realm: reactions may call host capabilities (console.log) and
    // read globals, both of which require an active execution context.
    let realm = context.realm();
    if realm::with_realm(realm, crate::promise::drain_microtasks_all).is_none() {
        crate::vm::with_current_context(context, crate::promise::drain_microtasks_all);
    }
    result
}

pub fn execute_code_with_context(
    code: crate::machine::CodeView<'_>,
    context: &VmContext,
) -> Result<Value, VmError> {
    crate::builtins::reset_intrinsic_prototype_state();
    let result = crate::vm::with_current_context(context, || {
        let mut registers = crate::register_file::RegisterFile::new();
        prepare_register_stack(&mut registers);
        let environment = crate::environment::Environment::child(
            &crate::environment::Environment::new(),
            registers.to_values(),
        );
        execute_code_in_environment(code, &mut registers, context, environment)
    });
    let realm = context.realm();
    if realm::with_realm(realm, crate::promise::drain_microtasks_all).is_none() {
        crate::vm::with_current_context(context, crate::promise::drain_microtasks_all);
    }
    result
}

/// The context currently active on this thread, or a default one.
/// Hosts use this to re-enter the VM from inside a capability call.
pub fn current_context() -> Rc<VmContext> {
    current_context_or_default()
}

/// Call a function value from host code through the current context.
pub fn call_value(target: &Value, receiver: &Value, arguments: &[Value]) -> Result<Value, VmError> {
    crate::functions::execute_target(target, receiver, arguments)
}

pub fn execute_with_registers_context(
    ops: &[Op],
    registers: Vec<Value>,
    context: &VmContext,
) -> Result<Value, VmError> {
    // Install the caller's context before creating environments or resolving
    // globals.  Host values (console/process/require) are carried by this
    // context; delaying installation until the first VM step lets setup and
    // re-entrant continuations observe the thread's stale/default realm.
    crate::vm::with_current_context(context, || {
        let mut registers = crate::register_file::RegisterFile::from_values(registers);
        prepare_register_stack(&mut registers);
        let environment = crate::environment::Environment::child(
            &crate::environment::Environment::new(),
            registers.to_values(),
        );
        execute_in_environment(ops, &mut registers, context, environment)
    })
}

/// Prepare the VM's contiguous register stack once at entry.
///
/// Register indices are u16, so the representable stack limit is fixed.  We
/// reserve a small hot-path working set up front; subsequent writes use Vec's
/// geometric growth without introducing a second frame representation.
fn prepare_register_stack(registers: &mut crate::register_file::RegisterFile) {
    const INITIAL_REGISTER_CAPACITY: usize = 16;
    const MAX_REGISTER_COUNT: usize = u16::MAX as usize + 1;
    debug_assert!(registers.len() <= MAX_REGISTER_COUNT);
    if registers.capacity() < INITIAL_REGISTER_CAPACITY {
        registers.reserve(INITIAL_REGISTER_CAPACITY - registers.capacity());
    }
}
/// Execute a fragment inside the currently installed lexical environment.
pub fn execute_in_current_context(
    ops: &[Op],
    registers: &mut crate::register_file::RegisterFile,
) -> Result<Value, VmError> {
    let context = current_context_or_default();
    execute_in_place_context(ops, registers, &context)
}

pub fn execute_in_place_context(
    ops: &[Op],
    registers: &mut crate::register_file::RegisterFile,
    context: &VmContext,
) -> Result<Value, VmError> {
    prepare_register_stack(registers);
    let parent = crate::locals::current();
    let environment = crate::environment::Environment::in_place_child(&parent, registers.to_values());
    execute_in_environment(ops, registers, context, environment)
}

pub fn execute_code_in_place_context(
    code: crate::machine::CodeView<'_>,
    registers: &mut crate::register_file::RegisterFile,
    context: &VmContext,
) -> Result<Value, VmError> {
    prepare_register_stack(registers);
    let parent = crate::locals::current();
    let environment = crate::environment::Environment::in_place_child(&parent, registers.to_values());
    execute_code_in_environment(code, registers, context, environment)
}

pub(crate) fn execute_code_in_place(
    code: crate::machine::CodeView<'_>,
    registers: &mut crate::register_file::RegisterFile,
) -> Result<Value, VmError> {
    let context = current_context_or_default();
    execute_code_in_place_context(code, registers, &context)
}

fn execute_completion_in_place_context(
    ops: &[Op],
    registers: &mut crate::register_file::RegisterFile,
    context: &VmContext,
) -> Result<crate::completion::Completion, VmError> {
    prepare_register_stack(registers);
    let _context_guard = ContextGuard::install(context);
    let _global_guard = GlobalObjectGuard::install();
    drive_completion(ops, registers, context)
}

pub(crate) fn execute_completion_in_current_frame(
    ops: &[Op],
    registers: &mut crate::register_file::RegisterFile,
) -> Result<crate::completion::Completion, VmError> {
    let context = current_context_or_default();
    drive_completion(ops, registers, &context)
}

pub(crate) fn execute_code_completion_in_current_frame(
    code: crate::machine::CodeView<'_>,
    registers: &mut crate::register_file::RegisterFile,
) -> Result<crate::completion::Completion, VmError> {
    let context = current_context_or_default();
    drive_code_completion(code, registers, &context)
}

/// Execute a nested function-code fragment with its own tier profile.
///
/// Structured operations (branches, handlers, `with`, and binding bodies)
/// are represented by `FunctionCode` ranges in the canonical store.  Passing
/// only a `CodeView` through those gateways silently discarded the fragment's
/// invocation counters, so hot nested bodies could never admit their own
/// baseline plan.  Keep the owner attached at this boundary just as ordinary
/// function calls do; the semantics and register frame remain unchanged.
pub(crate) fn execute_function_code_completion_in_current_frame(
    owner: &crate::machine::FunctionCode,
    registers: &mut crate::register_file::RegisterFile,
) -> Result<crate::completion::Completion, VmError> {
    let code = owner.code().ok_or(VmError::MissingReturn)?;
    let context = current_context_or_default();
    execute_function_code_completion_with_context(owner, code, registers, &context)
}

pub(crate) fn execute_function_code_completion_with_context(
    owner: &crate::machine::FunctionCode,
    code: crate::machine::CodeView<'_>,
    registers: &mut crate::register_file::RegisterFile,
    context: &VmContext,
) -> Result<crate::completion::Completion, VmError> {
    let _ = owner.enter_invocation();
    if let (Some(optimizing), Some(baseline)) = (owner.executable_optimizing_plan(), owner.baseline_plan()) {
        drive_code_completion_with_optimizing_plan(code, registers, context, &optimizing, &baseline)
    } else if let Some(plan) = owner.baseline_plan() {
        drive_code_completion_with_plan(code, registers, context, &plan, Some(owner))
    } else {
        drive_code_completion_with_tier(code, registers, context, owner)
    }
}

/// Execute residual compact code with a context already owned by the caller.
/// Structured loops use this to avoid re-reading and cloning the same TLS
/// context for every iteration.
pub(crate) fn execute_code_completion_with_context(
    code: crate::machine::CodeView<'_>,
    registers: &mut crate::register_file::RegisterFile,
    context: &VmContext,
) -> Result<crate::completion::Completion, VmError> {
    drive_code_completion(code, registers, context)
}

/// Drive a compact fragment while retaining the [`FunctionCode`] that owns
/// it. Counted-loop fragments use this entry so repeated iterations can tier
/// up independently without reconstructing their code or reinstalling an
/// environment guard around every instruction.
pub(crate) fn execute_code_completion_with_owner(
    code: crate::machine::CodeView<'_>,
    owner: &crate::machine::FunctionCode,
    registers: &mut crate::register_file::RegisterFile,
) -> Result<crate::completion::Completion, VmError> {
    let context = current_context_or_default();
    if let (Some(optimizing), Some(baseline)) = (owner.executable_optimizing_plan(), owner.baseline_plan()) {
        drive_code_completion_with_optimizing_plan(code, registers, &context, &optimizing, &baseline)
    } else if let Some(plan) = owner.baseline_plan() {
        drive_code_completion_with_plan(code, registers, &context, &plan, Some(owner))
    } else {
        drive_code_completion_with_tier(code, registers, &context, owner)
    }
}

/// Execute one owner-aware fragment step without erasing the exact next PC.
/// Structured async loops use this boundary to record the operation that
/// actually suspended instead of searching their source for a likely await.
pub(crate) fn execute_code_completion_step_with_owner(
    code: crate::machine::CodeView<'_>,
    owner: &crate::machine::FunctionCode,
    pc: usize,
    registers: &mut crate::register_file::RegisterFile,
) -> Result<CompletionStep, VmError> {
    let context = current_context_or_default();
    if let (Some(optimizing), Some(baseline)) =
        (owner.executable_optimizing_plan(), owner.baseline_plan())
    {
        let (completion, next) = crate::vm::execute_optimized_code_step_from(
            code,
            &optimizing,
            &baseline,
            pc,
            registers,
            &context,
        )?;
        return Ok(CompletionStep { completion, next, suspended_pc: None });
    }
    if let Some(plan) = owner.baseline_plan() {
        return crate::vm::execute_baseline_completion_step_from_with_owner(
            code, &plan, pc, registers, &context, owner,
        );
    }
    run_code_completion_step_from_with_owner(
        code,
        pc,
        registers,
        &context,
        Some(owner),
    )
}

fn drive_completion(
    ops: &[Op],
    registers: &mut crate::register_file::RegisterFile,
    context: &VmContext,
) -> Result<crate::completion::Completion, VmError> {
    // Freeze the compact operation tree once for the whole drive. Rebuilding
    // an ExecutableCode on every step would repeatedly lower and allocate the
    // same bytecode, defeating the immutable-code/dispatch split used by the
    // machine and making long ordinary functions pay an avoidable O(n²) cost.
    let executable = crate::machine::ExecutableCode::from_ops(ops.to_vec());
    let code = executable.code();
    let mut pc = 0;
    loop {
        let step = run_code_completion_step_from(code, pc, registers, context)?;
        pc = step.next;
        match step.completion {
            crate::completion::Completion::Call(continuation) => {
                if let Err(VmError::Thrown(value)) =
                    crate::vm::vm_ops::execute_call_continuation(registers, continuation)
                {
                    return Ok(crate::completion::Completion::Throw(value));
                }
            }
            completion => return preserve_frame_completion(completion),
        }
    }
}

fn drive_code_completion(
    code: crate::machine::CodeView<'_>,
    registers: &mut crate::register_file::RegisterFile,
    context: &VmContext,
) -> Result<crate::completion::Completion, VmError> {
    let mut pc = 0;
    loop {
        let step = run_code_completion_step_from(code, pc, registers, context)?;
        pc = step.next;
        match step.completion {
            crate::completion::Completion::Call(continuation) => {
                if let Err(VmError::Thrown(value)) =
                    crate::vm::vm_ops::execute_call_continuation(registers, continuation)
                {
                    return Ok(crate::completion::Completion::Throw(value));
                }
            }
            completion => return preserve_frame_completion(completion),
        }
    }
}

fn drive_code_completion_with_plan(
    code: crate::machine::CodeView<'_>,
    registers: &mut crate::register_file::RegisterFile,
    context: &VmContext,
    plan: &crate::machine::BaselinePlan,
    tier_owner: Option<&crate::machine::FunctionCode>,
) -> Result<crate::completion::Completion, VmError> {
    let mut pc = 0;
    loop {
        let step = if let Some(owner) = tier_owner {
            let (completion, next) = crate::vm::execute_baseline_code_step_from_with_owner(
                code, plan, pc, registers, context, owner,
            )?;
            crate::vm::CompletionStep { completion, next, suspended_pc: None }
        } else {
            let (completion, next) = crate::vm::execute_baseline_code_step_from(
                code, plan, pc, registers, context,
            )?;
            crate::vm::CompletionStep { completion, next, suspended_pc: None }
        };
        pc = step.next;
        match step.completion {
            crate::completion::Completion::Call(continuation) => {
                if let Err(VmError::Thrown(value)) =
                    crate::vm::vm_ops::execute_call_continuation(registers, continuation)
                {
                    return Ok(crate::completion::Completion::Throw(value));
                }
                if let Some(owner) = tier_owner {
                    if let (Some(optimizing), Some(baseline)) =
                        (owner.executable_optimizing_plan(), owner.baseline_plan())
                    {
                        return drive_code_completion_with_optimizing_plan(
                            code,
                            registers,
                            context,
                            &optimizing,
                            &baseline,
                        );
                    }
                }
            }
            completion => {
                if !matches!(completion, crate::completion::Completion::Normal) {
                    return preserve_frame_completion(completion);
                }
                // The baseline step already drives ordinary fall-through and
                // branches until it reaches a completion boundary.  At the
                // end of the compact body, Normal is the completed fragment,
                // not a request to re-enter at the out-of-range successor.
                if pc >= code.len() {
                    return Ok(crate::completion::Completion::Normal);
                }
                if let Some(owner) = tier_owner {
                    if let (Some(optimizing), Some(baseline)) =
                        (owner.executable_optimizing_plan(), owner.baseline_plan())
                    {
                        return drive_code_completion_with_optimizing_plan(
                            code,
                            registers,
                            context,
                            &optimizing,
                            &baseline,
                        );
                    }
                }
            }
        }
    }
}

fn drive_code_completion_with_optimizing_plan(
    code: crate::machine::CodeView<'_>,
    registers: &mut crate::register_file::RegisterFile,
    context: &VmContext,
    optimizing: &crate::machine::OptimizingPlan,
    baseline: &crate::machine::BaselinePlan,
) -> Result<crate::completion::Completion, VmError> {
    let mut pc = 0;
    loop {
        let (completion, next) = crate::vm::execute_optimized_code_step_from(
            code,
            optimizing,
            baseline,
            pc,
            registers,
            context,
        )?;
        pc = next;
        match completion {
            crate::completion::Completion::Call(continuation) => {
                if let Err(VmError::Thrown(value)) =
                    crate::vm::vm_ops::execute_call_continuation(registers, continuation)
                {
                    return Ok(crate::completion::Completion::Throw(value));
                }
            }
            crate::completion::Completion::Normal => {
                if pc >= code.len() {
                    return Ok(crate::completion::Completion::Normal);
                }
                continue;
            }
            completion => return preserve_frame_completion(completion),
        }
    }
}

/// Drive one function body while retaining its tier owner.  The interpreter
/// dispatcher counts each instruction and can therefore perform OSR at a hot
/// back-edge; once the owner publishes a plan, the next step naturally uses
/// the baseline driver with the same registers.
fn drive_code_completion_with_tier(
    code: crate::machine::CodeView<'_>,
    registers: &mut crate::register_file::RegisterFile,
    context: &VmContext,
    owner: &crate::machine::FunctionCode,
) -> Result<crate::completion::Completion, VmError> {
    let mut pc = 0;
    loop {
        let (completion, next) = crate::vm::execute_function_code_step_from(
            code,
            owner,
            pc,
            registers,
            context,
        )?;
        pc = next;
        match completion {
            crate::completion::Completion::Call(continuation) => {
                if let Err(VmError::Thrown(value)) =
                    crate::vm::vm_ops::execute_call_continuation(registers, continuation)
                {
                    return Ok(crate::completion::Completion::Throw(value));
                }
            }
            completion => return preserve_frame_completion(completion),
        }
    }
}
fn preserve_frame_completion(
    completion: crate::completion::Completion,
) -> Result<crate::completion::Completion, VmError> {
    use crate::completion::Completion;
    Ok(match completion {
        Completion::TailCall(request) => tail_call_completion(request),
        completion => completion,
    })
}

fn tail_call_completion(
    request: crate::completion::TailCallRequest,
) -> crate::completion::Completion {
    crate::completion::Completion::TailCall(request)
}

pub(crate) fn execute_in_environment(
    ops: &[Op],
    registers: &mut crate::register_file::RegisterFile,
    context: &VmContext,
    environment: Rc<crate::environment::Environment>,
) -> Result<Value, VmError> {
    let _context_guard = ContextGuard::install(context);
    let _global_guard = GlobalObjectGuard::install();
    let _environment_guard = crate::locals::EnvironmentGuard::install(Rc::clone(&environment));
    let register_count = registers.len().min(usize::from(u16::MAX)) as u16;
    let mut machine = crate::machine::Machine::with_register_count(
        crate::machine::CodeId(0),
        crate::machine::EnvironmentRef(0),
        register_count,
    );
    machine.restore_registers(std::mem::take(registers));
    let mut pc = 0;
    loop {
        let step = run_ops_completion_step_from(ops, pc, machine.registers_mut(), context)?;
        let completion = step.completion;
        let next = step.next;
        match completion {
            crate::completion::Completion::Call(mut continuation) => {
                continuation.caller_code = machine.code_id();
                continuation.caller_pc = next as u32;
                machine.push_call_frame(continuation);
                let continuation = machine.pop_call_frame().expect("call frame just pushed");
                crate::vm::vm_ops::execute_call_continuation(
                    machine.registers_mut(),
                    continuation,
                )?;
                pc = next;
            }
            crate::completion::Completion::TailCall(request) => {
                // A top-level code slice has no caller continuation to
                // consume a promoted call.  Re-enter the packed call-frame
                // driver with a synthetic result register; it handles the
                // callee iteratively (including nested tail calls) and keeps
                // ordinary calls on the upstream fast path.
                let mut caller_registers = machine.take_registers();
                let continuation = crate::completion::CallContinuation {
                    callee: request.callee,
                    receiver: request.receiver,
                    arguments: request.arguments,
                    caller_code: crate::identity::CodeId(0),
                    caller_pc: 0,
                    caller_registers: std::mem::take(&mut caller_registers),
                    caller_environment: crate::identity::EnvironmentRef(0),
                    destination: 0,
                    guards: crate::completion::ContinuationGuards::default(),
                };
                crate::vm::vm_ops::execute_call_continuation(
                    &mut caller_registers,
                    continuation,
                )?;
                let value = crate::vm::read_register(&caller_registers, 0)?;
                *registers = caller_registers;
                return Ok(value);
            }
            completion => {
                *registers = machine.take_registers();
                return completion_result(completion);
            }
        }
    }
}

#[inline(never)]
pub(crate) fn execute_code_in_environment(
    code: crate::machine::CodeView<'_>,
    registers: &mut crate::register_file::RegisterFile,
    context: &VmContext,
    environment: Rc<crate::environment::Environment>,
) -> Result<Value, VmError> {
    let _context_guard = ContextGuard::install(context);
    let _global_guard = GlobalObjectGuard::install();
    let _environment_guard = crate::locals::EnvironmentGuard::install(environment);
    let _environment_root = crate::cycle_collector::protect_environment(&crate::locals::current());
    let mut pc = 0;
    loop {
        let step = run_code_completion_step_from(code, pc, registers, context)?;
        pc = step.next;
        match step.completion {
            crate::completion::Completion::Call(continuation) => {
                crate::vm::vm_ops::execute_call_continuation(registers, continuation)?;
            }
            crate::completion::Completion::TailCall(request) => {
                let mut caller_registers = std::mem::take(registers);
                let continuation = crate::completion::CallContinuation {
                    callee: request.callee,
                    receiver: request.receiver,
                    arguments: request.arguments,
                    caller_code: crate::identity::CodeId(0),
                    caller_pc: 0,
                    caller_registers: std::mem::take(&mut caller_registers),
                    caller_environment: crate::identity::EnvironmentRef(0),
                    destination: 0,
                    guards: crate::completion::ContinuationGuards::default(),
                };
                crate::vm::vm_ops::execute_call_continuation(
                    &mut caller_registers,
                    continuation,
                )?;
                let value = crate::vm::read_register(&caller_registers, 0)?;
                *registers = caller_registers;
                return Ok(value);
            }
            completion => return completion_result(completion),
        }
    }
}
pub(crate) fn execute_frame_completion(
    ops: &[Op],
    registers: &mut crate::register_file::RegisterFile,
    context: &VmContext,
    environment: Rc<crate::environment::Environment>,
) -> Result<crate::completion::Completion, VmError> {
    let _context_guard = ContextGuard::install(context);
    let _global_guard = GlobalObjectGuard::install();
    let _environment_guard = crate::locals::EnvironmentGuard::install(environment);
    let executable = crate::machine::ExecutableCode::from_ops(ops.to_vec());
    let code = executable.code();
    let mut pc = 0;
    loop {
        let step = run_code_completion_step_from(code, pc, registers, context)?;
        pc = step.next;
        match step.completion {
            crate::completion::Completion::Call(continuation) => {
                if let Err(VmError::Thrown(value)) =
                    crate::vm::vm_ops::execute_call_continuation(registers, continuation)
                {
                    return Ok(crate::completion::Completion::Throw(value));
                }
            }
            completion => return Ok(completion),
        }
    }
}

pub(crate) fn execute_code_frame_completion(
    code: crate::machine::CodeView<'_>,
    registers: &mut crate::register_file::RegisterFile,
    context: &VmContext,
    environment: Rc<crate::environment::Environment>,
) -> Result<crate::completion::Completion, VmError> {
    execute_code_frame_completion_with_plan(code, registers, context, environment, None)
}

pub(crate) fn execute_code_frame_completion_with_plan(
    code: crate::machine::CodeView<'_>,
    registers: &mut crate::register_file::RegisterFile,
    context: &VmContext,
    environment: Rc<crate::environment::Environment>,
    plan: Option<std::rc::Rc<crate::machine::BaselinePlan>>,
) -> Result<crate::completion::Completion, VmError> {
    let _context_guard = ContextGuard::install(context);
    let _global_guard = GlobalObjectGuard::install();
    let pooled = std::rc::Rc::clone(&environment);
    let _environment_root = crate::cycle_collector::protect_environment(&environment);
    let environment_guard = crate::locals::EnvironmentGuard::install(environment);
    let result = if let Some(plan) = plan {
        drive_code_completion_with_plan(code, registers, context, &plan, None)
    } else {
        drive_code_completion(code, registers, context)
    };
    drop(environment_guard);
    if result
        .as_ref()
        .ok()
        .is_some_and(|completion| !completion.is_suspension())
    {
        crate::environment::Environment::recycle_frame(pooled);
    }
    result
}

/// Function-owned frame entry used by ordinary synchronous calls.  It keeps
/// the existing frame/environment lifecycle but supplies the owner needed for
/// current-invocation OSR when no baseline plan exists yet.
pub(crate) fn execute_code_frame_completion_with_owner(
    code: crate::machine::CodeView<'_>,
    owner: &crate::machine::FunctionCode,
    registers: &mut crate::register_file::RegisterFile,
    context: &VmContext,
    environment: Rc<crate::environment::Environment>,
) -> Result<crate::completion::Completion, VmError> {
    let _context_guard = ContextGuard::install(context);
    let _global_guard = GlobalObjectGuard::install();
    let pooled = Rc::clone(&environment);
    let _environment_root = crate::cycle_collector::protect_environment(&environment);
    let environment_guard = crate::locals::EnvironmentGuard::install(environment.clone());
    let result = if let (Some(optimizing), Some(baseline)) =
        (owner.executable_optimizing_plan(), owner.baseline_plan())
    {
        drive_code_completion_with_optimizing_plan(
            code,
            registers,
            context,
            &optimizing,
            &baseline,
        )
    } else if let Some(plan) = owner.baseline_plan() {
        drive_code_completion_with_plan(code, registers, context, &plan, Some(owner))
    } else {
        drive_code_completion_with_tier(code, registers, context, owner)
    };
    drop(environment_guard);
    if result
        .as_ref()
        .ok()
        .is_some_and(|completion| !completion.is_suspension())
    {
        crate::environment::Environment::recycle_frame(pooled);
    }
    result
}
pub(crate) fn execute_indirect_eval(code: crate::machine::CodeView<'_>) -> Result<Value, VmError> {
    let context = CURRENT_CONTEXT
        .with(|current| current.borrow().clone())
        .unwrap_or_else(|| Rc::new(VmContext::default()));
    let caller = crate::locals::current();
    let caller_global = caller.get(0);
    if matches!(&caller_global, Value::Object(object) if object.iter().any(|(name, _)| name == crate::vm::SCRIPT_GLOBAL_VIEW)) {
        let environment = crate::environment::Environment::new();
        environment.set(0, caller_global.clone());
        let mut registers = crate::register_file::RegisterFile::new();
        let _with_scope = crate::with_scope::FunctionGuard::isolate();
        return execute_code_in_environment(code, &mut registers, &context, environment);
    }
    if realm::context(context.realm()).is_some() {
        return execute_indirect_eval_in_realm(context.realm(), code);
    }
    let global = match caller_global {
        Value::Undefined => current_global_object(),
        value => value,
    };
    let environment = crate::environment::Environment::new();
    environment.set(0, global.clone());
    let mut registers = crate::register_file::RegisterFile::new();
    let _with_scope = crate::with_scope::FunctionGuard::isolate();
    let result = execute_code_in_environment(code, &mut registers, &context, environment);
    caller.replace_value(&global, &current_global_object());
    result
}
pub(crate) fn execute_indirect_eval_in_realm(
    realm_id: RealmId,
    code: crate::machine::CodeView<'_>,
) -> Result<Value, VmError> {
    realm::execute(realm_id, code)
}

#[cfg(test)]
mod tests {
    use std::rc::Rc;

    use crate::execute::VmError;
    use crate::value::{ObjectData, Value};

    /// Bug reproducer: the replacement log is the forwarding table that
    /// keeps heap-resident object references (event-loop callbacks, timers)
    /// pointing at the latest copy-on-write snapshot. Re-entering the VM
    /// for another top-level execution of the same program must not clear
    /// it — otherwise those references silently revert to stale snapshots.
    #[test]
    fn execute_with_context_preserves_replacements() {
        let old = Value::Object(Rc::new(ObjectData::new(vec![])));
        let new = Value::Object(Rc::new(ObjectData::new(vec![])));
        crate::locals::replace_value(&old, &new);
        let program = crate::reduce::reduce_source("0;").expect("source reduces");
        crate::vm::execute_code_with_context(program.code(), &crate::vm::VmContext::default())
            .expect("program runs");
        let resolved = crate::locals::resolved_replacement(old);
        let (Value::Object(resolved), Value::Object(expected)) = (resolved, new) else {
            panic!("replacement must resolve to the new snapshot");
        };
        assert!(Rc::ptr_eq(&resolved, &expected));
    }

    #[test]
    fn ordinary_calls_preserve_semantics_at_bounded_depth() {
        let source = r#"
            function descend(n) {
                if (n === 0) return 7;
                return descend(n - 1);
            }
            if (descend(32) !== 7) throw "bounded ordinary call returned the wrong value";
        "#;
        let program = crate::reduce::reduce_source(source).expect("source reduces");
        let result =
            crate::vm::execute_code_with_context(program.code(), &crate::vm::VmContext::default())
                .expect("bounded ordinary calls run");
        assert_eq!(result, Value::Undefined);
    }

    #[test]
    fn ordinary_source_prototype_property_lookup_preserves_semantics() {
        let source = r#"
            function read(o) { return o.value; }
            var prototype = { value: 11 };
            var receiver = Object.create(prototype);
            if (read(receiver) !== 11) throw "prototype lookup changed";
            prototype.value = 13;
            if (read(receiver) !== 13) throw "prototype mutation was stale";
        "#;
        let program = crate::reduce::reduce_source(source).expect("source reduces");
        let result = crate::vm::execute_code_with_context(
            program.code(),
            &crate::vm::VmContext::default(),
        )
        .expect("prototype source executes");
        assert_eq!(result, Value::Undefined);
    }

    #[test]
    fn owner_baseline_step_retires_completed_fragment() {
        let owner = crate::machine::FunctionCode::from_ops(vec![crate::ops::Op::Move {
            dst: 0,
            src: 0,
        }]);
        owner.set_tier_threshold_for_test(1);
        owner.retire(1);
        assert_eq!(
            owner.enter_invocation(),
            crate::machine::TierTransition::CompileBaseline
        );
        let code = owner.code().expect("materialized owner code");
        let mut registers = crate::register_file::RegisterFile::from_values(vec![
            Value::Number(7.0),
        ]);
        let completion = super::execute_code_completion_with_owner(code, &owner, &mut registers)
            .expect("baseline fragment executes");
        assert_eq!(completion, crate::completion::Completion::Normal);
        assert_eq!(registers.read_number(0), Some(7.0));
        assert_eq!(owner.tier_profile().retired, 2);
    }

    #[test]
    fn statement_completion_survives_replace_and_observed_loop_break() {
        let source = r#"
            var trim = /^\s*|\s*$/g;
            var text = "  value  ";
            for (var index = 0; index < 3; index++) {
                text.replace(trim, "");
            }
            if (text !== "  value  ") throw "replace result was assigned";
            if (eval("while (true) { 'last'; break; }") !== "last") {
                throw "loop completion was lost";
            }
        "#;
        let program = crate::reduce::reduce_source(source).expect("source reduces");
        let result = crate::vm::execute_code_with_context(
            program.code(),
            &crate::vm::VmContext::default(),
        )
        .expect("statement completions run");
        assert_eq!(result, Value::Undefined);
    }

    #[test]
    fn staged_global_resolves_math_for_property_assignment() {
        let source = r#"
            Math.random = function () { return 1; };
            if (Math.random() !== 1) throw "Math must resolve while globals are staged";
        "#;
        let program = crate::reduce::reduce_source(source).expect("source reduces");
        let result =
            crate::vm::execute_code_with_context(program.code(), &crate::vm::VmContext::default())
                .expect("Math assignment runs");
        assert_eq!(result, Value::Undefined);
    }
    #[test]
    fn staged_math_random_binding_preserves_callable_state() {
        let source = r#"
            Math.random = (function () {
                var next = 0;
                return function () { next = next + 1; return next; };
            })();
            if (Math.random() !== 1 || Math.random() !== 2) {
                throw "staged Math.random lost its closure state";
            }
        "#;
        let program = crate::reduce::reduce_source(source).expect("source reduces");
        let result =
            crate::vm::execute_code_with_context(program.code(), &crate::vm::VmContext::default())
                .expect("stateful Math.random assignment runs");
        assert_eq!(result, Value::Undefined);
    }

    #[test]
    fn call_rhs_preserves_member_assignment_target() {
        let source = r#"
            function value() { return 42; }
            const target = {};
            target.answer = value();
            if (target.answer !== 42) throw "call RHS lost assignment target";
        "#;
        let program = crate::reduce::reduce_source(source).expect("source reduces");
        let result =
            crate::vm::execute_code_with_context(program.code(), &crate::vm::VmContext::default())
                .expect("member assignment with call RHS runs");
        assert_eq!(result, Value::Undefined);
    }

    #[test]
    fn ordinary_calls_return_through_nested_continuations() {
        let source = r#"
            function inner(value) { return value + 1; }
            function middle(value) { return inner(value * 2); }
            if (middle(20) !== 41) throw "nested ordinary call returned the wrong value";
        "#;
        let program = crate::reduce::reduce_source(source).expect("source reduces");
        let result =
            crate::vm::execute_code_with_context(program.code(), &crate::vm::VmContext::default())
                .expect("nested calls run");
        assert_eq!(result, Value::Undefined);
    }

    #[test]
    fn ordinary_calls_preserve_receiver_and_arguments() {
        let source = r#"
            function read(prefix, suffix) {
                return prefix + this.value + suffix + arguments.length;
            }
            if (read.call({ value: 40 }, 1, 2) !== 45) {
                throw "receiver call returned the wrong value";
            }
        "#;
        let program = crate::reduce::reduce_source(source).expect("source reduces");
        let result =
            crate::vm::execute_code_with_context(program.code(), &crate::vm::VmContext::default())
                .expect("receiver call runs");
        assert_eq!(result, Value::Undefined);
    }

    #[test]
    fn ordinary_call_throw_unwinds_to_top_level() {
        let source = r#"
            function fail() { throw 17; }
            function wrapper() { return fail(); }
            wrapper();
        "#;
        let program = crate::reduce::reduce_source(source).expect("source reduces");
        let result =
            crate::vm::execute_code_with_context(program.code(), &crate::vm::VmContext::default());
        assert!(matches!(
            result,
            Err(crate::vm::VmError::Thrown(Value::Number(17.0)))
        ));
    }

    #[test]
    fn ordinary_calls_retain_closure_environment() {
        let source = r#"
            function makeAdder(base) {
                return function (value) { return base + value; };
            }
            const add = makeAdder(9);
            if (add(33) !== 42) throw "closure call returned the wrong value";
        "#;
        let program = crate::reduce::reduce_source(source).expect("source reduces");
        let result =
            crate::vm::execute_code_with_context(program.code(), &crate::vm::VmContext::default())
                .expect("closure call runs");
        assert_eq!(result, Value::Undefined);
    }

    /// The cycle collector may run while a nested call is active. The caller
    /// environment is then held by the Rust call driver rather than by a JS
    /// object edge; it must remain a root so closures that resume after the
    /// call are not cleared as unreachable cycles.
    #[test]
    fn cycle_collection_preserves_caller_continuation_after_allocation_churn() {
        let source = r#"
            function churn(count) {
                for (var i = 0; i < count; i++) {
                    var left = {}, right = {};
                    left.peer = right;
                    right.peer = left;
                }
            }
            function caller() {
                var marker = 41;
                function continuation() { return marker + 1; }
                churn(2000);
                return continuation();
            }
            if (caller() !== 42) throw "caller continuation was reclaimed";
        "#;
        let program = crate::reduce::reduce_source(source).expect("source reduces");
        let result = crate::vm::execute_code_with_context(
            program.code(),
            &crate::vm::VmContext::default(),
        )
        .expect("allocation churn preserves continuation");
        assert_eq!(result, Value::Undefined);
    }

    /// A closure returned from a completed call keeps its captured suffix
    /// alive while later allocation checkpoints run.  This guards both frame
    /// pooling (which must not clear a captured SlotStore) and trial deletion
    /// (which must retain the globally reachable closure between calls).
    #[test]
    fn returned_closure_survives_cycle_collection_and_reuse() {
        let source = r#"
            function makeReader() {
                var payload = { value: 41 };
                return function () { return payload.value + 1; };
            }
            var reader = makeReader();
            function churn(count) {
                for (var i = 0; i < count; i++) {
                    var left = {}, right = {};
                    left.peer = right;
                    right.peer = left;
                }
            }
            churn(3000);
            if (reader() !== 42) throw "returned closure was reclaimed";
        "#;
        let program = crate::reduce::reduce_source(source).expect("source reduces");
        let result = crate::vm::execute_code_with_context(
            program.code(),
            &crate::vm::VmContext::default(),
        )
        .expect("returned closure survives collection");
        assert_eq!(result, Value::Undefined);
    }

    #[test]
    fn tail_calls_are_driven_by_machine_loop() {
        let mut machine = crate::machine::Machine::new(
            crate::machine::CodeId(0),
            crate::machine::EnvironmentRef(0),
        );
        let mut remaining = 10_000;
        let completion = machine
            .run_until_complete(
                crate::completion::Completion::Normal,
                |_, _| -> Result<crate::completion::Completion, ()> {
                    if remaining == 0 {
                        Ok(crate::completion::Completion::Return(Value::Number(7.0)))
                    } else {
                        remaining -= 1;
                        Ok(crate::completion::Completion::TailCall(
                            crate::completion::TailCallRequest {
                                callee: Value::Undefined,
                                receiver: Value::Undefined,
                                arguments: Vec::new().into(),
                            },
                        ))
                    }
                },
            )
            .expect("tail-call loop completes");
        assert_eq!(
            completion,
            crate::completion::Completion::Return(Value::Number(7.0))
        );
        assert_eq!(machine.frame_count(), 0);
    }

    #[test]
    fn dispatch_loop_has_observable_result_and_budget() {
        use std::time::{Duration, Instant};

        // This intentionally exercises the ordinary run_op path rather than a
        // native shortcut. The loop's side effects keep dispatch observable.
        let source = r#"
            let value = 0;
            for (let i = 0; i < 50_000; i++) value += i;
            // Keep the arithmetic loop observable without coupling this
            // dispatch benchmark to call-frame register restoration.
        "#;
        let program = crate::reduce::reduce_source(source).expect("source reduces");
        let operation_count = program.code().len();
        assert!(
            operation_count >= 10,
            "benchmark must contain a meaningful dispatch sequence"
        );
        let started = Instant::now();
        let result =
            crate::vm::execute_code_with_context(program.code(), &crate::vm::VmContext::default())
                .expect("dispatch benchmark runs");
        let elapsed = started.elapsed();
        assert_eq!(result, Value::Undefined);
        // Generous wall-clock guard: this is evidence against pathological
        // compatibility failures, not a machine-specific performance target.
        assert!(
            elapsed < Duration::from_secs(2),
            "dispatch loop exceeded 2s budget: {elapsed:?} ({operation_count} ops)"
        );
    }
    #[test]
    fn cold_not_callable_error_renders_canonical_type_error() {
        assert_eq!(
            super::not_callable().render(),
            "TypeError: value is not callable"
        );
    }

    #[test]
    fn cold_thrown_rendering_preserves_primitive_value() {
        assert_eq!(
            VmError::Thrown(Value::String("boom".to_string())).render(),
            "boom"
        );
    }
    #[test]
    fn cold_error_constructors_render_canonical_names() {
        assert_eq!(
            crate::value::error::throw_reference_error("missing").render(),
            "ReferenceError: missing"
        );
        assert_eq!(
            crate::value::error::throw_syntax_error("bad syntax").render(),
            "SyntaxError: bad syntax"
        );
        assert_eq!(
            crate::value::error::throw_range_error("too large").render(),
            "RangeError: too large"
        );
        assert_eq!(
            crate::value::error::throw_uri_error("bad URI").render(),
            "URIError: bad URI"
        );
    }
}
