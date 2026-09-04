include!("vm_generator_step.rs");
include!("vm_completion_step.rs");

/// Opaque state passed through the generated baseline entry trampoline. The
/// lifetime is only used while the synchronous call is active; the machine
/// code receives a raw pointer and never retains it.
#[repr(C)]
pub(crate) struct NativeDispatchContext<'a> {
    code: crate::machine::CodeView<'a>,
    pc: usize,
    entry: crate::machine::BaselineEntry,
    registers: *mut crate::register_file::RegisterFile,
    context: *const VmContext,
    result: Option<DispatchTransition>,
    error: Option<VmError>,
}

impl<'a> NativeDispatchContext<'a> {
    pub(crate) fn new(
        code: crate::machine::CodeView<'a>,
        pc: usize,
        entry: crate::machine::BaselineEntry,
        registers: &mut crate::register_file::RegisterFile,
        context: &VmContext,
    ) -> Self {
        Self {
            code,
            pc,
            entry,
            registers,
            context,
            result: None,
            error: None,
        }
    }

    pub(crate) fn finish(
        self,
        status: u64,
    ) -> Result<DispatchTransition, crate::machine::NativeDispatchError> {
        match status {
            NATIVE_DISPATCH_OK => self.result.ok_or_else(|| {
                crate::machine::NativeDispatchError::Physical(
                    "native bridge returned without a transition".into(),
                )
            }),
            NATIVE_DISPATCH_SEMANTIC_ERROR => self.error.map_or_else(
                || {
                    Err(crate::machine::NativeDispatchError::Physical(
                        "native bridge returned an empty semantic error".into(),
                    ))
                },
                |error| Err(crate::machine::NativeDispatchError::Semantic(error)),
            ),
            _ => Err(crate::machine::NativeDispatchError::Physical(
                "native bridge returned an invalid status".into(),
            )),
        }
    }
}

/// Synchronous context for a generated fused-region entry. The region is
/// bounded by the build-time operation list; no runtime table can grow and the
/// bridge never executes an operation not present in that list.
#[repr(C)]
pub(crate) struct NativeRegionContext<'a> {
    code: crate::machine::CodeView<'a>,
    pc: usize,
    operations: &'static [crate::ir::Opcode],
    registers: *mut crate::register_file::RegisterFile,
    context: *const VmContext,
    result: Option<DispatchTransition>,
    error: Option<VmError>,
}

impl<'a> NativeRegionContext<'a> {
    pub(crate) fn new(
        code: crate::machine::CodeView<'a>,
        pc: usize,
        operations: &'static [crate::ir::Opcode],
        registers: &mut crate::register_file::RegisterFile,
        context: &VmContext,
    ) -> Self {
        Self {
            code,
            pc,
            operations,
            registers,
            context,
            result: None,
            error: None,
        }
    }

    pub(crate) fn finish(
        self,
        status: u64,
    ) -> Result<DispatchTransition, crate::machine::NativeDispatchError> {
        match status {
            NATIVE_DISPATCH_OK => self.result.ok_or_else(|| {
                crate::machine::NativeDispatchError::Physical(
                    "native region bridge returned without a transition".into(),
                )
            }),
            NATIVE_DISPATCH_SEMANTIC_ERROR => self.error.map_or_else(
                || {
                    Err(crate::machine::NativeDispatchError::Physical(
                        "native region bridge returned an empty semantic error".into(),
                    ))
                },
                |error| Err(crate::machine::NativeDispatchError::Semantic(error)),
            ),
            _ => Err(crate::machine::NativeDispatchError::Physical(
                "native region bridge returned an invalid status".into(),
            )),
        }
    }
}

const NATIVE_DISPATCH_OK: u64 = 1;
const NATIVE_DISPATCH_SEMANTIC_ERROR: u64 = 2;

// Keep the CPS fast path shallow enough that the large transition frame does
// not accumulate on long-running ARM64 loops. The stack-safe segment takes
// over at this boundary and preserves the same canonical transitions.
const DISPATCH_RECURSION_LIMIT: usize = 64;

/// The only code pointer embedded in the generated all-opcode trampoline.
/// It does no operation selection and owns no values; it simply invokes the
/// same baseline handler/control path used by the non-native driver.
pub(crate) extern "C" fn native_dispatch_bridge(raw: *mut std::ffi::c_void) -> u64 {
    if raw.is_null() {
        return 0;
    }
    // The context is created and consumed synchronously by NativeDispatchPlan;
    // no callee can retain the erased lifetime or pointer beyond this call.
    let dispatch = unsafe { &mut *(raw.cast::<NativeDispatchContext<'static>>()) };
    let result = unsafe {
        run_baseline_instruction(
            dispatch.code,
            dispatch.pc,
            dispatch.entry,
            &mut *dispatch.registers,
            &*dispatch.context,
        )
    };
    match result {
        Ok(transition) => {
            dispatch.result = Some(transition);
            NATIVE_DISPATCH_OK
        }
        Err(error) => {
            dispatch.error = Some(error);
            NATIVE_DISPATCH_SEMANTIC_ERROR
        }
    }
}

/// Execute a build-admitted straight-line region through the same canonical
/// handlers used by the ordinary baseline path. Every instruction and
/// transition is checked before it is accepted; a changed quickened opcode,
/// branch, completion, or malformed window returns the physical-failure code
/// so the caller retries the complete ordinary path exactly once.
pub(crate) extern "C" fn native_region_bridge(raw: *mut std::ffi::c_void) -> u64 {
    if raw.is_null() {
        return 0;
    }
    let region = unsafe { &mut *(raw.cast::<NativeRegionContext<'static>>()) };
    // Validate the complete window before invoking even the first canonical
    // handler.  This is what makes an Unknown/quickened mismatch an atomic
    // fallback: the caller can retry the whole span without replaying a
    // prefix that may already have mutated registers or heap state.
    for (offset, expected) in region.operations.iter().copied().enumerate() {
        let pc = match region.pc.checked_add(offset) {
            Some(pc) => pc,
            None => return 0,
        };
        let Some(instruction) = region.code.instruction(pc) else {
            return 0;
        };
        if instruction.opcode != expected {
            return 0;
        }
    }

    let mut last = None;
    for (offset, expected) in region.operations.iter().copied().enumerate() {
        let pc = match region.pc.checked_add(offset) {
            Some(pc) => pc,
            None => return 0,
        };
        let Some(instruction) = region.code.instruction(pc) else {
            return 0;
        };
        if instruction.opcode != expected {
            return 0;
        }
        let entry = crate::machine::BaselineEntry {
            instruction,
            handler: instruction.opcode.handler(),
            control: instruction.opcode.control_operands(instruction),
        };
        let transition = unsafe {
            run_baseline_instruction(
                region.code,
                pc,
                entry,
                &mut *region.registers,
                &*region.context,
            )
        };
        let transition = match transition {
            Ok(transition) => transition,
            Err(error) => {
                region.error = Some(error);
                return NATIVE_DISPATCH_SEMANTIC_ERROR;
            }
        };
        let final_op = offset + 1 == region.operations.len();
        let expected_next = pc + 1;
        if !final_op
            && (transition.target != DispatchTarget::Callee(expected_next)
                || transition
                    .completion
                    .as_ref()
                    .is_some_and(|completion| {
                        !matches!(completion, crate::completion::Completion::Normal)
                    }))
        {
            return 0;
        }
        last = Some(transition);
    }
    region.result = last;
    NATIVE_DISPATCH_OK
}

fn run_ops(
    ops: &[Op],
    registers: &mut crate::register_file::RegisterFile,
    context: &VmContext,
) -> Result<Value, VmError> {
    completion_result(run_ops_completion(ops, registers, context)?)
}

fn run_ops_completion(
    ops: &[Op],
    registers: &mut crate::register_file::RegisterFile,
    context: &VmContext,
) -> Result<crate::completion::Completion, VmError> {
    Ok(run_ops_completion_step(ops, registers, context)?.completion)
}

pub(crate) fn execute_ops_from(
    ops: &[Op],
    start: usize,
    registers: &mut crate::register_file::RegisterFile,
    context: &VmContext,
    environment: Rc<crate::environment::Environment>,
) -> Result<(crate::completion::Completion, usize), VmError> {
    let _context_guard = ContextGuard::install(context);
    let _global_guard = GlobalObjectGuard::install();
    let _environment_guard = crate::locals::EnvironmentGuard::install(environment);
    let step = run_ops_completion_step_from(ops, start, registers, context)?;
    Ok((step.completion, step.next))
}

pub(crate) fn execute_code_from(
    code: crate::machine::CodeView<'_>,
    start: usize,
    registers: &mut crate::register_file::RegisterFile,
    context: &VmContext,
    environment: Rc<crate::environment::Environment>,
) -> Result<(crate::completion::Completion, usize), VmError> {
    let _context_guard = ContextGuard::install(context);
    let _global_guard = GlobalObjectGuard::install();
    let _environment_guard = crate::locals::EnvironmentGuard::install(environment);
    let step = run_code_completion_step_from(code, start, registers, context)?;
    Ok((step.completion, step.next))
}

/// Execute a function through its predecoded baseline plan. The plan removes
/// bytecode decoding from the hot path, but deliberately reuses the same
/// `run_instruction` handlers and transition machinery as the interpreter.
/// Any mismatch falls back to the canonical interpreter step.
pub(crate) fn execute_baseline_code_from(
    code: crate::machine::CodeView<'_>,
    plan: &crate::machine::BaselinePlan,
    start: usize,
    registers: &mut crate::register_file::RegisterFile,
    context: &VmContext,
    environment: Rc<crate::environment::Environment>,
) -> Result<(crate::completion::Completion, usize), VmError> {
    let _context_guard = ContextGuard::install(context);
    let _global_guard = GlobalObjectGuard::install();
    let _environment_guard = crate::locals::EnvironmentGuard::install(environment);
    let step = run_baseline_completion_step_from(code, plan, start, registers, context)?;
    Ok((step.completion, step.next))
}

fn run_ops_completion_step(
    ops: &[Op],
    registers: &mut crate::register_file::RegisterFile,
    context: &VmContext,
) -> Result<CompletionStep, VmError> {
    run_ops_completion_step_from(ops, 0, registers, context)
}

fn run_ops_completion_step_from(
    ops: &[Op],
    start: usize,
    registers: &mut crate::register_file::RegisterFile,
    context: &VmContext,
) -> Result<CompletionStep, VmError> {
    let executable = crate::machine::ExecutableCode::from_ops(ops.to_vec());
    run_code_completion_step_from(executable.code(), start, registers, context)
}

#[inline]
fn run_code_completion_step_from(
    code: crate::machine::CodeView<'_>,
    start: usize,
    registers: &mut crate::register_file::RegisterFile,
    context: &VmContext,
) -> Result<CompletionStep, VmError> {
    let mut dispatch = DispatchState {
        code,
        registers,
        context,
        tier_owner: None,
    };
    // The stable-Rust backend cannot promise a machine tail call. Enter the
    // stack-safe callee-directed loop directly so ordinary interpreter work
    // does not accumulate one native frame per bytecode before reaching the
    // safepoint segment.
    dispatch_segment(&mut dispatch, start)
}

/// Execute an interpreter function with its tier owner attached.  The owner
/// lets back-edge retirement compile the baseline plan and transfer the
/// current invocation without waiting for a function return.
pub(crate) fn execute_function_code_from(
    code: crate::machine::CodeView<'_>,
    owner: &crate::machine::FunctionCode,
    start: usize,
    registers: &mut crate::register_file::RegisterFile,
    context: &VmContext,
    environment: Rc<crate::environment::Environment>,
) -> Result<(crate::completion::Completion, usize), VmError> {
    let _context_guard = ContextGuard::install(context);
    let _global_guard = GlobalObjectGuard::install();
    let _environment_guard = crate::locals::EnvironmentGuard::install(environment);
    let mut dispatch = DispatchState {
        code,
        registers,
        context,
        tier_owner: Some(owner),
    };
    let step = dispatch_segment(&mut dispatch, start)?;
    Ok((step.completion, step.next))
}

/// Owner-aware single step for callers that already installed the VM/TLS
/// guards around a whole drive. Keeping this separate from
/// `execute_function_code_from` avoids reinstalling three guards on every
/// retired instruction in the top-level driver.
pub(crate) fn execute_function_code_step_from(
    code: crate::machine::CodeView<'_>,
    owner: &crate::machine::FunctionCode,
    start: usize,
    registers: &mut crate::register_file::RegisterFile,
    context: &VmContext,
) -> Result<(crate::completion::Completion, usize), VmError> {
    let mut dispatch = DispatchState {
        code,
        registers,
        context,
        tier_owner: Some(owner),
    };
    let step = dispatch_segment(&mut dispatch, start)?;
    Ok((step.completion, step.next))
}

/// Baseline counterpart of [`execute_function_code_step_from`].
pub(crate) fn execute_baseline_code_step_from(
    code: crate::machine::CodeView<'_>,
    plan: &crate::machine::BaselinePlan,
    start: usize,
    registers: &mut crate::register_file::RegisterFile,
    context: &VmContext,
) -> Result<(crate::completion::Completion, usize), VmError> {
    let step = run_baseline_completion_step_from(code, plan, start, registers, context)?;
    Ok((step.completion, step.next))
}

/// Baseline driver entry that accounts every retired compact instruction for
/// the owning function. The unowned entry above is used by fragments that do
/// not participate in tiering; keeping the profile hook here prevents the
/// baseline loop from collapsing an entire long-running body into one sample.
pub(crate) fn execute_baseline_code_step_from_with_owner(
    code: crate::machine::CodeView<'_>,
    plan: &crate::machine::BaselinePlan,
    start: usize,
    registers: &mut crate::register_file::RegisterFile,
    context: &VmContext,
    owner: &crate::machine::FunctionCode,
) -> Result<(crate::completion::Completion, usize), VmError> {
    // Count locally while the baseline body is executing.  Publishing an
    // optimizing plan from inside the body would change the tier observed by
    // the still-running baseline loop and can make a one-step fragment spin.
    // Retire the completed step only after its transition is fully known.
    let mut retired = 0u64;
    let mut count = || retired = retired.saturating_add(1);
    let result = run_baseline_completion_step_from_with_hook(
        code,
        plan,
        start,
        registers,
        context,
        &mut count,
    );
    owner.retire(retired);
    let step = result?;
    Ok((step.completion, step.next))
}

/// Execute through the Rust optimizing view.  The optimized view specializes
/// only the already-admitted native leaves; the first unsupported operation
/// deliberately hands the remainder to the baseline driver, preserving the
/// complete canonical handler and all observable completion behavior.
pub(crate) fn execute_optimized_code_step_from(
    code: crate::machine::CodeView<'_>,
    plan: &crate::machine::OptimizingPlan,
    baseline: &crate::machine::BaselinePlan,
    start: usize,
    registers: &mut crate::register_file::RegisterFile,
    context: &VmContext,
) -> Result<(crate::completion::Completion, usize), VmError> {
    if plan.len() != code.len() || baseline.len() != code.len() {
        return execute_baseline_code_step_from(code, baseline, start, registers, context);
    }
    let Some(entry) = plan.entry(start) else {
        return Ok((crate::completion::Completion::Normal, code.len()));
    };
    // A fused region owns a complete, contiguous operation window.  Its
    // bridge validates the whole window before executing; a mismatch is a
    // physical miss and falls through to the ordinary per-instruction path,
    // never to a partially executed region.
    if let Some(native) = entry.native_region.as_ref() {
        match native
            .borrow_mut()
            .execute(code, start, registers, context)
        {
            Ok(transition) => {
                crate::execution_trace::event(crate::execution_trace::Event::LeafHit);
                let next = match transition.target {
                    DispatchTarget::Callee(next) => next,
                    DispatchTarget::Exit => transition.next_pc,
                };
                let completion = transition
                    .completion
                    .unwrap_or(crate::completion::Completion::Normal);
                return Ok((completion, next));
            }
            Err(crate::machine::NativeDispatchError::Semantic(error)) => {
                return completion_step_after_error(registers, error, start + 1)
                    .map(|step| (step.completion, step.next));
            }
            Err(crate::machine::NativeDispatchError::Physical(_)) => {
                crate::execution_trace::leaf_rejection("optimizing_native_region");
            }
        }
    }
    let instruction = entry.baseline.instruction;
    let _decode_guard = crate::execution_trace::compact(instruction.opcode);
    crate::execution_trace::compact_site(code, start);
    crate::execution_trace::operands(instruction);
    // These operations are pure and have no dynamic semantic gateway. Their
    // compact operands are already validated by the canonical lowering, so a
    // direct optimized step can avoid the baseline handler call entirely.
    match instruction.opcode {
        crate::ir::Opcode::LoadConst => {
            let Some((_, value)) = code.constant_at(start) else {
                return execute_baseline_code_step_from(code, baseline, start, registers, context);
            };
            write_value(registers, instruction.a, value.into());
            return Ok((crate::completion::Completion::Normal, start + 1));
        }
        crate::ir::Opcode::Return => {
            if let Ok(value) = read_register(registers, instruction.a) {
                return Ok((
                    crate::completion::Completion::Return(value),
                    start + 1,
                ));
            }
        }
        crate::ir::Opcode::Jump => {
            return Ok((
                crate::completion::Completion::Normal,
                usize::from(instruction.a),
            ));
        }
        crate::ir::Opcode::JumpIfFalse => {
            if let Some(truthy) = registers.word_truthiness(usize::from(instruction.a)) {
                return Ok((
                    crate::completion::Completion::Normal,
                    if truthy {
                        start + 1
                    } else {
                        usize::from(instruction.b)
                    },
                ));
            }
        }
        _ => {}
    }
    if instruction.opcode == crate::ir::Opcode::Move && instruction.flags == 0 {
        if let Some(native) = entry.native_move.as_ref() {
            if let Some(source) = registers.word_ptr(usize::from(instruction.b)) {
                if let Ok(bits) = native.borrow_mut().execute(source) {
                    if registers
                        .write_tagged_bits(usize::from(instruction.a), bits)
                        .is_some()
                    {
                        crate::execution_trace::event(crate::execution_trace::Event::LeafHit);
                        return Ok((crate::completion::Completion::Normal, start + 1));
                    }
                }
            }
            crate::execution_trace::leaf_rejection("optimizing_native_move");
        }
        if copy_register(registers, instruction.a, instruction.b).is_ok() {
            return Ok((crate::completion::Completion::Normal, start + 1));
        }
    }
    if instruction.opcode == crate::ir::Opcode::GetN && instruction.flags == 0 {
        if let Some(native) = entry.native_property.as_ref() {
            let slot = registers
                .read_object(usize::from(instruction.b))
                .filter(|object| {
                    !object.has_replacement()
                        && !object.is_dictionary()
                        && !object.is_realm_global()
                        && !object.is_script_global_view()
                        && !object.has_regexp_internal_slot()
                })
                .and_then(|object| {
                    let key = code
                        .metadata_at(start)
                        .and_then(|metadata| metadata.name.as_deref())?;
                    quickened_own_slot_data(code, start, &object, key)
                        .map(|word| word as *const crate::register_file::SlotWord)
                });
            if let Some(slot) = slot {
                if let Some(site) = code.quickening_site(start) {
                    let site = site.borrow();
                    if let Ok(bits) = native.borrow_mut().execute(slot, &site) {
                        if registers
                            .write_tagged_bits(usize::from(instruction.a), bits)
                            .is_some()
                        {
                            crate::execution_trace::event(crate::execution_trace::Event::LeafHit);
                            return Ok((crate::completion::Completion::Normal, start + 1));
                        }
                    }
                }
            }
            crate::execution_trace::leaf_rejection("optimizing_native_property");
        }
    }
    if let Some(native) = entry.native_binary.as_ref() {
        let operands = if instruction.opcode == crate::ir::Opcode::AddConst {
            registers
                .read_number(usize::from(instruction.b))
                .and_then(|lhs| {
                    let crate::ops::Constant::Number(rhs) = code.constant(instruction.c)? else {
                        return None;
                    };
                    Some((lhs, *rhs))
                })
        } else {
            registers.read_number_pair(
                usize::from(instruction.b),
                usize::from(instruction.c),
            )
        };
        if let Some((lhs, rhs)) = operands {
            if let Ok(result) = native.borrow_mut().execute(lhs, rhs) {
                write_value(registers, instruction.a, Value::Number(result));
                crate::execution_trace::event(crate::execution_trace::Event::LeafHit);
                return Ok((crate::completion::Completion::Normal, start + 1));
            }
        }
        crate::execution_trace::leaf_rejection("optimizing_native_execution");
    }
    if let Some(native) = entry.native_dispatch.as_ref() {
        match native
            .borrow_mut()
            .execute(code, start, entry.baseline, registers, context)
        {
            Ok(transition) => {
                crate::execution_trace::event(crate::execution_trace::Event::LeafHit);
                let next = match transition.target {
                    DispatchTarget::Callee(next) => next,
                    DispatchTarget::Exit => transition.next_pc,
                };
                let completion = transition
                    .completion
                    .unwrap_or(crate::completion::Completion::Normal);
                return Ok((completion, next));
            }
            Err(crate::machine::NativeDispatchError::Semantic(error)) => {
                return completion_step_after_error(registers, error, start + 1)
                    .map(|step| (step.completion, step.next));
            }
            Err(crate::machine::NativeDispatchError::Physical(_)) => {
                crate::execution_trace::leaf_rejection("optimizing_native_dispatch");
            }
        }
    }
    // Property leaves need the live shape site and ownership-retaining write
    // path used by the baseline driver; keep that edge in one implementation.
    execute_baseline_code_step_from(code, baseline, start, registers, context)
}

fn run_baseline_completion_step_from(
    code: crate::machine::CodeView<'_>,
    plan: &crate::machine::BaselinePlan,
    start: usize,
    registers: &mut crate::register_file::RegisterFile,
    context: &VmContext,
) -> Result<CompletionStep, VmError> {
    let mut no_profile = || {};
    run_baseline_completion_step_from_with_hook(
        code,
        plan,
        start,
        registers,
        context,
        &mut no_profile,
    )
}

fn run_baseline_completion_step_from_with_hook<F: FnMut()>(
    code: crate::machine::CodeView<'_>,
    plan: &crate::machine::BaselinePlan,
    start: usize,
    registers: &mut crate::register_file::RegisterFile,
    context: &VmContext,
    on_instruction: &mut F,
) -> Result<CompletionStep, VmError> {
    if plan.len() != code.len() {
        return run_code_completion_step_from(code, start, registers, context);
    }
    let mut pc = start;
    loop {
        let Some(entry) = plan.entry(pc) else {
            return completion_step_after_transition(
                registers,
                crate::completion::Completion::Normal,
                code.len(),
            );
        };
        let Some(instruction) = plan.instruction(pc) else {
            return completion_step_after_transition(
                registers,
                crate::completion::Completion::Normal,
                code.len(),
            );
        };
        on_instruction();
        // Build-generated Number binary stencils are real baseline machine-code
        // leaves. Admission is structural and the numeric guard is checked
        // before entering them; every other value uses the canonical handler.
        // The lookup is per instruction, so a proven leaf can remain native
        // inside an otherwise ordinary function body rather than requiring a
        // whole-function shape match.
        if instruction.opcode == crate::ir::Opcode::GetN && instruction.flags == 0 {
            if let Some(native) = plan.native_property_at(pc) {
                let slot = registers
                    .read_object(usize::from(instruction.b))
                    .filter(|object| {
                        !object.has_replacement()
                            && !object.is_dictionary()
                            && !object.is_realm_global()
                            && !object.is_script_global_view()
                            && !object.has_regexp_internal_slot()
                    })
                    .and_then(|object| {
                        let key = code
                            .metadata_at(pc)
                            .and_then(|metadata| metadata.name.as_deref())?;
                        quickened_own_slot_data(code, pc, &object, key)
                            .map(|word| word as *const crate::register_file::SlotWord)
                });
                if let Some(slot) = slot {
                    if let Some(site) = code.quickening_site(pc) {
                        let site = site.borrow();
                        if let Ok(bits) = native.borrow_mut().execute(slot, &site) {
                            if registers
                                .write_tagged_bits(usize::from(instruction.a), bits)
                                .is_some()
                            {
                                crate::execution_trace::event(
                                    crate::execution_trace::Event::LeafHit,
                                );
                                pc += 1;
                                continue;
                            }
                        }
                    }
                    crate::execution_trace::leaf_rejection("native_property");
                }
            }
        }
        if instruction.opcode == crate::ir::Opcode::Move && instruction.flags == 0 {
        if let Some(native) = plan.native_move_at(pc) {
                if let Some(source) = registers.word_ptr(usize::from(instruction.b)) {
                    if let Ok(bits) = native.borrow_mut().execute(source) {
                        if registers
                            .write_tagged_bits(usize::from(instruction.a), bits)
                            .is_some()
                        {
                            crate::execution_trace::event(crate::execution_trace::Event::LeafHit);
                            pc += 1;
                            continue;
                        }
                    }
                }
                crate::execution_trace::leaf_rejection("native_move");
            }
        }
        if let Some(native) = plan.native_binary_at(pc) {
            let _leaf = crate::execution_trace::leaf_compact(instruction.opcode);
            let operands = if instruction.opcode == crate::ir::Opcode::AddConst {
                registers
                    .read_number(usize::from(instruction.b))
                    .and_then(|lhs| {
                        let crate::ops::Constant::Number(rhs) = code.constant(instruction.c)?
                        else {
                            return None;
                        };
                        Some((lhs, *rhs))
                    })
            } else {
                registers.read_number_pair(
                    usize::from(instruction.b),
                    usize::from(instruction.c),
                )
            };
            if let Some((lhs, rhs)) = operands {
                if let Ok(result) = native.borrow_mut().execute(lhs, rhs) {
                    crate::execution_trace::event(crate::execution_trace::Event::LeafHit);
                    let value = Value::Number(result);
                    write_value(registers, instruction.a, value);
                    pc += 1;
                    continue;
                }
                crate::execution_trace::leaf_rejection("native_execution");
            } else {
                crate::execution_trace::event(crate::execution_trace::Event::LeafReject);
                crate::execution_trace::leaf_rejection("number_guard");
            }
        }
        if skip_index_coercible(code, pc, instruction, registers) {
            pc += 1;
            continue;
        }
        // Every compact opcode has a generated executable entry on supported
        // targets. Specialized leaves above get first refusal; this generic
        // trampoline then invokes the canonical handler with the exact same
        // transition object. A physical failure falls back, while a semantic
        // error is propagated once (never retried, which could duplicate an
        // observable effect).
        let transition = match plan.native_dispatch_at(pc) {
            Some(native) => match native
                .borrow_mut()
                .execute(code, pc, entry, registers, context)
            {
                Ok(transition) => {
                    crate::execution_trace::event(crate::execution_trace::Event::LeafHit);
                    transition
                }
                Err(crate::machine::NativeDispatchError::Semantic(error)) => {
                    return completion_step_after_error(registers, error, pc + 1)
                }
                Err(crate::machine::NativeDispatchError::Physical(_)) => {
                    crate::execution_trace::leaf_rejection("native_dispatch");
                    match run_baseline_instruction(code, pc, entry, registers, context) {
                        Ok(transition) => transition,
                        Err(error) => {
                            return completion_step_after_error(registers, error, pc + 1)
                        }
                    }
                }
            },
            None => match run_baseline_instruction(code, pc, entry, registers, context) {
                Ok(transition) => transition,
                Err(error) => return completion_step_after_error(registers, error, pc + 1),
            },
        };
        let next = match transition.target {
            DispatchTarget::Callee(next) => next,
            DispatchTarget::Exit => transition.next_pc,
        };
        if let Some(completion) = transition
            .completion
            .filter(|value| !matches!(value, crate::completion::Completion::Normal))
        {
            return completion_step_after_transition(registers, completion, next);
        }
        match transition.target {
            DispatchTarget::Callee(_) => pc = next,
            DispatchTarget::Exit => {
                return completion_step_after_transition(
                    registers,
                    crate::completion::Completion::Normal,
                    next,
                )
            }
        }
    }
}

/// State shared by each continuation in the hot dispatch chain.
///
/// Passing one stable state pointer keeps the code/frame/context facts in one
/// ABI argument across recursive continuation calls. This is the safe Rust
/// approximation of Deegen's fixed-register pinning: ownership and observable
/// semantics are unchanged, and LLVM remains responsible for allocation.
struct DispatchState<'code, 'state> {
    code: crate::machine::CodeView<'code>,
    registers: &'state mut crate::register_file::RegisterFile,
    context: &'state VmContext,
    tier_owner: Option<&'state crate::machine::FunctionCode>,
}

/// Execute one continuation by calling the target supplied by its predecessor.
/// The entry/exit shim above owns no mutable program counter and never derives a
/// successor after a handler returns.  This is the interpreter's CPS-shaped
/// path; each normal transition immediately invokes the next callee.
#[inline(always)]
fn dispatch_callee<'code, 'state>(
    state: &mut DispatchState<'code, 'state>,
    pc: usize,
    depth: usize,
) -> Result<CompletionStep, VmError> {
    // Rust has no portable guaranteed-tail-call ABI.  Keep the direct callee
    // chain bounded, then enter a safepoint segment that consumes only the
    // already-produced targets.  This prevents an adversarial backward branch
    // from turning continuation depth into unbounded native stack growth.
    if depth == DISPATCH_RECURSION_LIMIT {
        return dispatch_segment(state, pc);
    }
    let Some(instruction) = state.code.instruction(pc) else {
        return completion_step_after_transition(
            state.registers,
            crate::completion::Completion::Normal,
            state.code.len(),
        );
    };
    if skip_index_coercible(state.code, pc, instruction, state.registers) {
        if let Some(step) = maybe_osr_switch(
            state.code,
            state.tier_owner,
            pc,
            pc + 1,
            state.registers,
            state.context,
        )? {
            return Ok(step);
        }
        return dispatch_callee(state, pc + 1, depth + 1);
    }
    #[cfg(not(feature = "execution-trace"))]
    let result = match run_instruction_hot(state.code, pc, instruction, state.registers) {
        Some(result) => result,
        None => run_instruction(state.code, pc, instruction, state.registers, state.context),
    };
    #[cfg(feature = "execution-trace")]
    let result = run_instruction(state.code, pc, instruction, state.registers, state.context);
    let transition = match result {
        Ok(transition) => transition,
        Err(error) => {
            if let Some(owner) = state.tier_owner {
                owner.retire(1);
            }
            return completion_step_after_error(state.registers, error, pc + 1);
        }
    };
    let next = match transition.target {
        DispatchTarget::Callee(next) => next,
        DispatchTarget::Exit => transition.next_pc,
    };
    // Retire before observing completion so calls, returns, and other exits
    // contribute to the profile. `maybe_osr_switch` can only admit a plan at
    // an actual back-edge, so ordinary exits are merely counted.
    if let Some(step) = maybe_osr_switch(
        state.code,
        state.tier_owner,
        pc,
        next,
        state.registers,
        state.context,
    )? {
        return Ok(step);
    }
    if let Some(completion) = transition
        .completion
        .filter(|value| !matches!(value, crate::completion::Completion::Normal))
    {
        return completion_step_after_transition(state.registers, completion, next);
    }
    match transition.target {
        DispatchTarget::Callee(_) => dispatch_callee(state, next, depth + 1),
        DispatchTarget::Exit => completion_step_after_transition(
            state.registers,
            crate::completion::Completion::Normal,
            next,
        ),
    }
}

/// Stack-safe safepoint shim for targets that cannot be represented by a
/// guaranteed machine-level tail call on stable Rust. It never computes a
/// successor: every next offset comes from the handler's `DispatchTarget`.
fn dispatch_segment<'code, 'state>(
    state: &mut DispatchState<'code, 'state>,
    start: usize,
) -> Result<CompletionStep, VmError> {
    let mut pc = start;
    loop {
        let Some(instruction) = state.code.instruction(pc) else {
            return completion_step_after_transition(
                state.registers,
                crate::completion::Completion::Normal,
                state.code.len(),
            );
        };
        if skip_index_coercible(state.code, pc, instruction, state.registers) {
            if let Some(step) = maybe_osr_switch(
                state.code,
                state.tier_owner,
                pc,
                pc + 1,
                state.registers,
                state.context,
            )? {
                return Ok(step);
            }
            pc += 1;
            continue;
        }
        #[cfg(not(feature = "execution-trace"))]
        let result = match run_instruction_hot(state.code, pc, instruction, state.registers) {
            Some(result) => result,
            None => run_instruction(state.code, pc, instruction, state.registers, state.context),
        };
        #[cfg(feature = "execution-trace")]
        let result = run_instruction(state.code, pc, instruction, state.registers, state.context);
        let transition = match result {
            Ok(transition) => transition,
            Err(error) => {
                if let Some(owner) = state.tier_owner {
                    owner.retire(1);
                }
                return completion_step_after_error(state.registers, error, pc + 1);
            }
        };
        let next = match transition.target {
            DispatchTarget::Callee(next) => next,
            DispatchTarget::Exit => transition.next_pc,
        };
        if let Some(step) = maybe_osr_switch(
            state.code,
            state.tier_owner,
            pc,
            next,
            state.registers,
            state.context,
        )? {
            return Ok(step);
        }
        if let Some(completion) = transition
            .completion
            .filter(|value| !matches!(value, crate::completion::Completion::Normal))
        {
            return completion_step_after_transition(state.registers, completion, next);
        }
        match transition.target {
            DispatchTarget::Callee(_) => pc = next,
            DispatchTarget::Exit => {
                return completion_step_after_transition(
                    state.registers,
                    crate::completion::Completion::Normal,
                    next,
                )
            }
        }
    }
}

/// Inline the representation-only operations that cannot call host code or
/// suspend. Keeping these out of the general opcode dispatcher removes a
/// function call and enum dispatch from every ordinary local/arithmetic step;
/// any operation with observable behavior falls through to its canonical
/// handler below.
#[cfg(not(feature = "execution-trace"))]
#[inline(always)]
fn run_instruction_hot(
    code: crate::machine::CodeView<'_>,
    pc: usize,
    instruction: crate::ir::Instruction,
    registers: &mut crate::register_file::RegisterFile,
) -> Option<Result<DispatchTransition, VmError>> {
    use crate::ir::Opcode;
    let result = match instruction.opcode {
        Opcode::LoadConst => code
            .constant_at(pc)
            .ok_or(VmError::MissingReturn)
            .map(|(_, value)| {
                write_value(registers, instruction.a, value.into());
                handler_transition(pc, None)
            }),
        Opcode::Move => {
            let copied = if instruction.flags == 1 {
                crate::locals::move_proven_local(
                    registers,
                    instruction.a,
                    instruction.b,
                    instruction.c,
                )
            } else {
                copy_register(registers, instruction.a, instruction.b)
            };
            copied.map(|_| handler_transition(pc, None))
        }
        Opcode::LoadLocal => crate::locals::load_proven(registers, instruction.a, instruction.b)
            .map(|_| handler_transition(pc, None)),
        Opcode::LoadLocalChecked => {
            let name = code
                .metadata_at(pc)
                .and_then(|metadata| metadata.name.as_deref())
                .unwrap_or("binding");
            crate::locals::load_checked(registers, instruction.a, instruction.b, name)
                .map(|_| handler_transition(pc, None))
        }
        Opcode::StoreLocal => crate::locals::store_proven(registers, instruction.a, instruction.b)
            .map(|_| handler_transition(pc, None)),
        Opcode::Add | Opcode::Sub | Opcode::Mul | Opcode::Div | Opcode::Binary => {
            let operator = instruction.opcode.numeric_operator().or_else(|| {
                crate::ir::compact_binary_operator(instruction.flags)
            })?;
            vm_arithmetic::execute_binary(
                registers,
                instruction.a,
                operator,
                instruction.b,
                instruction.c,
            )
            .map(|_| handler_transition(pc, None))
        }
        Opcode::Return => read_register(registers, instruction.a)
            .map(|value| handler_transition(pc, Some(crate::completion::Completion::Return(value)))),
        _ => return None,
    };
    Some(result)
}

/// Count one retired interpreter instruction and transfer to the newly
/// compiled baseline plan only at an admitted hot back-edge.
fn maybe_osr_switch(
    code: crate::machine::CodeView<'_>,
    tier_owner: Option<&crate::machine::FunctionCode>,
    pc: usize,
    next: usize,
    registers: &mut crate::register_file::RegisterFile,
    context: &VmContext,
) -> Result<Option<CompletionStep>, VmError> {
    let Some(owner) = tier_owner else {
        return Ok(None);
    };
    if owner.retire_at(pc) != crate::machine::TierTransition::CompileBaseline
        || !owner.is_osr_entry(pc)
    {
        return Ok(None);
    }
    owner.record_osr_transfer();
    let plan = owner
        .baseline_plan()
        .ok_or_else(|| VmError::EvalError("baseline tier compiled without a plan".into()))?;
    execute_baseline_code_step_from_with_owner(code, &plan, next, registers, context, owner)
        .map(|(completion, next)| CompletionStep { completion, next })
        .map(Some)
}

/// A dynamic indexed access still owns its complete property semantics, but
/// an adjacent `RequireObjectCoercible` is redundant once the object word is
/// known non-nullish.  The match is deliberately local: labels, callbacks,
/// and every non-indexed operation remain on the ordinary slow path.
#[inline(always)]
fn skip_index_coercible(
    code: crate::machine::CodeView<'_>,
    pc: usize,
    instruction: crate::ir::Instruction,
    registers: &crate::register_file::RegisterFile,
) -> bool {
    if instruction.opcode != crate::ir::Opcode::Slow {
        return false;
    }
    let Some(crate::ops::Op::RequireObjectCoercible { src }) = code.cold(instruction) else {
        return false;
    };
    let Some(next) = code.instruction(pc + 1) else {
        return false;
    };
    let object = match next.opcode {
        crate::ir::Opcode::AGetI => next.b,
        crate::ir::Opcode::ASetI => next.a,
        _ => return false,
    };
    *src == object && registers.word_is_non_nullish(usize::from(object)) == Some(true)
}

#[inline(always)]
fn run_object_index_get(
    code: crate::machine::CodeView<'_>,
    pc: usize,
    registers: &mut crate::register_file::RegisterFile,
    destination: u16,
    object_register: u16,
    index: usize,
) -> bool {
    let Some(object) = registers.read_object(usize::from(object_register)) else {
        return false;
    };
    if object.has_replacement() || object.is_realm_global() || object.is_script_global_view() {
        return false;
    }
    let key = index.to_string();
    let Some(value) = quickened_own_get_data(code, pc, object, &key) else {
        return false;
    };
    write_value(registers, destination, value);
    true
}

#[inline(always)]
fn run_object_index_set(
    code: crate::machine::CodeView<'_>,
    pc: usize,
    registers: &mut crate::register_file::RegisterFile,
    object_register: u16,
    index: usize,
    number: f64,
) -> Result<bool, VmError> {
    crate::properties::execute_set_index_number_cached(
        registers,
        object_register,
        index,
        number,
        code.quickening_site(pc),
    )
}

#[inline(always)]
fn write_named_cached_payload(
    registers: &mut crate::register_file::RegisterFile,
    destination: u16,
    payload: NamedCachedPayload,
) {
    match payload {
        NamedCachedPayload::Word(word) => {
            unsafe { &*word }.copy_to_register(registers, usize::from(destination))
        }
        NamedCachedPayload::Cell(cell) => unsafe { &*cell }
            .with_word(|word| registers.write_owned(usize::from(destination), word)),
        NamedCachedPayload::Value(value) => write_value(registers, destination, value),
    }
}

/// The explicit transition returned by the catalog-selected dispatch boundary.
///
/// Keeping the next program counter beside the completion makes dispatch a
/// data-flow boundary: the driver consumes this value rather than inferring a
/// successor after every handler call.  Branch and jump roles are decoded from
/// the same operation facts before the semantic handler runs.
#[derive(Debug)]
pub(crate) struct DispatchTransition {
    pub(crate) next_pc: usize,
    pub(crate) completion: Option<crate::completion::Completion>,
    /// Callee-directed continuation target.  The ordinary driver consumes the
    /// target at the frame boundary; handlers never infer a successor by
    /// mutating a driver-owned pc.
    pub(crate) target: DispatchTarget,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) enum DispatchTarget {
    /// The handler supplies the next operation's entry offset.  The driver is
    /// only an entry/exit shim and does not derive this successor.
    Callee(usize),
    Exit,
}

impl DispatchTransition {
    #[inline(always)]
    fn next(next_pc: usize) -> Self {
        Self {
            next_pc,
            completion: None,
            target: DispatchTarget::Callee(next_pc),
        }
    }
}

#[inline(always)]
fn handler_transition(
    pc: usize,
    completion: Option<crate::completion::Completion>,
) -> DispatchTransition {
    let target = completion
        .as_ref()
        .filter(|value| !matches!(value, crate::completion::Completion::Normal))
        .map_or(DispatchTarget::Callee(pc + 1), |_| DispatchTarget::Exit);
    DispatchTransition {
        next_pc: pc + 1,
        completion,
        target,
    }
}

fn run_instruction(
    code: crate::machine::CodeView<'_>,
    pc: usize,
    instruction: crate::ir::Instruction,
    registers: &mut crate::register_file::RegisterFile,
    context: &VmContext,
) -> Result<DispatchTransition, VmError> {
    let _decode_guard = crate::execution_trace::compact(instruction.opcode);
    crate::execution_trace::compact_site(code, pc);
    crate::execution_trace::operands(instruction);
    if let Some(transition) = run_control_operands(
        instruction.opcode.control_operands(instruction),
        pc,
        registers,
    )? {
        return Ok(transition);
    }
    instruction
        .opcode
        .dispatch(code, pc, instruction, registers, context)
}

#[inline(always)]
fn run_baseline_instruction(
    code: crate::machine::CodeView<'_>,
    pc: usize,
    entry: crate::machine::BaselineEntry,
    registers: &mut crate::register_file::RegisterFile,
    context: &VmContext,
) -> Result<DispatchTransition, VmError> {
    let _decode_guard = crate::execution_trace::compact(entry.instruction.opcode);
    crate::execution_trace::compact_site(code, pc);
    crate::execution_trace::operands(entry.instruction);
    if let Some(transition) = run_control_operands(entry.control, pc, registers)? {
        return Ok(transition);
    }
    (entry.handler)(code, pc, entry.instruction, registers, context)
}

#[inline(always)]
fn run_control_operands(
    control: crate::ir::ControlOperands,
    pc: usize,
    registers: &mut crate::register_file::RegisterFile,
) -> Result<Option<DispatchTransition>, VmError> {
    match control {
        crate::ir::ControlOperands::Jump { target } => {
            Ok(Some(DispatchTransition::next(usize::from(target))))
        }
        crate::ir::ControlOperands::Branch { condition, target } => {
            let truthy = registers
                .word_truthiness(usize::from(condition))
                .map_or_else(
                    || read_register(registers, condition).map(|value| is_truthy(&value)),
                    Ok,
                )?;
            Ok(Some(DispatchTransition::next(if truthy {
                pc + 1
            } else {
                usize::from(target)
            })))
        }
        _ => Ok(None),
    }
}

#[inline(always)]
pub(crate) fn run_load_const(
    code: crate::machine::CodeView<'_>,
    pc: usize,
    instruction: crate::ir::Instruction,
    registers: &mut crate::register_file::RegisterFile,
    _context: &VmContext,
) -> Result<DispatchTransition, VmError> {
    let (_, value) = code.constant_at(pc).ok_or(VmError::MissingReturn)?;
    write_value(registers, instruction.a, value.into());
    Ok(handler_transition(pc, None))
}

#[inline(always)]
pub(crate) fn run_move(
    _code: crate::machine::CodeView<'_>,
    pc: usize,
    instruction: crate::ir::Instruction,
    registers: &mut crate::register_file::RegisterFile,
    _context: &VmContext,
) -> Result<DispatchTransition, VmError> {
    if instruction.flags == 1 {
        crate::locals::move_proven_local(registers, instruction.a, instruction.b, instruction.c)?;
    } else {
        copy_register(registers, instruction.a, instruction.b)?;
    }
    Ok(handler_transition(pc, None))
}

#[inline(always)]
pub(crate) fn run_arithmetic(
    _code: crate::machine::CodeView<'_>,
    pc: usize,
    instruction: crate::ir::Instruction,
    registers: &mut crate::register_file::RegisterFile,
    _context: &VmContext,
) -> Result<DispatchTransition, VmError> {
    let operator = instruction
        .opcode
        .numeric_operator()
        .ok_or_else(|| VmError::EvalError("arithmetic opcode has no numeric operator".into()))?;
    vm_arithmetic::execute_binary(
        registers,
        instruction.a,
        operator,
        instruction.b,
        instruction.c,
    )?;
    Ok(handler_transition(pc, None))
}

#[inline(always)]
pub(crate) fn run_compact_add_const(
    code: crate::machine::CodeView<'_>,
    pc: usize,
    instruction: crate::ir::Instruction,
    registers: &mut crate::register_file::RegisterFile,
    _context: &VmContext,
) -> Result<DispatchTransition, VmError> {
    let source = read_register(registers, instruction.b)?;
    let constant = code
        .constant(instruction.c)
        .ok_or_else(|| VmError::EvalError("missing compact constant".into()))?;
    let constant: crate::value::Value = constant.into();
    let (left, right) = if instruction.add_const_is_left() {
        (constant, source)
    } else {
        (source, constant)
    };
    let result = vm_arithmetic::evaluate_binary(&left, &right, crate::ops::BinaryOp::Add)?;
    write_value(registers, instruction.a, result);
    Ok(handler_transition(pc, None))
}

#[inline(always)]
pub(crate) fn run_compact_numeric_update(
    _code: crate::machine::CodeView<'_>,
    pc: usize,
    instruction: crate::ir::Instruction,
    registers: &mut crate::register_file::RegisterFile,
    _context: &VmContext,
) -> Result<DispatchTransition, VmError> {
    vm_arithmetic::execute_numeric_update(
        registers,
        instruction.a,
        instruction.b,
        instruction.flags != 0,
    )?;
    Ok(handler_transition(pc, None))
}

#[inline(always)]
pub(crate) fn run_local(
    _code: crate::machine::CodeView<'_>,
    pc: usize,
    instruction: crate::ir::Instruction,
    registers: &mut crate::register_file::RegisterFile,
    _context: &VmContext,
) -> Result<DispatchTransition, VmError> {
    crate::locals::load_proven(registers, instruction.a, instruction.b)?;
    Ok(handler_transition(pc, None))
}

#[inline(always)]
pub(crate) fn run_load_local_checked(
    code: crate::machine::CodeView<'_>,
    pc: usize,
    instruction: crate::ir::Instruction,
    registers: &mut crate::register_file::RegisterFile,
    _context: &VmContext,
) -> Result<DispatchTransition, VmError> {
    let name = code
        .metadata_at(pc)
        .and_then(|metadata| metadata.name.as_deref())
        .unwrap_or("binding");
    crate::locals::load_checked(registers, instruction.a, instruction.b, name)?;
    Ok(handler_transition(pc, None))
}

#[inline(always)]
pub(crate) fn run_store_local_checked(
    code: crate::machine::CodeView<'_>,
    pc: usize,
    instruction: crate::ir::Instruction,
    registers: &mut crate::register_file::RegisterFile,
    _context: &VmContext,
) -> Result<DispatchTransition, VmError> {
    let name = code
        .metadata_at(pc)
        .and_then(|metadata| metadata.name.as_deref())
        .unwrap_or("binding");
    crate::locals::check_initialized(instruction.a, name)?;
    crate::locals::store(registers, instruction.a, instruction.b)?;
    Ok(handler_transition(pc, None))
}

#[inline(always)]
pub(crate) fn run_store_local(
    _code: crate::machine::CodeView<'_>,
    pc: usize,
    instruction: crate::ir::Instruction,
    registers: &mut crate::register_file::RegisterFile,
    _context: &VmContext,
) -> Result<DispatchTransition, VmError> {
    crate::locals::store_proven(registers, instruction.a, instruction.b)?;
    Ok(handler_transition(pc, None))
}

#[inline(always)]
pub(crate) fn run_init_local(
    _code: crate::machine::CodeView<'_>,
    pc: usize,
    instruction: crate::ir::Instruction,
    registers: &mut crate::register_file::RegisterFile,
    _context: &VmContext,
) -> Result<DispatchTransition, VmError> {
    crate::locals::store(registers, instruction.a, instruction.b)?;
    crate::locals::initialize(instruction.a);
    Ok(handler_transition(pc, None))
}

#[inline(always)]
pub(crate) fn run_update_local(
    _code: crate::machine::CodeView<'_>,
    pc: usize,
    instruction: crate::ir::Instruction,
    registers: &mut crate::register_file::RegisterFile,
    _context: &VmContext,
) -> Result<DispatchTransition, VmError> {
    crate::locals::update(
        registers,
        instruction.a,
        instruction.b,
        instruction.c,
        instruction.flags != 0,
    )?;
    Ok(handler_transition(pc, None))
}

#[inline(always)]
pub(crate) fn run_binary_instruction(
    _code: crate::machine::CodeView<'_>,
    pc: usize,
    instruction: crate::ir::Instruction,
    registers: &mut crate::register_file::RegisterFile,
    _context: &VmContext,
) -> Result<DispatchTransition, VmError> {
    let operator = crate::ir::compact_binary_operator(instruction.flags)
        .ok_or_else(|| VmError::EvalError("invalid compact binary operator".into()))?;
    vm_arithmetic::execute_binary(
        registers,
        instruction.a,
        operator,
        instruction.b,
        instruction.c,
    )?;
    Ok(handler_transition(pc, None))
}

#[inline(always)]
pub(crate) fn run_return(
    _code: crate::machine::CodeView<'_>,
    pc: usize,
    instruction: crate::ir::Instruction,
    registers: &mut crate::register_file::RegisterFile,
    _context: &VmContext,
) -> Result<DispatchTransition, VmError> {
    read_register(registers, instruction.a)
        .map(crate::completion::Completion::Return)
        .map(|completion| handler_transition(pc, Some(completion)))
}

#[inline(always)]
pub(crate) fn run_compact_call(
    code: crate::machine::CodeView<'_>,
    pc: usize,
    instruction: crate::ir::Instruction,
    registers: &mut crate::register_file::RegisterFile,
    _context: &VmContext,
) -> Result<DispatchTransition, VmError> {
    let argument = [instruction.c];
    let spreads = [false];
    let (arguments, spreads) = if instruction.flags == 0 {
        (&[][..], &[][..])
    } else if instruction.flags == 1 {
        (&argument[..], &spreads[..])
    } else {
        return Err(VmError::EvalError("invalid compact call arity".into()));
    };
    if let Some(completion) = quickened_direct_call(
        code,
        pc,
        registers,
        instruction.a,
        instruction.b,
        arguments,
        spreads,
    )? {
        return Ok(handler_transition(pc, Some(completion)));
    }
    run_compact_call_fallback(registers, instruction.a, instruction.b, arguments, spreads)
        .map(|completion| handler_transition(pc, completion))
}

/// Complete call semantics live out of line so the normal compact-call loop
/// contains only arity decoding and the reusable callable guard.  This is a
/// layout hint, not a semantic shortcut: every non-eligible callee still
/// enters the ordinary call gateway.
#[cold]
#[inline(never)]
fn run_compact_call_fallback(
    registers: &mut crate::register_file::RegisterFile,
    destination: u16,
    callee: u16,
    arguments: &[u16],
    spreads: &[bool],
) -> Result<Option<crate::completion::Completion>, VmError> {
    crate::vm::vm_ops::execute_call(registers, destination, callee, None, arguments, spreads)
        .map(Some)
}

/// Use a callable-identity IC only after the callee is a direct, synchronous
/// function. Installation falls through to `execute_call`, so the first
/// observation retains the complete call/throw/suspension protocol.
#[inline(always)]
fn quickened_direct_call(
    code: crate::machine::CodeView<'_>,
    pc: usize,
    registers: &mut crate::register_file::RegisterFile,
    destination: u16,
    callee_register: u16,
    argument_registers: &[u16],
    spreads: &[bool],
) -> Result<Option<crate::completion::Completion>, VmError> {
    let callee = read_register(registers, callee_register)?;
    let crate::value::Value::Function(function) = &callee else {
        return Ok(None);
    };
    if !crate::functions::direct_call_eligible(function) {
        return Ok(None);
    }
    let Some(site) = code.quickening_site(pc) else {
        return Ok(None);
    };
    let decision = site.borrow_mut().observe_callable(function);
    if !matches!(
        decision,
        crate::quickening::QuickeningDecision::GuardedCallHit
    ) {
        return Ok(None);
    }
    let arguments =
        crate::vm::vm_ops::collect_call_arguments(registers, argument_registers, spreads)?;
    let receiver =
        crate::with_scope::receiver_for_callable(&callee).unwrap_or(crate::value::Value::Undefined);
    let value = crate::functions::execute_direct(function, &receiver, &arguments)?;
    write_value(registers, destination, value);
    crate::execution_trace::kernel("CallIC", false);
    Ok(Some(crate::completion::Completion::Normal))
}

#[inline(always)]
pub(crate) fn run_compact_get_named(
    code: crate::machine::CodeView<'_>,
    pc: usize,
    instruction: crate::ir::Instruction,
    registers: &mut crate::register_file::RegisterFile,
    _context: &VmContext,
) -> Result<DispatchTransition, VmError> {
    if instruction.flags == 0 {
        let object = read_register(registers, instruction.b)?;
        let key = code
            .metadata_at(pc)
            .and_then(|metadata| metadata.name.as_deref())
            .ok_or(VmError::MissingReturn)?;
        if let Some(value) = quickened_own_get(code, pc, &object, key) {
            write_value(registers, instruction.a, value);
            return Ok(handler_transition(pc, None));
        }
    }
    let metadata = code.metadata_at(pc).ok_or(VmError::MissingReturn)?;
    let key = metadata.name.as_deref().ok_or(VmError::MissingReturn)?;
    if instruction.flags == crate::ir::GETN_GLOBAL_FLAG {
        let global = crate::vm::current_global_object();
        let value =
            crate::vm::get_global_named_property_result(&global, key, &metadata.named_cache)?;
        write_value(registers, instruction.a, value);
        return Ok(handler_transition(pc, None));
    }
    if instruction.flags == crate::ir::GETN_LENGTH_FLAG {
        if let Some(array) = registers
            .read_array(usize::from(instruction.b))
            .filter(|array| crate::locals::array_word_is_current(array))
        {
            if array.is_arguments() {
                registers.write(usize::from(instruction.a), array.arguments_length_value());
                crate::execution_trace::event(crate::execution_trace::Event::NamedPropertyHit);
                return Ok(handler_transition(pc, None));
            }
            registers.write_number(usize::from(instruction.a), array.header_length() as f64);
            crate::execution_trace::event(crate::execution_trace::Event::NamedPropertyHit);
            crate::execution_trace::named_property_word("own", "number");
            return Ok(handler_transition(pc, None));
        }
    }
    let object = registers.read_object(usize::from(instruction.b));
    let global_like = object
        .as_ref()
        .is_some_and(|object| object.is_realm_global() || object.is_script_global_view());
    if let Some(payload) = object
        .as_ref()
        .filter(|_| !global_like)
        .and_then(|object| get_named_cached_payload(object, key, &metadata.named_cache))
    {
        // The source register roots pointer-backed payloads through the
        // complete retain-before-replace copy, including dst=src.
        write_named_cached_payload(registers, instruction.a, payload);
        return Ok(handler_transition(pc, None));
    }
    let object = read_register(registers, instruction.b)?;
    run_compact_get_named_fallback(
        registers,
        instruction.a,
        &object,
        key,
        &metadata.named_cache,
    )
    .map(|completion| handler_transition(pc, completion))
}

#[cold]
#[inline(never)]
fn run_compact_get_named_fallback(
    registers: &mut crate::register_file::RegisterFile,
    destination: u16,
    object: &crate::value::Value,
    key: &str,
    cache: &std::cell::Cell<u64>,
) -> Result<Option<crate::completion::Completion>, VmError> {
    let value = get_named_property_result(object, key, cache)?;
    write_value(registers, destination, value);
    Ok(None)
}

#[inline(always)]
pub(crate) fn run_compact_set_named(
    code: crate::machine::CodeView<'_>,
    pc: usize,
    instruction: crate::ir::Instruction,
    registers: &mut crate::register_file::RegisterFile,
    _context: &VmContext,
) -> Result<DispatchTransition, VmError> {
    let metadata = code.metadata_at(pc).ok_or(VmError::MissingReturn)?;
    let key = metadata.name.as_deref().ok_or(VmError::MissingReturn)?;
    crate::properties::execute_set_named_cached(
        registers,
        instruction.a,
        key,
        instruction.b,
        instruction.flags != 0,
        &metadata.named_cache,
        code.quickening_site(pc),
    )?;
    Ok(handler_transition(pc, None))
}

#[inline(always)]
pub(crate) fn run_compact_call_named(
    code: crate::machine::CodeView<'_>,
    pc: usize,
    instruction: crate::ir::Instruction,
    registers: &mut crate::register_file::RegisterFile,
    _context: &VmContext,
) -> Result<DispatchTransition, VmError> {
    if instruction.flags != 0 {
        crate::methods::execute_registered(registers, instruction, code, pc)
            .map(|completion| handler_transition(pc, completion))
    } else {
        let metadata = code.metadata_at(pc).ok_or(VmError::MissingReturn)?;
        let key = metadata.name.as_deref().ok_or(VmError::MissingReturn)?;
        crate::methods::execute_named(
            registers,
            instruction,
            key,
            &metadata.named_cache,
            code.quickening_site(pc),
        )
        .map(|completion| handler_transition(pc, completion))
    }
}

#[inline(always)]
pub(crate) fn run_compact_set_index(
    code: crate::machine::CodeView<'_>,
    pc: usize,
    instruction: crate::ir::Instruction,
    registers: &mut crate::register_file::RegisterFile,
    _context: &VmContext,
) -> Result<DispatchTransition, VmError> {
    let index = registers.read_array_index(usize::from(instruction.b));
    let number = registers.read_number(usize::from(instruction.c));
    if let Some((index, number)) = index.zip(number) {
        if run_object_index_set(code, pc, registers, instruction.a, index, number)? {
            crate::execution_trace::event(crate::execution_trace::Event::NamedPropertySetHit);
            return Ok(handler_transition(pc, None));
        }
    }
    let stored = index.zip(number).is_some_and(|(index, number)| {
        registers
            .read_array(usize::from(instruction.a))
            .filter(|array| crate::locals::array_word_is_current(array))
            .is_some_and(|array| {
                array.set_existing_f64(index, number)
                    || array.append_preallocated_f64(index, number)
            })
    });
    if stored {
        crate::execution_trace::event(crate::execution_trace::Event::PackedArraySet);
        return Ok(handler_transition(pc, None));
    }
    crate::execution_trace::packed_miss("other");
    run_compact_set_index_fallback(
        registers,
        instruction.a,
        instruction.b,
        instruction.c,
        instruction.flags != 0,
    )
    .map(|completion| handler_transition(pc, completion))
}

#[cold]
#[inline(never)]
fn run_compact_set_index_fallback(
    registers: &mut crate::register_file::RegisterFile,
    object: u16,
    key: u16,
    source: u16,
    strict: bool,
) -> Result<Option<crate::completion::Completion>, VmError> {
    crate::properties::execute_set_property(
        registers,
        &crate::ops::Op::SetPropertyDynamic {
            object,
            key,
            src: source,
            strict,
        },
    )?;
    Ok(None)
}

#[inline(always)]
pub(crate) fn run_compact_get_property(
    code: crate::machine::CodeView<'_>,
    pc: usize,
    instruction: crate::ir::Instruction,
    registers: &mut crate::register_file::RegisterFile,
    _context: &VmContext,
) -> Result<DispatchTransition, VmError> {
    let object = read_register(registers, instruction.b)?;
    let key = read_register(registers, instruction.c)?;
    let key = crate::properties::dynamic_property_key(&key)?;
    if let Some(value) = quickened_own_get(code, pc, &object, &key) {
        write_value(registers, instruction.a, value);
        return Ok(handler_transition(pc, None));
    }
    run_compact_get_property_fallback(registers, instruction.a, &object, &key)
        .map(|completion| handler_transition(pc, completion))
}

/// The complete property gateway (coercion, accessors, proxies, and throws)
/// is deliberately outlined from the guarded own-slot probe above.
#[cold]
#[inline(never)]
fn run_compact_get_property_fallback(
    registers: &mut crate::register_file::RegisterFile,
    destination: u16,
    object: &crate::value::Value,
    key: &str,
) -> Result<Option<crate::completion::Completion>, VmError> {
    let value = get_property_result(object, key)?;
    write_value(registers, destination, value);
    Ok(None)
}

/// Use the generated shape site only after the complete ordinary lookup has
/// established a plain own-data slot. Installation deliberately falls through
/// for this access; only a subsequent guarded hit may bypass the gateway.
#[inline(always)]
fn quickened_own_get(
    code: crate::machine::CodeView<'_>,
    pc: usize,
    object: &crate::value::Value,
    key: &str,
) -> Option<crate::value::Value> {
    let crate::value::Value::Object(data) = object else {
        return None;
    };
    quickened_own_get_data(code, pc, data, key)
}

#[inline(always)]
fn quickened_own_get_data(
    code: crate::machine::CodeView<'_>,
    pc: usize,
    data: &crate::value::ObjectData,
    key: &str,
) -> Option<crate::value::Value> {
    quickened_own_slot_data(code, pc, data, key).map(crate::register_file::SlotWord::load)
}

/// Return the physical slot only after the same shape, descriptor, and value
/// checks used by the complete own-property fast path.  The native baseline
/// leaf consumes this pointer; installation on a miss still falls through to
/// complete property semantics.
#[inline(always)]
fn quickened_own_slot_data<'a>(
    code: crate::machine::CodeView<'a>,
    pc: usize,
    data: &'a crate::value::ObjectData,
    key: &str,
) -> Option<&'a crate::register_file::SlotWord> {
    if data.has_replacement() || data.is_dictionary() {
        return None;
    }
    if let Some((opcode, cached_shape, cached_property, cached_slot)) = code.quickened_state(pc) {
        let property = crate::identity::property_key_id(key);
        if cached_shape == data.semantic_layout_id() && cached_property == property.0 {
            if let Some(word) = crate::vm::cached_plain_own_word(
                data,
                key,
                cached_shape,
                cached_slot,
            ) {
                let valid = !matches!(
                    word.load(),
                    crate::value::Value::BindingCell(_) | crate::value::Value::WeakFunction(_)
                );
                if valid {
                    return Some(word);
                }
            }
        }
        // A rewritten opcode is only a guarded fast path. Any shape/key or
        // descriptor mismatch restores the canonical opcode and re-enters the
        // complete generic-IC path below.
        if matches!(
            opcode,
            crate::ir::Opcode::GetPropertyQuickened
                | crate::ir::Opcode::GetNQuickened
                | crate::ir::Opcode::AGetIQuickened
        ) {
            code.dequicken_instruction(pc);
        }
    }
    let site = code.quickening_site(pc)?;
    // The named-property cache already interns this object's canonical
    // property layout.  Reuse that derived identity for the IC guard instead
    // of rescanning every visible property to recompute an FNV shape hash.
    // Internal descriptor/deletion entries remain part of the layout identity,
    // so mutation invalidation still forces the complete semantic path.
    let shape = crate::identity::ShapeId(data.semantic_layout_id());
    let property = crate::identity::property_key_id(key);
    let mut site = site.borrow_mut();
    if let Some(cached_slot) = site.probe_shape(shape, property) {
        // The cached slot is the λᵢ state. Validate only the cheap physical
        // storage/name lookup; descriptor/accessor metadata is also part of
        // the proof. A descriptor object may mutate in place without changing
        // this receiver's layout, so re-use the shared cache validator rather
        // than exposing a stale raw data slot.
        if let Some(word) = crate::vm::cached_plain_own_word(data, key, shape.0, cached_slot) {
            let valid = !matches!(
                word.load(),
                crate::value::Value::BindingCell(_) | crate::value::Value::WeakFunction(_)
            );
            if valid {
                if let Some(quickened_opcode) = code.instruction(pc).and_then(|instruction| {
                    match instruction.opcode {
                        crate::ir::Opcode::GetProperty => {
                            Some(crate::ir::Opcode::GetPropertyQuickened)
                        }
                        crate::ir::Opcode::GetN => Some(crate::ir::Opcode::GetNQuickened),
                        crate::ir::Opcode::AGetI => Some(crate::ir::Opcode::AGetIQuickened),
                        _ => None,
                    }
                }) {
                    code.quicken_instruction(
                        pc,
                        quickened_opcode,
                        shape.0,
                        property.0,
                        cached_slot,
                    );
                }
                return Some(word);
            }
            site.invalidate_shape(shape);
            return None;
        }
        site.invalidate_shape(shape);
        return None;
    }
    // Only a miss derives the slot. Installation falls through for this
    // access; a later hit can now bypass `proven_own_slot` entirely.
    let current_slot = crate::vm::proven_own_slot(data, key)?;
    match site.observe(shape, property, u32::try_from(current_slot).ok()?) {
        crate::quickening::QuickeningDecision::InstallGuard { .. }
        | crate::quickening::QuickeningDecision::Fallback
        | crate::quickening::QuickeningDecision::GuardedCallHit
        | crate::quickening::QuickeningDecision::InstallCallGuard
        | crate::quickening::QuickeningDecision::GuardedHit { .. } => None,
    }
}

#[inline(always)]
pub(crate) fn run_compact_get_index(
    code: crate::machine::CodeView<'_>,
    pc: usize,
    instruction: crate::ir::Instruction,
    registers: &mut crate::register_file::RegisterFile,
    _context: &VmContext,
) -> Result<DispatchTransition, VmError> {
    let index = registers.read_array_index(usize::from(instruction.c));
    if index
        .is_some_and(|index| {
            run_object_index_get(code, pc, registers, instruction.a, instruction.b, index)
        })
    {
        return Ok(handler_transition(pc, None));
    }
    let raw_array = registers.read_array(usize::from(instruction.b));
    let array = raw_array.filter(|array| crate::locals::array_word_is_current(array));
    if let Some((array, index)) = array.filter(|array| array.is_packed_ordinary()).zip(index) {
        if let Some(number) = array.dense_number_at(index) {
            crate::execution_trace::event(crate::execution_trace::Event::PackedArrayGet);
            registers.write_number(usize::from(instruction.a), number);
            return Ok(handler_transition(pc, None));
        }
        if let Some(value) = array.dense_value_at(index) {
            crate::execution_trace::event(crate::execution_trace::Event::PackedArrayGet);
            write_value(registers, instruction.a, value);
            return Ok(handler_transition(pc, None));
        }
    }
    if let Some(array) = array.filter(|array| !array.is_packed_ordinary()) {
        crate::execution_trace::packed_kind_miss(array.kind());
        return run_compact_get_property(code, pc, instruction, registers, _context);
    }
    let reason = if array.is_none() {
        crate::execution_trace::packed_kind_reason(if raw_array.is_some() {
            "stale"
        } else {
            "non_array"
        });
        None
    } else if index.is_none() {
        Some("other")
    } else if index.expect("checked index") >= array.expect("checked array").logical_len() {
        Some("oob")
    } else {
        Some("hole")
    };
    if let Some(reason) = reason {
        crate::execution_trace::packed_miss(reason);
    }
    run_compact_get_index_fallback(code, pc, instruction, registers, _context)
}

#[cold]
#[inline(never)]
fn run_compact_get_index_fallback(
    code: crate::machine::CodeView<'_>,
    pc: usize,
    instruction: crate::ir::Instruction,
    registers: &mut crate::register_file::RegisterFile,
    context: &VmContext,
) -> Result<DispatchTransition, VmError> {
    run_compact_get_property(code, pc, instruction, registers, context)
}

/// Execute the fused `obj[index++]` spelling while preserving evaluation
/// order: capture the old key, update the index register, then perform the
/// ordinary property read. The generic property gateway remains authoritative
/// for coercion, accessors, proxies, and exceptions.
#[inline(always)]
pub(crate) fn run_compact_get_index_inc(
    code: crate::machine::CodeView<'_>,
    pc: usize,
    instruction: crate::ir::Instruction,
    registers: &mut crate::register_file::RegisterFile,
    _context: &VmContext,
) -> Result<DispatchTransition, VmError> {
    let old_index = read_register(registers, instruction.c)?;
    vm_arithmetic::execute_numeric_update(registers, instruction.c, instruction.c, false)?;
    let object = read_register(registers, instruction.b)?;
    let key = crate::properties::dynamic_property_key(&old_index)?;
    if let Some(value) = quickened_own_get(code, pc, &object, &key) {
        write_value(registers, instruction.a, value);
        return Ok(handler_transition(pc, None));
    }
    run_compact_get_index_inc_fallback(registers, instruction.a, &object, &key)
        .map(|completion| handler_transition(pc, completion))
}

#[cold]
#[inline(never)]
fn run_compact_get_index_inc_fallback(
    registers: &mut crate::register_file::RegisterFile,
    destination: u16,
    object: &crate::value::Value,
    key: &str,
) -> Result<Option<crate::completion::Completion>, VmError> {
    let value = get_property_result(object, key)?;
    write_value(registers, destination, value);
    Ok(None)
}

#[inline(always)]
pub(crate) fn run_instruction_fallback(
    code: crate::machine::CodeView<'_>,
    _pc: usize,
    instruction: crate::ir::Instruction,
    registers: &mut crate::register_file::RegisterFile,
    context: &VmContext,
) -> Result<DispatchTransition, VmError> {
    use crate::ir::Opcode;
    match instruction.opcode {
        Opcode::Slow => enter_slow_path(code, _pc, instruction, registers, context),
        // ForI is a reserved residual-loop encoding.  Lowering currently
        // keeps counted loops as structured `Op::Loop`; if a serialized
        // residual carries that operation in cold metadata, execute the same
        // complete loop gateway rather than manufacturing a partial kernel.
        Opcode::ForI => {
            let Some(operation) = code.cold(instruction) else {
                return Err(VmError::EvalError(
                    "ForI compact instruction is missing structured loop state".into(),
                ));
            };
            match operation {
                crate::ops::Op::Loop { .. } => crate::loops::execute(registers, operation)
                    .map(Some)
                    .map(|completion| handler_transition(_pc, completion)),
                _ => Err(VmError::EvalError(
                    "ForI compact instruction has invalid structured loop state".into(),
                )),
            }
        }
        _ => Err(VmError::EvalError("unsupported compact instruction".into())),
    }
}

/// Enter the canonical slow-path body as a one-way VM transition.
///
/// Deegen's `EnterSlowPath` is CPS-shaped: the fast component does not call
/// into a value-returning helper and then decide what to do with its result.
/// Rust cannot promise a machine-level tail-call ABI, so the equivalent here
/// is an explicit `DispatchTransition` whose callee target is consumed by the
/// outer dispatch shim.  The body is `#[cold]`/`#[inline(never)]`, shared by
/// every stencil and never copied into rendered bytes; all misses therefore
/// retain the complete ordinary semantics at one out-of-line entry point.
#[cold]
#[inline(never)]
fn enter_slow_path(
    code: crate::machine::CodeView<'_>,
    pc: usize,
    instruction: crate::ir::Instruction,
    registers: &mut crate::register_file::RegisterFile,
    context: &VmContext,
) -> Result<DispatchTransition, VmError> {
    let operation = code
        .cold(instruction)
        .ok_or_else(|| VmError::EvalError("missing cold instruction".into()))?;
    run_op(registers, operation, context).map(|completion| handler_transition(pc, completion))
}

fn error_completion(error: VmError) -> Result<crate::completion::Completion, VmError> {
    crate::completion::Completion::from_vm_error(error)
}

#[cold]
fn completion_step_after_error(
    registers: &mut crate::register_file::RegisterFile,
    error: VmError,
    next: usize,
) -> Result<CompletionStep, VmError> {
    crate::vm::flush_global_declaration_batch(registers);
    error_completion(error).map(|completion| CompletionStep { completion, next })
}

#[cold]
fn completion_step_after_transition(
    registers: &mut crate::register_file::RegisterFile,
    completion: crate::completion::Completion,
    next: usize,
) -> Result<CompletionStep, VmError> {
    crate::vm::flush_global_declaration_batch(registers);
    Ok(CompletionStep { completion, next })
}

pub(crate) fn completion_result(
    completion: crate::completion::Completion,
) -> Result<Value, VmError> {
    completion.into_vm_error()
}

struct GlobalObjectGuard {
    previous: Option<ObjectProperties>,
    restore: bool,
    realm: Option<RealmId>,
}
include!("vm_global.rs");

pub(crate) fn bare_call_receiver(
    function: &crate::value::FunctionValue,
    this_value: &Value,
) -> Value {
    if matches!(
        function.kind,
        FunctionKind::Ordinary | FunctionKind::Method | FunctionKind::Generator
    ) && matches!(function.strictness, FunctionStrictness::Sloppy)
    {
        let realm = function
            .properties
            .borrow()
            .iter()
            .find_map(|(key, value)| {
                (key == "\0realm")
                    .then(|| crate::vm::realm_id_for_intrinsic_receiver(Some(value)))
                    .flatten()
            })
            .or_else(|| crate::vm::realm_id_for_global_value(&function.captures.get(0)));
        let global = realm
            .and_then(|realm| {
                crate::vm::with_realm(realm, || Some(crate::vm::current_global_object()))
            })
            .flatten()
            .unwrap_or_else(crate::vm::current_global_object);
        return to_object_value_in_realm(this_value, &global);
    }
    this_value.clone()
}

fn to_object_value_in_realm(this_value: &Value, global: &Value) -> Value {
    let Some(realm) = crate::vm::realm_id_for_global_value(global) else {
        return to_object_value(this_value);
    };
    crate::vm::with_realm(realm, || to_object_value(this_value))
        .unwrap_or_else(|| to_object_value(this_value))
}

fn to_object_value(this_value: &Value) -> Value {
    match this_value {
        Value::WeakFunction(function) => to_object_value(&function.value()),
        Value::Object(_)
        | Value::Array(_)
        | Value::Function(_)
        | Value::BoundFunction(_)
        | Value::Builtin(_)
        | Value::ObjectAlias(_)
        | Value::Proxy(_)
        | Value::Promise(_)
        | Value::Map(_)
        | Value::Set(_)
        | Value::ArrayBuffer(_)
        | Value::DataView(_)
        | Value::Float32Array(_)
        | Value::Float64Array(_)
        | Value::Int8Array(_)
        | Value::Int16Array(_)
        | Value::Int32Array(_)
        | Value::Uint8Array(_)
        | Value::Uint8ClampedArray(_)
        | Value::Uint16Array(_)
        | Value::Uint32Array(_)
        | Value::BigInt64Array(_)
        | Value::BigUint64Array(_)
        | Value::Iterator(_)
        | Value::Generator(_)
        | Value::HostCapability(_) => this_value.clone(),
        Value::Null | Value::Undefined => crate::vm::current_global_object(),
        Value::Number(_) => boxed_primitive(this_value, crate::ops::Builtin::Number),
        Value::Boolean(_) => boxed_primitive(this_value, crate::ops::Builtin::Boolean),
        Value::String(_) => boxed_primitive(this_value, crate::ops::Builtin::String),
        Value::StringUnits(_) => boxed_primitive(this_value, crate::ops::Builtin::String),
        Value::BigInt(_) => boxed_primitive(this_value, crate::ops::Builtin::BigInt),
        Value::BindingCell(_) => this_value.clone(),
    }
}

fn boxed_primitive(value: &Value, constructor: crate::ops::Builtin) -> Value {
    let prototype = match constructor {
        Builtin::Boolean => Builtin::BooleanPrototype,
        Builtin::String => Builtin::StringPrototype,
        Builtin::BigInt => Builtin::BigIntPrototype,
        Builtin::Number => Builtin::NumberPrototype,
        _ => Builtin::ObjectPrototype,
    };
    let mut properties = vec![
        ("_value".to_string(), value.clone()),
        (
            "\0prototype".to_string(),
            crate::vm::realm_intrinsic(prototype),
        ),
    ];
    if constructor != Builtin::Number {
        properties.push((
            "constructor".to_string(),
            crate::vm::realm_intrinsic(constructor),
        ));
    }
    Value::Object(std::rc::Rc::new(crate::value::ObjectData::new(properties)))
}

pub fn execute_builtin_with_receiver(
    builtin: Builtin,
    arguments: &[Value],
    receiver: Option<&Value>,
) -> Result<Value, VmError> {
    if let Some(result) = stateful_builtin(builtin, receiver, arguments) {
        return result;
    }
    if builtin == Builtin::Print {
        return execute_print(arguments);
    }
    if is_object_special(builtin) {
        return crate::builtins::object::execute_special(builtin, receiver, arguments);
    }
    if let Some(result) = define_builtin(builtin, arguments) {
        return result;
    }
    if let Some(result) = early_dispatch(builtin, receiver, arguments) {
        return result;
    }
    if is_data_view_builtin(builtin) {
        return execute_data_view_builtin(builtin, receiver, arguments);
    }
    if is_shared_array_buffer_builtin(builtin) {
        return execute_shared_array_buffer_builtin(builtin, receiver, arguments);
    }
    if let Builtin::HostCapability(kind) = builtin {
        return vm_ops::execute_host_capability(kind, receiver, arguments);
    }
    match builtin {
        _ if is_function_builtin(builtin) => {
            crate::functions::function_builtin(builtin, receiver, arguments)
        }
        _ if is_simple_builtin(builtin) => execute_simple_builtin(builtin, arguments, receiver),
        _ => vm_ops::execute_builtin_tail(builtin, arguments, receiver),
    }
}

fn is_shared_array_buffer_builtin(builtin: Builtin) -> bool {
    matches!(
        builtin,
        Builtin::ArrayBufferByteLengthGetter
            | Builtin::ArrayBufferDetachedGetter
            | Builtin::ArrayBufferImmutableGetter
            | Builtin::ArrayBufferMaxByteLengthGetter
            | Builtin::ArrayBufferResizableGetter
            | Builtin::SharedArrayBufferByteLengthGetter
            | Builtin::SharedArrayBufferGrow
            | Builtin::ArrayBufferSlice
            | Builtin::SharedArrayBufferSlice
            | Builtin::SharedArrayBufferGrowableGetter
            | Builtin::SharedArrayBufferMaxByteLengthGetter
    )
}

fn define_builtin(builtin: Builtin, arguments: &[Value]) -> Option<Result<Value, VmError>> {
    match builtin {
        Builtin::ObjectDefineProperty => Some(crate::builtins::define_property(arguments)),
        Builtin::ObjectDefineProperties => Some(crate::builtins::define_properties(arguments)),
        _ => None,
    }
}

fn stateful_builtin(
    builtin: Builtin,
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Option<Result<Value, VmError>> {
    match builtin {
        Builtin::GeneratorNext => Some(crate::generator::next(receiver, arguments)),
        Builtin::AsyncGeneratorNext => Some(crate::generator::async_next(receiver, arguments)),
        Builtin::GeneratorReturn => Some(crate::generator::return_(receiver, arguments)),
        Builtin::AsyncGeneratorReturn => Some(crate::generator::async_return(receiver, arguments)),
        Builtin::GeneratorThrow => Some(crate::generator::throw(receiver, arguments)),
        Builtin::AsyncGeneratorThrow => Some(crate::generator::async_throw(receiver, arguments)),
        Builtin::AsyncIteratorDispose => Some(crate::generator::async_dispose(receiver)),
        Builtin::AsyncIteratorDisposeFulfilled => Some(Ok(Value::Undefined)),
        Builtin::ProxyRevoke => Some(crate::proxy::revoke(receiver)),
        Builtin::Math => Some(Err(not_callable())),
        builtin @ (Builtin::AtomicsAdd
        | Builtin::AtomicsAnd
        | Builtin::AtomicsOr
        | Builtin::AtomicsSub
        | Builtin::AtomicsXor
        | Builtin::AtomicsCompareExchange) => {
            Some(crate::atomics::execute(builtin, receiver, arguments))
        }
        Builtin::AtomicsIsLockFree => Some(crate::atomics::is_lock_free(arguments)),
        Builtin::AtomicsNotify => Some(crate::atomics::notify(arguments)),
        Builtin::AtomicsWait => Some(crate::atomics::wait(arguments)),
        Builtin::AtomicsLoad | Builtin::AtomicsStore => {
            Some(crate::atomics::load_store(builtin, arguments))
        }
        Builtin::AtomicsExchange => Some(crate::atomics::exchange(arguments)),
        Builtin::AtomicsWaitAsync => Some(crate::atomics::wait_async(arguments)),
        Builtin::AtomicsPause => Some(Ok(Value::Undefined)),
        _ => None,
    }
}

fn is_object_special(builtin: Builtin) -> bool {
    matches!(
        builtin,
        Builtin::ObjectHasOwnProperty
            | Builtin::ObjectHasOwn
            | Builtin::ObjectGetOwnPropertyDescriptor
            | Builtin::ObjectGetOwnPropertyDescriptors
            | Builtin::ObjectGetOwnPropertyNames
            | Builtin::ObjectGetOwnPropertySymbols
            | Builtin::ObjectKeys
            | Builtin::ObjectValues
            | Builtin::ObjectEntries
            | Builtin::ObjectAssign
            | Builtin::ObjectFromEntries
            | Builtin::ObjectGroupBy
            | Builtin::ObjectCreate
            | Builtin::ObjectGetPrototypeOf
            | Builtin::ObjectSetPrototypeOf
            | Builtin::ObjectPropertyIsEnumerable
            | Builtin::ObjectPrototypeIsPrototypeOf
            | Builtin::ObjectPrototypeDefineGetter
            | Builtin::ObjectPrototypeDefineSetter
            | Builtin::ObjectPrototypeLookupGetter
            | Builtin::ObjectPrototypeLookupSetter
    )
}

include!("vm_host.rs");
include!("vm_boolean_value.rs");
include!("vm_builtins.rs");
include!("vm_properties.rs");
include!("vm_dispatch.rs");

#[cfg(test)]
mod compact_handler_tests {
    use super::{
        quickened_own_get, run_compact_call, run_compact_get_index, run_compact_get_named,
        run_compact_set_index, run_instruction,
    };
    use crate::ops::Op;
    use crate::value::{ObjectData, Value};
    use std::rc::Rc;

    #[test]
    fn catalog_handler_returns_explicit_next_transition() {
        let executable =
            crate::machine::ExecutableCode::from_ops(vec![Op::Move { dst: 0, src: 1 }]);
        let code = executable.code();
        let instruction = code.instruction(0).expect("lowered move");
        let mut registers = crate::register_file::RegisterFile::from_values(vec![
            Value::Undefined,
            Value::Number(3.0),
        ]);
        let context = crate::vm::current_context_or_default();
        let transition = run_instruction(code, 0, instruction, &mut registers, &context)
            .expect("move transition");
        assert_eq!(transition.next_pc, 1);
        assert_eq!(transition.target, super::DispatchTarget::Callee(1));
        assert!(transition.completion.is_none());
        assert_eq!(registers.read(0), Some(Value::Number(3.0)));
    }

    #[test]
    fn slow_path_enters_one_way_transition_for_control_completions() {
        // Throw, break, and continue are all canonical cold operations.  They
        // lower to Opcode::Slow and must cross the same named, out-of-line
        // gateway rather than returning a value to a second dispatch policy.
        let cases = [
            (
                Op::Throw { src: 0 },
                crate::completion::Completion::Throw(Value::Number(7.0)),
            ),
            (
                Op::Break {
                    label: Some("outer".into()),
                    value: Some(0),
                },
                crate::completion::Completion::Break {
                    label: Some("outer".into()),
                    value: Some(Value::Number(7.0)),
                },
            ),
            (
                Op::Continue {
                    label: None,
                    value: Some(0),
                },
                crate::completion::Completion::Continue {
                    label: None,
                    value: Some(Value::Number(7.0)),
                },
            ),
        ];

        for (op, expected) in cases {
            let executable = crate::machine::ExecutableCode::from_ops(vec![op]);
            let code = executable.code();
            let instruction = code.instruction(0).expect("cold instruction");
            assert_eq!(instruction.opcode, crate::ir::Opcode::Slow);
            let mut registers = crate::register_file::RegisterFile::from_values(vec![
                Value::Number(7.0),
            ]);
            let context = crate::vm::current_context_or_default();
            let transition = super::enter_slow_path(
                code,
                0,
                instruction,
                &mut registers,
                &context,
            )
            .expect("slow-path transition");
            assert_eq!(transition.next_pc, 1);
            assert_eq!(transition.target, super::DispatchTarget::Exit);
            assert_eq!(transition.completion, Some(expected));
        }
    }

    #[test]
    fn owner_profile_retires_the_instruction_that_exits() {
        let function = crate::machine::FunctionCode::from_ops(vec![
            Op::Move { dst: 0, src: 1 },
            Op::Return { src: 0 },
        ]);
        let code = function.code().expect("function code");
        let mut registers = crate::register_file::RegisterFile::from_values(vec![
            Value::Undefined,
            Value::Number(8.0),
        ]);
        let context = crate::vm::current_context_or_default();
        let completion = crate::vm::execute_function_code_from(
            code,
            &function,
            0,
            &mut registers,
            &context,
            crate::environment::Environment::new(),
        )
        .expect("owner-aware execution")
        .0;
        assert_eq!(completion, crate::completion::Completion::Return(Value::Number(8.0)));
        assert_eq!(function.tier_counts(), (0, 2));
    }

    #[test]
    fn generated_shape_site_installs_then_hits_plain_own_property() {
        let executable = crate::machine::ExecutableCode::from_ops(vec![Op::GetPropertyDynamic {
            dst: 0,
            object: 1,
            key: 2,
        }]);
        let code = executable.code();
        let object = Value::Object(Rc::new(ObjectData::new(vec![(
            "value".into(),
            Value::Number(7.0),
        )])));

        assert_eq!(quickened_own_get(code, 0, &object, "value"), None);
        assert_eq!(
            quickened_own_get(code, 0, &object, "value"),
            Some(Value::Number(7.0))
        );
        assert_eq!(
            code.instruction(0).expect("rewritten instruction").opcode,
            crate::ir::Opcode::AGetIQuickened
        );
        let other = Value::Object(Rc::new(ObjectData::new(vec![
            ("other".into(), Value::Number(1.0)),
            ("value".into(), Value::Number(9.0)),
        ])));
        // A shape miss dequickens the logical instruction and takes the
        // complete generic path; only the following confirmed hit rewrites
        // it again for the new bounded state.
        assert_eq!(quickened_own_get(code, 0, &other, "value"), None);
        assert_eq!(code.instruction(0).expect("generic instruction").opcode, crate::ir::Opcode::AGetI);
        assert_eq!(quickened_own_get(code, 0, &other, "value"), Some(Value::Number(9.0)));
        assert_eq!(code.instruction(0).expect("rewritten instruction").opcode, crate::ir::Opcode::AGetIQuickened);
    }

    #[test]
    fn generated_shape_site_rechecks_in_place_descriptor_mutation() {
        let executable = crate::machine::ExecutableCode::from_ops(vec![Op::GetPropertyDynamic {
            dst: 0,
            object: 1,
            key: 2,
        }]);
        let code = executable.code();
        let descriptor = Rc::new(ObjectData::new(Vec::new()));
        let object = Value::Object(Rc::new(ObjectData::new(vec![
            ("value".into(), Value::Number(7.0)),
            (
                crate::builtins::descriptor_key("value"),
                Value::Object(Rc::clone(&descriptor)),
            ),
        ])));

        // First observation installs the guarded physical slot; the following
        // hit proves the cache is active before the descriptor changes.
        assert_eq!(quickened_own_get(code, 0, &object, "value"), None);
        assert_eq!(
            quickened_own_get(code, 0, &object, "value"),
            Some(Value::Number(7.0))
        );

        // Descriptor objects are mutable independently of the receiver's
        // property-name layout. Once a getter marker appears, the raw slot is
        // no longer a valid data projection and must return to the gateway.
        assert!(crate::execute::set_property_in_place(
            &Value::Object(Rc::clone(&descriptor)),
            "get",
            Value::Undefined,
        ));
        assert_eq!(quickened_own_get(code, 0, &object, "value"), None);
        assert_eq!(
            crate::vm::get_property_result(&object, "value").unwrap(),
            Value::Undefined
        );
    }

    #[test]
    fn generated_named_get_handler_uses_the_attached_shape_site() {
        let executable = crate::machine::ExecutableCode::from_ops(vec![Op::GetProperty {
            dst: 0,
            object: 1,
            key: "value".into(),
        }]);
        let code = executable.code();
        let instruction = code.instruction(0).expect("lowered named get");
        let object = Value::Object(Rc::new(ObjectData::new(vec![(
            "value".into(),
            Value::Number(9.0),
        )])));
        let mut registers =
            crate::register_file::RegisterFile::from_values(vec![Value::Undefined, object]);
        let context = crate::vm::current_context_or_default();

        run_compact_get_named(code, 0, instruction, &mut registers, &context)
            .expect("ordinary first lookup");
        assert_eq!(registers.read(0), Some(Value::Number(9.0)));
        registers.write(0, Value::Undefined);
        run_compact_get_named(code, 0, instruction, &mut registers, &context)
            .expect("guarded second lookup");
        assert_eq!(registers.read(0), Some(Value::Number(9.0)));
    }

    #[test]
    fn generated_index_get_reuses_shape_site_after_complete_first_lookup() {
        let executable = crate::machine::ExecutableCode::from_ops(vec![Op::GetPropertyDynamic {
            dst: 0,
            object: 1,
            key: 2,
        }]);
        let code = executable.code();
        let instruction = code.instruction(0).expect("lowered indexed get");
        let object = Value::Object(Rc::new(ObjectData::new(vec![
            ("0".into(), Value::Number(11.0)),
        ])));
        let mut registers = crate::register_file::RegisterFile::from_values(vec![
            Value::Undefined,
            object,
            Value::Number(0.0),
        ]);
        let context = crate::vm::current_context_or_default();

        run_compact_get_index(code, 0, instruction, &mut registers, &context)
            .expect("first indexed lookup");
        assert_eq!(registers.read(0), Some(Value::Number(11.0)));
        registers.write(0, Value::Undefined);
        run_compact_get_index(code, 0, instruction, &mut registers, &context)
            .expect("guarded indexed lookup");
        assert_eq!(registers.read(0), Some(Value::Number(11.0)));
    }

    #[test]
    fn indexed_set_does_not_bypass_non_extensible_fallback() {
        let executable = crate::machine::ExecutableCode::from_ops(vec![Op::SetPropertyDynamic {
            object: 0,
            key: 1,
            src: 2,
            strict: false,
        }]);
        let code = executable.code();
        let instruction = code.instruction(0).expect("lowered indexed set");
        let object = Value::Object(Rc::new(ObjectData::new(Vec::new())));
        let object = crate::properties::prevent_extensions(Some(&object)).expect("seal object");
        let mut registers = crate::register_file::RegisterFile::from_values(vec![
            object.clone(),
            Value::Number(0.0),
            Value::Number(1.0),
        ]);
        let context = crate::vm::current_context_or_default();

        run_compact_set_index(code, 0, instruction, &mut registers, &context)
            .expect("non-strict indexed write fallback");
        assert_eq!(crate::vm::get_property_result(&object, "0").unwrap(), Value::Undefined);
    }

    #[test]
    fn indexed_set_installs_and_reuses_shape_site() {
        let executable = crate::machine::ExecutableCode::from_ops(vec![Op::SetPropertyDynamic {
            object: 0,
            key: 1,
            src: 2,
            strict: false,
        }]);
        let code = executable.code();
        let instruction = code.instruction(0).expect("lowered indexed set");
        let object = Value::Object(Rc::new(ObjectData::new(vec![(
            "0".into(),
            Value::Number(0.0),
        )])));
        let mut registers = crate::register_file::RegisterFile::from_values(vec![
            object.clone(),
            Value::Number(0.0),
            Value::Number(3.0),
        ]);
        let context = crate::vm::current_context_or_default();

        run_compact_set_index(code, 0, instruction, &mut registers, &context)
            .expect("first indexed write");
        assert_eq!(crate::vm::get_property_result(&object, "0").unwrap(), Value::Number(3.0));
        assert_eq!(code.quickening_site(0).unwrap().borrow().cache_len(), 1);

        registers.write(2, Value::Number(4.0));
        run_compact_set_index(code, 0, instruction, &mut registers, &context)
            .expect("guarded indexed write");
        assert_eq!(crate::vm::get_property_result(&object, "0").unwrap(), Value::Number(4.0));
    }

    #[test]
    fn generated_call_site_installs_then_hits_callable_identity() {
        let executable = crate::machine::ExecutableCode::from_ops(vec![Op::Call {
            dst: 0,
            callee: 1,
            receiver: None,
            args: Vec::new(),
            spreads: Vec::new(),
        }]);
        let code = executable.code();
        let function = Value::Function(Rc::new(crate::value::FunctionValue {
            code: crate::machine::FunctionCode::from_ops(vec![
                Op::Const {
                    dst: 0,
                    value: crate::ops::Constant::Undefined,
                },
                Op::Return { src: 0 },
            ]),
            params: 0,
            captures: crate::environment::Environment::new(),
            with_captures: Vec::new(),
            properties: Rc::new(std::cell::RefCell::new(Vec::new())),
            private_slots: Rc::new(std::cell::RefCell::new(Vec::new())),
            private_environment: crate::private_environment::PrivateEnvironment::default(),
            instance_fields: Rc::new(std::cell::RefCell::new(Vec::new())),
            kind: crate::ops::FunctionKind::Ordinary,
            strictness: crate::ops::FunctionStrictness::Sloppy,
            is_async: false,
            mapped_arguments: false,
        }));
        let mut registers = crate::register_file::RegisterFile::from_values(vec![
            Value::Undefined,
            function.clone(),
        ]);
        let context = crate::vm::current_context_or_default();

        run_compact_call(
            code,
            0,
            code.instruction(0).unwrap(),
            &mut registers,
            &context,
        )
        .expect("ordinary first call");
        assert_eq!(registers.read(0), Some(Value::Undefined));
        let site = code.quickening_site(0).expect("call site");
        assert_eq!(site.borrow().callable_cache_len(), 1);

        run_compact_call(
            code,
            0,
            code.instruction(0).unwrap(),
            &mut registers,
            &context,
        )
        .expect("guarded second call");
        assert_eq!(registers.read(0), Some(Value::Undefined));
        assert_eq!(site.borrow().callable_cache_len(), 1);

        let replacement = Value::Function(Rc::new(crate::value::FunctionValue {
            code: crate::machine::FunctionCode::from_ops(vec![
                Op::Const {
                    dst: 0,
                    value: crate::ops::Constant::Undefined,
                },
                Op::Return { src: 0 },
            ]),
            params: 0,
            captures: crate::environment::Environment::new(),
            with_captures: Vec::new(),
            properties: Rc::new(std::cell::RefCell::new(Vec::new())),
            private_slots: Rc::new(std::cell::RefCell::new(Vec::new())),
            private_environment: crate::private_environment::PrivateEnvironment::default(),
            instance_fields: Rc::new(std::cell::RefCell::new(Vec::new())),
            kind: crate::ops::FunctionKind::Ordinary,
            strictness: crate::ops::FunctionStrictness::Sloppy,
            is_async: false,
            mapped_arguments: false,
        }));
        registers.write(1, replacement);
        run_compact_call(
            code,
            0,
            code.instruction(0).unwrap(),
            &mut registers,
            &context,
        )
        .expect("identity-changing call");
        assert_eq!(registers.read(0), Some(Value::Undefined));
        assert_eq!(site.borrow().callable_cache_len(), 2);
    }

    #[test]
    fn call_region_matches_canonical_handler_and_rejects_hostile_opcode() {
        let callee = Value::Function(Rc::new(crate::value::FunctionValue {
                code: crate::machine::FunctionCode::from_ops(vec![
                    Op::Const {
                        dst: 0,
                        value: crate::ops::Constant::Number(41.0),
                    },
                    Op::Return { src: 0 },
                ]),
                params: 0,
                captures: crate::environment::Environment::new(),
                with_captures: Vec::new(),
                properties: Rc::new(std::cell::RefCell::new(Vec::new())),
                private_slots: Rc::new(std::cell::RefCell::new(Vec::new())),
                private_environment: crate::private_environment::PrivateEnvironment::default(),
                instance_fields: Rc::new(std::cell::RefCell::new(Vec::new())),
                kind: crate::ops::FunctionKind::Ordinary,
                strictness: crate::ops::FunctionStrictness::Sloppy,
                is_async: false,
                mapped_arguments: false,
            }));
        let executable = crate::machine::ExecutableCode::from_ops(vec![Op::Call {
            dst: 0,
            callee: 1,
            receiver: None,
            args: Vec::new(),
            spreads: Vec::new(),
        }]);
        let code = executable.code();
        let context = crate::vm::current_context_or_default();
        let mut ordinary = crate::register_file::RegisterFile::from_values(vec![
            Value::Undefined,
            callee.clone(),
        ]);
        let expected = run_compact_call(
            code,
            0,
            code.instruction(0).expect("call instruction"),
            &mut ordinary,
            &context,
        )
        .expect("canonical call handler");

        let mut fused = crate::register_file::RegisterFile::from_values(vec![
            Value::Undefined,
            callee.clone(),
        ]);
        let mut region = crate::machine::NativeRegionPlan::new_for_test(
            crate::stencil_select::call_region_key(),
        )
        .expect("call region admission");
        let actual = region
            .execute(code, 0, &mut fused, &context)
            .expect("call region execution");
        assert_transition_equal(&actual, &expected);
        assert_eq!(fused, ordinary);

        let hostile = crate::machine::ExecutableCode::from_ops(vec![Op::Call {
            dst: 0,
            callee: 1,
            receiver: None,
            args: Vec::new(),
            spreads: Vec::new(),
        }]);
        let hostile_code = hostile.code();
        hostile_code.quicken_instruction(0, crate::ir::Opcode::Slow, 0, 0, 0);
        let mut hostile_registers = crate::register_file::RegisterFile::from_values(vec![
            Value::Undefined,
            callee,
        ]);
        let before = hostile_registers.clone();
        let mut hostile_region = crate::machine::NativeRegionPlan::new_for_test(
            crate::stencil_select::call_region_key(),
        )
        .expect("call region admission");
        assert!(matches!(
            hostile_region.execute(hostile_code, 0, &mut hostile_registers, &context),
            Err(crate::machine::NativeDispatchError::Physical(_))
        ));
        assert_eq!(hostile_registers, before);
    }

    #[test]
    fn baseline_number_leaf_executes_stencil_and_non_number_falls_back() {
        let function = crate::machine::FunctionCode::from_ops(vec![
            Op::Binary {
                dst: 0,
                operator: crate::ops::BinaryOp::Add,
                lhs: 1,
                rhs: 2,
            },
            Op::Return { src: 0 },
        ]);
        function.set_tier_threshold_for_test(1);
        function.retire(1);
        assert_eq!(
            function.enter_invocation(),
            crate::machine::TierTransition::CompileBaseline
        );
        let plan = function.baseline_plan().expect("baseline plan");
        let code = function.code().expect("function code");
        let context = crate::vm::current_context_or_default();
        let environment = crate::environment::Environment::new();
        let mut registers = crate::register_file::RegisterFile::from_values(vec![
            Value::Undefined,
            Value::Number(2.0),
            Value::Number(3.0),
        ]);
        let (completion, _) = crate::vm::execute_baseline_code_from(
            code,
            &plan,
            0,
            &mut registers,
            &context,
            environment.clone(),
        )
        .expect("native number leaf");
        assert_eq!(completion, crate::completion::Completion::Return(Value::Number(5.0)));

        registers.write(1, Value::String("a".into()));
        registers.write(2, Value::String("b".into()));
        let (completion, _) = crate::vm::execute_baseline_code_from(
            code,
            &plan,
            0,
            &mut registers,
            &context,
            environment,
        )
        .expect("ordinary add fallback");
        assert_eq!(
            completion,
            crate::completion::Completion::Return(Value::String("ab".into()))
        );
    }

    fn execute_one_at_a_time(
        code: crate::machine::CodeView<'_>,
        registers: &mut crate::register_file::RegisterFile,
        context: &crate::vm::VmContext,
    ) -> crate::vm::DispatchTransition {
        let mut last = None;
        for pc in 0..code.len() {
            let instruction = code.instruction(pc).expect("ordinary instruction");
            let transition = run_instruction(code, pc, instruction, registers, context)
                .expect("ordinary handler");
            let done = transition.completion.is_some();
            last = Some(transition);
            if done {
                break;
            }
        }
        last.expect("non-empty sequence")
    }

    fn assert_transition_equal(
        actual: &crate::vm::DispatchTransition,
        expected: &crate::vm::DispatchTransition,
    ) {
        assert_eq!(actual.next_pc, expected.next_pc);
        assert_eq!(actual.target, expected.target);
        match (&actual.completion, &expected.completion) {
            (
                Some(crate::completion::Completion::Return(Value::Number(left))),
                Some(crate::completion::Completion::Return(Value::Number(right))),
            ) => assert_eq!(left.to_bits(), right.to_bits()),
            _ => assert_eq!(actual.completion, expected.completion),
        }
    }

    #[test]
    fn fused_multi_op_regions_match_one_at_a_time_execution() {
        let cases = [
            (
                crate::stencil_select::arithmetic_glue_region_key(),
                vec![
                    Op::Const {
                        dst: 1,
                        value: crate::ops::Constant::Number(3.0),
                    },
                    Op::CheckInitialized {
                        slot: 0,
                        name: "x".into(),
                    },
                    Op::LoadLocal { dst: 2, slot: 0 },
                    Op::Binary {
                        dst: 3,
                        operator: crate::ops::BinaryOp::NumericAdd,
                        lhs: 2,
                        rhs: 1,
                    },
                    Op::LoadLocal { dst: 4, slot: 0 },
                    Op::Const {
                        dst: 5,
                        value: crate::ops::Constant::Number(1.0),
                    },
                    Op::Binary {
                        dst: 6,
                        operator: crate::ops::BinaryOp::NumericAdd,
                        lhs: 4,
                        rhs: 5,
                    },
                    Op::StoreLocal { slot: 0, src: 6 },
                    Op::StoreLocal { slot: 2, src: 3 },
                ],
                vec![Value::Number(2.0), Value::Undefined, Value::Undefined],
            ),
            (
                crate::stencil_select::binary_glue_region_key(),
                vec![
                    Op::LoadLocal { dst: 1, slot: 1 },
                    Op::Const {
                        dst: 2,
                        value: crate::ops::Constant::Number(3.0),
                    },
                    Op::Binary {
                        dst: 0,
                        operator: crate::ops::BinaryOp::NumericAdd,
                        lhs: 1,
                        rhs: 2,
                    },
                    Op::Return { src: 0 },
                ],
                vec![Value::Undefined, Value::Number(2.0)],
            ),
            (
                crate::stencil_select::update_return_region_key(),
                vec![
                    Op::LoadLocal {
                        dst: 2,
                        slot: 1,
                    },
                    Op::Const {
                        dst: 3,
                        value: crate::ops::Constant::Number(1.0),
                    },
                    Op::Binary {
                        dst: 4,
                        operator: crate::ops::BinaryOp::NumericAdd,
                        lhs: 2,
                        rhs: 3,
                    },
                    Op::CheckInitialized {
                        slot: 1,
                        name: "x".into(),
                    },
                    Op::StoreLocal { slot: 1, src: 4 },
                    Op::Return { src: 1 },
                ],
                vec![Value::Undefined, Value::Number(9.0)],
            ),
        ];
        let context = crate::vm::current_context_or_default();
        for (key, ops, values) in cases {
            let executable = crate::machine::ExecutableCode::from_ops(ops);
            let code = executable.code();
            let record = crate::stencil_select::select_region(key).expect("region record");
            assert_eq!(
                code.len(),
                record.operations.len(),
                "test sequence must lower to the admitted span"
            );
            for (instruction, expected) in (0..code.len())
                .map(|pc| code.instruction(pc).expect("lowered instruction"))
                .zip(record.operations.iter().copied())
            {
                assert_eq!(instruction.opcode, expected);
            }

            let mut ordinary = crate::register_file::RegisterFile::from_values(values.clone());
            let expected_transition = {
                let environment = crate::environment::Environment::new();
                environment.set(0, values[0].clone());
                environment.set(1, values[1].clone());
                let _environment_guard = crate::locals::EnvironmentGuard::install(environment);
                execute_one_at_a_time(code, &mut ordinary, &context)
            };
            let expected_registers = ordinary.clone();
            let mut fused = crate::register_file::RegisterFile::from_values(values);
            let actual_transition = {
                let environment = crate::environment::Environment::new();
                environment.set(0, expected_registers.read(0).unwrap_or(Value::Undefined));
                environment.set(1, expected_registers.read(1).unwrap_or(Value::Undefined));
                let _environment_guard = crate::locals::EnvironmentGuard::install(environment);
                let mut region = crate::machine::NativeRegionPlan::new_for_test(key)
                    .expect("fused region test plan");
                region
                    .execute(code, 0, &mut fused, &context)
                    .expect("fused region execution")
            };
            assert_transition_equal(&actual_transition, &expected_transition);
            assert_eq!(fused, expected_registers);
        }
    }

    #[test]
    fn fused_region_unknown_interior_falls_back_atomically() {
        let executable = crate::machine::ExecutableCode::from_ops(vec![
            Op::LoadLocal { dst: 1, slot: 1 },
            Op::Const {
                dst: 2,
                value: crate::ops::Constant::Number(3.0),
            },
            Op::Binary {
                dst: 0,
                operator: crate::ops::BinaryOp::NumericAdd,
                lhs: 1,
                rhs: 2,
            },
            Op::Return { src: 0 },
        ]);
        let code = executable.code();
        let context = crate::vm::current_context_or_default();
        let environment = crate::environment::Environment::new();
        environment.set(1, Value::Number(2.0));
        let _environment_guard = crate::locals::EnvironmentGuard::install(environment.clone());
        let mut region = crate::machine::NativeRegionPlan::new_for_test(
            crate::stencil_select::binary_glue_region_key(),
        )
        .expect("fused region test plan");
        let mut registers = crate::register_file::RegisterFile::from_values(vec![
            Value::Undefined,
            Value::Number(2.0),
        ]);
        let before = registers.clone();
        // Simulate a mid-span Unknown/quickened fact.  The bridge must inspect
        // the whole window before invoking the first handler.
        code.quicken_instruction(1, crate::ir::Opcode::Slow, 0, 0, 0);
        assert!(matches!(
            region.execute(code, 0, &mut registers, &context),
            Err(crate::machine::NativeDispatchError::Physical(_))
        ));
        assert_eq!(registers, before, "partial match executed a prefix");

        // The caller's ordinary path remains complete and can execute the
        // canonical plan from the beginning after the fused admission fails.
        let function = crate::machine::FunctionCode::from_ops(vec![
            Op::LoadLocal { dst: 1, slot: 1 },
            Op::Const {
                dst: 2,
                value: crate::ops::Constant::Number(3.0),
            },
            Op::Binary {
                dst: 0,
                operator: crate::ops::BinaryOp::NumericAdd,
                lhs: 1,
                rhs: 2,
            },
            Op::Return { src: 0 },
        ]);
        function.set_tier_threshold_for_test(1);
        function.retire(1);
        assert_eq!(
            function.enter_invocation(),
            crate::machine::TierTransition::CompileBaseline
        );
        let baseline = function.baseline_plan().expect("baseline plan");
        let canonical = function.code().expect("canonical code");
        let mut fallback_registers = before;
        let (completion, next) = crate::vm::execute_baseline_code_from(
            canonical,
            &baseline,
            0,
            &mut fallback_registers,
            &context,
            environment,
        )
        .expect("ordinary whole-span fallback");
        assert_eq!(completion, crate::completion::Completion::Return(Value::Number(3.0)));
        assert_eq!(next, canonical.len());
    }

    #[test]
    fn baseline_move_leaf_preserves_tagged_word_ownership() {
        let function = crate::machine::FunctionCode::from_ops(vec![
            Op::Move { dst: 0, src: 1 },
            Op::Return { src: 0 },
        ]);
        function.set_tier_threshold_for_test(1);
        function.retire(1);
        assert_eq!(
            function.enter_invocation(),
            crate::machine::TierTransition::CompileBaseline
        );
        let plan = function.baseline_plan().expect("baseline plan");
        if cfg!(target_arch = "x86_64") {
            assert!(plan.native_move_at(0).is_some());
        }
        let code = function.code().expect("function code");
        let context = crate::vm::current_context_or_default();
        let mut registers = crate::register_file::RegisterFile::from_values(vec![
            Value::Undefined,
            Value::Object(Rc::new(ObjectData::new(vec![(
                "value".into(),
                Value::Number(7.0),
            )]))),
        ]);
        let expected = registers.read(1).expect("source object");
        let (completion, _) = crate::vm::execute_baseline_code_from(
            code,
            &plan,
            0,
            &mut registers,
            &context,
            crate::environment::Environment::new(),
        )
        .expect("move leaf");
        assert_eq!(completion, crate::completion::Completion::Return(expected.clone()));
        registers.write(1, Value::Undefined);
        assert_eq!(registers.read(0), Some(expected));
    }

    #[test]
    fn optimizing_plan_reuses_native_leaf_and_preserves_fallback() {
        let function = crate::machine::FunctionCode::from_ops(vec![
            Op::Binary {
                dst: 0,
                operator: crate::ops::BinaryOp::Add,
                lhs: 1,
                rhs: 2,
            },
            Op::Return { src: 0 },
        ]);
        function.set_tier_threshold_for_test(1);
        function.retire(1);
        assert_eq!(
            function.enter_invocation(),
            crate::machine::TierTransition::CompileBaseline
        );
        for _ in 0..6 {
            assert_eq!(
                function.enter_invocation(),
                crate::machine::TierTransition::Baseline
            );
        }
        assert_eq!(
            function.enter_invocation(),
            crate::machine::TierTransition::CompileOptimizing
        );
        let optimizing = function.optimizing_plan().expect("optimizing plan");
        let baseline = function.baseline_plan().expect("baseline plan");
        let code = function.code().expect("function code");
        let context = crate::vm::current_context_or_default();
        let mut registers = crate::register_file::RegisterFile::from_values(vec![
            Value::Undefined,
            Value::Number(2.0),
            Value::Number(3.0),
        ]);
        let (completion, next) = crate::vm::execute_optimized_code_step_from(
            code,
            &optimizing,
            &baseline,
            0,
            &mut registers,
            &context,
        )
        .expect("optimized native step");
        if cfg!(target_arch = "x86_64") {
            assert_eq!(completion, crate::completion::Completion::Normal);
            assert_eq!(next, 1);
        } else {
            assert_eq!(
                completion,
                crate::completion::Completion::Return(Value::Number(5.0))
            );
        }
        assert_eq!(registers.read(0), Some(Value::Number(5.0)));

        registers.write(1, Value::String("a".into()));
        registers.write(2, Value::String("b".into()));
        let (completion, _) = crate::vm::execute_optimized_code_step_from(
            code,
            &optimizing,
            &baseline,
            0,
            &mut registers,
            &context,
        )
        .expect("optimized fallback step");
        if cfg!(target_arch = "x86_64") {
            assert_eq!(completion, crate::completion::Completion::Normal);
        } else {
            assert_eq!(
                completion,
                crate::completion::Completion::Return(Value::String("ab".into()))
            );
        }
        assert_eq!(registers.read(0), Some(Value::String("ab".into())));
    }

    #[test]
    fn add_const_fallback_preserves_constant_left_operand_order() {
        let function = crate::machine::FunctionCode::from_ops(vec![
            Op::Const {
                dst: 1,
                value: crate::ops::Constant::String("a".into()),
            },
            Op::Binary {
                dst: 0,
                operator: crate::ops::BinaryOp::Add,
                lhs: 1,
                rhs: 2,
            },
            Op::Return { src: 0 },
        ]);
        function.set_tier_threshold_for_test(1);
        function.retire(1);
        assert_eq!(
            function.enter_invocation(),
            crate::machine::TierTransition::CompileBaseline
        );
        let plan = function.baseline_plan().expect("baseline plan");
        let code = function.code().expect("function code");
        let instruction = code.instruction(0).expect("fused add");
        assert!(instruction.add_const_is_left());
        let context = crate::vm::current_context_or_default();
        let mut registers = crate::register_file::RegisterFile::from_values(vec![
            Value::Undefined,
            Value::Undefined,
            Value::String("b".into()),
        ]);
        let (completion, _) = crate::vm::execute_baseline_code_from(
            code,
            &plan,
            0,
            &mut registers,
            &context,
            crate::environment::Environment::new(),
        )
        .expect("complete add fallback");
        assert_eq!(
            completion,
            crate::completion::Completion::Return(Value::String("ab".into()))
        );
    }

    #[test]
    fn add_const_fallback_preserves_constant_right_operand_order() {
        let function = crate::machine::FunctionCode::from_ops(vec![
            Op::Const {
                dst: 1,
                value: crate::ops::Constant::String("b".into()),
            },
            Op::Binary {
                dst: 0,
                operator: crate::ops::BinaryOp::Add,
                lhs: 2,
                rhs: 1,
            },
            Op::Return { src: 0 },
        ]);
        function.set_tier_threshold_for_test(1);
        function.retire(1);
        assert_eq!(
            function.enter_invocation(),
            crate::machine::TierTransition::CompileBaseline
        );
        let plan = function.baseline_plan().expect("baseline plan");
        let code = function.code().expect("function code");
        let instruction = code.instruction(0).expect("fused add");
        assert!(!instruction.add_const_is_left());
        let context = crate::vm::current_context_or_default();
        let mut registers = crate::register_file::RegisterFile::from_values(vec![
            Value::Undefined,
            Value::Undefined,
            Value::String("a".into()),
        ]);
        let (completion, _) = crate::vm::execute_baseline_code_from(
            code,
            &plan,
            0,
            &mut registers,
            &context,
            crate::environment::Environment::new(),
        )
        .expect("complete add fallback");
        assert_eq!(
            completion,
            crate::completion::Completion::Return(Value::String("ab".into()))
        );
    }

    #[test]
    fn baseline_numeric_stencils_cover_subtract_and_multiply() {
        for (operator, expected) in [
            (crate::ops::BinaryOp::Subtract, 6.0),
            (crate::ops::BinaryOp::Multiply, 27.0),
            (crate::ops::BinaryOp::Divide, 3.0),
        ] {
            let function = crate::machine::FunctionCode::from_ops(vec![
                Op::Binary {
                    dst: 0,
                    operator,
                    lhs: 1,
                    rhs: 2,
                },
                Op::Return { src: 0 },
            ]);
            function.set_tier_threshold_for_test(1);
            function.retire(1);
            assert_eq!(
                function.enter_invocation(),
                crate::machine::TierTransition::CompileBaseline
            );
            let plan = function.baseline_plan().expect("baseline plan");
            let code = function.code().expect("function code");
            let context = crate::vm::current_context_or_default();
            let mut registers = crate::register_file::RegisterFile::from_values(vec![
                Value::Undefined,
                Value::Number(9.0),
                Value::Number(3.0),
            ]);
            let (completion, _) = crate::vm::execute_baseline_code_from(
                code,
                &plan,
                0,
                &mut registers,
                &context,
                crate::environment::Environment::new(),
            )
            .expect("native numeric leaf");
            assert_eq!(
                completion,
                crate::completion::Completion::Return(Value::Number(expected))
            );
        }
    }

    #[test]
    fn baseline_numeric_path_keeps_constant_pool_and_arithmetic_canonical() {
        let function = crate::machine::FunctionCode::from_ops(vec![
            Op::Const {
                dst: 2,
                value: crate::ops::Constant::Number(4.0),
            },
            Op::Binary {
                dst: 0,
                operator: crate::ops::BinaryOp::Add,
                lhs: 1,
                rhs: 2,
            },
            Op::Return { src: 0 },
        ]);
        function.set_tier_threshold_for_test(1);
        function.retire(1);
        assert_eq!(
            function.enter_invocation(),
            crate::machine::TierTransition::CompileBaseline
        );
        let plan = function.baseline_plan().expect("baseline plan");
        let code = function.code().expect("function code");
        assert_eq!(
            code.instruction(0).map(|instruction| instruction.opcode),
            Some(crate::ir::Opcode::AddConst)
        );
        assert_eq!(
            code.instruction(1).map(|instruction| instruction.opcode),
            Some(crate::ir::Opcode::Return)
        );
        let context = crate::vm::current_context_or_default();
        let mut registers = crate::register_file::RegisterFile::from_values(vec![
            Value::Undefined,
            Value::Number(6.0),
            Value::Undefined,
        ]);
        let (completion, _) = crate::vm::execute_baseline_code_from(
            code,
            &plan,
            0,
            &mut registers,
            &context,
            crate::environment::Environment::new(),
        )
        .expect("baseline constant path");
        assert_eq!(
            completion,
            crate::completion::Completion::Return(Value::Number(10.0))
        );
    }

    #[test]
    fn baseline_numeric_stencils_can_be_used_inside_a_longer_body() {
        let function = crate::machine::FunctionCode::from_ops(vec![
            Op::Binary {
                dst: 3,
                operator: crate::ops::BinaryOp::Add,
                lhs: 1,
                rhs: 2,
            },
            Op::Binary {
                dst: 4,
                operator: crate::ops::BinaryOp::Multiply,
                lhs: 3,
                rhs: 2,
            },
            Op::Return { src: 4 },
        ]);
        function.set_tier_threshold_for_test(1);
        function.retire(1);
        assert_eq!(
            function.enter_invocation(),
            crate::machine::TierTransition::CompileBaseline
        );
        let plan = function.baseline_plan().expect("baseline plan");
        let code = function.code().expect("function code");
        let context = crate::vm::current_context_or_default();
        let mut registers = crate::register_file::RegisterFile::from_values(vec![
            Value::Undefined,
            Value::Number(2.0),
            Value::Number(3.0),
            Value::Undefined,
            Value::Undefined,
        ]);
        let (completion, _) = crate::vm::execute_baseline_code_from(
            code,
            &plan,
            0,
            &mut registers,
            &context,
            crate::environment::Environment::new(),
        )
        .expect("native leaves in body");
        assert_eq!(
            completion,
            crate::completion::Completion::Return(Value::Number(15.0))
        );
    }
}
