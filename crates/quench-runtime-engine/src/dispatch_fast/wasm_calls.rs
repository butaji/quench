//! Wasm calls the lane enters as activations: statically known calls and reference calls. They run
//! the one call step JavaScript calls run (`staged::wasm_invoke`); this file is the lane's state the
//! step's Wasm frame runs on (`LaneCall`) and the lane's own steps.
#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;
use quench_runtime::staged::machine::{Machine, MachineStage};
use quench_runtime::staged::wasm_activation::{self as activation, offer_entry, CallTarget, EntryOffer, NativeCall, WasmActivation, WasmNativeEntry};
use quench_runtime::vm::program_store::HandoffTiers;
use super::lane_stack_growth::LaneStackFull;
use quench_runtime::vm::JsError;

/// The known call's handler, for the shared handler table.
pub(crate) const fn known_call_handler() -> Handler {
    lane_call_known
}

/// The reference call's handler, for Wasm views.
pub(crate) const fn reference_call_handler() -> Handler {
    lane_call_wasm_reference
}

/// A Wasm call as the lane makes it: the context, the caller's registers and record, and its
/// memory view. The rules of `staged::wasm_activation` decide the call over these primitives.
struct LaneCall<'a> {
    cx: *mut LaneContext,
    r: Registers,
    ip: Ip,
    i: &'a LaneInstruction,
    memory: MemoryView,
    /// The code the tiers offered for the callee (`WasmNativeEntry::offer`).
    offered: Option<std::rc::Rc<quench_runtime::vm::engine_types::CompiledProgram>>,
}

impl LaneCall<'_> {
    /// The callee's function index from the row of the indirect target the lane made just before
    /// this call (`IndirectSlot`), where this call is that target's next record and reads its
    /// register and reference unchanged, and the row names a function of the running instance (one
    /// instance record per environment and program, so its address decides). Otherwise `None`, and
    /// the reference is resolved from the heap. The slot is consumed either way.
    #[inline(always)]
    fn indirect_callee(&mut self) -> Option<u32> {
        // SAFETY: the context outlives the lane run.
        let slot = unsafe { (*self.cx).indirect };
        unsafe { (*self.cx).indirect.next = std::ptr::null() };
        if slot.next != self.ip || slot.register != self.i.b || slot.reference != self.r.get(self.i.b).to_bits() {
            return None;
        }
        let running = running_instance_address(self.cx);
        if running == 0 || slot.instance != running as u64 {
            return None;
        }
        u16::try_from(slot.entry).ok().map(u32::from)
    }
}

impl Machine for LaneCall<'_> {
    type Word = u64;
}

quench_runtime::projection!(LaneCall<'_> => Concrete);

impl MachineStage for LaneCall<'_> {
    type Exit = JsError;

    quench_runtime::scalar_operators! {}

    fn trap_when(&mut self, flag: u64, trap: quench_runtime::WasmTrap) -> Result<(), JsError> {
        if flag == 0 { Ok(()) } else { Err(JsError::wasm_trap_error(trap)) }
    }
}

impl WasmNativeEntry for LaneCall<'_> {
    type Resume = Resumed;
    #[inline(always)]
    fn native_mode(&mut self) -> bool {
        unsafe { vm(self.cx) }.execution_mode == super::ExecutionMode::Compiled && self.i.a & quench_runtime::bytecode::RETURN_REGISTER == 0
    }

    /// The lane makes no compile: one an entry makes due leaves for the general loop (`HandoffTiers`).
    #[inline(always)]
    fn offer(&mut self, function: u32) -> EntryOffer<()> {
        let (vm, program) = (unsafe { vm(self.cx) }, unsafe { (*self.cx).program });
        offer_entry(&mut HandoffTiers { store: &mut vm.programs, program }, function).keep(&mut self.offered)
    }

    #[inline(always)]
    fn enter_compiled(&mut self, function: u32) -> NativeCall {
        // SAFETY: the window is the caller's registers, read into raw words before native code
        // runs, so before guest code could make the activations frames.
        let args = unsafe { call_window_in_lane(self.r, self.i) };
        let (program, env) = (unsafe { (*self.cx).program }, instance_env_in_lane(self.cx));
        let offered = self.offered.take().expect("a native entry runs the code its tiers offered");
        let called = in_place_in_lane(self.cx, self.ip, |vm, _| {
            vm.run_offered_values(offered, program, env, function, args).transpose()
        });
        match called {
            None => NativeCall::Left,
            Some(None) => NativeCall::Declined,
            Some(Some(value)) => NativeCall::Ran(value.to_bits()),
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
        let view = Carried { memory, integers: quench_runtime::value::IntegerEncoding::TAG, acc: Slot::of(Value::UNDEFINED) };
        Resumed::At(r, following(self.ip), view)
    }
}

impl WasmActivation for LaneCall<'_> {
    type Shape = (ActivationShape, Ip);
    type Full = LaneStackFull;
    type Guard = quench_runtime::stack::GuestCallGuard;

    #[inline(always)]
    fn caller_registers(&mut self) -> u64 { self.r.0.expose_provenance() as u64 }

    #[inline(always)]
    fn reference(&mut self) -> CallTarget {
        if let Some(function) = self.indirect_callee() {
            return CallTarget::Function(function);
        }
        let vm = unsafe { vm(self.cx) };
        let running = (unsafe { (*self.cx).program }, Some(instance_env_in_lane(self.cx)));
        match vm.function_record(self.r.get(self.i.b)) {
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
        vm.lane_stack.activation_count() < vm.lane_stack.activation_limit.get()
    }

    #[inline(always)]
    fn frame_room(&mut self) -> bool {
        let vm = unsafe { vm(self.cx) };
        vm.frames.len() + vm.lane_stack.activation_count() < super::dispatch_frame::WASM_ACTIVATION_LIMIT
    }

    #[inline(always)]
    fn storage_room(&mut self, registers: usize) -> bool {
        // Depth and room are per call: each activation the lane pushes or pops moves them, so
        // they are not facts a link proves once.
        let vm = unsafe { vm(self.cx) };
        registers.saturating_mul(std::mem::size_of::<Value>())
            <= quench_runtime::stack::GuestCallGuard::remaining_storage_bytes()
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
        // A call of this program's own functions (`lane_call_known`, `lane_call_wasm_reference`).
        let function_program = unsafe { (*self.cx).program };
        let callee = fill_callee_window(vm, self.r, self.i, shape);
        // SAFETY: capacity holds the activation; the window's start has not moved.
        unsafe {
            let stack = &mut vm.lane_stack;
            let at = stack.values.as_mut_ptr().wrapping_add(window);
            stack.push_activation(
                Activation::of_parts(
                    function,
                    self.ip,
                    (*self.cx).base,
                    self.r,
                    (*self.cx).program,
                    ActivationKind::Wasm { memory: self.memory, frame_storage_bytes, program: function_program, pc: 0 },
                ),
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
            integers: quench_runtime::value::IntegerEncoding::TAG,
            acc: Slot::of(Value::UNDEFINED),
        };
        Resumed::At(callee, base, view)
    }

    #[inline(always)]
    fn frame_window_room(&mut self) -> usize {
        // SAFETY: the context's code outlives the lane run.
        let code = unsafe { &*(*self.cx).code };
        let count = usize::from(code.function(usize::from(self.i.b)).registers);
        fill_room::<ACTIVATION_FILL_BLOCK>(count)
    }

    #[inline(always)]
    fn stack_full(&mut self, room: usize) -> LaneStackFull {
        let vm = unsafe { vm(self.cx) };
        vm.lane_stack.full(room)
    }

    fn materialize_activations(&mut self) {
        let vm = unsafe { vm(self.cx) };
        vm.materialize_lane_activations(self.cx);
    }

    fn grow_stack(&mut self, full: LaneStackFull, room: usize) {
        let vm = unsafe { vm(self.cx) };
        vm.lane_stack.grow(full, room);
    }

    #[inline(always)]
    fn caller_frame(&mut self) -> usize {
        unsafe { (*self.cx).frame() }
    }

    fn push_callee_frame(&mut self, caller: usize) -> Option<quench_runtime::stack::GuestCallGuard> {
        let vm = unsafe { vm(self.cx) };
        let code = unsafe { &*(*self.cx).code };
        let window = quench_runtime::bytecode::RegisterWindow {
            base: ImmediateLayout::call_window_base(self.i.imm()),
            count: ImmediateLayout::argument_count(self.i.imm()),
        };
        vm.push_wasm_frame(code, caller, u32::from(self.i.b), window).ok()
    }

    fn push_pending_call(&mut self, caller: usize, guard: quench_runtime::stack::GuestCallGuard) {
        // SAFETY: `ip` is a record of the active view.
        let call_pc = unsafe { self.ip.offset_from((*self.cx).base) } as usize;
        let vm = unsafe { vm(self.cx) };
        vm.frames[caller].pc = call_pc + 1;
        vm.frames[caller].pc_is_resume = true;
        unsafe { &mut *(*self.cx).pending }.push(PendingGeneralCall {
            caller,
            caller_cursor: None,
            call_pc: call_pc as u32,
            destination: self.i.a,
            completion: CallCompletion::Value,
            returns: false,
            call_guard: Some(guard),
            caller_program: None,
        });
    }

    fn refresh_activation_limit(&mut self) {
        let vm = unsafe { vm(self.cx) };
        vm.lane_stack.refresh_activation_limit();
    }

    fn enter_frame(&mut self, frame: usize) -> Resumed {
        enter_frame_in_lane(self.cx, frame, 0)
    }

    /// A Wasm reference call of a JavaScript-backed import whose callee the lane activates as a
    /// JavaScript call of another program would (`lane_script_call_cross`): the scalar arguments
    /// in their JavaScript form (ToJSValue, which allocates nothing for i32, f32 and f64), an
    /// `undefined` receiver, and a return that gives the Wasm caller the result's Wasm form
    /// (`Completion::ToWasm`). The import's wrapper is of the running realm and its native
    /// boundary is never observed: the callee runs as the wrapper's call of it would. `None`,
    /// which changes nothing, for any other import, callee or call.
    #[inline(always)]
    fn activate_import(&mut self) -> Option<Resumed> {
        let (cx, r, ip, memory) = (self.cx, self.r, self.ip, self.memory);
        // SAFETY: called only from the call handler of the record at `ip`, while the lane runs.
        let i = unsafe { &*ip };
        let vm = unsafe { vm(cx) };
        // A tier that may run the callee enters it natively from the general call, which a
        // lane activation would bypass.
        if i.a & quench_runtime::bytecode::RETURN_REGISTER != 0 || vm.tier_may_run(crate::compiled::admission::Tier::ANY) {
            return None;
        }
        let plan = js_import_of(vm, r.get(i.b))?;
        if plan.realm != vm.realm.globals {
            return None;
        }
        // SAFETY: the window is the caller's registers.
        let window = unsafe { call_window_in_lane(r, i) };
        if window.len() != usize::from(plan.argc) {
            return None;
        }
        let args = import_arguments(vm, window, &plan.kinds)?;
        let memory_binding = import_memory(cx, memory)?;
        let argc = window.len() as u16;
        let target = import_activation_target(cx, vm, plan.callee, argc)?;
        let this = call_this_in_lane(vm, &target, Value::UNDEFINED)?;
        Some(push_import_activation(cx, r, ip, &target, &args, argc, this, memory_binding, plan.result))
    }

    #[inline(always)]
    fn import_native(&mut self) -> NativeCall {
        let vm = unsafe { vm(self.cx) };
        let callee = self.r.get(self.i.b);
        let Some((target, capture, signature)) = vm.host_import_js_target(callee) else {
            return NativeCall::Declined;
        };
        // SAFETY: the window is the caller's registers, copied before guest code runs.
        let args: smallvec::SmallVec<[Value; 8]> =
            unsafe { call_window_in_lane(self.r, self.i) }.iter().copied().collect();
        if args.len() != signature.params.len() {
            return NativeCall::Declined;
        }
        let program = unsafe { (*self.cx).program };
        let called = in_place_in_lane(self.cx, self.ip, |vm, code| {
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
}

lane_handler! {
    /// A Wasm call to a function of the same program, through the one call step.
    fn lane_call_known<>(cx, r, ip, i, view) {
        let mut call = LaneCall { cx, r, ip, i, memory: view.memory, offered: None };
        match quench_runtime::staged::wasm_invoke::step_call_known_of(&mut call, i.b, (ImmediateLayout::call_window_base(i.imm()), ImmediateLayout::argument_count(i.imm())), i.a) {
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
        let mut call = LaneCall { cx, r, ip, i, memory: view.memory, offered: None };
        match quench_runtime::staged::wasm_invoke::step_call_reference_of(&mut call, (ImmediateLayout::call_window_base(i.imm()), ImmediateLayout::argument_count(i.imm())), i.a) {
            Some(resumed) => resume!(cx, resumed),
            None => return exit(ip),
        }
    }
}

/// The address of the running instance's record (`Vm::wasm_instance_address`), cached on the lane
/// for the environment (its bits) and program it was found for. A record is one per environment
/// and program, so the address names the running instance.
#[inline(always)]
fn running_instance_address(cx: *mut LaneContext) -> usize {
    let env = instance_env_in_lane(cx);
    // SAFETY: the context outlives the lane run.
    let program = unsafe { (*cx).program };
    let (bits, cached, address) = unsafe { (*cx).instance_address };
    if bits == env.to_bits() && cached == program {
        return address;
    }
    let address = unsafe { vm(cx) }.wasm_instance_address(env, program).unwrap_or(0);
    unsafe { (*cx).instance_address = (env.to_bits(), program, address) };
    address
}

/// The callee's frame at the top of the value stack: its registers filled with the shape's
/// initial value, its parameters from the call's arguments, and its `this`, environment and
/// capture from the caller's. Returns the frame's registers.
#[inline(always)]
fn fill_callee_window(vm: &mut Vm, r: Registers, i: &LaneInstruction, shape: ActivationShape) -> Registers {
    let stack = &mut vm.lane_stack;
    let window = stack.values.len();
    let count = usize::from(shape.registers);
    let arguments = ImmediateLayout::call_window_base(i.imm());
    let passed = ImmediateLayout::argument_count(i.imm()).min(shape.params);
    // SAFETY: capacity holds the window; each slot is written before the length covers it, and
    // no activation's window moves.
    unsafe {
        let registers = stack.values.as_mut_ptr().add(window);
        fill_window(registers, usize::from(shape.fill), shape.initial);
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

/// The arguments' scalar kind of a JavaScript import's plan (`JS_IMPORT_I32`, `_F32`, `_F64`) as
/// the Wasm type it converts from.
#[inline(always)]
fn import_kind(kind: u8) -> quench_runtime::WasmType {
    match kind {
        JS_IMPORT_I32 => quench_runtime::WasmType::I32,
        JS_IMPORT_F32 => quench_runtime::WasmType::F32,
        _ => quench_runtime::WasmType::F64,
    }
}

/// The JavaScript arguments of an activated import: each window word converted by its kind
/// (`staged::wasm_activation::import_argument`). `None` where a word is of another
/// representation, which leaves for the general path.
#[inline(always)]
fn import_arguments(
    vm: &Vm,
    window: &[Value],
    kinds: &[u8; JS_IMPORT_ARGUMENTS],
) -> Option<[Value; JS_IMPORT_ARGUMENTS]> {
    let mut args = [Value::UNDEFINED; JS_IMPORT_ARGUMENTS];
    for ((slot, &value), &kind) in args.iter_mut().zip(window).zip(kinds) {
        let ty = import_kind(kind);
        let word = match ty {
            quench_runtime::WasmType::I32 | quench_runtime::WasmType::F32 => u64::from(value.as_int()? as u32),
            _ => vm.decode_bits64(value)?,
        };
        *slot = Value::number(activation::import_argument(ty, word));
    }
    Some(args)
}

/// The memory binding a JavaScript import's return reads again: `Some(None)` for no memory, the
/// binding of the run's resolved view, or `None` for a view of another binding, which leaves.
#[inline(always)]
fn import_memory(cx: *mut LaneContext, memory: MemoryView) -> Option<Option<Value>> {
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
fn import_activation_target(
    cx: *mut LaneContext,
    vm: &mut Vm,
    callee: Value,
    argc: u16,
) -> Option<ScriptCallTarget> {
    if constructing_in_lane(vm, callee) || vm.with_scope_entered {
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
        || !activation_room_in_lane(vm, &target)
    {
        return None;
    }
    Some(target)
}

/// The activation of a JavaScript import's callee, whose return gives the Wasm caller the result
/// (`Completion::ToWasm`): the callee's frame takes the converted arguments, its caller's
/// instance, memory and result type are kept for the return.
#[inline(always)]
fn push_import_activation(
    cx: *mut LaneContext,
    r: Registers,
    ip: Ip,
    target: &ScriptCallTarget,
    args: &[Value; JS_IMPORT_ARGUMENTS],
    argc: u16,
    this: Value,
    memory: Option<Value>,
    result: Option<quench_runtime::WasmType>,
) -> Resumed {
    let instance = unsafe { (*cx).instance };
    let (registers, start) =
        push_passed_in_lane::<true, false>(cx, r, ip, target, args.as_ptr(), argc.min(target.params), this, false);
    let vm = unsafe { vm(cx) };
    vm.lane_stack.recomplete_top(Completion::ToWasm);
    vm.lane_stack.attach_payload(SidePayload::ToWasm(ToWasmCall { instance, memory, result }));
    unsafe {
        (*cx).inline_calls = false;
        (*cx).script_calls = true;
        (*cx).instance = Value::DELETED;
    }
    let view = Carried::script(cx);
    unsafe { (*cx).memory = view.memory };
    Resumed::At(registers, start, view)
}

/// The call facts of the import `wrapper`, from the lane's cache of its last import, else
/// derived (`js_import_plan`) and cached.
#[inline(always)]
fn js_import_of(vm: &mut Vm, wrapper: Value) -> Option<JsImportPlan> {
    match vm.lane_stack.js_import {
        Some(plan) if plan.wrapper == wrapper => Some(plan),
        _ => {
            let plan = js_import_plan(vm, wrapper)?;
            vm.lane_stack.js_import = Some(plan);
            Some(plan)
        }
    }
}

/// The call facts of the import `wrapper` (`JsImportPlan`): `None` for any other callee, a
/// host-backed or memcpy import, or a signature past the scalars the activation converts.
#[inline(never)]
fn js_import_plan(vm: &Vm, wrapper: Value) -> Option<JsImportPlan> {
    let record = vm.function_record(wrapper)?;
    let (callee, capture, signature) = vm.host_import_js_target(wrapper)?;
    if capture.is_some() {
        return None;
    }
    let result = match signature.results.as_slice() {
        [] => None,
        [ty @ (quench_runtime::WasmType::I32 | quench_runtime::WasmType::F32 | quench_runtime::WasmType::F64)] => Some(*ty),
        _ => return None,
    };
    if signature.params.len() > JS_IMPORT_ARGUMENTS {
        return None;
    }
    let mut kinds = [JS_IMPORT_I32; JS_IMPORT_ARGUMENTS];
    for (kind, ty) in kinds.iter_mut().zip(&signature.params) {
        *kind = match ty {
            quench_runtime::WasmType::I32 => JS_IMPORT_I32,
            quench_runtime::WasmType::F32 => JS_IMPORT_F32,
            quench_runtime::WasmType::F64 => JS_IMPORT_F64,
            _ => return None,
        };
    }
    Some(JsImportPlan { wrapper, realm: vm.record_realm(record), callee, argc: signature.params.len() as u8, kinds, result })
}
