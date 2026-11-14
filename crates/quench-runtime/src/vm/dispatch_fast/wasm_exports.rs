//! JavaScript's calls of Wasm exports, made from the lane: the export's body becomes an
//! activation of the lane that runs the caller, and its return resumes the caller directly.
//! The lane's run is JavaScript or Wasm at any moment (`LaneContext::script_calls`,
//! `inline_calls`); an export's activation flips it to Wasm and its return flips it back.
//! Made a frame (`Vm::materialize_lane_activations`), the activation is the Wasm frame of its
//! instance above its JavaScript caller's frame, a trap leaving it as the exported function's
//! boundary makes it a `RuntimeError` (`Vm::export_boundary_error`).
use super::*;

/// Entries of the export cache: the exports a program calls from JavaScript are few.
const EXPORT_TARGETS: usize = 16;

/// An export the lane enters as an activation: every parameter and its one result an `i32`,
/// so the values JavaScript passes and receives are the registers' as they are. A callee the
/// lane never enters is recorded too, with no view, so the call's native path pays no search.
#[derive(Clone, Copy)]
pub(super) struct ExportTarget {
    /// The exported function's wrapper (`Native::WasmExportedFunction`) a call names.
    callee: Value,
    /// The `LaneStack::generation` it was resolved in.
    generation: ScriptGeneration,
    program: ProgramId,
    function: u32,
    /// The instance environment its bindings load from.
    env: Value,
    view: *const LaneView,
    shape: ActivationShape,
}

const EMPTY_EXPORT_TARGET: ExportTarget = ExportTarget {
    callee: Value::UNDEFINED,
    generation: NO_SCRIPT_GENERATION,
    program: ProgramId::MAIN,
    function: 0,
    env: Value::UNDEFINED,
    view: std::ptr::null(),
    shape: ActivationShape { registers: 0, parameters: 0, params: 0, initial: Value::UNDEFINED },
};

/// The exports of the running lane, direct-mapped by the wrapper's heap index.
pub(super) struct ExportTargets([ExportTarget; EXPORT_TARGETS]);

impl Default for ExportTargets {
    fn default() -> Self {
        Self([EMPTY_EXPORT_TARGET; EXPORT_TARGETS])
    }
}

impl ExportTargets {
    fn slot(callee: Value) -> Option<usize> {
        callee.heap_index().map(|index| index as usize % EXPORT_TARGETS)
    }
}

impl<H: Host> Vm<H> {
    /// `callee`'s entry in the export cache: the recorded one while its generation holds, else
    /// the one its function resolves to, recorded; `None` for any other callee or a body whose
    /// lane view is not derived yet, which the call's general path runs and derives.
    #[inline(always)]
    fn export_target(&mut self, callee: Value) -> Option<ExportTarget> {
        let slot = ExportTargets::slot(callee)?;
        let mut target = self.lane_stack.wasm_exports.0[slot];
        if target.callee != callee || target.generation != self.lane_stack.generation {
            target = self.resolve_export(callee)?;
            self.lane_stack.wasm_exports.0[slot] = target;
        }
        Some(target).filter(|target| !target.view.is_null())
    }

    #[cold]
    #[inline(never)]
    fn resolve_export(&self, callee: Value) -> Option<ExportTarget> {
        let wrapper = self.heap.function(callee)?;
        let FunctionKind::Native(Native::WasmExportedFunction) = wrapper.kind else { return None };
        let FunctionRecord { kind: FunctionKind::User(program, function), env, realm, .. } = *self.heap.function(wrapper.env)? else {
            return None;
        };
        let signatures = self.programs.wasm_signatures_shared(program)?;
        let signature = signatures.defined_signature(function)?;
        let declined = ExportTarget { callee, generation: self.lane_stack.generation, ..EMPTY_EXPORT_TARGET };
        if realm != self.realm.globals || !signature.is_i32_call() {
            return Some(declined);
        }
        // A body not derived yet runs its first call on the native's path, which derives it.
        let view = self.programs.lane_views(program)?.get(function as usize).and_then(super::program_store::derived_lane_view)?;
        let Some(shape) = view.activation else { return Some(declined) };
        Some(ExportTarget { program, function, env, view: std::ptr::from_ref(view), shape, ..declined })
    }

    /// Whether an activation of `shape` fits: the guest stack the thread admits, the
    /// activations' capacity and the lane's values (`enter_wasm_function!`).
    #[inline(always)]
    fn export_admitted(&self, shape: ActivationShape) -> bool {
        let stack = &self.lane_stack;
        usize::from(shape.registers) * std::mem::size_of::<Value>()
            <= crate::stack::GuestCallGuard::remaining_storage_bytes().saturating_sub(stack.wasm_storage_bytes)
            && stack.activations.len() < stack.activation_limit.get()
            && self.frames.len() + stack.activations.len() < super::dispatch_frame::WASM_ACTIVATION_LIMIT
            && stack.values.room() >= fill_room::<ACTIVATION_FILL_BLOCK>(usize::from(shape.registers))
    }

    /// The error a trap or exception becomes leaving the Wasm frame of `callee` for the
    /// JavaScript frame of `caller` (`call_wasm_exported_function`'s boundary): only a Wasm
    /// frame called from JavaScript is an export's, which a materialized activation is.
    pub(in crate::vm) fn export_boundary_error(&mut self, callee: &ResidualProgram, caller: &ResidualProgram, error: JsError) -> JsError {
        if callee.kind == crate::bytecode::ProgramKind::Wasm && caller.kind != crate::bytecode::ProgramKind::Wasm {
            return self.trap_raised_in_javascript(caller, error);
        }
        error
    }
}

/// The environment of the Wasm instance the running function belongs to: the context's while
/// an export's activation runs, else its frame's (`LaneContext::env`).
#[inline(always)]
pub(super) fn running_env<H: Host>(cx: *mut LaneContext<H>) -> Value {
    let env = unsafe { (*cx).env };
    if env.is_deleted() { unsafe { vm(cx) }.frames[unsafe { (*cx).frame }].env } else { env }
}

script_handler! {
    /// A `Call` of a Wasm export: its body's activation where the call enters one
    /// (`lane_enter_export`), else the native's call.
    #[inline(never)]
    fn lane_call_export<>(cx, r, ip, i, view) {
        if let Some((registers, base)) = lane_enter_export(cx, r, ip, i) {
            let view = Carried { memory: MemoryView::NONE, integers: crate::value::IntegerEncoding::TAG, acc: Slot::of(Value::UNDEFINED) };
            next!(cx, registers, base, view)
        }
        resume!(cx, lane_native_call_value(cx, r, ip, i, Native::WasmExportedFunction))
    }
}

/// The export call's handler, for the native call's dispatch.
pub(super) const fn call_export_handler<H: Host>() -> Handler {
    lane_call_export::<H>
}

/// The activation of the export the call record at `ip` calls, with the arguments it passes
/// (`i32` values, absent ones zero), and its first record; `None` leaves the call to the native.
#[inline(always)]
fn lane_enter_export<H: Host>(cx: *mut LaneContext<H>, r: Registers, ip: Ip, i: &LaneInstruction) -> Option<(Registers, Ip)> {
    let vm = unsafe { vm(cx) };
    if !unsafe { (*cx).script_calls }
        || vm.with_scope_entered
        || vm.execution_mode == super::ExecutionMode::Compiled
        || i.a & crate::bytecode::RETURN_REGISTER != 0 && !lane_returning_call_admitted(cx)
    {
        return None;
    }
    let target = vm.export_target(r.get(i.b))?;
    if !vm.export_admitted(target.shape) {
        return None;
    }
    let registers = export_window(vm, r, i, &target)?;
    // SAFETY: `export_admitted` left room for the activation.
    unsafe {
        let call = Activation {
            native: crate::vm::compiled::ShadowFrame::EMPTY,
            function: target.function,
            call: ip,
            caller_base: (*cx).base,
            caller_registers: r,
            caller_program: (*cx).program,
            kind: ActivationKind::WasmExport { frame_storage_bytes: usize::from(target.shape.registers) * std::mem::size_of::<Value>(), caller_env: (*cx).env },
        };
        vm.lane_stack.push_activation(call, registers.0);
    }
    Some(lane_run_export(cx, registers, &target))
}

/// The registers of an activation of `target` for the call record `i`, whose window starts at
/// `r`'s `first`, pushed on the lane's values: the arguments in their parameter registers.
/// `None`, which pushes nothing, for one that is not an `i32`.
#[inline(always)]
fn export_window<H: Host>(vm: &mut Vm<H>, r: Registers, i: &LaneInstruction, target: &ExportTarget) -> Option<Registers> {
    let shape = target.shape;
    let (first, passed) = (ImmediateLayout::call_window_base(i.imm()), ImmediateLayout::argument_count(i.imm()));
    let stack = &mut vm.lane_stack;
    // SAFETY: capacity holds the window; each slot is written before the length covers it,
    // and no activation's window moves.
    unsafe {
        let registers = stack.values.top();
        fill_window(registers, usize::from(shape.registers), shape.initial);
        for parameter in 0..shape.params {
            let value = if parameter < passed { r.get(first + parameter).as_int()? } else { 0 };
            registers.add(usize::from(shape.parameters + parameter)).write(Value::integer(value));
        }
        stack.values.set_top(registers.add(usize::from(shape.registers)));
        Some(Registers(registers))
    }
}

/// Runs `target`'s activation (`registers`) as a Wasm lane: its program and instance's
/// environment, its memory not yet bound, and no JavaScript call or construction of its own.
/// Returns its first record.
#[inline(always)]
fn lane_run_export<H: Host>(cx: *mut LaneContext<H>, registers: Registers, target: &ExportTarget) -> (Registers, Ip) {
    lane_enter_program(cx, target.program);
    // SAFETY: the program store keeps a derived view alive with its program.
    let base = unsafe { (*target.view).records() };
    unsafe {
        (*cx).registers = registers;
        (*cx).base = base;
        (*cx).env = target.env;
        (*cx).memory = MemoryView::NONE;
        // JavaScript between two calls may have grown a memory the run resolved.
        (*cx).resolved = (Value::UNDEFINED, MemoryView::NONE);
        (*cx).script_calls = false;
        (*cx).inline_calls = true;
    }
    (registers, base)
}

/// The return of the Wasm function the innermost activation, an export's, runs: `value` is
/// the call's result in its JavaScript caller, which resumes after the call in its program, or,
/// for a call that returns its result, completes in turn (`lane_native_complete`). `None`,
/// which changes nothing, for any other activation.
#[inline(always)]
pub(super) fn lane_complete_export<H: Host>(cx: *mut LaneContext<H>, value: Value) -> Option<Resumed> {
    let vm = unsafe { vm(cx) };
    let stack = &mut vm.lane_stack;
    let top = stack.activations.last()?;
    let ActivationKind::WasmExport { frame_storage_bytes, caller_env } = top.kind else { return None };
    let (call, caller_registers, caller_base, caller_program) = (top.call, top.caller_registers, top.caller_base, top.caller_program);
    let window = top.native.window as *mut Value;
    // SAFETY: a Wasm activation holds no drop glue; its window is in the values.
    unsafe {
        stack.activations.set_len(stack.activations.len() - 1);
        stack.values.set_top(window);
    }
    stack.wasm_storage_bytes -= frame_storage_bytes;
    lane_enter_program(cx, caller_program);
    unsafe {
        (*cx).registers = caller_registers;
        (*cx).base = caller_base;
        (*cx).env = caller_env;
        (*cx).memory = MemoryView::script(cx);
        (*cx).script_calls = true;
        (*cx).inline_calls = false;
    }
    // SAFETY: the call record belongs to the caller's live view.
    Some(lane_native_complete(cx, call, unsafe { &*call }, Some(value)))
}
