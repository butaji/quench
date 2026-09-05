include!("vm_generator_step.rs");
include!("vm_completion_step.rs");
include!("vm_native_status.rs");

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
    error_pc: Option<usize>,
    entry_started: bool,
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
            error_pc: None,
            entry_started: false,
        }
    }

    pub(crate) fn finish(
        self,
        status: u64,
    ) -> Result<DispatchTransition, crate::machine::NativeDispatchError> {
        match NativeStatus::from(status) {
            NativeStatus::Ok => self.result.ok_or_else(|| {
                if self.entry_started {
                    crate::machine::NativeDispatchError::Committed(
                        "native bridge entered without a transition".into(),
                    )
                } else {
                    crate::machine::NativeDispatchError::Physical(
                        "native bridge returned without a transition".into(),
                    )
                }
            }),
            NativeStatus::SemanticError => self.error.map_or_else(
                || {
                    Err(if self.entry_started {
                        crate::machine::NativeDispatchError::Committed(
                            "native bridge lost its post-entry error".into(),
                        )
                    } else {
                        crate::machine::NativeDispatchError::Physical(
                            "native bridge returned an empty semantic error".into(),
                        )
                    })
                },
                |error| match self.error_pc {
                    Some(pc) => Err(crate::machine::NativeDispatchError::SemanticAt {
                        pc,
                        error,
                    }),
                    None => Err(crate::machine::NativeDispatchError::Semantic(error)),
                },
            ),
            NativeStatus::Interrupt if self.entry_started => Err(
                crate::machine::NativeDispatchError::Committed(
                    "native bridge interrupted after committed progress".into(),
                ),
            ),
            NativeStatus::Interrupt => Err(crate::machine::NativeDispatchError::Physical(
                "native bridge interrupted before entry".into(),
            )),
            NativeStatus::CommittedError | NativeStatus::Unknown(_) if self.entry_started => Err(
                crate::machine::NativeDispatchError::Committed(
                "native bridge returned an invalid post-entry status".into(),
                ),
            ),
            NativeStatus::CommittedError | NativeStatus::Unknown(_) => Err(
                crate::machine::NativeDispatchError::Physical(
                "native bridge returned an invalid entry status".into(),
                ),
            ),
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
    abi: crate::stencil_select::RegionAbi,
    registers: *mut crate::register_file::RegisterFile,
    context: *const VmContext,
    result: Option<DispatchTransition>,
    error: Option<VmError>,
    error_pc: Option<usize>,
    entry_started: bool,
    /// Set immediately before invoking rendered bytes; unlike a successful
    /// transition this remains a witness even when the physical call exits
    /// with a committed failure.
    pub(crate) native_entered: bool,
    #[cfg(test)]
    force_committed_status: bool,
}

impl<'a> NativeRegionContext<'a> {
    pub(crate) fn new(
        code: crate::machine::CodeView<'a>,
        pc: usize,
        operations: &'static [crate::ir::Opcode],
        registers: &mut crate::register_file::RegisterFile,
        context: &VmContext,
    ) -> Self {
        Self::new_with_abi(
            code,
            pc,
            operations,
            crate::stencil_select::RegionAbi::Bridge,
            registers,
            context,
        )
    }

    pub(crate) fn new_with_abi(
        code: crate::machine::CodeView<'a>,
        pc: usize,
        operations: &'static [crate::ir::Opcode],
        abi: crate::stencil_select::RegionAbi,
        registers: &mut crate::register_file::RegisterFile,
        context: &VmContext,
    ) -> Self {
        Self {
            code,
            pc,
            operations,
            abi,
            registers,
            context,
            result: None,
            error: None,
            error_pc: None,
            entry_started: false,
            native_entered: false,
            #[cfg(test)]
            force_committed_status: false,
        }
    }

    pub(crate) fn finish(
        self,
        status: u64,
    ) -> Result<DispatchTransition, crate::machine::NativeDispatchError> {
        match NativeStatus::from(status) {
            NativeStatus::Ok => match self.result {
                Some(result) => Ok(result),
                None if self.entry_started => Err(
                    crate::machine::NativeDispatchError::Committed(
                        "native region entered without a transition".into(),
                    ),
                ),
                None => Err(crate::machine::NativeDispatchError::Physical(
                    "native region rejected without a transition".into(),
                )),
            },
            NativeStatus::SemanticError => match self.error {
                Some(error) => match self.error_pc {
                    Some(pc) => Err(crate::machine::NativeDispatchError::SemanticAt {
                        pc,
                        error,
                    }),
                    None => Err(crate::machine::NativeDispatchError::Semantic(error)),
                },
                None if self.entry_started => Err(
                    crate::machine::NativeDispatchError::Committed(
                        "native region lost its post-entry error".into(),
                    ),
                ),
                None => Err(crate::machine::NativeDispatchError::Physical(
                    "native region rejected without a semantic error".into(),
                )),
            },
            NativeStatus::CommittedError => Err(crate::machine::NativeDispatchError::Committed(
                "native region reported a post-entry failure".into(),
            )),
            NativeStatus::Interrupt if self.entry_started => Err(
                crate::machine::NativeDispatchError::Committed(
                    "native region interrupted after committed progress".into(),
                ),
            ),
            NativeStatus::Interrupt => Err(crate::machine::NativeDispatchError::Physical(
                "native region interrupted before entry".into(),
            )),
            NativeStatus::Unknown(_) if self.entry_started => Err(
                crate::machine::NativeDispatchError::Committed(
                    "native region returned an invalid post-entry status".into(),
                ),
            ),
            NativeStatus::Unknown(_) => Err(crate::machine::NativeDispatchError::Physical(
                "native region returned an invalid entry status".into(),
            )),
        }
    }
}

const NATIVE_DISPATCH_OK: u64 = 1;
const NATIVE_DISPATCH_SEMANTIC_ERROR: u64 = 2;
const NATIVE_DISPATCH_COMMITTED_ERROR: u64 = 3;
const NATIVE_DISPATCH_INTERRUPT: u64 = 4;

/// Native loops are deliberately bounded and poll an explicit interrupt flag
/// at each backedge. Keeping a finite chunk also prevents an admitted byte
/// region from monopolizing the VM thread; larger spans remain semantically
/// complete through the ordinary residual loop.
const MAX_NATIVE_ARRAY_LOOP_ITERATIONS: usize = 4096;
const FRAME_ROOT_EFFECTS: &[crate::facts::OperationEffect] = &[
    crate::facts::OperationEffect::Allocate,
    crate::facts::OperationEffect::MayThrow,
    crate::facts::OperationEffect::Observable,
    crate::facts::OperationEffect::WriteHeap,
];

// Keep the CPS fast path shallow enough that the large transition frame does
// not accumulate on long-running ARM64 loops. The stack-safe segment takes
// over at this boundary and preserves the same canonical transitions.
const DISPATCH_RECURSION_LIMIT: usize = 64;

fn try_native_word_truthiness(
    native: &std::cell::RefCell<crate::machine::NativeTruthinessPlan>,
    registers: &crate::register_file::RegisterFile,
    index: usize,
) -> Option<bool> {
    let bits = registers.word_bits(index)?;
    match crate::tagged_value::TaggedValue::from_bits(bits).decode() {
        crate::tagged_value::DecodedValue::Bool(_)
        | crate::tagged_value::DecodedValue::Null
        | crate::tagged_value::DecodedValue::Undefined => {
            native.borrow_mut().execute_word(bits).ok()
        }
        crate::tagged_value::DecodedValue::ObjectPtr(_)
        | crate::tagged_value::DecodedValue::ArrayPtr(_)
        | crate::tagged_value::DecodedValue::FunctionPtr(_) => {
            native.borrow_mut().execute_pointer(bits).ok()
        }
        crate::tagged_value::DecodedValue::Number(_)
        | crate::tagged_value::DecodedValue::I31(_)
        | crate::tagged_value::DecodedValue::HeapPtr(_)
        | crate::tagged_value::DecodedValue::HeapRef(_) => None,
    }
}

fn try_native_logical_not(
    native: &std::cell::RefCell<crate::machine::NativeTruthinessPlan>,
    registers: &crate::register_file::RegisterFile,
    index: usize,
) -> Option<bool> {
    if let Some(truthy) = try_native_word_truthiness(native, registers, index) {
        return Some(!truthy);
    }
    registers
        .read_number(index)
        .and_then(|value| native.borrow_mut().execute(value).ok())
        .map(|truthy| !truthy)
}

fn identity_word(bits: u64) -> bool {
    matches!(
        crate::tagged_value::TaggedValue::from_bits(bits).decode(),
        crate::tagged_value::DecodedValue::Bool(_)
            | crate::tagged_value::DecodedValue::Null
            | crate::tagged_value::DecodedValue::Undefined
            | crate::tagged_value::DecodedValue::ObjectPtr(_)
            | crate::tagged_value::DecodedValue::ArrayPtr(_)
            | crate::tagged_value::DecodedValue::FunctionPtr(_)
    )
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
fn try_native_identity_compare(
    native: &std::cell::RefCell<crate::machine::NativeBinaryPlan>,
    registers: &crate::register_file::RegisterFile,
    lhs: usize,
    rhs: usize,
) -> Option<bool> {
    let left = registers.word_bits(lhs)?;
    let right = registers.word_bits(rhs)?;
    (identity_word(left) && identity_word(right))
        .then(|| native.borrow_mut().execute_tagged(left, right).ok())
        .flatten()
}

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
    dispatch.entry_started = true;
    let registers = unsafe { &*dispatch.registers };
    let environment = crate::locals::current();
    let _frame_roots = crate::cycle_collector::protect_frame(registers, &environment);
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
            dispatch.error_pc = Some(dispatch.pc);
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
    // This hook exists only in unit tests; production has no synthetic failure
    // branch in the native hot path.
    #[cfg(test)]
    if region.force_committed_status {
        return NATIVE_DISPATCH_COMMITTED_ERROR;
    }
    // Validate the complete window before invoking even the first canonical
    // handler. This is the atomic Unknown/fallback boundary.
    if validate_residual_window(region).is_err() {
        return 0;
    }
    // From this point on a handler or physical kernel may have committed
    // effects. Any failure is therefore an exit, never a retryable miss.
    region.entry_started = true;

    // Physical dispatch is selected from the generated declaration's ABI.
    // The kernel still verifies its operand wiring, while this boundary no
    // longer grows a second opcode-sequence allowlist.
    if matches!(
        region.abi,
        crate::stencil_select::RegionAbi::ArrayKernel
            | crate::stencil_select::RegionAbi::ArrayNumericLoop
    ) {
        match execute_composed_array_loop(region) {
            Some(Ok(transition)) => {
                crate::execution_trace::stencil_observation(
                    region.code,
                    region.pc,
                    "composed_array_loop",
                    true,
                );
                region.result = Some(transition);
                return NATIVE_DISPATCH_OK;
            }
            Some(Err(error)) => {
                region.error_pc = Some(region.pc + 4);
                region.error = Some(error);
                return NATIVE_DISPATCH_SEMANTIC_ERROR;
            }
            None => {}
        }
    }

    match execute_region_fallback(region) {
        Ok(transition) => {
            region.result = Some(transition);
            NATIVE_DISPATCH_OK
        }
        Err(crate::machine::NativeDispatchError::SemanticAt { pc, error }) => {
            region.error_pc = Some(pc);
            region.error = Some(error);
            NATIVE_DISPATCH_SEMANTIC_ERROR
        }
        Err(crate::machine::NativeDispatchError::Semantic(error)) => {
            region.error_pc = Some(region.pc);
            region.error = Some(error);
            NATIVE_DISPATCH_SEMANTIC_ERROR
        }
        Err(crate::machine::NativeDispatchError::Physical(_)) => 0,
        Err(crate::machine::NativeDispatchError::Committed(_)) => NATIVE_DISPATCH_COMMITTED_ERROR,
    }
}

/// Execute a selected region through canonical handlers after a physical
/// admission miss. The full window is validated before the first handler and
/// each intermediate operation must fall through normally; a guard or
/// completion that cannot be represented by the region is returned as the
/// already-committed transition, so callers resume from that exact state
/// without replaying a prefix.
pub(crate) fn execute_region_fallback(
    region: &mut NativeRegionContext<'_>,
) -> Result<DispatchTransition, crate::machine::NativeDispatchError> {
    // First validate the complete immutable window. This pass performs no
    // handler call, so a stale opcode/operand/edge can only reject before
    // effects.
    validate_residual_window(region)?;
    // Both bridge regions and guarded raw-region misses can execute canonical
    // handlers that allocate, throw, or re-enter. Root the live frame for the
    // whole semantic span; helper-free native entries do not take this path.
    let needs_roots = region.abi.contract().may_call_helper
        || region.operations.iter().copied().any(|opcode| {
            FRAME_ROOT_EFFECTS
                .iter()
                .copied()
            .any(|effect| opcode.has_effect(effect))
        });
    let _frame_roots = if needs_roots {
        let registers = unsafe { &*region.registers };
        let environment = crate::locals::current();
        Some(crate::cycle_collector::protect_frame(registers, &environment))
    } else {
        None
    };
    let mut last = None;
    for (offset, _expected) in region.operations.iter().copied().enumerate() {
        let pc = region.pc.checked_add(offset).ok_or_else(|| {
            crate::machine::NativeDispatchError::Physical("region pc overflow".into())
        })?;
        let instruction = region.code.instruction(pc).ok_or_else(|| {
            crate::machine::NativeDispatchError::Physical("region instruction missing".into())
        })?;
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
        }
        .map_err(|error| crate::machine::NativeDispatchError::SemanticAt { pc, error })?;
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
            return Ok(transition);
        }
        last = Some(transition);
    }
    last.ok_or_else(|| crate::machine::NativeDispatchError::Physical("empty region".into()))
}

fn validate_residual_window(
    region: &NativeRegionContext<'_>,
) -> Result<(), crate::machine::NativeDispatchError> {
    let end = region
        .pc
        .checked_add(region.operations.len())
        .ok_or_else(|| crate::machine::NativeDispatchError::Physical("region pc overflow".into()))?;
    for (offset, expected) in region.operations.iter().copied().enumerate() {
        let pc = region
            .pc
            .checked_add(offset)
            .ok_or_else(|| crate::machine::NativeDispatchError::Physical("region pc overflow".into()))?;
        let instruction = region.code.instruction(pc).ok_or_else(|| {
            crate::machine::NativeDispatchError::Physical("region instruction missing".into())
        })?;
        if instruction.opcode != expected
            || !expected.operands_are_canonical([instruction.a, instruction.b, instruction.c])
        {
            return Err(crate::machine::NativeDispatchError::Physical(
                "region operation changed during admission".into(),
            ));
        }
        match expected.control_operands(instruction) {
            crate::ir::ControlOperands::Return { .. } if pc + 1 != end => {
                return Err(crate::machine::NativeDispatchError::Physical(
                    "region returns before its declared boundary".into(),
                ));
            }
            crate::ir::ControlOperands::Branch { target, .. }
            | crate::ir::ControlOperands::Jump { target }
                if usize::from(target) < region.pc || usize::from(target) > end =>
            {
                return Err(crate::machine::NativeDispatchError::Physical(
                    "region successor leaves its declared boundary".into(),
                ));
            }
            crate::ir::ControlOperands::Loop { .. } => {
                return Err(crate::machine::NativeDispatchError::Physical(
                    "structured loop requires ordinary execution".into(),
                ));
            }
            _ => {}
        }
    }
    if region.operations.is_empty() {
        return Err(crate::machine::NativeDispatchError::Physical(
            "empty region".into(),
        ));
    }
    Ok(())
}

/// Execute the admitted numeric array block as one physical operation.  This
/// is deliberately a fixed-shape function generated from the declaration
/// above, rather than a second interpreter: operands come from the canonical
/// residual instructions and every failed proof returns `None` before any
/// register, environment, or array mutation occurs.
fn execute_composed_array_loop(
    region: &mut NativeRegionContext<'_>,
) -> Option<Result<DispatchTransition, VmError>> {
    let code = region.code;
    let pc = region.pc;
    let registers = unsafe { &mut *region.registers };
    let i0 = code.instruction(pc)?;
    let i1 = code.instruction(pc.checked_add(1)?)?;
    let i2 = code.instruction(pc.checked_add(2)?)?;
    let i3 = code.instruction(pc.checked_add(3)?)?;
    let i4 = code.instruction(pc.checked_add(4)?)?;
    if i0.opcode != crate::ir::Opcode::LoadLocalChecked
        || i1.opcode != crate::ir::Opcode::AGetI
        || i2.opcode != crate::ir::Opcode::Add
        || i3.opcode != crate::ir::Opcode::ASetI
        || i4.opcode != crate::ir::Opcode::Return
        || i0.flags != 0
        || i1.flags != 0
        || i2.flags != 0
        || i3.flags != 0
        || i4.flags != 0
    {
        return None;
    }
    // The store must update the same array/index loaded above, and the add
    // must consume the loaded element.  These relationships are the physical
    // wiring contract; adjacency alone is never sufficient admission.
    if i1.b != i0.a
        || i3.a != i0.a
        || i3.b != i1.c
        || i3.c != i2.a
        || i2.b != i1.a
        || i4.a != i2.a
        // LoadLocal and AGetI commit these destinations before the later
        // operands are consumed. Reject aliases that would make the early
        // scalar reads differ from sequential residual execution.
        || i1.c == i0.a
        // A fused entry reads the array/index/addend before committing any
        // destination registers.  Reject destination aliases that would make
        // those early reads differ from sequential residual execution.
        || i1.a == i0.a
        || i2.a == i0.a
        || i2.a == i1.a
        || i2.c == i0.a
        || i2.c == i1.a
    {
        return None;
    }
    let environment = crate::locals::current();
    if environment.is_uninitialized(i0.b) {
        return None;
    }
    let array_value = environment.get(i0.b);
    let crate::value::Value::Array(array) = array_value.clone() else {
        return None;
    };
    let index = registers.read_array_index(usize::from(i1.c))?;
    let addend = registers.read_number(usize::from(i2.c))?;
    if !array.is_plain_dense_access() {
        return None;
    }
    let element = array.dense_number_at(index)?;
    if !array.has_kernel_numeric_index(index) {
        return None;
    }
    let result = element + addend;
    if !crate::value::ArrayData::set_kernel_existing_f64(&array, index, result) {
        return None;
    }
    registers.write(usize::from(i0.a), array_value);
    registers.write_number(usize::from(i1.a), element);
    registers.write_number(usize::from(i2.a), result);
    let value = match read_register(registers, i4.a) {
        Ok(value) => value,
        Err(error) => return Some(Err(error)),
    };
    Some(Ok(handler_transition(
        pc + 4,
        Some(crate::completion::Completion::Return(value)),
    )))
}

/// Context consumed by the direct AArch64 array stencil.  The context is
/// intentionally a plain ABI record: no Rust references or VM objects cross
/// the executable boundary.  Admission keeps the owning `Rc` and exclusive
/// numeric borrow alive until the call returns; the kernel only touches the
/// proven backing words and publishes its result here.
#[repr(C)]
pub(crate) struct NativeArrayKernelContext {
    pub(crate) data: *mut f64,
    pub(crate) len: usize,
    pub(crate) index: usize,
    pub(crate) addend: f64,
    pub(crate) result: f64,
}

#[inline]
fn valid_f64_span(data: *const f64, len: usize) -> bool {
    if len == 0 {
        return true;
    }
    let Some(bytes) = len.checked_mul(std::mem::size_of::<f64>()) else {
        return false;
    };
    !data.is_null()
        && (data as usize) % std::mem::align_of::<f64>() == 0
        && (data as usize).checked_add(bytes).is_some()
}

impl NativeArrayKernelContext {
    #[inline]
    fn is_valid(&self) -> bool {
        self.index < self.len && valid_f64_span(self.data, self.len)
    }
}

#[repr(C)]
pub(crate) struct NativeArrayElementContext {
    pub(crate) element: *const f64,
    pub(crate) result: f64,
}

#[repr(C)]
pub(crate) struct NativeArrayElementStoreContext {
    pub(crate) element: *mut f64,
    pub(crate) value: f64,
}

#[repr(C)]
pub(crate) struct NativeArrayGetIncContext {
    pub(crate) element: *const f64,
    pub(crate) result: f64,
    pub(crate) index: usize,
    pub(crate) next_index: usize,
}

impl NativeArrayGetIncContext {
    #[inline]
    fn is_valid(&self) -> bool {
        !self.element.is_null()
            && (self.element as usize) % std::mem::align_of::<f64>() == 0
            && self.next_index == self.index
    }
}

impl NativeArrayElementStoreContext {
    #[inline]
    fn is_valid(&self) -> bool {
        !self.element.is_null()
            && (self.element as usize) % std::mem::align_of::<f64>() == 0
    }
}

impl NativeArrayElementContext {
    #[inline]
    fn is_valid(&self) -> bool {
        !self.element.is_null()
            && (self.element as usize) % std::mem::align_of::<f64>() == 0
    }
}

#[cfg(target_arch = "aarch64")]
fn execute_composed_array_get(
    region: &mut NativeRegionContext<'_>,
    invoke: impl FnOnce(*mut std::ffi::c_void) -> Result<u64, crate::stencil_arena::ArenaError>,
) -> Result<Option<DispatchTransition>, crate::machine::NativeDispatchError> {
    let code = region.code;
    let pc = region.pc;
    if region.registers.is_null() || region.operations.len() != 1 {
        return Ok(None);
    }
    let registers = unsafe { &mut *region.registers };
    let instruction = code.instruction(pc).ok_or_else(|| {
        crate::machine::NativeDispatchError::Physical("array get entry missing".into())
    })?;
    if instruction.opcode != crate::ir::Opcode::AGetI || instruction.flags != 0 {
        return Ok(None);
    }
    let Some(index) = registers.read_array_index(usize::from(instruction.c)) else {
        return Ok(None);
    };
    let Some(array) = registers
        .read_array(usize::from(instruction.b))
        .filter(|array| crate::locals::array_word_is_current(array))
        .filter(|array| array.is_plain_dense_access())
    else {
        return Ok(None);
    };
    if !array.has_kernel_numeric_index(index) {
        return Ok(None);
    }
    let words = array.numeric_kernel_words().ok_or_else(|| {
        crate::machine::NativeDispatchError::Physical("array numeric storage missing".into())
    })?;
    let Some(element) = words.get(index) else {
        return Ok(None);
    };
    let expected = *element;
    let mut kernel = NativeArrayElementContext {
        element: element as *const f64,
        result: expected,
    };
    if !kernel.is_valid() {
        return Ok(None);
    }
    region.native_entered = true;
    let status = invoke((&mut kernel as *mut NativeArrayElementContext).cast())
        .map_err(|error| {
            crate::machine::NativeDispatchError::Committed(format!(
                "array get execution failed after entry: {error:?}"
            ))
    })?;
    drop(words);
    if status != NATIVE_DISPATCH_OK {
        return Err(crate::machine::NativeDispatchError::Committed(
            "array get returned invalid status".into(),
        ));
    }
    if kernel.result.to_bits() != expected.to_bits() {
        return Err(crate::machine::NativeDispatchError::Committed(
            "array get changed the proven element".into(),
        ));
    }
    registers.write_number(usize::from(instruction.a), kernel.result);
    crate::execution_trace::stencil_iterations(code, pc, "composed_array_get", 1);
    Ok(Some(handler_transition(pc, None)))
}

#[cfg(target_arch = "aarch64")]
fn execute_composed_array_get_inc(
    region: &mut NativeRegionContext<'_>,
    invoke: impl FnOnce(*mut std::ffi::c_void) -> Result<u64, crate::stencil_arena::ArenaError>,
) -> Result<Option<DispatchTransition>, crate::machine::NativeDispatchError> {
    let code = region.code;
    let pc = region.pc;
    if region.registers.is_null() || region.operations.len() != 1 {
        return Ok(None);
    }
    let registers = unsafe { &mut *region.registers };
    let instruction = code.instruction(pc).ok_or_else(|| {
        crate::machine::NativeDispatchError::Physical("array get-inc entry missing".into())
    })?;
    if instruction.opcode != crate::ir::Opcode::AGetIInc || instruction.flags != 0 {
        return Ok(None);
    }
    // The canonical operation increments `c` before writing the loaded value
    // to `a`.  A destructive `a == c` alias therefore leaves the loaded value
    // in the index register; the raw kernel publishes `next_index` after the
    // result and would otherwise change the observable final value.  Reject
    // this relationship before entry so the ordinary handler preserves the
    // sequential write order.
    if instruction.a == instruction.c {
        return Ok(None);
    }
    let Some(index) = registers.read_array_index(usize::from(instruction.c)) else {
        return Ok(None);
    };
    let Some(next_index) = index.checked_add(1) else {
        return Ok(None);
    };
    let Some(array) = registers
        .read_array(usize::from(instruction.b))
        .filter(|array| crate::locals::array_word_is_current(array))
        .filter(|array| array.is_plain_dense_access())
    else {
        return Ok(None);
    };
    if !array.has_kernel_numeric_index(index) {
        return Ok(None);
    }
    let words = array.numeric_kernel_words().ok_or_else(|| {
        crate::machine::NativeDispatchError::Physical("array get-inc storage missing".into())
    })?;
    let Some(element) = words.get(index) else {
        return Ok(None);
    };
    let expected = *element;
    let mut kernel = NativeArrayGetIncContext {
        element: element as *const f64,
        result: expected,
        index,
        next_index: index,
    };
    if !kernel.is_valid() {
        return Ok(None);
    }
    region.native_entered = true;
    let status = invoke((&mut kernel as *mut NativeArrayGetIncContext).cast())
        .map_err(|error| {
            crate::machine::NativeDispatchError::Committed(format!(
                "array get-inc execution failed after entry: {error:?}"
            ))
        })?;
    drop(words);
    if status != NATIVE_DISPATCH_OK
        || kernel.result.to_bits() != expected.to_bits()
        || kernel.next_index != next_index
    {
        return Err(crate::machine::NativeDispatchError::Committed(
            "array get-inc returned invalid committed state".into(),
        ));
    }
    registers.write_number(usize::from(instruction.a), kernel.result);
    registers.write_number(usize::from(instruction.c), next_index as f64);
    crate::execution_trace::stencil_iterations(code, pc, "composed_array_get_inc", 1);
    Ok(Some(handler_transition(pc, None)))
}

#[cfg(target_arch = "aarch64")]
fn execute_composed_array_set(
    region: &mut NativeRegionContext<'_>,
    invoke: impl FnOnce(*mut std::ffi::c_void) -> Result<u64, crate::stencil_arena::ArenaError>,
) -> Result<Option<DispatchTransition>, crate::machine::NativeDispatchError> {
    let code = region.code;
    let pc = region.pc;
    if region.registers.is_null() || region.operations.len() != 1 {
        return Ok(None);
    }
    let registers = unsafe { &mut *region.registers };
    let instruction = code.instruction(pc).ok_or_else(|| {
        crate::machine::NativeDispatchError::Physical("array set entry missing".into())
    })?;
    if instruction.opcode != crate::ir::Opcode::ASetI || instruction.flags != 0 {
        return Ok(None);
    }
    let Some(index) = registers.read_array_index(usize::from(instruction.b)) else {
        return Ok(None);
    };
    let Some(value) = registers.read_number(usize::from(instruction.c)) else {
        return Ok(None);
    };
    let Some(array) = registers
        .read_array(usize::from(instruction.a))
        .filter(|array| crate::locals::array_word_is_current(array))
        .filter(|array| array.is_plain_dense_access())
    else {
        return Ok(None);
    };
    if !array.has_kernel_numeric_index(index) {
        return Ok(None);
    }
    let mut words = array.numeric_kernel_words_mut().ok_or_else(|| {
        crate::machine::NativeDispatchError::Physical("array numeric storage missing".into())
    })?;
    let Some(element) = words.get_mut(index) else {
        return Ok(None);
    };
    let expected = value;
    let mut kernel = NativeArrayElementStoreContext {
        element: element as *mut f64,
        value,
    };
    if !kernel.is_valid() {
        return Ok(None);
    }
    region.native_entered = true;
    let status = invoke((&mut kernel as *mut NativeArrayElementStoreContext).cast())
        .map_err(|error| {
            crate::machine::NativeDispatchError::Committed(format!(
                "array set execution failed after entry: {error:?}"
            ))
        })?;
    let written = *element;
    drop(words);
    if status != NATIVE_DISPATCH_OK || written.to_bits() != expected.to_bits() {
        return Err(crate::machine::NativeDispatchError::Committed(
            "array set returned invalid committed state".into(),
        ));
    }
    crate::execution_trace::stencil_iterations(code, pc, "composed_array_set", 1);
    Ok(Some(handler_transition(pc, None)))
}

#[cfg(target_arch = "aarch64")]
fn execute_composed_array_update(
    region: &mut NativeRegionContext<'_>,
    invoke: impl FnOnce(*mut std::ffi::c_void) -> Result<u64, crate::stencil_arena::ArenaError>,
) -> Result<Option<DispatchTransition>, crate::machine::NativeDispatchError> {
    let code = region.code;
    let pc = region.pc;
    if region.registers.is_null() || region.operations.len() != 3 {
        return Ok(None);
    }
    let registers = unsafe { &mut *region.registers };
    let Some(load) = code.instruction(pc) else { return Ok(None) };
    let Some(add) = code.instruction(pc + 1) else { return Ok(None) };
    let Some(store) = code.instruction(pc + 2) else { return Ok(None) };
    if load.opcode != crate::ir::Opcode::AGetI
        || !matches!(add.opcode, crate::ir::Opcode::Add | crate::ir::Opcode::AddConst)
        || store.opcode != crate::ir::Opcode::ASetI
        || load.flags != 0
        || add.flags != 0
        || store.flags != 0
        || add.b != load.a
        || store.c != add.a
        || add.a == load.c
        || add.a == load.b
    {
        return Ok(None);
    }
    let Some(index) = registers.read_array_index(usize::from(load.c)) else {
        return Ok(None);
    };
    let Some(store_index) = registers.read_array_index(usize::from(store.b)) else {
        return Ok(None);
    };
    if store_index != index {
        return Ok(None);
    }
    let addend = if add.opcode == crate::ir::Opcode::Add {
        if add.c == load.a {
            return Ok(None);
        }
        let Some(value) = registers.read_number(usize::from(add.c)) else {
            return Ok(None);
        };
        value
    } else {
        if add.add_const_is_left() {
            return Ok(None);
        }
        let Some(crate::ops::Constant::Number(value)) = code.constant(add.c) else {
            return Ok(None);
        };
        *value
    };
    let Some(array) = registers
        .read_array(usize::from(load.b))
        .filter(|array| crate::locals::array_word_is_current(array))
        .filter(|array| array.is_plain_dense_access())
    else {
        return Ok(None);
    };
    let Some(store_array) = registers
        .read_array(usize::from(store.a))
        .filter(|array| crate::locals::array_word_is_current(array))
        .filter(|array| array.is_plain_dense_access())
    else {
        return Ok(None);
    };
    if !std::ptr::eq(array, store_array) {
        return Ok(None);
    }
    if !array.has_kernel_numeric_index(index) {
        return Ok(None);
    }
    let mut words = array.numeric_kernel_words_mut().ok_or_else(|| {
        crate::machine::NativeDispatchError::Physical("array update storage missing".into())
    })?;
    if index >= words.len() {
        return Ok(None);
    }
    let element = unsafe { words.as_mut_ptr().add(index) };
    let initial = unsafe { *element };
    let mut kernel = NativeArrayKernelContext {
        data: words.as_mut_ptr(),
        len: words.len(),
        index,
        addend,
        result: initial,
    };
    if !kernel.is_valid() {
        return Ok(None);
    }
    region.native_entered = true;
    let status = invoke((&mut kernel as *mut NativeArrayKernelContext).cast())
        .map_err(|error| {
            crate::machine::NativeDispatchError::Committed(format!(
                "array update execution failed after entry: {error:?}"
            ))
        })?;
    let written = unsafe { *element };
    drop(words);
    if status != NATIVE_DISPATCH_OK || written.to_bits() != kernel.result.to_bits() {
        return Err(crate::machine::NativeDispatchError::Committed(
            "array update returned invalid committed state".into(),
        ));
    }
    registers.write_number(usize::from(load.a), initial);
    registers.write_number(usize::from(add.a), kernel.result);
    crate::execution_trace::stencil_iterations(code, pc, "composed_array_update", 1);
    Ok(Some(handler_transition(pc + 2, None)))
}

#[repr(C)]
pub(crate) struct NativeArrayLoopContext {
    pub data: *mut f64,
    pub len: usize,
    pub index: usize,
    pub end: usize,
    pub addend: f64,
    pub result: f64,
    pub interrupt: *const std::sync::atomic::AtomicBool,
}

impl NativeArrayLoopContext {
    #[inline]
    fn is_valid(&self) -> bool {
        self.index <= self.end
            && self.end <= self.len
            && valid_f64_span(self.data, self.len)
            && !self.interrupt.is_null()
    }
}

/// Enter the direct physical array kernel after one complete semantic guard.
/// `Ok(None)` means the facts were not strong enough; callers must execute the
/// complete canonical region and may not invoke the bytes.  A physical error
/// is distinct because the kernel was entered and cannot be safely retried.
#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[inline]
fn region_matches_generated_array_decl(
    region: &NativeRegionContext<'_>,
    key: crate::stencil_fact::RegionKey,
) -> bool {
    crate::stencil_select::select_region(key).is_some_and(|record| {
        matches!(
            record.abi,
            crate::stencil_select::RegionAbi::ArrayKernel
                | crate::stencil_select::RegionAbi::ArrayNumericLoop
        ) && record.operations == region.operations
    })
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
pub(crate) fn execute_composed_array_kernel(
    region: &mut NativeRegionContext<'_>,
    invoke: impl FnOnce(*mut std::ffi::c_void) -> Result<u64, crate::stencil_arena::ArenaError>,
) -> Result<Option<DispatchTransition>, crate::machine::NativeDispatchError> {
    #[cfg(target_arch = "aarch64")]
    if region_matches_generated_array_decl(
        region,
        crate::stencil_select::array_get_number_region_key(),
    ) {
        return execute_composed_array_get(region, invoke);
    }
    #[cfg(target_arch = "aarch64")]
    if region_matches_generated_array_decl(
        region,
        crate::stencil_select::array_get_inc_number_region_key(),
    ) {
        return execute_composed_array_get_inc(region, invoke);
    }
    #[cfg(target_arch = "aarch64")]
    if region_matches_generated_array_decl(
        region,
        crate::stencil_select::array_set_number_region_key(),
    ) {
        return execute_composed_array_set(region, invoke);
    }
    #[cfg(target_arch = "aarch64")]
    if region_matches_generated_array_decl(
        region,
        crate::stencil_select::array_numeric_update_region_key(),
    ) {
        return execute_composed_array_update(region, invoke);
    }
    #[cfg(target_arch = "aarch64")]
    if region_matches_generated_array_decl(
        region,
        crate::stencil_select::array_numeric_update_const_region_key(),
    ) {
        return execute_composed_array_update(region, invoke);
    }
    let code = region.code;
    let pc = region.pc;
    // The typed region ABI carries a borrowed register window.  A malformed
    // bridge must reject before touching it; otherwise a null C-ABI pointer
    // would turn an entry miss into undefined behavior rather than an
    // ordinary semantic fallback.
    if region.registers.is_null() {
        return Ok(None);
    }
    let registers = unsafe { &mut *region.registers };
    let i0 = code.instruction(pc).ok_or_else(|| {
        crate::machine::NativeDispatchError::Physical("array kernel entry missing".into())
    })?;
    let i1 = code.instruction(pc.checked_add(1).ok_or_else(|| {
        crate::machine::NativeDispatchError::Physical("array kernel pc overflow".into())
    })?).ok_or_else(|| {
        crate::machine::NativeDispatchError::Physical("array kernel index missing".into())
    })?;
    let i2 = code.instruction(pc.checked_add(2).ok_or_else(|| {
        crate::machine::NativeDispatchError::Physical("array kernel add missing".into())
    })?).ok_or_else(|| {
        crate::machine::NativeDispatchError::Physical("array kernel add missing".into())
    })?;
    let i3 = code.instruction(pc.checked_add(3).ok_or_else(|| {
        crate::machine::NativeDispatchError::Physical("array kernel store missing".into())
    })?).ok_or_else(|| {
        crate::machine::NativeDispatchError::Physical("array kernel store missing".into())
    })?;
    let i4 = code.instruction(pc.checked_add(4).ok_or_else(|| {
        crate::machine::NativeDispatchError::Physical("array kernel return missing".into())
    })?).ok_or_else(|| {
        crate::machine::NativeDispatchError::Physical("array kernel return missing".into())
    })?;
    if i0.opcode != crate::ir::Opcode::LoadLocalChecked
        || i1.opcode != crate::ir::Opcode::AGetI
        || i2.opcode != crate::ir::Opcode::Add
        || i3.opcode != crate::ir::Opcode::ASetI
        || i4.opcode != crate::ir::Opcode::Return
        || i0.flags != 0
        || i1.flags != 0
        || i2.flags != 0
        || i3.flags != 0
        || i4.flags != 0
        || i1.b != i0.a
        || i3.a != i0.a
        || i3.b != i1.c
        || i3.c != i2.a
        || i2.b != i1.a
        || i4.a != i2.a
        || i1.c == i0.a
        || i1.a == i0.a
        || i2.a == i0.a
        || i2.a == i1.a
        || i2.c == i0.a
        || i2.c == i1.a
    {
        return Ok(None);
    }
    let environment = crate::locals::current();
    if environment.is_uninitialized(i0.b) {
        return Ok(None);
    }
    let array_value = environment.get(i0.b);
    let crate::value::Value::Array(array) = array_value.clone() else {
        return Ok(None);
    };
    let Some(index) = registers.read_array_index(usize::from(i1.c)) else {
        return Ok(None);
    };
    let Some(addend) = registers.read_number(usize::from(i2.c)) else {
        return Ok(None);
    };
    if !array.is_plain_dense_access() || !array.has_kernel_numeric_index(index) {
        return Ok(None);
    }
    let element = array.dense_number_at(index).ok_or_else(|| {
        crate::machine::NativeDispatchError::Physical("array kernel element decode failed".into())
    })?;
    let mut words = match array.numeric_kernel_words_mut() {
        Some(words) if index < words.len() => words,
        _ => return Ok(None),
    };
    let mut kernel = NativeArrayKernelContext {
        data: words.as_mut_ptr(),
        len: words.len(),
        index,
        addend,
        result: element,
    };
    if !kernel.is_valid() {
        return Ok(None);
    }
    region.native_entered = true;
    let status = invoke((&mut kernel as *mut NativeArrayKernelContext).cast());
    drop(words);
    let status = status.map_err(|error| {
        crate::machine::NativeDispatchError::Committed(format!(
            "array kernel execution failed after entry: {error:?}"
        ))
    })?;
    if status != NATIVE_DISPATCH_OK {
        return Err(crate::machine::NativeDispatchError::Committed(
            "array kernel returned invalid status".into(),
        ));
    }
    // The exclusive borrow and all shape checks remain valid across the
    // machine call, so this setter only records the monotonic representation
    // fact and performs the same guarded numeric write for non-Cell payloads.
    if !crate::value::ArrayData::set_kernel_existing_f64(&array, index, kernel.result) {
        return Err(crate::machine::NativeDispatchError::Committed(
            "array kernel commit lost its proven slot".into(),
        ));
    }
    registers.write(usize::from(i0.a), array_value);
    registers.write_number(usize::from(i1.a), element);
    registers.write_number(usize::from(i2.a), kernel.result);
    crate::execution_trace::stencil_iterations(code, pc, "composed_array_kernel", 1);
    let value = read_register(registers, i4.a).map_err(|error| {
        crate::machine::NativeDispatchError::SemanticAt { pc: pc + 4, error }
    })?;
    Ok(Some(handler_transition(
        pc + 4,
        Some(crate::completion::Completion::Return(value)),
    )))
}

/// Admit the canonical lowered numeric-array loop shape. The declaration is
/// a contiguous residual span; operand relationships are checked from the
/// actual instructions, so opcode adjacency alone cannot authorize entry.
#[cfg(target_arch = "aarch64")]
pub(crate) fn execute_composed_array_numeric_loop(
    region: &mut NativeRegionContext<'_>,
    invoke: impl FnOnce(*mut std::ffi::c_void) -> Result<u64, crate::stencil_arena::ArenaError>,
) -> Result<Option<DispatchTransition>, crate::machine::NativeDispatchError> {
    let code = region.code;
    let pc = region.pc;
    // Both pointers are part of the raw loop-entry contract.  Rejecting a
    // malformed context before any register read keeps this boundary
    // fail-closed and preserves the complete ordinary path.
    if region.registers.is_null() || region.context.is_null() {
        return Ok(None);
    }
    let registers = unsafe { &mut *region.registers };
    if !region_matches_generated_array_decl(
        region,
        crate::stencil_select::array_numeric_loop_region_key(),
    ) || region.operations.len() != 19
    {
        return Ok(None);
    }
    let instructions = (0..19)
        .map(|offset| code.instruction(pc + offset))
        .collect::<Option<Vec<_>>>();
    let Some(instructions) = instructions else { return Ok(None) };
    if instructions
        .iter()
        .zip(region.operations.iter().copied())
        .any(|(instruction, opcode)| {
            instruction.opcode != opcode
                || (instruction.flags != 0 && opcode != crate::ir::Opcode::Binary)
        })
    {
        return Ok(None);
    }
    let [load_index, load_end, compare, branch, load_array, move_array, load_index_body,
        move_index, load_array_body, require_object, load_index_get, get, add_value, set,
        move_result, load_index_update, add_index, store_index, jump] = instructions.as_slice()
    else { return Ok(None) };
    if !array_loop_roles_are_disjoint(&instructions) {
        return Ok(None);
    }
    if compare.a != branch.a
        || compare.b != load_index.a
        || compare.c != load_end.a
        || usize::from(branch.b) != pc + 19
        || usize::from(jump.a) != pc
        || load_array.b != load_array_body.b
        || load_array_body.a != get.b
        || load_index_body.b != load_index_update.b
        || load_index_body.b != load_index_get.b
        || load_index.a != store_index.a
        || add_index.b != load_index_update.a
        || store_index.b != add_index.a
        || move_index.a != set.a
        || get.a != add_value.b
        || add_value.a != set.c
        || move_result.b != add_value.a
        || move_array.b != load_array.a
        || move_index.b != move_array.a
    {
        return Ok(None);
    }
    let Some(crate::ops::Op::RequireObjectCoercible { src }) = code.cold_at(pc + 9) else {
        return Ok(None);
    };
    if *src != load_array_body.a {
        return Ok(None);
    }
    let Some(crate::value::Value::Number(end)) = code.constant(load_end.b).map(Into::into) else {
        return Ok(None);
    };
    let Some(crate::value::Value::Number(addend)) = code
        .constant(add_value.c)
        .map(Into::into)
    else {
        return Ok(None);
    };
    if !end.is_finite() || end < 0.0 || end.fract() != 0.0 || addend.is_nan() {
        return Ok(None);
    }
    let Some(index) = registers.read_array_index(usize::from(load_index.a)) else {
        return Ok(None);
    };
    let end = end as usize;
    if index > end {
        return Ok(None);
    }
    if end.saturating_sub(index) > MAX_NATIVE_ARRAY_LOOP_ITERATIONS {
        return Ok(None);
    }
    let environment = crate::locals::current();
    if environment.is_uninitialized(load_array.b) {
        return Ok(None);
    }
    let array_value = environment.get(load_array.b);
    let crate::value::Value::Array(array) = array_value.clone() else {
        return Ok(None);
    };
    if !array.is_plain_dense_access() || end > array.len() {
        return Ok(None);
    }
    let mut words = match array.numeric_kernel_words_mut() {
        Some(words) if end <= words.len() => words,
        _ => return Ok(None),
    };
    let vm_context = unsafe { &*region.context };
    let interrupt = vm_context.interrupt_flag();
    if interrupt.is_null() {
        return Ok(None);
    }
    let mut kernel = NativeArrayLoopContext {
        data: words.as_mut_ptr(),
        len: words.len(),
        index,
        end,
        addend,
        result: 0.0,
        interrupt,
    };
    if !kernel.is_valid() {
        return Ok(None);
    }
    region.native_entered = true;
    let status = invoke((&mut kernel as *mut NativeArrayLoopContext).cast());
    drop(words);
    let status = status.map_err(|error| {
        crate::machine::NativeDispatchError::Committed(format!(
            "array loop kernel failed after entry: {error:?}"
        ))
    })?;
    if status == NATIVE_DISPATCH_INTERRUPT && kernel.index < end {
        vm_context.clear_interrupt();
        crate::locals::write(store_index.b, crate::value::Value::Number(kernel.index as f64));
        registers.write(usize::from(set.a), array_value);
        registers.write_number(usize::from(add_value.a), kernel.result);
        registers.write_number(usize::from(move_result.a), kernel.result);
        registers.write_number(usize::from(load_index.a), kernel.index as f64);
        crate::execution_trace::stencil_iterations(
            code,
            pc,
            "composed_array_numeric_loop_interrupt",
            kernel.index.saturating_sub(index),
        );
        return Ok(Some(resume_region_transition(pc)));
    }
    let completed_after_interrupt =
        status == NATIVE_DISPATCH_INTERRUPT && kernel.index == end;
    if (status != NATIVE_DISPATCH_OK && !completed_after_interrupt) || kernel.index != end {
        return Err(crate::machine::NativeDispatchError::Committed(
            "array loop kernel returned incomplete progress".into(),
        ));
    }
    crate::locals::write(store_index.b, crate::value::Value::Number(kernel.index as f64));
    registers.write(usize::from(set.a), array_value);
    registers.write_number(usize::from(add_value.a), kernel.result);
    registers.write_number(usize::from(move_result.a), kernel.result);
    registers.write_number(usize::from(load_index.a), kernel.index as f64);
    crate::execution_trace::stencil_iterations(
        code,
        pc,
        "composed_array_numeric_loop",
        kernel.index.saturating_sub(index),
    );
    Ok(Some(handler_transition(pc + 19, None)))
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
    let _environment_guard =
        crate::locals::EnvironmentGuard::install(Rc::clone(&environment));
    let step = run_baseline_completion_step_from_with_environment(
        code,
        plan,
        start,
        registers,
        context,
        Some(environment.as_ref()),
    )?;
    Ok((step.completion, step.next))
}

#[cfg(target_arch = "aarch64")]
fn array_loop_roles_are_disjoint(instructions: &[crate::ir::Instruction]) -> bool {
    // These are independent live values in the canonical loop lowering. The
    // remaining equalities (for example set.a == move_index.a) are explicit
    // forwarding edges checked by the caller and are intentionally omitted.
    let roles = [
        instructions[0].a,
        instructions[1].a,
        instructions[4].a,
        instructions[6].a,
        instructions[8].a,
        instructions[10].a,
        instructions[11].a,
        instructions[12].a,
        instructions[15].a,
        instructions[16].a,
    ];
    roles.iter().enumerate().all(|(index, role)| {
        roles[index + 1..].iter().all(|other| other != role)
    })
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
        environment: None,
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
    let _environment_guard =
        crate::locals::EnvironmentGuard::install(Rc::clone(&environment));
    let mut dispatch = DispatchState {
        code,
        registers,
        context,
        environment: Some(environment.as_ref()),
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
        environment: None,
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
    // The frame guard owns the current environment for this whole drive. Use
    // its stable pointer rather than cloning an Rc or reopening TLS for every
    // local instruction; the pointer is valid only for this callback.
    let result = crate::locals::with_current_ref(|environment| {
        run_baseline_completion_step_from_with_hook(
            code,
            plan,
            start,
            registers,
            context,
            environment,
            &mut count,
        )
    });
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
        let (result, native_executed) = {
            let mut plan = native.borrow_mut();
            let result = plan.execute(code, start, registers, context);
            (result, plan.last_native_execution())
        };
        match result {
            Ok(transition) => {
                crate::execution_trace::stencil_observation(
                    code,
                    start,
                    "region",
                    native_executed,
                );
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
                crate::execution_trace::stencil_observation(
                    code,
                    start,
                    "region",
                    native_executed,
                );
                return completion_step_after_error(registers, error, start + 1)
                    .map(|step| (step.completion, step.next));
            }
            Err(crate::machine::NativeDispatchError::SemanticAt { pc, error }) => {
                crate::execution_trace::stencil_observation(
                    code,
                    start,
                    "region",
                    native_executed,
                );
                return completion_step_after_error(registers, error, pc + 1)
                    .map(|step| (step.completion, step.next));
            }
            Err(crate::machine::NativeDispatchError::Physical(_)) => {
                crate::execution_trace::stencil_observation(code, start, "region", false);
                crate::execution_trace::leaf_rejection("optimizing_native_region");
            }
            Err(crate::machine::NativeDispatchError::Committed(message)) => {
                return Err(VmError::EvalError(format!(
                    "committed native region failure: {message}"
                )));
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
            if let Some(native) = entry.native_load_const.as_ref() {
                if let Ok(bits) = native.borrow_mut().execute() {
                    if registers
                        .write_tagged_bits(usize::from(instruction.a), bits)
                        .is_some()
                    {
                        crate::execution_trace::stencil_observation(
                            code, start, "load_const", true,
                        );
                        crate::execution_trace::event(crate::execution_trace::Event::LeafHit);
                        return Ok((crate::completion::Completion::Normal, start + 1));
                    }
                }
                crate::execution_trace::stencil_observation(code, start, "load_const", false);
                crate::execution_trace::leaf_rejection("optimizing_native_load_const");
            }
            let Some((_, value)) = code.constant_at(start) else {
                return execute_baseline_code_step_from(code, baseline, start, registers, context);
            };
            write_constant(registers, instruction.a, value);
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
            if let Some(native) = entry.native_truthiness.as_ref() {
                if let Some(truthy) = try_native_word_truthiness(
                    native,
                    registers,
                    usize::from(instruction.a),
                ) {
                    crate::execution_trace::stencil_observation(
                        code, start, "truthy_word", true,
                    );
                    crate::execution_trace::event(crate::execution_trace::Event::LeafHit);
                    return Ok((
                        crate::completion::Completion::Normal,
                        if truthy { start + 1 } else { usize::from(instruction.b) },
                    ));
                }
                if let Some(value) = registers.read_number(usize::from(instruction.a)) {
                    if let Ok(truthy) = native.borrow_mut().execute(value) {
                        crate::execution_trace::stencil_observation(
                            code, start, "truthy_number", true,
                        );
                        crate::execution_trace::event(crate::execution_trace::Event::LeafHit);
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
                crate::execution_trace::stencil_observation(code, start, "truthy_number", false);
                crate::execution_trace::leaf_rejection("optimizing_native_truthy_number");
            }
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
    if instruction.opcode == crate::ir::Opcode::Unary
        && instruction.flags == crate::ir::compact_unary_id(crate::ops::UnaryOp::Not)
    {
        if let Some(native) = entry.native_truthiness.as_ref() {
            if let Some(result) = try_native_logical_not(
                native,
                registers,
                usize::from(instruction.b),
            ) {
                crate::execution_trace::stencil_observation(code, start, "logical_not", true);
                crate::execution_trace::event(crate::execution_trace::Event::LeafHit);
                write_value(registers, instruction.a, Value::Boolean(result));
                return Ok((crate::completion::Completion::Normal, start + 1));
            }
            crate::execution_trace::stencil_observation(code, start, "logical_not", false);
            crate::execution_trace::leaf_rejection("optimizing_native_logical_not");
        }
    }
    if instruction.opcode == crate::ir::Opcode::LoadLocal {
        if let Some(native) = entry.native_load_local.as_ref() {
            let source = crate::locals::with_current_ref(|environment| {
                environment.and_then(|environment| {
                    environment.proven_word_ptr(instruction.b)
                })
            });
            if let Some(source) = source {
                if let Ok(bits) = native.borrow_mut().execute(source) {
                    if registers
                        .write_tagged_bits(usize::from(instruction.a), bits)
                        .is_some()
                    {
                        crate::execution_trace::stencil_observation(
                            code, start, "load_local", true,
                        );
                        crate::execution_trace::event(crate::execution_trace::Event::LeafHit);
                        return Ok((crate::completion::Completion::Normal, start + 1));
                    }
                }
            }
            crate::execution_trace::stencil_observation(code, start, "load_local", false);
            crate::execution_trace::leaf_rejection("optimizing_native_load_local");
        }
    }
    if instruction.opcode == crate::ir::Opcode::StoreLocal {
        if let Some(native) = entry.native_store_local.as_ref() {
            let can_store = crate::locals::with_current_ref(|environment| {
                environment.is_some_and(|environment| {
                    environment.can_store_proven_tagged_bits(instruction.a)
                })
            });
            if !can_store {
                crate::execution_trace::stencil_observation(
                    code, start, "store_local", false,
                );
                crate::execution_trace::leaf_rejection("optimizing_native_store_local_guard");
            } else if let Some(source) = registers.word_ptr(usize::from(instruction.b)) {
                if let Ok(bits) = native.borrow_mut().execute(source) {
                    let stored = crate::locals::with_current_ref(|environment| {
                        environment.is_some_and(|environment| {
                            environment.store_proven_tagged_bits(instruction.a, bits)
                        })
                    });
                    if stored {
                        crate::execution_trace::stencil_observation(
                            code, start, "store_local", true,
                        );
                        crate::execution_trace::event(crate::execution_trace::Event::LeafHit);
                        return Ok((crate::completion::Completion::Normal, start + 1));
                    }
                }
            }
            crate::execution_trace::stencil_observation(code, start, "store_local", false);
            crate::execution_trace::leaf_rejection("optimizing_native_store_local");
        }
    }
    if instruction.opcode == crate::ir::Opcode::Move && instruction.flags == 0 {
        if let Some(native) = entry.native_move.as_ref() {
            if let Some(source) = registers.word_ptr(usize::from(instruction.b)) {
                if let Ok(bits) = native.borrow_mut().execute(source) {
                    if registers
                        .write_tagged_bits(usize::from(instruction.a), bits)
                        .is_some()
                    {
                        crate::execution_trace::stencil_observation(
                            code, start, "move", true,
                        );
                        crate::execution_trace::event(crate::execution_trace::Event::LeafHit);
                        return Ok((crate::completion::Completion::Normal, start + 1));
                    }
                }
            }
            crate::execution_trace::stencil_observation(code, start, "move", false);
            crate::execution_trace::leaf_rejection("optimizing_native_move");
        }
        if copy_register(registers, instruction.a, instruction.b).is_ok() {
            return Ok((crate::completion::Completion::Normal, start + 1));
        }
    }
    if instruction.opcode == crate::ir::Opcode::Unary
        && instruction.flags == crate::ir::compact_unary_id(crate::ops::UnaryOp::IsNullish)
    {
        if let Some(native) = entry.native_nullish.as_ref() {
            if let Some(bits) = registers.word_bits(usize::from(instruction.b)) {
                if let Ok(result) = native.borrow_mut().execute(bits) {
                    registers.write_boolean(usize::from(instruction.a), result);
                    crate::execution_trace::stencil_observation(
                        code, start, "nullish_word", true,
                    );
                    crate::execution_trace::event(crate::execution_trace::Event::LeafHit);
                    return Ok((crate::completion::Completion::Normal, start + 1));
                }
            }
            crate::execution_trace::stencil_observation(code, start, "nullish_word", false);
            crate::execution_trace::leaf_rejection("optimizing_native_nullish_word");
        }
    }
    if instruction.opcode == crate::ir::Opcode::Unary {
        if let Some(native) = entry.native_unary.as_ref() {
            if let Some(value) = registers.read_number(usize::from(instruction.b)) {
                if let Ok(result) = native.borrow_mut().execute(value) {
                    write_value(registers, instruction.a, Value::Number(result));
                    crate::execution_trace::stencil_observation(code, start, "unary", true);
                    crate::execution_trace::event(crate::execution_trace::Event::LeafHit);
                    return Ok((crate::completion::Completion::Normal, start + 1));
                }
            }
            crate::execution_trace::stencil_observation(code, start, "unary", false);
            crate::execution_trace::leaf_rejection("optimizing_native_unary");
        }
    }
    if instruction.opcode == crate::ir::Opcode::SetN && instruction.flags == 0 {
        if let Some(native) = entry.native_store_property.as_ref() {
            if try_native_property_store(
                native,
                code,
                start,
                registers,
                instruction.a,
                instruction.b,
            ) {
                crate::execution_trace::stencil_observation(
                    code,
                    start,
                    "property_store",
                    true,
                );
                crate::execution_trace::event(crate::execution_trace::Event::LeafHit);
                return Ok((crate::completion::Completion::Normal, start + 1));
            }
            crate::execution_trace::stencil_observation(code, start, "property_store", false);
            crate::execution_trace::leaf_rejection("optimizing_native_property_store");
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
                        .or_else(|| {
                            object
                                .hot_properties()
                                .position_rev(key)
                                .is_none()
                                .then(|| quickened_prototype_slot_data(&object, key))
                                .flatten()
                        })
                });
            if let Some(slot) = slot {
                if let Some(site) = code.quickening_site(start) {
                    let site = site.borrow();
                    if let Ok(bits) = native.borrow_mut().execute(slot, &site) {
                        if registers
                            .write_tagged_bits(usize::from(instruction.a), bits)
                            .is_some()
                        {
                            crate::execution_trace::stencil_observation(
                                code, start, "property", true,
                            );
                            crate::execution_trace::event(crate::execution_trace::Event::LeafHit);
                            return Ok((crate::completion::Completion::Normal, start + 1));
                        }
                    }
                }
            }
            crate::execution_trace::stencil_observation(code, start, "property", false);
            crate::execution_trace::leaf_rejection("optimizing_native_property");
        }
    }
    if instruction.opcode == crate::ir::Opcode::Binary {
        if let Some(native) = entry.native_binary.as_ref() {
            if let Some(result) = try_native_identity_compare(
                native,
                registers,
                usize::from(instruction.b),
                usize::from(instruction.c),
            ) {
                crate::execution_trace::stencil_observation(code, start, "binary_word", true);
                crate::execution_trace::event(crate::execution_trace::Event::LeafHit);
                write_value(registers, instruction.a, Value::Boolean(result));
                return Ok((crate::completion::Completion::Normal, start + 1));
            }
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
        } else if instruction.opcode == crate::ir::Opcode::IncI {
            registers
                .read_number(usize::from(instruction.b))
                .map(|value| (value, if instruction.flags == 0 { 1.0 } else { -1.0 }))
        } else {
            registers.read_number_pair(
                usize::from(instruction.b),
                usize::from(instruction.c),
            )
        };
        if let Some((lhs, rhs)) = operands {
            let returns_boolean = native.borrow().returns_boolean();
            let result = { native.borrow_mut().execute(lhs, rhs) };
            if let Ok(result) = result {
                crate::execution_trace::stencil_observation(
                    code,
                    start,
                    if instruction.opcode == crate::ir::Opcode::IncI {
                        "increment"
                    } else {
                        "binary"
                    },
                    true,
                );
                let value = if returns_boolean {
                    Value::Boolean(result != 0.0)
                } else {
                    Value::Number(result)
                };
                write_value(registers, instruction.a, value);
                crate::execution_trace::event(crate::execution_trace::Event::LeafHit);
                return Ok((crate::completion::Completion::Normal, start + 1));
            }
        }
        crate::execution_trace::stencil_observation(
            code,
            start,
            if instruction.opcode == crate::ir::Opcode::IncI {
                "increment"
            } else {
                "binary"
            },
            false,
        );
        crate::execution_trace::leaf_rejection("optimizing_native_execution");
    }
    if let Some(native) = entry.native_dispatch.as_ref() {
        match native
            .borrow_mut()
            .execute(code, start, entry.baseline, registers, context)
        {
            Ok(transition) => {
                crate::execution_trace::stencil_observation(code, start, "dispatch", true);
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
                crate::execution_trace::stencil_observation(code, start, "dispatch", true);
                return completion_step_after_error(registers, error, start + 1)
                    .map(|step| (step.completion, step.next));
            }
            Err(crate::machine::NativeDispatchError::SemanticAt { pc, error }) => {
                crate::execution_trace::stencil_observation(code, start, "dispatch", true);
                return completion_step_after_error(registers, error, pc + 1)
                    .map(|step| (step.completion, step.next));
            }
            Err(crate::machine::NativeDispatchError::Physical(_)) => {
                crate::execution_trace::stencil_observation(code, start, "dispatch", false);
                crate::execution_trace::leaf_rejection("optimizing_native_dispatch");
            }
            Err(crate::machine::NativeDispatchError::Committed(message)) => {
                return Err(VmError::EvalError(format!(
                    "committed native dispatch failure: {message}"
                )));
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
    run_baseline_completion_step_from_with_environment(
        code, plan, start, registers, context, None,
    )
}

fn run_baseline_completion_step_from_with_environment(
    code: crate::machine::CodeView<'_>,
    plan: &crate::machine::BaselinePlan,
    start: usize,
    registers: &mut crate::register_file::RegisterFile,
    context: &VmContext,
    environment: Option<&crate::environment::Environment>,
) -> Result<CompletionStep, VmError> {
    let mut no_profile = || {};
    run_baseline_completion_step_from_with_hook(
        code,
        plan,
        start,
        registers,
        context,
        environment,
        &mut no_profile,
    )
}

fn run_baseline_completion_step_from_with_hook<F: FnMut()>(
    code: crate::machine::CodeView<'_>,
    plan: &crate::machine::BaselinePlan,
    start: usize,
    registers: &mut crate::register_file::RegisterFile,
    context: &VmContext,
    environment: Option<&crate::environment::Environment>,
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
        // A composed region is a baseline admission consequence, not an
        // optimizing-view-only experiment. Try it at the same canonical
        // residual PC used by ordinary execution. A physical rejection is a
        // pre-entry miss and falls through to the existing per-op handlers;
        // every post-entry outcome is propagated without replay.
        if let Some(native) = plan.native_region_at(pc) {
            let (region_result, native_executed) = {
                let mut native = native.borrow_mut();
                let result = native.execute(code, pc, registers, context);
                (result, native.last_native_execution())
            };
            crate::execution_trace::stencil_observation(
                code,
                pc,
                "baseline_region",
                native_executed,
            );
            match region_result {
                Ok(transition) => {
                    crate::execution_trace::event(crate::execution_trace::Event::LeafHit);
                    let target = transition.target;
                    let next = match target {
                        DispatchTarget::Callee(next) => next,
                        DispatchTarget::Exit => transition.next_pc,
                    };
                    if let Some(completion) = transition
                        .completion
                        .filter(|value| !matches!(value, crate::completion::Completion::Normal))
                    {
                        return completion_step_after_transition(registers, completion, next);
                    }
                    match target {
                        DispatchTarget::Callee(_) => {
                            pc = next;
                            continue;
                        }
                        DispatchTarget::Exit => {
                            return completion_step_after_transition(
                                registers,
                                crate::completion::Completion::Normal,
                                next,
                            );
                        }
                    }
                }
                Err(crate::machine::NativeDispatchError::Semantic(error)) => {
                    return completion_step_after_error(registers, error, pc + 1);
                }
                Err(crate::machine::NativeDispatchError::SemanticAt { pc: fault_pc, error }) => {
                    return completion_step_after_error(registers, error, fault_pc + 1);
                }
                Err(crate::machine::NativeDispatchError::Committed(message)) => {
                    return Err(VmError::EvalError(format!(
                        "committed native region failure: {message}"
                    )));
                }
                Err(crate::machine::NativeDispatchError::Physical(_)) => {
                    crate::execution_trace::leaf_rejection("baseline_native_region");
                }
            }
        }
        // Indexed property lowering emits an explicit coercibility check before
        // AGetI/ASetI. Once the object word is already known non-nullish, the
        // check has no observable work left: elide it and keep the canonical
        // indexed handler as the semantic owner. This applies equally to
        // baseline plans (the hot path used by loop fragments), not just the
        // unplanned interpreter dispatcher.
        if skip_proven_object_coercible(code, pc, instruction, registers) {
            pc += 1;
            continue;
        }
        // Proven lexical transfers are pure frame-word operations. When the
        // baseline entry owns its Environment, keep the same frame fact for
        // both load and store instead of reopening TLS for each instruction.
        if let Some(environment) = environment {
            match instruction.opcode {
                crate::ir::Opcode::LoadLocal => {
                    if let Some(native) = plan.native_load_local_at(pc) {
                        let source = environment.proven_word_ptr(instruction.b);
                        if let Some(source) = source {
                            if let Ok(bits) = native.borrow_mut().execute(source) {
                                if registers
                                    .write_tagged_bits(usize::from(instruction.a), bits)
                                    .is_some()
                                {
                                    crate::execution_trace::stencil_observation(
                                        code, pc, "load_local", true,
                                    );
                                    crate::execution_trace::event(
                                        crate::execution_trace::Event::LeafHit,
                                    );
                                    pc += 1;
                                    continue;
                                }
                            }
                        }
                        crate::execution_trace::stencil_observation(
                            code, pc, "load_local", false,
                        );
                        crate::execution_trace::leaf_rejection("native_load_local");
                    }
                    crate::locals::load_proven_in(
                        environment,
                        registers,
                        instruction.a,
                        instruction.b,
                    )?;
                    pc += 1;
                    continue;
                }
                crate::ir::Opcode::StoreLocal => {
                    if let Some(native) = plan.native_store_local_at(pc) {
                        if !environment.can_store_proven_tagged_bits(instruction.a) {
                            crate::execution_trace::stencil_observation(
                                code, pc, "store_local", false,
                            );
                            crate::execution_trace::leaf_rejection("native_store_local_guard");
                            crate::locals::store_proven_in(
                                environment,
                                registers,
                                instruction.a,
                                instruction.b,
                            )?;
                            pc += 1;
                            continue;
                        }
                        if let Some(source) = registers.word_ptr(usize::from(instruction.b)) {
                            if let Ok(bits) = native.borrow_mut().execute(source) {
                                if environment.store_proven_tagged_bits(instruction.a, bits) {
                                    crate::execution_trace::stencil_observation(
                                        code, pc, "store_local", true,
                                    );
                                    crate::execution_trace::event(
                                        crate::execution_trace::Event::LeafHit,
                                    );
                                    pc += 1;
                                    continue;
                                }
                            }
                        }
                        crate::execution_trace::stencil_observation(
                            code, pc, "store_local", false,
                        );
                        crate::execution_trace::leaf_rejection("native_store_local");
                    }
                    crate::locals::store_proven_in(
                        environment,
                        registers,
                        instruction.a,
                        instruction.b,
                    )?;
                    pc += 1;
                    continue;
                }
                _ => {}
            }
        }
        // Build-generated Number binary stencils are real baseline machine-code
        // leaves. Admission is structural and the numeric guard is checked
        // before entering them; every other value uses the canonical handler.
        // The lookup is per instruction, so a proven leaf can remain native
        // inside an otherwise ordinary function body rather than requiring a
        // whole-function shape match.
        if instruction.opcode == crate::ir::Opcode::LoadConst {
            if let Some(native) = plan.native_load_const_at(pc) {
                if let Ok(bits) = native.borrow_mut().execute() {
                    if registers
                        .write_tagged_bits(usize::from(instruction.a), bits)
                        .is_some()
                    {
                        crate::execution_trace::stencil_observation(
                            code, pc, "load_const", true,
                        );
                        crate::execution_trace::event(crate::execution_trace::Event::LeafHit);
                        pc += 1;
                        continue;
                    }
                }
                crate::execution_trace::stencil_observation(code, pc, "load_const", false);
                crate::execution_trace::leaf_rejection("native_load_const");
            }
        }
        if instruction.opcode == crate::ir::Opcode::JumpIfFalse {
            if let Some(native) = plan.native_truthiness_at(pc) {
                if let Some(truthy) = try_native_word_truthiness(
                    native,
                    registers,
                    usize::from(instruction.a),
                ) {
                    crate::execution_trace::stencil_observation(code, pc, "truthy_word", true);
                    crate::execution_trace::event(crate::execution_trace::Event::LeafHit);
                    pc = if truthy { pc + 1 } else { usize::from(instruction.b) };
                    continue;
                }
                if let Some(value) = registers.read_number(usize::from(instruction.a)) {
                    if let Ok(truthy) = native.borrow_mut().execute(value) {
                        crate::execution_trace::stencil_observation(
                            code, pc, "truthy_number", true,
                        );
                        crate::execution_trace::event(crate::execution_trace::Event::LeafHit);
                        pc = if truthy {
                            pc + 1
                        } else {
                            usize::from(instruction.b)
                        };
                        continue;
                    }
                }
                crate::execution_trace::stencil_observation(code, pc, "truthy_number", false);
                crate::execution_trace::leaf_rejection("native_truthy_number");
            }
        }
        if instruction.opcode == crate::ir::Opcode::Unary
            && instruction.flags == crate::ir::compact_unary_id(crate::ops::UnaryOp::Not)
        {
            if let Some(native) = plan.native_truthiness_at(pc) {
                if let Some(result) = try_native_logical_not(
                    native,
                    registers,
                    usize::from(instruction.b),
                ) {
                    registers.write_boolean(usize::from(instruction.a), result);
                    crate::execution_trace::stencil_observation(
                        code, pc, "logical_not", true,
                    );
                    crate::execution_trace::event(crate::execution_trace::Event::LeafHit);
                    pc += 1;
                    continue;
                }
                crate::execution_trace::stencil_observation(code, pc, "logical_not", false);
                crate::execution_trace::leaf_rejection("native_logical_not");
            }
        }
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
                            .or_else(|| {
                                object
                                    .hot_properties()
                                    .position_rev(key)
                                    .is_none()
                                    .then(|| quickened_prototype_slot_data(&object, key))
                                    .flatten()
                            })
                    });
                if let Some(slot) = slot {
                    if let Some(site) = code.quickening_site(pc) {
                        let site = site.borrow();
                        if let Ok(bits) = native.borrow_mut().execute(slot, &site) {
                            if registers
                                .write_tagged_bits(usize::from(instruction.a), bits)
                                .is_some()
                            {
                                crate::execution_trace::stencil_observation(
                                    code, pc, "property", true,
                                );
                                crate::execution_trace::event(
                                    crate::execution_trace::Event::LeafHit,
                                );
                                pc += 1;
                                continue;
                            }
                        }
                    }
                    crate::execution_trace::stencil_observation(code, pc, "property", false);
                    crate::execution_trace::leaf_rejection("native_property");
                }
            }
        }
        if instruction.opcode == crate::ir::Opcode::SetN && instruction.flags == 0 {
            if let Some(native) = plan.native_store_property_at(pc) {
                if try_native_property_store(
                    native,
                    code,
                    pc,
                    registers,
                    instruction.a,
                    instruction.b,
                ) {
                    crate::execution_trace::stencil_observation(
                        code,
                        pc,
                        "property_store",
                        true,
                    );
                    crate::execution_trace::event(crate::execution_trace::Event::LeafHit);
                    pc += 1;
                    continue;
                }
                crate::execution_trace::stencil_observation(code, pc, "property_store", false);
                crate::execution_trace::leaf_rejection("native_property_store");
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
                            crate::execution_trace::stencil_observation(
                                code, pc, "move", true,
                            );
                            crate::execution_trace::event(crate::execution_trace::Event::LeafHit);
                            pc += 1;
                            continue;
                        }
                    }
                }
                crate::execution_trace::stencil_observation(code, pc, "move", false);
                crate::execution_trace::leaf_rejection("native_move");
            }
        }
        if instruction.opcode == crate::ir::Opcode::Unary
            && instruction.flags == crate::ir::compact_unary_id(crate::ops::UnaryOp::IsNullish)
        {
            if let Some(native) = plan.native_nullish_at(pc) {
                if let Some(bits) = registers.word_bits(usize::from(instruction.b)) {
                    if let Ok(result) = native.borrow_mut().execute(bits) {
                        registers.write_boolean(usize::from(instruction.a), result);
                        crate::execution_trace::stencil_observation(
                            code, pc, "nullish_word", true,
                        );
                        crate::execution_trace::event(crate::execution_trace::Event::LeafHit);
                        pc += 1;
                        continue;
                    }
                }
                crate::execution_trace::stencil_observation(code, pc, "nullish_word", false);
                crate::execution_trace::leaf_rejection("native_nullish_word");
            }
        }
        if instruction.opcode == crate::ir::Opcode::Unary {
            if let Some(native) = plan.native_unary_at(pc) {
                if let Some(value) = registers.read_number(usize::from(instruction.b)) {
                    if let Ok(result) = native.borrow_mut().execute(value) {
                        write_value(registers, instruction.a, Value::Number(result));
                        crate::execution_trace::stencil_observation(code, pc, "unary", true);
                        crate::execution_trace::event(crate::execution_trace::Event::LeafHit);
                        pc += 1;
                        continue;
                    }
                }
                crate::execution_trace::stencil_observation(code, pc, "unary", false);
                crate::execution_trace::leaf_rejection("native_unary");
            }
        }
        if instruction.opcode == crate::ir::Opcode::Add && instruction.flags == 0 {
            if let Some(native) = plan.native_add_chain_at(pc) {
                let next = plan.instruction(pc + 1);
                let chain_shape = next.filter(|next| {
                    next.opcode == crate::ir::Opcode::Add
                        && next.flags == 0
                        && next.b == instruction.a
                        // Reading the first result as the second add's third
                        // operand would require an intermediate materialized
                        // value; leave that alias to the complete handlers.
                        && next.c != instruction.a
                });
                let operands = chain_shape.and_then(|next| {
                    let first = registers.read_number_pair(
                        usize::from(instruction.b),
                        usize::from(instruction.c),
                    )?;
                    let third = registers.read_number(usize::from(next.c))?;
                    Some((first.0, first.1, third, next.a))
                });
                if let Some((lhs, rhs, third, destination)) = operands {
                    if let Ok(result) = native.borrow_mut().execute(lhs, rhs, third) {
                        crate::execution_trace::stencil_observation(
                            code, pc, "add_chain", true,
                        );
                        write_value(registers, destination, Value::Number(result));
                        crate::execution_trace::event(crate::execution_trace::Event::LeafHit);
                        pc += 2;
                        continue;
                    }
                    crate::execution_trace::stencil_observation(code, pc, "add_chain", false);
                    crate::execution_trace::leaf_rejection("native_add_chain");
                } else {
                    crate::execution_trace::stencil_observation(code, pc, "add_chain", false);
                    crate::execution_trace::event(crate::execution_trace::Event::LeafReject);
                    crate::execution_trace::leaf_rejection("native_add_chain_guard");
                }
            }
        }
        if instruction.opcode == crate::ir::Opcode::Binary {
            if let Some(native) = plan.native_binary_at(pc) {
                if let Some(result) = try_native_identity_compare(
                    native,
                    registers,
                    usize::from(instruction.b),
                    usize::from(instruction.c),
                ) {
                    crate::execution_trace::stencil_observation(code, pc, "binary_word", true);
                    crate::execution_trace::event(crate::execution_trace::Event::LeafHit);
                    registers.write_boolean(usize::from(instruction.a), result);
                    pc += 1;
                    continue;
                }
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
            } else if instruction.opcode == crate::ir::Opcode::IncI {
                registers
                    .read_number(usize::from(instruction.b))
                    .map(|value| (value, if instruction.flags == 0 { 1.0 } else { -1.0 }))
            } else {
                registers.read_number_pair(
                    usize::from(instruction.b),
                    usize::from(instruction.c),
                )
            };
            if let Some((lhs, rhs)) = operands {
                let returns_boolean = native.borrow().returns_boolean();
                let result = { native.borrow_mut().execute(lhs, rhs) };
                if let Ok(result) = result {
                    crate::execution_trace::stencil_observation(
                        code,
                        pc,
                        if instruction.opcode == crate::ir::Opcode::IncI {
                            "increment"
                        } else {
                            "binary"
                        },
                        true,
                    );
                    crate::execution_trace::event(crate::execution_trace::Event::LeafHit);
                    let value = if returns_boolean {
                        Value::Boolean(result != 0.0)
                    } else {
                        Value::Number(result)
                    };
                    write_value(registers, instruction.a, value);
                    pc += 1;
                    continue;
                }
                crate::execution_trace::stencil_observation(
                    code,
                    pc,
                    if instruction.opcode == crate::ir::Opcode::IncI {
                        "increment"
                    } else {
                        "binary"
                    },
                    false,
                );
                crate::execution_trace::leaf_rejection("native_execution");
            } else {
                crate::execution_trace::stencil_observation(code, pc, "binary", false);
                crate::execution_trace::event(crate::execution_trace::Event::LeafReject);
                crate::execution_trace::leaf_rejection("number_guard");
            }
        }
        if skip_proven_object_coercible(code, pc, instruction, registers) {
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
                    crate::execution_trace::stencil_observation(code, pc, "dispatch", true);
                    crate::execution_trace::event(crate::execution_trace::Event::LeafHit);
                    transition
                }
                Err(crate::machine::NativeDispatchError::Semantic(error)) => {
                    crate::execution_trace::stencil_observation(code, pc, "dispatch", true);
                    return completion_step_after_error(registers, error, pc + 1)
                }
                Err(crate::machine::NativeDispatchError::SemanticAt { pc: fault_pc, error }) => {
                    crate::execution_trace::stencil_observation(code, pc, "dispatch", true);
                    return completion_step_after_error(registers, error, fault_pc + 1)
                }
                Err(crate::machine::NativeDispatchError::Physical(_)) => {
                    crate::execution_trace::stencil_observation(code, pc, "dispatch", false);
                    crate::execution_trace::leaf_rejection("native_dispatch");
                    match run_baseline_instruction(code, pc, entry, registers, context) {
                        Ok(transition) => transition,
                        Err(error) => {
                            return completion_step_after_error(registers, error, pc + 1)
                        }
                    }
                }
                Err(crate::machine::NativeDispatchError::Committed(message)) => {
                    return Err(VmError::EvalError(format!(
                        "committed native dispatch failure: {message}"
                    )));
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
    environment: Option<&'state crate::environment::Environment>,
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
    if skip_proven_object_coercible(state.code, pc, instruction, state.registers) {
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
    // The build-time arithmetic-glue row is also reachable from the ordinary
    // function dispatcher (before a baseline plan exists).  Admit it at the
    // same boundary as the canonical instruction stream so a hot function does
    // not need to wait for a tier transition to remove the three per-op
    // dispatches.  The helper is all-or-nothing and returns the exact residual
    // successor; an unknown shape simply falls through below.
    #[cfg(not(feature = "execution-trace"))]
    let result = match run_instruction_hot(
        state.code,
        pc,
        instruction,
        state.registers,
        state.environment,
    ) {
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
        if skip_proven_object_coercible(state.code, pc, instruction, state.registers) {
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
        let result = match run_instruction_hot(
            state.code,
            pc,
            instruction,
            state.registers,
            state.environment,
        ) {
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
    environment: Option<&crate::environment::Environment>,
) -> Option<Result<DispatchTransition, VmError>> {
    use crate::ir::Opcode;
    let result = match instruction.opcode {
        Opcode::LoadConst => code
            .constant_at(pc)
            .ok_or(VmError::MissingReturn)
            .map(|(_, value)| {
                write_constant(registers, instruction.a, value);
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
        Opcode::LoadLocal => {
            let loaded = match environment {
                Some(environment) => crate::locals::load_proven_in(
                    environment,
                    registers,
                    instruction.a,
                    instruction.b,
                ),
                None => crate::locals::load_proven(registers, instruction.a, instruction.b),
            };
            loaded.map(|_| handler_transition(pc, None))
        }
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

/// `RequireObjectCoercible` has no observable work once its source word is
/// known non-nullish. The fact is exact (the tagged word distinguishes only
/// null/undefined here), so this removes the gateway even when lowering placed
/// harmless loads between the check and its eventual indexed access. Unknown
/// or nullish words retain the complete slow operation.
#[inline(always)]
fn skip_proven_object_coercible(
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
    registers.word_is_non_nullish(usize::from(*src)) == Some(true)
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
    let Some(word) = quickened_own_slot_data(code, pc, object, &key) else {
        return false;
    };
    let Some(bits) = word.plain_tagged_bits() else {
        return false;
    };
    registers
        .write_tagged_bits(usize::from(destination), bits)
        .is_some()
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

#[inline]
fn resume_region_transition(pc: usize) -> DispatchTransition {
    DispatchTransition {
        next_pc: pc,
        completion: None,
        target: DispatchTarget::Callee(pc),
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
fn write_constant(
    registers: &mut crate::register_file::RegisterFile,
    destination: u16,
    constant: &crate::ops::Constant,
) {
    match constant {
        // Numbers have a first-class tagged representation. Keep the
        // immutable constant fact in that representation instead of cloning a
        // temporary Value and immediately encoding it again.
        crate::ops::Constant::Number(value) => {
            registers.write_number(usize::from(destination), *value);
        }
        _ => write_value(registers, destination, constant.into()),
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
    write_constant(registers, instruction.a, value);
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
pub(crate) fn run_unary_instruction(
    _code: crate::machine::CodeView<'_>,
    pc: usize,
    instruction: crate::ir::Instruction,
    registers: &mut crate::register_file::RegisterFile,
    _context: &VmContext,
) -> Result<DispatchTransition, VmError> {
    let operator = crate::ir::compact_unary_operator(instruction.flags)
        .ok_or_else(|| VmError::EvalError("invalid compact unary operator".into()))?;
    vm_arithmetic::execute_unary(registers, instruction.a, operator, instruction.b)?;
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
    #[cfg(feature = "execution-trace")]
    {
        // Compact calls previously contributed no target identity to the
        // corpus ledger, leaving the largest call category opaque. Keep this
        // extra read in diagnostic artifacts only; scored builds retain the
        // original hot path unchanged.
        let callee_value = read_register(registers, instruction.b)?;
        crate::execution_trace::call_method(
            arguments.len(),
            false,
            true,
            crate::execution_trace::call_target_name(&callee_value),
        );
    }
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
    // A rewritten named-load carries a proven shape/slot fact. Read the
    // immutable tagged word directly instead of decoding the receiver and
    // cloning the payload twice (once for validation and once for the write).
    // Binding cells/weak functions return `None` from the slot projection and
    // continue through the complete canonical path below.
    if instruction.opcode == crate::ir::Opcode::GetNQuickened && instruction.flags == 0 {
        if let Some(key) = code.metadata_at(pc).and_then(|metadata| metadata.name.as_deref()) {
            let bits = registers
                .read_object(usize::from(instruction.b))
                .and_then(|object| quickened_own_slot_data(code, pc, object, key))
                .and_then(crate::register_file::SlotWord::plain_tagged_bits);
            if let Some(bits) = bits {
                if registers
                    .write_tagged_bits(usize::from(instruction.a), bits)
                    .is_some()
                {
                    return Ok(handler_transition(pc, None));
                }
            }
        }
    }
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

fn try_native_property_store(
    native: &std::cell::RefCell<crate::machine::NativeMovePlan>,
    code: crate::machine::CodeView<'_>,
    pc: usize,
    registers: &mut crate::register_file::RegisterFile,
    object: u16,
    source: u16,
) -> bool {
    let Some(metadata) = code.metadata_at(pc) else {
        return false;
    };
    let Some(key) = metadata.name.as_deref() else {
        return false;
    };
    let Some(slot) = crate::properties::proven_named_writable_slot(
        registers,
        object,
        key,
        &metadata.named_cache,
    ) else {
        return false;
    };
    if !crate::properties::assignment_source_is_direct(registers, source) {
        return false;
    }
    let Some(source_ptr) = registers.word_ptr(usize::from(source)) else {
        return false;
    };
    let Ok(bits) = native.borrow_mut().execute(source_ptr) else {
        return false;
    };
    if registers.word_bits(usize::from(source)) != Some(bits) {
        return false;
    }
    // SAFETY: the slot came from the cache's descriptor/layout proof and the
    // native transfer cannot allocate, call JS, or resize the object.
    unsafe { &*slot }.store_from_register(registers, usize::from(source)).is_some()
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
                let valid = word.plain_tagged_bits().is_some();
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
            let valid = word.plain_tagged_bits().is_some();
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

/// Return a plain data slot from an immediate prototype chain. Every owner is
/// checked before advancing, so shadowing accessors and unstable objects fall
/// back to canonical property semantics.
fn quickened_prototype_slot_data(
    receiver: &crate::value::ObjectData,
    key: &str,
) -> Option<*const crate::register_file::SlotWord> {
    let mut owner = receiver as *const crate::value::ObjectData;
    for _ in 0..4 {
        let owner_ref = unsafe { owner.as_ref()? };
        if owner_ref.has_replacement()
            || owner_ref.is_dictionary()
            || owner_ref.is_realm_global()
            || owner_ref.is_script_global_view()
            || owner_ref.has_regexp_internal_slot()
        {
            return None;
        }
        if let Some(slot) = owner_ref.hot_properties().position_rev(key) {
            let layout = owner_ref.semantic_layout_id();
            let slot = u32::try_from(slot).ok()?;
            return crate::vm::cached_plain_own_word(owner_ref, key, layout, slot)
                .map(|word| word as *const crate::register_file::SlotWord);
        }
        let proto_slot = owner_ref.hot_properties().position_rev("\0prototype")?;
        owner = owner_ref
            .hot_properties()
            .slot_word(proto_slot)?
            .object_or_null_ptr()??;
    }
    None
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
    fn raw_array_context_layout_matches_emitted_offsets() {
        use std::mem::{align_of, offset_of, size_of};
        assert_eq!(align_of::<super::NativeArrayKernelContext>(), 8);
        assert_eq!(size_of::<super::NativeArrayKernelContext>(), 40);
        assert_eq!(offset_of!(super::NativeArrayKernelContext, data), 0);
        assert_eq!(offset_of!(super::NativeArrayKernelContext, len), 8);
        assert_eq!(offset_of!(super::NativeArrayKernelContext, index), 16);
        assert_eq!(offset_of!(super::NativeArrayKernelContext, addend), 24);
        assert_eq!(offset_of!(super::NativeArrayKernelContext, result), 32);

        assert_eq!(align_of::<super::NativeArrayLoopContext>(), 8);
        assert_eq!(size_of::<super::NativeArrayLoopContext>(), 56);
        assert_eq!(offset_of!(super::NativeArrayLoopContext, data), 0);
        assert_eq!(offset_of!(super::NativeArrayLoopContext, len), 8);
        assert_eq!(offset_of!(super::NativeArrayLoopContext, index), 16);
        assert_eq!(offset_of!(super::NativeArrayLoopContext, end), 24);
        assert_eq!(offset_of!(super::NativeArrayLoopContext, addend), 32);
        assert_eq!(offset_of!(super::NativeArrayLoopContext, result), 40);
        assert_eq!(offset_of!(super::NativeArrayLoopContext, interrupt), 48);

        let interrupt = std::sync::atomic::AtomicBool::new(false);
        let mut data = [0.0_f64; 1];
        let valid = super::NativeArrayLoopContext {
            data: data.as_mut_ptr(),
            len: 1,
            index: 0,
            end: 1,
            addend: 1.0,
            result: 0.0,
            interrupt: &interrupt,
        };
        assert!(valid.is_valid());
        let mut invalid = valid;
        invalid.end = 2;
        assert!(!invalid.is_valid());
        invalid.end = 1;
        invalid.interrupt = std::ptr::null();
        assert!(!invalid.is_valid());
        invalid.interrupt = &interrupt;
        invalid.data = (invalid.data.cast::<u8>()).wrapping_add(1).cast();
        assert!(!invalid.is_valid());

        let overflowing = super::NativeArrayLoopContext {
            data: 8usize as *mut f64,
            len: usize::MAX,
            index: 0,
            end: 0,
            addend: 1.0,
            result: 0.0,
            interrupt: &interrupt,
        };
        assert!(!overflowing.is_valid());
        let kernel_overflow = super::NativeArrayKernelContext {
            data: 8usize as *mut f64,
            len: usize::MAX,
            index: 0,
            addend: 1.0,
            result: 0.0,
        };
        assert!(!kernel_overflow.is_valid());
    }

    #[cfg(target_arch = "aarch64")]
    #[test]
    fn numeric_loop_alias_roles_fail_closed() {
        let mut aliased = vec![crate::ir::Instruction::load_local_checked(0, 0); 19];
        assert!(!super::array_loop_roles_are_disjoint(&aliased));
        for (index, instruction) in aliased.iter_mut().enumerate() {
            instruction.a = index as u16;
        }
        assert!(super::array_loop_roles_are_disjoint(&aliased));
    }

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
        let quickened = code.instruction(0).expect("quickened named get");
        assert_eq!(quickened.opcode, crate::ir::Opcode::GetNQuickened);
        registers.write(0, Value::Undefined);
        run_compact_get_named(code, 0, quickened, &mut registers, &context)
            .expect("direct tagged-word lookup");
        assert_eq!(registers.read(0), Some(Value::Number(9.0)));
    }

    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    #[test]
    fn baseline_named_store_uses_tagged_word_body_after_cache_warmup() {
        let object = Rc::new(ObjectData::new(vec![
            ("value".into(), Value::Number(1.0)),
        ]));
        let function = crate::machine::FunctionCode::from_ops(vec![
            Op::SetProperty {
                object: 0,
                key: "value".into(),
                src: 1,
                strict: false,
            },
            Op::Return { src: 1 },
        ]);
        let code = function.code().expect("lowered named store");
        assert_eq!(code.instruction(0).unwrap().opcode, crate::ir::Opcode::SetN);
        let plan = crate::machine::BaselinePlan::compile_for_test(
            code,
            crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test(),
        );
        assert!(plan.native_store_property_at(0).is_some());
        let context = crate::vm::current_context_or_default();
        let run = |value: f64| {
            let mut registers = crate::register_file::RegisterFile::from_values(vec![
                Value::Object(Rc::clone(&object)),
                Value::Number(value),
            ]);
            crate::vm::execute_baseline_code_from(
                code,
                &plan,
                0,
                &mut registers,
                &context,
                crate::environment::Environment::new(),
            )
            .expect("named store execution");
        };
        run(3.0);
        run(5.0);
        assert_eq!(
            crate::vm::get_property_result(&Value::Object(object), "value").unwrap(),
            Value::Number(5.0)
        );
        assert!(plan
            .native_store_property_at(0)
            .unwrap()
            .borrow()
            .native_entry_count()
            > 0);

        let cell = crate::value::BindingCell::new(Value::Number(2.0));
        let cell_object = Rc::new(ObjectData::new(vec![
            ("value".into(), Value::BindingCell(Rc::clone(&cell))),
        ]));
        let cell_plan = crate::machine::BaselinePlan::compile_for_test(
            code,
            crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test(),
        );
        let mut cell_registers = crate::register_file::RegisterFile::from_values(vec![
            Value::Object(cell_object),
            Value::Number(9.0),
        ]);
        crate::vm::execute_baseline_code_from(
            code,
            &cell_plan,
            0,
            &mut cell_registers,
            &context,
            crate::environment::Environment::new(),
        )
        .expect("binding-cell fallback");
        assert_eq!(cell.load(), Value::Number(9.0));
        assert_eq!(
            cell_plan
                .native_store_property_at(0)
                .unwrap()
                .borrow()
                .native_entry_count(),
            0
        );

        let source_cell = crate::value::BindingCell::new(Value::Number(11.0));
        let source_object = Rc::new(ObjectData::new(vec![
            ("value".into(), Value::Number(2.0)),
        ]));
        let source_plan = crate::machine::BaselinePlan::compile_for_test(
            code,
            crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test(),
        );
        let mut source_registers = crate::register_file::RegisterFile::from_values(vec![
            Value::Object(Rc::clone(&source_object)),
            Value::BindingCell(Rc::clone(&source_cell)),
        ]);
        crate::vm::execute_baseline_code_from(
            code,
            &source_plan,
            0,
            &mut source_registers,
            &context,
            crate::environment::Environment::new(),
        )
        .expect("binding-cell source fallback");
        assert_eq!(
            crate::vm::get_property_result(&Value::Object(source_object), "value").unwrap(),
            Value::Number(11.0)
        );
        assert_eq!(
            source_plan
                .native_store_property_at(0)
                .unwrap()
                .borrow()
                .native_entry_count(),
            0
        );
    }

    #[cfg(target_arch = "aarch64")]
    #[test]
    fn rendered_array_get_executes_native_bytes_for_dense_numeric_slot() {
        let executable = crate::machine::ExecutableCode::from_ops(vec![Op::GetPropertyDynamic {
            dst: 0,
            object: 1,
            key: 2,
        }]);
        let code = executable.code();
        code.quicken_instruction(0, crate::ir::Opcode::AGetI, 0, 0, 0);
        let array = Value::Array(Rc::new(crate::value::ArrayData::new(vec![
            Value::Number(4.5),
            Value::Number(-0.0),
        ])));
        let mut registers = crate::register_file::RegisterFile::from_values(vec![
            Value::Undefined,
            array,
            Value::Number(1.0),
        ]);
        let context = crate::vm::current_context_or_default();
        let mut region = crate::machine::NativeRegionPlan::new_for_test(
            crate::stencil_select::array_get_number_region_key(),
        )
        .expect("array-get native declaration");
        let transition = region
            .execute(code, 0, &mut registers, &context)
            .expect("native dense array get");
        assert!(region.last_native_execution());
        assert_eq!(transition.next_pc, 1);
        assert_eq!(registers.read(0), Some(Value::Number(-0.0)));
    }

    #[cfg(target_arch = "aarch64")]
    #[test]
    fn baseline_driver_routes_index_get_through_typed_native_region() {
        let executable = crate::machine::ExecutableCode::from_ops(vec![Op::GetPropertyDynamic {
            dst: 0,
            object: 1,
            key: 2,
        }]);
        let code = executable.code();
        code.quicken_instruction(0, crate::ir::Opcode::AGetI, 0, 0, 0);
        let plan = crate::machine::BaselinePlan::compile_for_test(
            code,
            crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test(),
        );
        let region = plan.native_region_at(0).expect("typed array region admission");
        assert_eq!(
            region.borrow().key_for_test(),
            crate::stencil_select::array_get_number_region_key()
        );
        let array = Value::Array(Rc::new(crate::value::ArrayData::new(vec![
            Value::Number(8.25),
        ])));
        let mut registers = crate::register_file::RegisterFile::from_values(vec![
            Value::Undefined,
            array,
            Value::Number(0.0),
        ]);
        let context = crate::vm::current_context_or_default();
        crate::vm::execute_baseline_code_from(
            code,
            &plan,
            0,
            &mut registers,
            &context,
            crate::environment::Environment::new(),
        )
        .expect("baseline typed array region");
        assert_eq!(registers.read(0), Some(Value::Number(8.25)));
        assert!(region.borrow().last_native_execution());
    }

    #[cfg(target_arch = "aarch64")]
    #[test]
    fn baseline_driver_routes_dense_index_store_through_typed_native_region() {
        let executable = crate::machine::ExecutableCode::from_ops(vec![Op::SetPropertyDynamic {
            object: 0,
            key: 1,
            src: 2,
            strict: false,
        }]);
        let code = executable.code();
        code.quicken_instruction(0, crate::ir::Opcode::ASetI, 0, 0, 0);
        let plan = crate::machine::BaselinePlan::compile_for_test(
            code,
            crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test(),
        );
        let region = plan.native_region_at(0).expect("typed array store admission");
        assert_eq!(
            region.borrow().key_for_test(),
            crate::stencil_select::array_set_number_region_key()
        );
        let array = Value::Array(Rc::new(crate::value::ArrayData::new(vec![
            Value::Number(2.0),
        ])));
        let mut registers = crate::register_file::RegisterFile::from_values(vec![
            array,
            Value::Number(0.0),
            Value::Number(f64::NEG_INFINITY),
        ]);
        let context = crate::vm::current_context_or_default();
        crate::vm::execute_baseline_code_from(
            code,
            &plan,
            0,
            &mut registers,
            &context,
            crate::environment::Environment::new(),
        )
        .expect("baseline typed array store");
        let stored = registers
            .read_array(0)
            .and_then(|array| array.dense_number_at(0))
            .expect("stored dense number");
        assert_eq!(stored, f64::NEG_INFINITY);
        assert!(region.borrow().last_native_execution());

        // An out-of-bounds index is outside the proven dense-slot contract;
        // the ordinary setter may grow the array, but the raw bytes must not
        // run or make the fallback retry an already-entered region.
        let mut hostile = crate::register_file::RegisterFile::from_values(vec![
            registers.read(0).expect("array remains live"),
            Value::Number(4.0),
            Value::Number(7.0),
        ]);
        crate::vm::execute_baseline_code_from(
            code,
            &plan,
            0,
            &mut hostile,
            &context,
            crate::environment::Environment::new(),
        )
        .expect("ordinary out-of-bounds array store");
        assert!(!region.borrow().last_native_execution());
    }

    #[cfg(target_arch = "aarch64")]
    #[test]
    fn baseline_driver_routes_index_increment_through_native_bytes() {
        let executable = crate::machine::ExecutableCode::from_ops(vec![Op::GetPropertyDynamic {
            dst: 0,
            object: 1,
            key: 2,
        }]);
        let code = executable.code();
        code.quicken_instruction(0, crate::ir::Opcode::AGetIInc, 0, 0, 0);
        let plan = crate::machine::BaselinePlan::compile_for_test(
            code,
            crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test(),
        );
        let region = plan.native_region_at(0).expect("get-inc admission");
        assert_eq!(
            region.borrow().key_for_test(),
            crate::stencil_select::array_get_inc_number_region_key()
        );
        let array = Value::Array(Rc::new(crate::value::ArrayData::new(vec![
            Value::Number(7.0),
        ])));
        let mut registers = crate::register_file::RegisterFile::from_values(vec![
            Value::Undefined,
            array,
            Value::Number(0.0),
        ]);
        let context = crate::vm::current_context_or_default();
        crate::vm::execute_baseline_code_from(
            code,
            &plan,
            0,
            &mut registers,
            &context,
            crate::environment::Environment::new(),
        )
        .expect("native get-inc");
        assert_eq!(registers.read(0), Some(Value::Number(7.0)));
        assert_eq!(registers.read(2), Some(Value::Number(1.0)));
        assert!(region.borrow().last_native_execution());

        let mut hostile = crate::register_file::RegisterFile::from_values(vec![
            registers.read(0).expect("array result register"),
            registers.read(1).expect("array register"),
            Value::String("0".into()),
        ]);
        crate::vm::execute_baseline_code_from(
            code,
            &plan,
            0,
            &mut hostile,
            &context,
            crate::environment::Environment::new(),
        )
        .expect("canonical get-inc coercion fallback");
        assert!(!region.borrow().last_native_execution());

        // When the destination aliases the induction register, canonical
        // order leaves the loaded element in that register after incrementing
        // it.  The raw body must reject this destructive alias before entry;
        // publishing its next-index field after the result would be wrong.
        let alias_code = crate::machine::ExecutableCode::from_ops(vec![
            Op::GetPropertyDynamic {
                dst: 2,
                object: 1,
                key: 2,
            },
        ]);
        let alias_view = alias_code.code();
        alias_view.quicken_instruction(0, crate::ir::Opcode::AGetIInc, 0, 0, 0);
        let alias_plan = crate::machine::BaselinePlan::compile_for_test(
            alias_view,
            crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test(),
        );
        let alias_region = alias_plan.native_region_at(0).expect("alias region");
        let alias_array = Value::Array(Rc::new(crate::value::ArrayData::new(vec![
            Value::Number(7.0),
        ])));
        let mut alias_registers = crate::register_file::RegisterFile::from_values(vec![
            Value::Undefined,
            alias_array,
            Value::Number(0.0),
        ]);
        crate::vm::execute_baseline_code_from(
            alias_view,
            &alias_plan,
            0,
            &mut alias_registers,
            &context,
            crate::environment::Environment::new(),
        )
        .expect("canonical aliased get-inc");
        assert_eq!(alias_registers.read(2), Some(Value::Number(7.0)));
        assert!(!alias_region.borrow().last_native_execution());
    }

    #[cfg(target_arch = "aarch64")]
    #[test]
    fn baseline_driver_composes_indexed_load_add_store_once() {
        let executable = crate::machine::ExecutableCode::from_ops(vec![
            Op::GetPropertyDynamic {
                dst: 3,
                object: 0,
                key: 1,
            },
            Op::Binary {
                dst: 4,
                operator: crate::ops::BinaryOp::Add,
                lhs: 3,
                rhs: 2,
            },
            Op::SetPropertyDynamic {
                object: 0,
                key: 1,
                src: 4,
                strict: false,
            },
        ]);
        let code = executable.code();
        code.quicken_instruction(0, crate::ir::Opcode::AGetI, 0, 0, 0);
        code.quicken_instruction(2, crate::ir::Opcode::ASetI, 0, 0, 0);
        let plan = crate::machine::BaselinePlan::compile_for_test(
            code,
            crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test(),
        );
        let region = plan.native_region_at(0).expect("composed update admission");
        assert_eq!(
            region.borrow().key_for_test(),
            crate::stencil_select::array_numeric_update_region_key()
        );
        let array = Value::Array(Rc::new(crate::value::ArrayData::new(vec![
            Value::Number(3.0),
        ])));
        let mut registers = crate::register_file::RegisterFile::from_values(vec![
            array,
            Value::Number(0.0),
            Value::Number(2.5),
            Value::Undefined,
            Value::Undefined,
        ]);
        let context = crate::vm::current_context_or_default();
        crate::vm::execute_baseline_code_from(
            code,
            &plan,
            0,
            &mut registers,
            &context,
            crate::environment::Environment::new(),
        )
        .expect("composed indexed update");
        assert_eq!(
            registers.read(3),
            Some(Value::Number(3.0)),
            "load live-out is preserved"
        );
        assert_eq!(registers.read(4), Some(Value::Number(5.5)));
        assert_eq!(
            registers
                .read_array(0)
                .and_then(|array| array.dense_number_at(0)),
            Some(5.5)
        );
        assert!(region.borrow().last_native_execution());

        // A destructive operand alias rejects the raw body and executes the
        // same residual sequence canonically, without a partial native write.
        let alias_code = crate::machine::ExecutableCode::from_ops(vec![
            Op::GetPropertyDynamic {
                dst: 3,
                object: 0,
                key: 1,
            },
            Op::Binary {
                dst: 4,
                operator: crate::ops::BinaryOp::Add,
                lhs: 3,
                rhs: 3,
            },
            Op::SetPropertyDynamic {
                object: 0,
                key: 1,
                src: 4,
                strict: false,
            },
        ]);
        let alias_view = alias_code.code();
        alias_view.quicken_instruction(0, crate::ir::Opcode::AGetI, 0, 0, 0);
        alias_view.quicken_instruction(2, crate::ir::Opcode::ASetI, 0, 0, 0);
        let alias_plan = crate::machine::BaselinePlan::compile_for_test(
            alias_view,
            crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test(),
        );
        let alias_region = alias_plan.native_region_at(0).expect("alias region candidate");
        let alias_array = Value::Array(Rc::new(crate::value::ArrayData::new(vec![
            Value::Number(3.0),
        ])));
        let mut alias_registers = crate::register_file::RegisterFile::from_values(vec![
            alias_array,
            Value::Number(0.0),
            Value::Number(0.0),
            Value::Undefined,
            Value::Undefined,
        ]);
        crate::vm::execute_baseline_code_from(
            alias_view,
            &alias_plan,
            0,
            &mut alias_registers,
            &context,
            crate::environment::Environment::new(),
        )
        .expect("aliased residual update");
        assert_eq!(
            alias_registers
                .read_array(0)
                .and_then(|array| array.dense_number_at(0)),
            Some(6.0)
        );
        assert!(!alias_region.borrow().last_native_execution());
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
    fn proven_object_coercibility_skip_is_guarded_by_object_word() {
        let executable = crate::machine::ExecutableCode::from_ops(vec![
            Op::RequireObjectCoercible { src: 1 },
            // The check is often separated from the eventual index access by
            // a local/key transfer; the source word itself is the complete
            // fact, so no adjacency assumption is needed.
            Op::Move { dst: 3, src: 2 },
            Op::GetPropertyDynamic {
                dst: 0,
                object: 1,
                key: 2,
            },
        ]);
        let code = executable.code();
        let check = code.instruction(0).expect("coercibility check");
        assert_eq!(check.opcode, crate::ir::Opcode::Slow);
        let object = Value::Object(Rc::new(ObjectData::new(vec![
            ("0".into(), Value::Number(4.0)),
        ])));
        let registers = crate::register_file::RegisterFile::from_values(vec![
            Value::Undefined,
            object,
            Value::Number(0.0),
        ]);
        assert!(super::skip_proven_object_coercible(code, 0, check, &registers));

        let nullish = crate::register_file::RegisterFile::from_values(vec![
            Value::Undefined,
            Value::Null,
            Value::Number(0.0),
        ]);
        assert!(!super::skip_proven_object_coercible(code, 0, check, &nullish));
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
    fn array_region_matches_packed_and_holey_fallbacks() {
        // The admission row is representation-agnostic; the canonical AGetI
        // handler owns the packed-numeric fast path and falls back for holes.
        // Compare both through the region bridge and ordinary dispatch so a
        // future elements-kind specialization cannot accidentally read a hole.
        let make_code = || {
            crate::machine::ExecutableCode::from_ops(vec![Op::GetPropertyDynamic {
                dst: 0,
                object: 1,
                key: 2,
            }])
        };
        let context = crate::vm::current_context_or_default();
        let packed = Value::Array(Rc::new(crate::value::ArrayData::new(vec![
            Value::Number(17.0),
        ])));
        let mut holey_data = crate::value::ArrayData::new(vec![Value::Number(17.0)]);
        holey_data.delete_property("0");
        let holey = Value::Array(Rc::new(holey_data));

        for object in [packed, holey] {
            let ordinary_code = make_code();
            let ordinary_view = ordinary_code.code();
            let ordinary_instruction = ordinary_view.instruction(0).expect("AGetI");
            let mut ordinary = crate::register_file::RegisterFile::from_values(vec![
                Value::Undefined,
                object.clone(),
                Value::Number(0.0),
            ]);
            let expected = run_instruction(
                ordinary_view,
                0,
                ordinary_instruction,
                &mut ordinary,
                &context,
            )
            .expect("ordinary array access");

            let native_code = make_code();
            let mut native = crate::register_file::RegisterFile::from_values(vec![
                Value::Undefined,
                object,
                Value::Number(0.0),
            ]);
            let mut region = crate::machine::NativeRegionPlan::new_for_test(
                crate::stencil_select::get_index_region_key(),
            )
            .expect("array region admission");
            let actual = region
                .execute(native_code.code(), 0, &mut native, &context)
                .expect("canonical array fallback");
            assert_transition_equal(&actual, &expected);
            assert_eq!(native, ordinary);
        }
    }

    #[test]
    fn composed_array_block_uses_one_native_entry() {
        let code = crate::machine::ExecutableCode::from_ops(vec![
            Op::CheckInitialized { slot: 0, name: "a".into() },
            Op::LoadLocal { dst: 1, slot: 0 },
            Op::GetPropertyDynamic { dst: 2, object: 1, key: 3 },
            Op::Binary { dst: 4, operator: crate::ops::BinaryOp::Add, lhs: 2, rhs: 5 },
            Op::SetPropertyDynamic { object: 1, key: 3, src: 4, strict: false },
            Op::Return { src: 4 },
        ]);
        let lowered = code.code();
        assert_eq!(
            (0..lowered.len())
                .map(|pc| lowered.instruction(pc).unwrap().opcode)
                .collect::<Vec<_>>(),
            vec![
                crate::ir::Opcode::LoadLocalChecked,
                crate::ir::Opcode::AGetI,
                crate::ir::Opcode::Add,
                crate::ir::Opcode::ASetI,
                crate::ir::Opcode::Return,
            ]
        );
        let array = Value::Array(Rc::new(crate::value::ArrayData::new(vec![
            Value::Number(4.0),
        ])));
        let mut registers = crate::register_file::RegisterFile::from_values(vec![
            Value::Undefined,
            Value::Undefined,
            Value::Undefined,
            Value::Number(0.0),
            Value::Undefined,
            Value::Number(3.0),
        ]);
        let environment = crate::environment::Environment::new();
        environment.set(0, array.clone());
        let _guard = crate::locals::EnvironmentGuard::install(environment);
        let context = crate::vm::current_context_or_default();

        // Exercise the physical ABI independently of the host ISA: the
        // callback models the emitted ARM64 load/add/store sequence while
        // `execute_composed_array_kernel` performs the real admission,
        // rooting, and exact residual-state materialization.
        let mut physical_registers = crate::register_file::RegisterFile::from_values(vec![
            Value::Undefined,
            Value::Undefined,
            Value::Undefined,
            Value::Number(0.0),
            Value::Undefined,
            Value::Number(3.0),
        ]);
        let mut physical_region = super::NativeRegionContext::new(
            lowered,
            0,
            crate::stencil_select::select_region(
                crate::stencil_select::array_loop_body_region_key(),
            )
            .expect("array region declaration")
            .operations,
            &mut physical_registers,
            &context,
        );
        let mut modeled_entries = 0usize;
        let physical = super::execute_composed_array_kernel(&mut physical_region, |raw| {
            modeled_entries += 1;
            let kernel = unsafe { &mut *(raw.cast::<super::NativeArrayKernelContext>()) };
            kernel.result = unsafe { *kernel.data.add(kernel.index) } + kernel.addend;
            unsafe { *kernel.data.add(kernel.index) = kernel.result };
            Ok(super::NATIVE_DISPATCH_OK)
        })
        .expect("physical array ABI");
        assert!(physical.is_some());
        assert_eq!(modeled_entries, 1, "modeled ABI callback must be entered once");
        assert_eq!(physical_registers.read(4), Some(Value::Number(7.0)));
        assert_eq!(crate::vm::get_property_result(&array, "0").unwrap(), Value::Number(7.0));
        let Value::Array(array_ref) = &array else { unreachable!() };
        assert!(crate::value::ArrayData::set_kernel_existing_f64(array_ref, 0, 4.0));

        let mut region = crate::machine::NativeRegionPlan::new_for_test(
            crate::stencil_select::array_loop_body_region_key(),
        )
        .expect("array-loop region admission");
        let transition = region
            .execute(lowered, 0, &mut registers, &context)
            .expect("native composed entry");
        assert!(matches!(
            transition.completion,
            Some(crate::completion::Completion::Return(Value::Number(value))) if value == 7.0
        ));
        assert_eq!(
            crate::vm::get_property_result(&array, "0").unwrap(),
            Value::Number(7.0)
        );

        // A hole invalidates the dense representation proof. The same
        // admitted entry must then use the complete property semantics
        // (which materialize the hole as `undefined`) without partially
        // committing the fused path.
        let mut holey_data = crate::value::ArrayData::new(vec![Value::Number(4.0)]);
        holey_data.delete_property("0");
        let holey = Value::Array(Rc::new(holey_data));
        let mut fallback_registers = crate::register_file::RegisterFile::from_values(vec![
            Value::Undefined,
            Value::Undefined,
            Value::Undefined,
            Value::Number(0.0),
            Value::Undefined,
            Value::Number(3.0),
        ]);
        let fallback_environment = crate::environment::Environment::new();
        fallback_environment.set(0, holey.clone());
        let _fallback_guard = crate::locals::EnvironmentGuard::install(fallback_environment);
        let mut fallback_region = crate::machine::NativeRegionPlan::new_for_test(
            crate::stencil_select::array_loop_body_region_key(),
        )
        .expect("array-loop fallback admission");
        fallback_region
            .execute(lowered, 0, &mut fallback_registers, &context)
            .expect("complete hole fallback");
        assert!(matches!(fallback_registers.read(4), Some(Value::Number(value)) if value.is_nan()));
        assert!(matches!(
            crate::vm::get_property_result(&holey, "0").unwrap(),
            Value::Number(value) if value.is_nan()
        ));

        // A stale operation anywhere in the window must reject before the
        // first effect. This guards the no-replay contract after the
        // two-pass fallback validation.
        let stale = crate::machine::ExecutableCode::from_ops(vec![
            Op::CheckInitialized { slot: 0, name: "a".into() },
            Op::LoadLocal { dst: 1, slot: 0 },
            Op::GetPropertyDynamic { dst: 2, object: 1, key: 3 },
            Op::Binary { dst: 4, operator: crate::ops::BinaryOp::Add, lhs: 2, rhs: 5 },
            Op::SetPropertyDynamic { object: 1, key: 3, src: 4, strict: false },
            Op::Return { src: 4 },
        ]);
        stale.code().quicken_instruction(4, crate::ir::Opcode::Slow, 0, 0, 0);
        let mut stale_registers = crate::register_file::RegisterFile::from_values(vec![
            Value::Undefined,
            Value::Undefined,
            Value::Undefined,
            Value::Number(0.0),
            Value::Undefined,
            Value::Number(3.0),
        ]);
        let stale_before = stale_registers.clone();
        let stale_array_before = crate::vm::get_property_result(&array, "0").unwrap();
        let mut stale_region = crate::machine::NativeRegionPlan::new_for_test(
            crate::stencil_select::array_loop_body_region_key(),
        )
        .expect("stale region admission");
        assert!(matches!(
            stale_region.execute(stale.code(), 0, &mut stale_registers, &context),
            Err(crate::machine::NativeDispatchError::Physical(_))
        ));
        assert_eq!(stale_registers, stale_before);
        assert_eq!(crate::vm::get_property_result(&array, "0").unwrap(), stale_array_before);
    }

    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    #[test]
    fn composed_array_admission_rejects_destination_alias_before_entry() {
        let code = crate::machine::ExecutableCode::from_ops(vec![
            Op::CheckInitialized { slot: 0, name: "a".into() },
            Op::LoadLocal { dst: 1, slot: 0 },
            Op::GetPropertyDynamic { dst: 1, object: 1, key: 3 },
            Op::Binary { dst: 4, operator: crate::ops::BinaryOp::Add, lhs: 1, rhs: 5 },
            Op::SetPropertyDynamic { object: 1, key: 3, src: 4, strict: false },
            Op::Return { src: 4 },
        ]);
        let lowered = code.code();
        let array = Value::Array(Rc::new(crate::value::ArrayData::new(vec![Value::Number(4.0)])));
        let environment = crate::environment::Environment::new();
        environment.set(0, array.clone());
        let _guard = crate::locals::EnvironmentGuard::install(environment);
        let context = crate::vm::current_context_or_default();
        let mut registers = crate::register_file::RegisterFile::from_values(vec![
            Value::Undefined,
            Value::Undefined,
            Value::Undefined,
            Value::Number(0.0),
            Value::Undefined,
            Value::Number(3.0),
        ]);
        let operations = crate::stencil_select::select_region(
            crate::stencil_select::array_loop_body_region_key(),
        )
        .expect("array region declaration")
        .operations;
        let mut region = super::NativeRegionContext::new(
            lowered,
            0,
            operations,
            &mut registers,
            &context,
        );
        let mut entered = false;
        let admitted = super::execute_composed_array_kernel(&mut region, |_raw| {
            entered = true;
            Ok(super::NATIVE_DISPATCH_OK)
        })
        .expect("alias guard should be a cheap rejection");
        assert!(admitted.is_none());
        assert!(!entered, "destination alias must reject before native entry");
        assert_eq!(crate::vm::get_property_result(&array, "0").unwrap(), Value::Number(4.0));
    }

    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    #[test]
    fn composed_array_post_entry_failure_is_not_retryable() {
        let code = crate::machine::ExecutableCode::from_ops(vec![
            Op::CheckInitialized { slot: 0, name: "a".into() },
            Op::LoadLocal { dst: 1, slot: 0 },
            Op::GetPropertyDynamic { dst: 2, object: 1, key: 3 },
            Op::Binary {
                dst: 4,
                operator: crate::ops::BinaryOp::Add,
                lhs: 2,
                rhs: 5,
            },
            Op::SetPropertyDynamic { object: 1, key: 3, src: 4, strict: false },
            Op::Return { src: 4 },
        ]);
        let array = Value::Array(Rc::new(crate::value::ArrayData::new(vec![Value::Number(4.0)])));
        let environment = crate::environment::Environment::new();
        environment.set(0, array.clone());
        let _guard = crate::locals::EnvironmentGuard::install(environment);
        let context = crate::vm::current_context_or_default();
        let mut registers = crate::register_file::RegisterFile::from_values(vec![
            Value::Undefined,
            Value::Undefined,
            Value::Undefined,
            Value::Number(0.0),
            Value::Undefined,
            Value::Number(3.0),
        ]);
        let mut region = super::NativeRegionContext::new(
            code.code(),
            0,
            crate::stencil_select::select_region(
                crate::stencil_select::array_loop_body_region_key(),
            )
            .expect("array region declaration")
            .operations,
            &mut registers,
            &context,
        );
        let result = super::execute_composed_array_kernel(&mut region, |_raw| {
            Err(crate::stencil_arena::ArenaError::ProtectionFailed)
        });
        assert!(matches!(
            result,
            Err(crate::machine::NativeDispatchError::Committed(_))
        ));
        assert!(region.native_entered);
        assert_eq!(crate::vm::get_property_result(&array, "0").unwrap(), Value::Number(4.0));
    }

    /// This test is compiled only on ARM64 and invokes the rendered stencil
    /// bytes directly. The portable host test above intentionally models the
    /// ABI callback; it is not evidence that machine code executed.
    #[cfg(target_arch = "aarch64")]
    #[test]
    fn rendered_array_kernel_executes_machine_bytes() {
        let record = crate::stencil_select::select_region(
            crate::stencil_select::array_loop_body_region_key(),
        )
        .expect("array kernel declaration");
        assert!(record.executable);
        let site = crate::quickening::QuickeningSite::<4>::new(
            crate::ir::Opcode::LoadLocalChecked,
        );
        let values = crate::stencil_fact::PatchValues::from_site(&site);
        let mut arena = crate::stencil_arena::StencilArena::new(4096).expect("arena");
        let mut cache = crate::stencil_select::RenderedRegionCache::new();
        let address = arena
            .render_or_get(
                &mut cache,
                crate::stencil_select::array_loop_body_region_key(),
                &record.stencil,
                &values,
            )
            .expect("render");
        arena.make_executable().expect("RX transition");
        let mut data = vec![4.0f64];
        let mut raw = super::NativeArrayKernelContext {
            data: data.as_mut_ptr(),
            len: data.len(),
            index: 0,
            addend: 3.0,
            result: 4.0,
        };
        let status = arena
            .execute_dispatch(
                address,
                (&mut raw as *mut super::NativeArrayKernelContext)
                    .cast::<std::ffi::c_void>(),
            )
            .expect("machine entry");
        assert_eq!(status, super::NATIVE_DISPATCH_OK);
        assert_eq!(data[0], 7.0);
        assert_eq!(raw.result, 7.0);
    }

    #[cfg(target_arch = "aarch64")]
    #[test]
    fn rendered_numeric_array_loop_executes_native_backedge() {
        let key = crate::stencil_select::array_numeric_loop_region_key();
        let record = crate::stencil_select::select_region(key).expect("numeric loop declaration");
        let site = crate::quickening::QuickeningSite::<4>::new(crate::ir::Opcode::LoadLocal);
        let values = crate::stencil_fact::PatchValues::from_site(&site);
        let mut arena = crate::stencil_arena::StencilArena::new(4096).expect("arena");
        let mut cache = crate::stencil_select::RenderedRegionCache::new();
        let address = arena
            .render_or_get(&mut cache, key, &record.stencil, &values)
            .expect("render numeric loop");
        arena.make_executable().expect("protect numeric loop");
        for (mut data, initial_result, expected_result, expected_data) in [
            (Vec::<f64>::new(), 0.0, 0.0, Vec::<f64>::new()),
            (vec![7.0], 0.0, 8.0, vec![8.0]),
            (vec![1.0, 2.0, 3.0], 0.0, 4.0, vec![2.0, 3.0, 4.0]),
            (vec![2.0, 3.0], 9.0, 4.0, vec![3.0, 4.0]),
        ] {
            let end = data.len();
            let interrupt = std::sync::atomic::AtomicBool::new(false);
            let mut raw = super::NativeArrayLoopContext {
                data: data.as_mut_ptr(),
                len: end,
                index: 0,
                end,
                addend: 1.0,
                result: initial_result,
                interrupt: &interrupt,
            };
            let status = arena
                .execute_dispatch(
                    address,
                    (&mut raw as *mut super::NativeArrayLoopContext).cast::<std::ffi::c_void>(),
                )
                .expect("execute native numeric loop");
            assert_eq!(status, super::NATIVE_DISPATCH_OK);
            assert_eq!(raw.index, end);
            assert_eq!(raw.result, expected_result);
            assert_eq!(data, expected_data);
        }
    }

    #[cfg(target_arch = "aarch64")]
    #[test]
    fn rendered_numeric_array_loop_poll_exits_after_committed_iteration() {
        let key = crate::stencil_select::array_numeric_loop_region_key();
        let record = crate::stencil_select::select_region(key).expect("numeric loop declaration");
        let site = crate::quickening::QuickeningSite::<4>::new(crate::ir::Opcode::LoadLocal);
        let values = crate::stencil_fact::PatchValues::from_site(&site);
        let mut arena = crate::stencil_arena::StencilArena::new(4096).expect("arena");
        let mut cache = crate::stencil_select::RenderedRegionCache::new();
        let address = arena
            .render_or_get(&mut cache, key, &record.stencil, &values)
            .expect("render numeric loop");
        arena.make_executable().expect("protect numeric loop");
        let interrupt = std::sync::atomic::AtomicBool::new(true);
        let mut data = vec![10.0, 20.0];
        let mut raw = super::NativeArrayLoopContext {
            data: data.as_mut_ptr(),
            len: data.len(),
            index: 0,
            end: data.len(),
            addend: 1.0,
            result: 0.0,
            interrupt: &interrupt,
        };
        let status = arena
            .execute_dispatch(
                address,
                (&mut raw as *mut super::NativeArrayLoopContext).cast::<std::ffi::c_void>(),
            )
            .expect("execute interruptible native loop");
        assert_eq!(status, super::NATIVE_DISPATCH_INTERRUPT);
        assert_eq!(raw.index, 1);
        assert_eq!(raw.result, 11.0);
        assert_eq!(data, vec![11.0, 20.0]);
    }

    #[test]
    fn committed_region_status_transport_preserves_non_retryable_kind() {
        let code = crate::machine::ExecutableCode::from_ops(vec![Op::Return { src: 0 }]);
        let mut registers = crate::register_file::RegisterFile::from_values(vec![Value::Number(9.0)]);
        let context = crate::vm::current_context_or_default();
        let mut bridge_context = super::NativeRegionContext::new(
            code.code(),
            0,
            crate::stencil_select::select_region(
                crate::stencil_select::array_loop_body_region_key(),
            )
            .expect("region declaration")
            .operations,
            &mut registers,
            &context,
        );
        bridge_context.force_committed_status = true;
        let status = super::native_region_bridge(
            (&mut bridge_context as *mut super::NativeRegionContext<'_>)
                .cast::<std::ffi::c_void>(),
        );
        assert_eq!(status, super::NATIVE_DISPATCH_COMMITTED_ERROR);
        let before = registers.clone();
        assert!(matches!(
            bridge_context.finish(status),
            Err(crate::machine::NativeDispatchError::Committed(_))
        ));
        assert_eq!(registers, before, "committed status must not replay the region");
    }

    #[test]
    fn interrupted_region_status_is_non_retryable_after_entry() {
        let code = crate::machine::ExecutableCode::from_ops(vec![Op::Return { src: 0 }]);
        let mut registers = crate::register_file::RegisterFile::from_values(vec![Value::Number(3.0)]);
        let context = crate::vm::current_context_or_default();
        let mut region = super::NativeRegionContext::new(
            code.code(),
            0,
            &[crate::ir::Opcode::Return],
            &mut registers,
            &context,
        );
        region.entry_started = true;
        assert!(matches!(
            region.finish(super::NATIVE_DISPATCH_INTERRUPT),
            Err(crate::machine::NativeDispatchError::Committed(message))
                if message.contains("committed progress")
        ));
    }

    #[test]
    fn unknown_region_status_keeps_entry_boundary_meaning() {
        let code = crate::machine::ExecutableCode::from_ops(vec![Op::Return { src: 0 }]);
        let context = crate::vm::current_context_or_default();
        let mut before_registers = crate::register_file::RegisterFile::from_values(vec![
            Value::Number(1.0),
        ]);
        let before = super::NativeRegionContext::new(
            code.code(),
            0,
            &[crate::ir::Opcode::Return],
            &mut before_registers,
            &context,
        );
        assert!(matches!(
            before.finish(0xFFFF),
            Err(crate::machine::NativeDispatchError::Physical(message))
                if message.contains("invalid entry status")
        ));

        let mut after_registers = crate::register_file::RegisterFile::from_values(vec![
            Value::Number(1.0),
        ]);
        let mut after = super::NativeRegionContext::new(
            code.code(),
            0,
            &[crate::ir::Opcode::Return],
            &mut after_registers,
            &context,
        );
        after.entry_started = true;
        assert!(matches!(
            after.finish(0xFFFF),
            Err(crate::machine::NativeDispatchError::Committed(message))
                if message.contains("invalid post-entry status")
        ));
    }

    #[test]
    fn fallback_consumes_post_entry_completion_without_retry() {
        const OPS: &[crate::ir::Opcode] = &[crate::ir::Opcode::StoreLocal, crate::ir::Opcode::Return];
        let code = crate::machine::ExecutableCode::from_ops(vec![
            Op::StoreLocal { slot: 0, src: 0 },
            Op::Return { src: 0 },
        ]);
        let mut registers = crate::register_file::RegisterFile::from_values(vec![Value::Number(11.0)]);
        let environment = crate::environment::Environment::new();
        let _guard = crate::locals::EnvironmentGuard::install(environment.clone());
        let context = crate::vm::current_context_or_default();
        let mut region = super::NativeRegionContext::new(
            code.code(),
            0,
            OPS,
            &mut registers,
            &context,
        );
        let transition = super::execute_region_fallback(&mut region)
            .expect("the final Return is an exact post-entry completion");
        assert!(matches!(
            transition.completion,
            Some(crate::completion::Completion::Return(Value::Number(value))) if value == 11.0
        ));
        assert_eq!(transition.next_pc, 2, "resume state identifies the completed op");
        assert_eq!(environment.get(0), Value::Number(11.0));
    }

    #[test]
    fn fallback_reports_exact_fault_pc_after_prior_effect() {
        const OPS: &[crate::ir::Opcode] = &[crate::ir::Opcode::StoreLocal, crate::ir::Opcode::Slow];
        let code = crate::machine::ExecutableCode::from_ops(vec![
            Op::StoreLocal { slot: 0, src: 0 },
            Op::RequireObjectCoercible { src: 1 },
        ]);
        let mut registers = crate::register_file::RegisterFile::from_values(vec![
            Value::Number(17.0),
            Value::Undefined,
        ]);
        let environment = crate::environment::Environment::new();
        let _guard = crate::locals::EnvironmentGuard::install(environment.clone());
        let context = crate::vm::current_context_or_default();
        let mut region = super::NativeRegionContext::new(
            code.code(),
            0,
            OPS,
            &mut registers,
            &context,
        );
        let result = super::execute_region_fallback(&mut region);
        assert!(matches!(
            result,
            Err(crate::machine::NativeDispatchError::SemanticAt { pc: 1, .. })
        ));
        assert_eq!(environment.get(0), Value::Number(17.0));
    }

    #[test]
    fn native_bridge_preserves_fault_pc_after_prior_effect() {
        const OPS: &[crate::ir::Opcode] = &[crate::ir::Opcode::StoreLocal, crate::ir::Opcode::Slow];
        let code = crate::machine::ExecutableCode::from_ops(vec![
            Op::StoreLocal { slot: 0, src: 0 },
            Op::RequireObjectCoercible { src: 1 },
        ]);
        let mut registers = crate::register_file::RegisterFile::from_values(vec![
            Value::Number(23.0),
            Value::Undefined,
        ]);
        let environment = crate::environment::Environment::new();
        let _guard = crate::locals::EnvironmentGuard::install(environment.clone());
        let context = crate::vm::current_context_or_default();
        let mut region = super::NativeRegionContext::new(
            code.code(),
            0,
            OPS,
            &mut registers,
            &context,
        );
        let status = super::native_region_bridge(
            (&mut region as *mut super::NativeRegionContext<'_>)
                .cast::<std::ffi::c_void>(),
        );
        assert_eq!(status, super::NATIVE_DISPATCH_SEMANTIC_ERROR);
        assert_eq!(environment.get(0), Value::Number(23.0));
        assert!(matches!(
            region.finish(status),
            Err(crate::machine::NativeDispatchError::SemanticAt { pc: 1, .. })
        ));
    }

    #[test]
    fn native_dispatch_finish_rejects_post_entry_malformed_status() {
        let code = crate::machine::ExecutableCode::from_ops(vec![Op::Return { src: 0 }]);
        let entry = crate::machine::BaselineEntry {
            instruction: code.code().instruction(0).expect("entry"),
            handler: crate::ir::Opcode::Return.handler(),
            control: crate::ir::Opcode::Return.control_operands(
                code.code().instruction(0).expect("entry"),
            ),
        };
        let mut registers = crate::register_file::RegisterFile::from_values(vec![Value::Number(1.0)]);
        let context = crate::vm::current_context_or_default();
        let mut dispatch = super::NativeDispatchContext::new(
            code.code(),
            0,
            entry,
            &mut registers,
            &context,
        );
        dispatch.entry_started = true;
        assert!(matches!(
            dispatch.finish(0),
            Err(crate::machine::NativeDispatchError::Committed(_))
        ));
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

    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    #[test]
    fn baseline_load_local_uses_native_tagged_word_for_proven_slot() {
        let function = crate::machine::FunctionCode::from_ops(vec![
            Op::LoadLocal { dst: 0, slot: 1 },
            Op::Return { src: 0 },
        ]);
        let plan = crate::machine::BaselinePlan::compile_for_test(
            function.code().expect("function code"),
            crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test(),
        );
        assert!(plan.native_load_local_at(0).is_some());
        let code = function.code().expect("function code");
        let environment = crate::environment::Environment::new();
        environment.set(1, Value::Number(42.5));
        let context = crate::vm::current_context_or_default();
        let mut registers = crate::register_file::RegisterFile::with_undefined(4);
        let (completion, _) = crate::vm::execute_baseline_code_from(
            code,
            &plan,
            0,
            &mut registers,
            &context,
            environment,
        )
        .expect("native proven local load");
        assert_eq!(completion, crate::completion::Completion::Return(Value::Number(42.5)));
        assert_eq!(
            plan.native_load_local_at(0)
                .unwrap()
                .borrow()
                .native_entry_count(),
            1
        );
    }

    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    #[test]
    fn baseline_store_local_commits_native_tagged_word_once() {
        let function = crate::machine::FunctionCode::from_ops(vec![
            Op::Const {
                dst: 0,
                value: crate::ops::Constant::Number(9.25),
            },
            Op::StoreLocal { slot: 1, src: 0 },
            Op::Return { src: 0 },
        ]);
        let code = function.code().expect("function code");
        let plan = crate::machine::BaselinePlan::compile_for_test(
            code,
            crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test(),
        );
        assert!(plan.native_store_local_at(1).is_some());
        let environment = crate::environment::Environment::new();
        environment.set(1, Value::Number(0.0));
        let context = crate::vm::current_context_or_default();
        let mut registers = crate::register_file::RegisterFile::with_undefined(4);
        let (completion, _) = crate::vm::execute_baseline_code_from(
            code,
            &plan,
            0,
            &mut registers,
            &context,
            environment.clone(),
        )
        .expect("native proven local store");
        assert_eq!(completion, crate::completion::Completion::Return(Value::Number(9.25)));
        assert_eq!(environment.get(1), Value::Number(9.25));
        assert_eq!(plan.native_store_local_at(1).unwrap().borrow().native_entry_count(), 1);

        environment.mark_immutable_slot(1);
        registers.write_number(0, 10.5);
        let result = crate::vm::execute_baseline_code_from(
            code,
            &plan,
            1,
            &mut registers,
            &context,
            environment,
        );
        assert!(result.is_err(), "immutable stores must use the canonical throw path");
        assert_eq!(plan.native_store_local_at(1).unwrap().borrow().native_entry_count(), 1);
    }

    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    #[test]
    fn baseline_nullish_unary_uses_native_word_predicate_and_falls_back() {
        let function = crate::machine::FunctionCode::from_ops(vec![
            Op::Unary {
                dst: 0,
                operator: crate::ops::UnaryOp::IsNullish,
                src: 1,
            },
            Op::Return { src: 0 },
        ]);
        let code = function.code().expect("function code");
        let plan = crate::machine::BaselinePlan::compile_for_test(
            code,
            crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test(),
        );
        assert!(plan.native_nullish_at(0).is_some());
        let context = crate::vm::current_context_or_default();
        let environment = crate::environment::Environment::new();
        let mut registers = crate::register_file::RegisterFile::from_values(vec![
            Value::Undefined,
            Value::Null,
        ]);
        let (completion, _) = crate::vm::execute_baseline_code_from(
            code,
            &plan,
            0,
            &mut registers,
            &context,
            environment.clone(),
        )
        .expect("native nullish predicate");
        assert_eq!(completion, crate::completion::Completion::Return(Value::Boolean(true)));
        assert_eq!(plan.native_nullish_at(0).unwrap().borrow().native_entry_count(), 1);

        registers.write(1, Value::Number(3.0));
        let (completion, _) = crate::vm::execute_baseline_code_from(
            code,
            &plan,
            0,
            &mut registers,
            &context,
            environment,
        )
        .expect("native non-nullish predicate");
        assert_eq!(completion, crate::completion::Completion::Return(Value::Boolean(false)));
        assert_eq!(plan.native_nullish_at(0).unwrap().borrow().native_entry_count(), 2);
    }

    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    #[test]
    fn ordinary_source_nullish_coalescing_admits_native_predicate() {
        let executable = crate::reduce::reduce_source(
            "function f(x) { return x ?? 7; } f(null);",
        )
        .expect("source lowers");
        let code = executable.code();
        fn find_and_check(
            view: crate::machine::CodeView<'_>,
            policy: crate::stencil_policy::ExecutionPolicy,
        ) -> bool {
            for pc in 0..view.len() {
                let Some(instruction) = view.instruction(pc) else {
                    continue;
                };
                if instruction.opcode == crate::ir::Opcode::Unary
                    && instruction.flags
                        == crate::ir::compact_unary_id(crate::ops::UnaryOp::IsNullish)
                {
                    let plan = crate::machine::BaselinePlan::compile_for_test(view, policy);
                    assert!(plan.native_nullish_at(pc).is_some());
                    return true;
                }
                let Some(op) = view.cold_at(pc) else {
                    continue;
                };
                let mut found = false;
                op.visit_bodies(&mut |body| {
                    if !found {
                        if let Some(nested) = body.code() {
                            found = find_and_check(nested, policy);
                        }
                    }
                });
                if found {
                    return true;
                }
            }
            false
        }
        let policy = crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test();
        assert!(find_and_check(code, policy), "lowered nullish predicate");
        let plan = crate::machine::BaselinePlan::compile_for_test(code, policy);
        let context = crate::vm::current_context_or_default();
        let environment = crate::environment::Environment::new();
        let mut registers = crate::register_file::RegisterFile::with_undefined(16);
        let result = crate::vm::execute_baseline_code_from(
            code,
            &plan,
            0,
            &mut registers,
            &context,
            environment,
        )
        .expect("ordinary nullish execution");
        assert!(
            matches!(result.0, crate::completion::Completion::Return(_)),
            "unexpected completion: {:?}",
            result.0
        );
    }

    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    #[test]
    fn baseline_fused_add_chain_matches_two_canonical_adds() {
        let function = crate::machine::FunctionCode::from_ops(vec![
            Op::Binary { dst: 0, operator: crate::ops::BinaryOp::Add, lhs: 1, rhs: 2 },
            Op::Binary { dst: 3, operator: crate::ops::BinaryOp::Add, lhs: 0, rhs: 4 },
            Op::Return { src: 3 },
        ]);
        function.set_tier_threshold_for_test(1);
        function.retire(1);
        assert_eq!(function.enter_invocation(), crate::machine::TierTransition::CompileBaseline);
        let plan = function.baseline_plan().expect("baseline plan");
        if crate::stencil_policy::current().native_leaves {
            assert!(plan.native_add_chain_at(0).is_some());
        }
        let code = function.code().expect("function code");
        let context = crate::vm::current_context_or_default();
        let environment = crate::environment::Environment::new();
        let mut registers = crate::register_file::RegisterFile::from_values(vec![
            Value::Undefined,
            Value::Number(1.5),
            Value::Number(2.25),
            Value::Undefined,
            Value::Number(4.0),
        ]);
        let (completion, _) = crate::vm::execute_baseline_code_from(
            code, &plan, 0, &mut registers, &context, environment,
        )
        .expect("fused numeric chain");
        assert_eq!(completion, crate::completion::Completion::Return(Value::Number(7.75)));
        if crate::stencil_policy::current().native_leaves {
            assert_eq!(
                plan.native_add_chain_at(0)
                    .expect("fused plan")
                    .borrow()
                    .native_entry_count(),
                1,
                "fused test must execute the physical entry"
            );
        }
    }

    #[test]
    fn fused_add_chain_is_rejected_when_first_result_remains_live() {
        let function = crate::machine::FunctionCode::from_ops(vec![
            Op::Binary {
                dst: 0,
                operator: crate::ops::BinaryOp::Add,
                lhs: 1,
                rhs: 2,
            },
            Op::Binary {
                dst: 3,
                operator: crate::ops::BinaryOp::Add,
                lhs: 0,
                rhs: 4,
            },
            // Keep the first result live after the pair. A fused stencil
            // returns only dst=3, so build-time admission must stay on the
            // canonical residual path here.
            Op::Move { dst: 5, src: 0 },
            Op::Return { src: 5 },
        ]);
        function.set_tier_threshold_for_test(1);
        function.retire(1);
        assert_eq!(
            function.enter_invocation(),
            crate::machine::TierTransition::CompileBaseline
        );
        let plan = function.baseline_plan().expect("baseline plan");
        assert!(plan.native_add_chain_at(0).is_none());
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
    fn profiled_loop_body_span_matches_ordinary_execution() {
        let executable = crate::machine::ExecutableCode::from_ops(vec![
            Op::CheckInitialized {
                slot: 0,
                name: "left".into(),
            },
            Op::LoadLocal { dst: 1, slot: 0 },
            Op::CheckInitialized {
                slot: 1,
                name: "right".into(),
            },
            Op::LoadLocal { dst: 2, slot: 1 },
            Op::Binary {
                dst: 3,
                operator: crate::ops::BinaryOp::Add,
                lhs: 1,
                rhs: 2,
            },
            Op::StoreLocal { slot: 3, src: 3 },
            Op::Move { dst: 4, src: 3 },
            Op::LoadLocal { dst: 5, slot: 0 },
            Op::Const {
                dst: 6,
                value: crate::ops::Constant::Number(1.0),
            },
            Op::Binary {
                dst: 7,
                operator: crate::ops::BinaryOp::NumericAdd,
                lhs: 5,
                rhs: 6,
            },
            Op::CheckInitialized {
                slot: 0,
                name: "left".into(),
            },
            Op::StoreLocal { slot: 0, src: 7 },
            Op::Return { src: 4 },
        ]);
        let code = executable.code();
        let key = crate::stencil_select::loop_body_region_key();
        let record = crate::stencil_select::select_region(key).expect("loop body row");
        assert_eq!(record.operations.len(), 7);
        assert_eq!(code.len(), record.operations.len());
        assert!(code
            .slice(0, record.operations.len())
            .is_some_and(|view| (0..view.len()).all(|pc| {
                view.instruction(pc).is_some_and(|instruction| {
                    instruction.opcode == record.operations[pc]
                })
            })));

        let values = vec![Value::Number(2.0), Value::Number(5.0), Value::Undefined];
        let context = crate::vm::current_context_or_default();
        let mut ordinary = crate::register_file::RegisterFile::from_values(values.clone());
        let expected_transition = {
            let environment = crate::environment::Environment::new();
            environment.set(0, values[0].clone());
            environment.set(1, values[1].clone());
            let _guard = crate::locals::EnvironmentGuard::install(environment);
            execute_one_at_a_time(code, &mut ordinary, &context)
        };
        let expected_registers = ordinary.clone();

        let mut fused = crate::register_file::RegisterFile::from_values(values);
        let actual_transition = {
            let environment = crate::environment::Environment::new();
            environment.set(0, Value::Number(2.0));
            environment.set(1, Value::Number(5.0));
            let _guard = crate::locals::EnvironmentGuard::install(environment);
            let mut region = crate::machine::NativeRegionPlan::new_for_test(key)
                .expect("loop body test plan");
            region
                .execute(code, 0, &mut fused, &context)
                .expect("loop body fused execution")
        };
        assert_transition_equal(&actual_transition, &expected_transition);
        assert_eq!(fused, expected_registers);

        // A stale fact in the middle of the seven-op window must reject the
        // entire span before its first handler mutates a register.
        let mut partial = crate::register_file::RegisterFile::from_values(vec![
            Value::Number(2.0),
            Value::Number(5.0),
            Value::Undefined,
        ]);
        let before = partial.clone();
        code.quicken_instruction(3, crate::ir::Opcode::Slow, 0, 0, 0);
        let environment = crate::environment::Environment::new();
        environment.set(0, Value::Number(2.0));
        environment.set(1, Value::Number(5.0));
        let _guard = crate::locals::EnvironmentGuard::install(environment);
        let mut region = crate::machine::NativeRegionPlan::new_for_test(key)
            .expect("loop body hostile test plan");
        assert!(matches!(
            region.execute(code, 0, &mut partial, &context),
            Err(crate::machine::NativeDispatchError::Physical(_))
        ));
        assert_eq!(partial, before, "hostile span executed a prefix");
    }

    #[test]
    fn ordinary_source_lowering_emits_numeric_backedge_and_executes() {
        let program = crate::reduce::reduce_source(
            "var a = [1, 2, 3]; for (var i = 0; i < 3; i = i + 1) a[i] = a[i] + 1; a;",
        )
        .expect("ordinary loop lowers");
        let code = program.code();
        fn contains_backedge(view: crate::machine::CodeView<'_>) -> bool {
            let direct = (0..view.len()).any(|pc| {
                view.instruction(pc).is_some_and(|instruction| {
                    instruction.opcode == crate::ir::Opcode::Jump
                        && usize::from(instruction.b) < pc
                })
            });
            direct || view.cold_ops().any(|(_, op)| {
                if matches!(op, crate::ops::Op::Loop { .. }) {
                    return true;
                }
                let mut found = false;
                op.visit_bodies(&mut |body| {
                    found |= body.code().is_some_and(contains_backedge);
                });
                found
            })
        }
        #[cfg(not(feature = "execution-trace"))]
        fn contains_shape(
            view: crate::machine::CodeView<'_>,
            shape: &[crate::ir::Opcode],
        ) -> bool {
            let direct = (0..view.len()).any(|pc| {
                (0..shape.len()).all(|offset| {
                    view.instruction(pc + offset)
                        .is_some_and(|instruction| instruction.opcode == shape[offset])
                })
            });
            direct || view.cold_ops().any(|(_, op)| {
                let mut found = false;
                op.visit_bodies(&mut |body| {
                    found |= body.code().is_some_and(|nested| contains_shape(nested, shape));
                });
                found
            })
        }
        let backedge = contains_backedge(code);
        assert!(backedge, "ordinary source must lower a nested loop body with a backward edge");
        let admitted_shape = crate::stencil_select::select_region(
            crate::stencil_select::array_numeric_loop_region_key(),
        )
        .expect("numeric loop declaration")
        .operations;
        #[cfg(not(feature = "execution-trace"))]
        assert!(
            contains_shape(code, admitted_shape),
            "ordinary source must expose the declared numeric-loop span"
        );
        #[cfg(all(target_arch = "aarch64", not(feature = "execution-trace")))]
        {
            let policy = crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test();
            let mut reaches_native_admission = false;
            let mut native_execution_verified = false;
            code.cold_ops().for_each(|(_, op)| {
                op.visit_bodies(&mut |body| {
                    if let Some(body_code) = body.code() {
                        let plan = crate::machine::BaselinePlan::compile_for_test(body_code, policy);
                        reaches_native_admission |= (0..body_code.len()).any(|pc| {
                            plan.native_region_at(pc).is_some_and(|region| {
                                region.borrow().key_for_test()
                                    == crate::stencil_select::array_numeric_loop_region_key()
                            })
                        });
                        if !native_execution_verified
                            && (0..body_code.len()).any(|pc| {
                                (0..admitted_shape.len()).all(|offset| {
                                    body_code.instruction(pc + offset).is_some_and(|instruction| {
                                        instruction.opcode == admitted_shape[offset]
                                    })
                                })
                            })
                        {
                            let plan = Rc::new(crate::machine::BaselinePlan::compile_for_test(
                                body_code, policy,
                            ));
                            let Some(shape_pc) = (0..body_code.len()).find(|pc| {
                                (0..admitted_shape.len()).all(|offset| {
                                    body_code.instruction(*pc + offset).is_some_and(|instruction| {
                                        instruction.opcode == admitted_shape[offset]
                                    })
                                })
                            }) else {
                                return;
                            };
                            let mut registers =
                                crate::register_file::RegisterFile::with_undefined(
                                    usize::from(body_code.register_count()).max(8),
                                );
                            let environment = crate::environment::Environment::new();
                            let mut array_slot = None;
                            for pc in 0..body_code.len() {
                                let instruction = body_code.instruction(pc).expect("body instruction");
                                match instruction.opcode {
                                    crate::ir::Opcode::LoadLocal
                                    | crate::ir::Opcode::LoadLocalChecked
                                    | crate::ir::Opcode::StoreLocal
                                    | crate::ir::Opcode::StoreLocalChecked => {
                                        let slot = if matches!(
                                            instruction.opcode,
                                            crate::ir::Opcode::StoreLocal
                                                | crate::ir::Opcode::StoreLocalChecked
                                        ) {
                                            instruction.a
                                        } else {
                                            instruction.b
                                        };
                                        environment.set(slot, Value::Number(0.0));
                                    }
                                    crate::ir::Opcode::AGetI => {
                                        array_slot = (0..body_code.len()).find_map(|candidate| {
                                            let load = body_code.instruction(candidate)?;
                                            (load.opcode == crate::ir::Opcode::LoadLocal
                                                && load.a == instruction.b)
                                                .then_some(load.b)
                                        });
                                    }
                                    _ => {}
                                }
                            }
                            let array_data = Rc::new(crate::value::ArrayData::new(vec![
                                Value::Number(1.0),
                                Value::Number(2.0),
                                Value::Number(3.0),
                            ]));
                            let array = Value::Array(Rc::clone(&array_data));
                            let Some(array_slot) = array_slot else { return };
                            environment.set(array_slot, array);
                            if let Some(index_register) = body_code
                                .instruction(shape_pc)
                                .map(|instruction| instruction.a)
                            {
                                registers.write_number(usize::from(index_register), 0.0);
                            }
                            let context = crate::vm::current_context_or_default();
                            // Exercise the real normal-driver checkpoint: the
                            // first native iteration must publish its state,
                            // clear the request, and resume the same residual
                            // entry without replaying that store.
                            context.request_interrupt();
                            let _environment_guard =
                                crate::locals::EnvironmentGuard::install(Rc::clone(&environment));
                            let execution = crate::vm::execute_baseline_code_from(
                                body_code,
                                &plan,
                                shape_pc,
                                &mut registers,
                                &context,
                                Rc::clone(&environment),
                            );
                            let native_execution = plan
                                .native_region_at(shape_pc)
                                .is_some_and(|region| region.borrow().last_native_execution());
                            if execution.is_ok() && native_execution {
                                assert!(
                                    unsafe { &*context.interrupt_flag() }
                                        .load(std::sync::atomic::Ordering::Acquire)
                                        == false,
                                    "normal driver must consume the native checkpoint"
                                );
                                assert_eq!(array_data.dense_number_at(0), Some(2.0));
                                assert_eq!(array_data.dense_number_at(1), Some(3.0));
                                assert_eq!(array_data.dense_number_at(2), Some(4.0));
                                native_execution_verified = true;
                            }
                        }
                    }
                });
            });
            assert!(reaches_native_admission, "ordinary ARM lowering must reach native loop admission");
            assert!(native_execution_verified, "ordinary lowered body must execute native bytes");
        }
        let result = crate::vm::execute_code_with_context(
            code,
            &crate::vm::VmContext::default(),
        )
        .expect("ordinary loop executes");
        assert!(matches!(result, Value::Undefined));
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
