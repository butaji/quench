//! Wasm calls the lane enters as activations: statically known calls and reference calls. The
//! call rules are `staged::wasm_activation`; this file is the lane's state they run on
//! (`LaneCall`) and the lane's own steps.
use super::*;
use crate::staged::machine::{Machine, MachineStage};
use crate::staged::wasm_activation::{self as activation, CallTarget, NativeCall, WasmActivation};
use crate::vm::JsError;

/// The known call's handler, for the shared handler table.
pub(super) const fn known_call_handler<H: Host>() -> Handler {
    lane_call_known::<H>
}

/// The reference call's handler, for Wasm views.
pub(super) const fn reference_call_handler<H: Host>() -> Handler {
    lane_call_wasm_reference::<H>
}

/// A Wasm call as the lane makes it: the context, the caller's registers and record, and its
/// memory view. The rules of `staged::wasm_activation` decide the call over these primitives.
struct LaneCall<'a, H: Host> {
    cx: *mut LaneContext<H>,
    r: Registers,
    ip: Ip,
    i: &'a LaneInstruction,
    memory: MemoryView,
}

impl<H: Host> Machine for LaneCall<'_, H> {
    type Word = u64;
}

impl<H: Host> MachineStage for LaneCall<'_, H> {
    type Exit = JsError;

    crate::staged::wasm_concrete::scalar_operators! {}

    fn trap_when(&mut self, flag: u64, trap: crate::WasmTrap) -> Result<(), JsError> {
        if flag == 0 { Ok(()) } else { Err(JsError::wasm_trap_error(trap)) }
    }
}

impl<H: Host> WasmActivation for LaneCall<'_, H> {
    type Resume = Resumed;
    type Shape = (ActivationShape, Ip);

    #[inline(always)]
    fn reference(&mut self) -> CallTarget {
        let vm = unsafe { vm(self.cx) };
        let running = (unsafe { (*self.cx).program }, Some(lane_instance_env(self.cx)));
        match vm.heap.function(self.r.get(self.i.b)) {
            Some(FunctionRecord { kind: FunctionKind::User(program, function), env, .. })
                if (*program, Some(*env)) == running =>
            {
                u16::try_from(*function).map_or(CallTarget::Other, |function| CallTarget::Function(u32::from(function)))
            }
            Some(FunctionRecord { kind: FunctionKind::Native(Native::WasmHost), .. }) => CallTarget::Import,
            _ => CallTarget::Other,
        }
    }

    #[inline(always)]
    fn inline_calls(&mut self) -> bool {
        unsafe { (*self.cx).inline_calls }
    }

    #[inline(always)]
    fn native_mode(&mut self) -> bool {
        let vm = unsafe { vm(self.cx) };
        vm.execution_mode == super::ExecutionMode::Compiled && self.i.a & crate::bytecode::RETURN_REGISTER == 0
    }

    #[inline(always)]
    fn has_native(&mut self, function: u32) -> bool {
        let vm = unsafe { vm(self.cx) };
        let (program, function) = (unsafe { (*self.cx).program }, function as usize);
        vm.programs.has_compiled_function_or_note_fallback(program, function)
            || vm.programs.has_wasm_tier_containing(program, function)
    }

    #[inline(always)]
    fn note_tier_entry(&mut self, function: u32) -> bool {
        let vm = unsafe { vm(self.cx) };
        let program = unsafe { (*self.cx).program };
        vm.programs.note_wasm_tier_lane_entry(program, function)
    }

    #[inline(always)]
    fn enter_compiled(&mut self, function: u32) -> NativeCall {
        // SAFETY: the window is the caller's registers, read into raw words before native code
        // runs, so before guest code could make the activations frames.
        let args = unsafe { lane_call_window(self.r, self.i) };
        let (program, env) = (unsafe { (*self.cx).program }, lane_instance_env(self.cx));
        let called = lane_in_place(self.cx, self.ip, |vm, _| {
            vm.run_compiled(program, env, function, args).transpose()
        });
        match called {
            None => NativeCall::Left,
            Some(None) => NativeCall::Declined,
            Some(Some(value)) => NativeCall::Ran(value.to_bits()),
        }
    }

    #[inline(always)]
    fn shape(&mut self, function: u32) -> Option<(ActivationShape, Ip)> {
        // SAFETY: the context's views outlive the lane run.
        let views = unsafe { &*(*self.cx).views };
        let lane_view = views.get(function as usize).and_then(super::program_store::derived_lane_view)?;
        Some((lane_view.activation?, lane_view.records()))
    }

    #[inline(always)]
    fn shape_registers(shape: Self::Shape) -> usize {
        usize::from(shape.0.registers)
    }

    #[inline(always)]
    fn activation_room(&mut self) -> bool {
        let vm = unsafe { vm(self.cx) };
        vm.lane_stack.activations.len() < vm.lane_stack.activation_limit.get()
    }

    #[inline(always)]
    fn frame_room(&mut self) -> bool {
        let vm = unsafe { vm(self.cx) };
        vm.frames.len() + vm.lane_stack.activations.len() < super::dispatch_frame::WASM_ACTIVATION_LIMIT
    }

    #[inline(always)]
    fn storage_room(&mut self, registers: usize) -> bool {
        // Depth and room are per call: each activation the lane pushes or pops moves them, so
        // they are not facts a link proves once.
        let vm = unsafe { vm(self.cx) };
        registers.saturating_mul(std::mem::size_of::<Value>())
            <= crate::stack::GuestCallGuard::remaining_storage_bytes()
                .saturating_sub(vm.lane_stack.wasm_storage_bytes)
    }

    #[inline(always)]
    fn values_room(&mut self, registers: usize) -> bool {
        let vm = unsafe { vm(self.cx) };
        vm.lane_stack.values.room() >= fill_room::<ACTIVATION_FILL_BLOCK>(registers)
    }

    #[inline(always)]
    fn push_activation(&mut self, function: u32, shape: Self::Shape) -> Resumed {
        let (shape, base) = shape;
        let vm = unsafe { vm(self.cx) };
        let window = vm.lane_stack.values.len();
        let frame_storage_bytes = usize::from(shape.registers).saturating_mul(std::mem::size_of::<Value>());
        let callee = fill_callee_window(vm, self.r, self.i, shape);
        // SAFETY: capacity holds the activation; the window's start has not moved.
        unsafe {
            let stack = &mut vm.lane_stack;
            let at = stack.values.as_mut_ptr().wrapping_add(window);
            stack.push_activation(
                Activation {
                    native: crate::vm::compiled::ShadowFrame::EMPTY,
                    function,
                    call: self.ip,
                    caller_base: (*self.cx).base,
                    caller_registers: self.r,
                    caller_program: (*self.cx).program,
                    kind: ActivationKind::Wasm { memory: self.memory, frame_storage_bytes },
                },
                at,
            );
        }
        // A fresh activation's memory register holds no memory until the function's instance
        // binding loads it.
        unsafe {
            (*self.cx).registers = callee;
            (*self.cx).base = base;
            (*self.cx).memory = MemoryView::NONE;
        }
        let view = Carried {
            memory: MemoryView::NONE,
            integers: crate::value::IntegerEncoding::TAG,
            acc: Slot::of(Value::UNDEFINED),
        };
        Resumed::At(callee, base, view)
    }

    #[inline(always)]
    fn frame_call(&mut self) -> Resumed {
        lane_call_frame::<H>(self.ip)
    }

    #[inline(always)]
    fn activate_import(&mut self) -> Option<Resumed> {
        lane_activate_js_import::<H>(self.cx, self.r, self.ip, self.memory)
    }

    #[inline(always)]
    fn import_native(&mut self) -> NativeCall {
        let vm = unsafe { vm(self.cx) };
        let callee = self.r.get(self.i.b);
        let Some((target, capture, signature)) = native_import_target(vm, callee) else {
            return NativeCall::Declined;
        };
        // SAFETY: the window is the caller's registers, copied before guest code runs.
        let args: smallvec::SmallVec<[Value; 8]> =
            unsafe { lane_call_window(self.r, self.i) }.iter().copied().collect();
        if args.len() != signature.params.len() {
            return NativeCall::Declined;
        }
        let program = unsafe { (*self.cx).program };
        let called = lane_in_place(self.cx, self.ip, |vm, code| {
            vm.call_compiled_js_wasm_import(
                program, code, callee, target, capture, &signature, Value::UNDEFINED, &args,
                Value::UNDEFINED, std::ptr::null_mut(),
            )
        });
        match called {
            None => NativeCall::Left,
            Some(value) => NativeCall::Ran(value.to_bits()),
        }
    }

    #[inline(always)]
    fn left(&mut self) -> Resumed {
        Resumed::Exit(exit(self.ip))
    }

    #[inline(always)]
    fn complete_call(&mut self, word: u64) -> Resumed {
        let vm = unsafe { vm(self.cx) };
        // The caller's view is of the run's resolved binding, which its return reads again: the
        // callee may have grown the memory.
        let (binding, resolved) = unsafe { (*self.cx).resolved };
        let memory = if self.memory.bytes == resolved.bytes {
            let fresh = memory_view(vm, binding);
            unsafe { (*self.cx).resolved = (binding, fresh) };
            fresh
        } else {
            MemoryView::NONE
        };
        // SAFETY: guest code the callee ran may have made the activations frames, whose
        // registers the context then names.
        let r = unsafe {
            (*self.cx).memory = memory;
            (*self.cx).registers
        };
        r.set(self.i.a, Value::from_bits(word));
        let view = Carried { memory, integers: crate::value::IntegerEncoding::TAG, acc: Slot::of(Value::UNDEFINED) };
        Resumed::At(r, following(self.ip), view)
    }
}

lane_handler! {
    /// A Wasm call to a function of the same program, pushed as the general
    /// loop's inline call would be.
    fn lane_call_known<>(cx, r, ip, i, view) {
        let mut call = LaneCall { cx, r, ip, i, memory: view.memory };
        match activation::call_known(&mut call, u32::from(i.b)) {
            Some(resumed) => resume!(cx, resumed),
            None => return exit(ip),
        }
    }
}

lane_handler! {
    /// A Wasm reference call (`call_ref`, or `call_indirect` past its target check) of a
    /// function of the running program and instance, entered as `lane_call_known` enters its
    /// callee. Validation proved the callee's type. Any other callee leaves for the general loop.
    fn lane_call_wasm_reference<>(cx, r, ip, i, view) {
        let mut call = LaneCall { cx, r, ip, i, memory: view.memory };
        match activation::call_reference(&mut call) {
            Some(resumed) => resume!(cx, resumed),
            None => return exit(ip),
        }
    }
}

/// The callee's frame at the top of the value stack: its registers filled with the shape's
/// initial value, its parameters from the call's arguments, and its `this`, environment and
/// capture from the caller's. Returns the frame's registers.
#[inline(always)]
fn fill_callee_window<H: Host>(vm: &mut Vm<H>, r: Registers, i: &LaneInstruction, shape: ActivationShape) -> Registers {
    let stack = &mut vm.lane_stack;
    let window = stack.values.len();
    let count = usize::from(shape.registers);
    let arguments = ImmediateLayout::call_window_base(i.imm());
    let passed = ImmediateLayout::argument_count(i.imm()).min(shape.params);
    // SAFETY: capacity holds the window; each slot is written before the length covers it, and
    // no activation's window moves.
    unsafe {
        let registers = stack.values.as_mut_ptr().add(window);
        fill_window(registers, count, shape.initial);
        let mut offset = 0;
        while offset < passed {
            registers
                .add(usize::from(shape.parameters + offset))
                .write(r.get(arguments + offset));
            offset += 1;
        }
        stack.values.set_len(window + count + lane_values::CONTEXT);
        // A Wasm activation runs with its caller's receiver and environment (`ActivationState::of`).
        Registers(registers).set_this(r.this());
        Registers(registers).set_env(r.env());
        Registers(registers).set_capture(r.capture());
        Registers(registers)
    }
}

/// A lane call without room for an activation, or to a function whose lane
/// view is not yet derived: the activations
/// become frames and the call pushes one, as the general loop would; the
/// emptied stack then grows so that later calls fit. Out of line so the
/// activation path saves no registers.
#[cold]
#[inline(never)]
fn lane_call_frame<H: Host>(ip: Ip) -> Resumed {
    let cx = lane_context::<H>();
    // SAFETY: called only while a lane runs, from its call handler.
    let i = unsafe { &*ip };
    let vm = unsafe { vm(cx) };
    let code = unsafe { &*(*cx).code };
    let callee = u32::from(i.b);
    let count = usize::from(code.function(usize::from(i.b)).registers);
    let window = fill_room::<ACTIVATION_FILL_BLOCK>(count);
    let stack = &vm.lane_stack;
    let full = stack.full(window);
    vm.materialize_lane_activations(cx);
    vm.lane_stack.grow(full, window);
    let caller = unsafe { (*cx).frame() };
    // SAFETY: `ip` is a record of the active view.
    let call_pc = unsafe { ip.offset_from((*cx).base) } as usize;
    let window = crate::bytecode::RegisterWindow {
        base: ImmediateLayout::call_window_base(i.imm()),
        count: ImmediateLayout::argument_count(i.imm()),
    };
    let Ok(call_guard) = vm.push_wasm_frame(code, caller, callee, window) else {
        return Resumed::Exit(exit(ip));
    };
    vm.frames[caller].pc = call_pc + 1;
    vm.frames[caller].pc_is_resume = true;
    unsafe { &mut *(*cx).pending }.push(PendingGeneralCall {
        caller,
        caller_cursor: None,
        call_pc: call_pc as u32,
        destination: i.a,
        completion: CallCompletion::Value,
        returns: false,
        call_guard: Some(call_guard),
        caller_program: None,
    });
    // The pushed frame's guard narrows the activations the thread admits.
    vm.lane_stack.refresh_activation_limit();
    lane_enter_frame(cx, caller + 1, 0)
}

/// The arguments' scalar kind of a JavaScript import's plan (`JS_IMPORT_I32`, `_F32`, `_F64`) as
/// the Wasm type it converts from.
#[inline(always)]
fn import_kind(kind: u8) -> crate::WasmType {
    match kind {
        JS_IMPORT_I32 => crate::WasmType::I32,
        JS_IMPORT_F32 => crate::WasmType::F32,
        _ => crate::WasmType::F64,
    }
}

/// The JavaScript arguments of an activated import: each window word converted by its kind
/// (`staged::wasm_activation::import_argument`). `None` where a word is of another
/// representation, which leaves for the general path.
#[inline(always)]
fn import_arguments<H: Host>(
    vm: &Vm<H>,
    window: &[Value],
    kinds: &[u8; JS_IMPORT_ARGUMENTS],
) -> Option<[Value; JS_IMPORT_ARGUMENTS]> {
    let mut args = [Value::UNDEFINED; JS_IMPORT_ARGUMENTS];
    for ((slot, &value), &kind) in args.iter_mut().zip(window).zip(kinds) {
        let ty = import_kind(kind);
        let word = match ty {
            crate::WasmType::I32 | crate::WasmType::F32 => u64::from(value.as_int()? as u32),
            _ => vm.decode_bits64(value)?,
        };
        *slot = Value::number(activation::import_argument(ty, word));
    }
    Some(args)
}

/// The memory binding a JavaScript import's return reads again: `Some(None)` for no memory, the
/// binding of the run's resolved view, or `None` for a view of another binding, which leaves.
#[inline(always)]
fn import_memory<H: Host>(cx: *mut LaneContext<H>, memory: MemoryView) -> Option<Option<Value>> {
    let (binding, resolved) = unsafe { (*cx).resolved };
    if memory.bytes.is_null() {
        Some(None)
    } else if memory.bytes == resolved.bytes {
        Some(Some(binding))
    } else {
        None
    }
}

/// The cross-program target an import's activation enters, where the lane may activate it with
/// `argc` arguments: a JavaScript callee of another program that activates across.
#[inline(always)]
fn import_activation_target<H: Host>(
    cx: *mut LaneContext<H>,
    vm: &mut Vm<H>,
    callee: Value,
    argc: u16,
) -> Option<ScriptCallTarget> {
    if lane_constructing(vm, callee) || vm.with_scope_entered {
        return None;
    }
    let target = match vm.cross_call_entry(callee) {
        // SAFETY: as `Vm::script_call_entry`; the entry is copied before anything resolves.
        Some(entry) => unsafe { *entry },
        None => vm.resolve_script_call_target(unsafe { &*(*cx).code }, callee)?,
    };
    if target.program() == unsafe { (*cx).program }
        || !target.entry.activates_across()
        || target.forwards()
        || !(argc <= target.params || target.drops_extra_arguments())
        || !lane_activation_room(vm, &target)
    {
        return None;
    }
    Some(target)
}

/// The activation of a JavaScript import's callee, whose return gives the Wasm caller the result
/// (`Completion::ToWasm`): the callee's frame takes the converted arguments, its caller's
/// instance, memory and result type are kept for the return.
#[inline(always)]
fn push_import_activation<H: Host>(
    cx: *mut LaneContext<H>,
    r: Registers,
    ip: Ip,
    target: &ScriptCallTarget,
    args: &[Value; JS_IMPORT_ARGUMENTS],
    argc: u16,
    this: Value,
    memory: Option<Value>,
    result: Option<crate::WasmType>,
) -> Resumed {
    let instance = unsafe { (*cx).instance };
    let (registers, start) =
        lane_push_passed::<H, true, false>(cx, r, ip, target, args.as_ptr(), argc.min(target.params), this, false);
    let vm = unsafe { vm(cx) };
    if let Some(Activation { kind: ActivationKind::Script(script), .. }) = vm.lane_stack.activations.last_mut() {
        script.completion = Completion::ToWasm;
    }
    vm.lane_stack.to_wasm.push(ToWasmCall { instance, memory, result });
    unsafe {
        (*cx).inline_calls = false;
        (*cx).script_calls = true;
        (*cx).instance = Value::DELETED;
    }
    let view = Carried::script(cx);
    unsafe { (*cx).memory = view.memory };
    Resumed::At(registers, start, view)
}

/// A Wasm reference call of a JavaScript-backed import whose callee the lane activates as a
/// JavaScript call of another program would (`lane_script_call_cross`): the scalar arguments
/// in their JavaScript form (ToJSValue, which allocates nothing for i32, f32 and f64), an
/// `undefined` receiver, and a return that gives the Wasm caller the result's Wasm form
/// (`Completion::ToWasm`). The import's wrapper is of the running realm and its native
/// boundary is never observed: the callee runs as the wrapper's call of it would. `None`,
/// which changes nothing, for any other import, callee or call.
#[inline(always)]
fn lane_activate_js_import<H: Host>(cx: *mut LaneContext<H>, r: Registers, ip: Ip, memory: MemoryView) -> Option<Resumed> {
    // SAFETY: called only from the call handler of the record at `ip`, while the lane runs.
    let i = unsafe { &*ip };
    let vm = unsafe { vm(cx) };
    // A tier that may run the callee enters it natively from the general call, which a
    // lane activation would bypass.
    if i.a & crate::bytecode::RETURN_REGISTER != 0 || vm.tier_may_run(crate::jit_mode::Tier::ANY) {
        return None;
    }
    let plan = js_import_of(vm, r.get(i.b))?;
    if plan.realm != vm.realm.globals {
        return None;
    }
    // SAFETY: the window is the caller's registers.
    let window = unsafe { lane_call_window(r, i) };
    if window.len() != usize::from(plan.argc) {
        return None;
    }
    let args = import_arguments(vm, window, &plan.kinds)?;
    let memory_binding = import_memory(cx, memory)?;
    let argc = window.len() as u16;
    let target = import_activation_target(cx, vm, plan.callee, argc)?;
    let this = lane_call_this(vm, &target, Value::UNDEFINED)?;
    Some(push_import_activation(cx, r, ip, &target, &args, argc, this, memory_binding, plan.result))
}

/// The call facts of the import `wrapper`, from the lane's cache of its last import, else
/// derived (`js_import_plan`) and cached.
#[inline(always)]
fn js_import_of<H: Host>(vm: &mut Vm<H>, wrapper: Value) -> Option<JsImportPlan> {
    match vm.lane_stack.js_import {
        Some(plan) if plan.wrapper == wrapper => Some(plan),
        _ => {
            let plan = js_import_plan(vm, wrapper)?;
            vm.lane_stack.js_import = Some(plan);
            Some(plan)
        }
    }
}

/// The JavaScript callee of a host-or-JavaScript import, its memcpy capture where it has one, and
/// its signature. `None` for a host-backed import or a callee that is no import.
fn native_import_target<H: Host>(
    vm: &Vm<H>,
    callee: Value,
) -> Option<(Value, Option<(Value, usize)>, crate::WasmSignature)> {
    let env = vm.heap.function(callee)?.env;
    match vm.heap.get(env) {
        Some(Cell::WasmHostFunction { target, signature }) => match target {
            crate::heap::WasmHostTarget::Js(target) => Some((*target, None, signature.clone())),
            crate::heap::WasmHostTarget::JsEmscriptenMemcpy(target) => Some((
                target.function,
                Some((target.view_environment, target.view_slot)),
                signature.clone(),
            )),
            crate::heap::WasmHostTarget::Host(_) => None,
        },
        _ => None,
    }
}

/// The call facts of the import `wrapper` (`JsImportPlan`): `None` for any other callee, a
/// host-backed or memcpy import, or a signature past the scalars the activation converts.
#[inline(never)]
fn js_import_plan<H: Host>(vm: &Vm<H>, wrapper: Value) -> Option<JsImportPlan> {
    let record = vm.heap.function(wrapper)?;
    let Some(Cell::WasmHostFunction { target: crate::heap::WasmHostTarget::Js(callee), signature }) = vm.heap.get(record.env) else {
        return None;
    };
    let result = match signature.results.as_slice() {
        [] => None,
        [ty @ (crate::WasmType::I32 | crate::WasmType::F32 | crate::WasmType::F64)] => Some(*ty),
        _ => return None,
    };
    if signature.params.len() > JS_IMPORT_ARGUMENTS {
        return None;
    }
    let mut kinds = [JS_IMPORT_I32; JS_IMPORT_ARGUMENTS];
    for (kind, ty) in kinds.iter_mut().zip(&signature.params) {
        *kind = match ty {
            crate::WasmType::I32 => JS_IMPORT_I32,
            crate::WasmType::F32 => JS_IMPORT_F32,
            crate::WasmType::F64 => JS_IMPORT_F64,
            _ => return None,
        };
    }
    Some(JsImportPlan { wrapper, realm: record.realm, callee: *callee, argc: signature.params.len() as u8, kinds, result })
}
