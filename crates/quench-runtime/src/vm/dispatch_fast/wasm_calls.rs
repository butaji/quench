//! Wasm calls the lane enters as activations: statically known calls and reference calls.
use super::*;

/// The known call's handler, for the shared handler table.
pub(super) const fn known_call_handler<H: Host>() -> Handler {
    lane_call_known::<H>
}

/// The reference call's handler, for Wasm views.
pub(super) const fn reference_call_handler<H: Host>() -> Handler {
    lane_call_wasm_reference::<H>
}

/// Enters `$function` of the running program and instance as a Wasm call: out of the lane when
/// native code may run it, else as an activation pushed as the general loop's inline call would
/// be; `$no_room` takes the call when the lane has no room. Shared by the known and reference calls.
macro_rules! enter_wasm_function {
    ($cx:ident, $r:ident, $ip:ident, $i:ident, $view:ident, $function:expr, $no_room:expr) => {{
        let (cx, r, ip, i, view) = ($cx, $r, $ip, $i, $view);
        if !unsafe { (*cx).inline_calls } {
            return exit(ip);
        }
        let vm = unsafe { vm(cx) };
        // Keep eligible calls on the general dispatcher so they can enter
        // native code. Unsupported calls stay on the fast interpreter lane.
        if vm.execution_mode == super::ExecutionMode::Compiled
            && i.a & crate::bytecode::RETURN_REGISTER == 0
        {
            let program = unsafe { (*cx).program };
            let function = usize::from($function);
            if vm
                .programs
                .has_compiled_function_or_note_fallback(program, function)
                || vm.programs.has_wasm_tier_containing(program, function)
            {
                if let Some(resumed) = lane_call_compiled::<H>(cx, r, ip, u32::from($function), view.memory) {
                    resume!(cx, resumed)
                }
            }
            // `run_compiled_inner` is the one-shot promotion claimant. Count
            // lane-only entries here, but leave the threshold entry for that
            // general-path hook so no handoff token or duplicate count is
            // needed. Terminal calls stay on their tail-call path above the
            // tier gate and are deliberately not observed here.
            if vm
                .programs
                .note_wasm_tier_lane_entry(program, u32::from($function))
            {
                return exit(ip);
            }
        }
        // SAFETY: the program's views outlive the lane run.
        let views = unsafe { &*(*cx).views };
        let stack: &mut LaneStack = &mut vm.lane_stack;
        if let Some(lane_view) = views.get(usize::from($function)).and_then(super::program_store::derived_lane_view)
            && let Some(shape) = lane_view.activation
            && usize::from(shape.registers)
                .saturating_mul(std::mem::size_of::<Value>())
                <= crate::stack::GuestCallGuard::remaining_storage_bytes()
                    .saturating_sub(stack.wasm_storage_bytes)
            && stack.activation_count() < stack.activation_limit.get()
            && vm.frames.len() + stack.activation_count()
                < super::dispatch_frame::WASM_ACTIVATION_LIMIT
            && stack.values.room()
                >= fill_room::<ACTIVATION_FILL_BLOCK>(usize::from(shape.registers))
        {
            let window = stack.values.len();
            let count = usize::from(shape.registers);
            let frame_storage_bytes = count.saturating_mul(std::mem::size_of::<Value>());
            let arguments = ImmediateLayout::call_window_base(i.imm());
            let passed = ImmediateLayout::argument_count(i.imm()).min(shape.params);
            // SAFETY: capacity holds the window; each slot is written before
            // the length covers it, and no activation's window moves.
            let callee_registers = unsafe {
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
            };
            // SAFETY: capacity holds the activation.
            unsafe {
                let at = stack.values.as_mut_ptr().wrapping_add(window);
                stack.push_activation(Activation::of_parts(
                    crate::vm::compiled::ShadowFrame::EMPTY, u32::from($function),
                    ip,
                    (*cx).base,
                    r,
                    (*cx).program,
                    ActivationKind::Wasm { memory: view.memory, frame_storage_bytes },
                ), at);
            }
            // A fresh activation's memory register holds no memory until
            // the function's instance binding loads it.
            let base = lane_view.records();
            unsafe {
                (*cx).registers = callee_registers;
                (*cx).base = base;
                (*cx).memory = MemoryView::NONE;
            }
            let view = Carried {
                memory: MemoryView::NONE,
                integers: crate::value::IntegerEncoding::TAG,
                acc: Slot::of(Value::UNDEFINED),
            };
            next!(cx, callee_registers, base, view)
        }
        $no_room
    }};
}

lane_handler! {
    /// A Wasm call to a function of the same program, pushed as the general
    /// loop's inline call would be.
    fn lane_call_known<>(cx, r, ip, i, view) {
        enter_wasm_function!(cx, r, ip, i, view, i.b, resume!(cx, lane_call_frame::<H>(ip)))
    }
}

lane_handler! {
    /// A Wasm reference call (`call_ref`, or `call_indirect` past its target check) of a
    /// function of the running program and instance, entered as `lane_call_known` enters its
    /// callee. Validation proved the callee's type. Any other callee leaves for the general loop.
    fn lane_call_wasm_reference<>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        let running = (unsafe { (*cx).program }, Some(lane_instance_env(cx)));
        let callee = match vm.heap.function(r.get(i.b)) {
            Some(FunctionRecord { kind: FunctionKind::User(program, function), env, .. })
                if (*program, Some(*env)) == running => u16::try_from(*function).ok(),
            Some(FunctionRecord { kind: FunctionKind::Native(Native::WasmHost), .. }) => {
                if let Some(resumed) = lane_call_js_import::<H>(cx, r, ip, view.memory) {
                    resume!(cx, resumed)
                }
                None
            }
            _ => None,
        };
        let Some(function) = callee else { return exit(ip) };
        enter_wasm_function!(cx, r, ip, i, view, function, exit(ip))
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

/// A Wasm call of `function`, which native code holds, made from the lane beside its
/// activations (`lane_in_place`), so the callers stay activations unless guest code the
/// callee reaches makes them frames. The caller resumes after the call as a returning
/// activation's does (`lane_resume_after_call`), its memory view read again: the callee may
/// have grown the memory. `None` when native code declines the call, which then enters as
/// an activation.
#[cold]
#[inline(never)]
fn lane_call_compiled<H: Host>(cx: *mut LaneContext<H>, r: Registers, ip: Ip, function: u32, memory: MemoryView) -> Option<Resumed> {
    // SAFETY: called only from the call handler of the record at `ip`, while the lane runs.
    let i = unsafe { &*ip };
    // SAFETY: the window is the caller's registers, read into raw words before native code
    // runs, so before guest code could make the activations frames.
    let args = unsafe { lane_call_window(r, i) };
    let vm = unsafe { vm(cx) };
    let (program, env) = (unsafe { (*cx).program }, lane_instance_env(cx));
    let Some(called) = lane_in_place(cx, ip, |vm, _| vm.run_compiled(program, env, function, args).transpose()) else {
        return Some(Resumed::Exit(exit(ip)));
    };
    let value = called?;
    let (binding, resolved) = unsafe { (*cx).resolved };
    let memory = if memory.bytes == resolved.bytes {
        let fresh = memory_view(vm, binding);
        unsafe { (*cx).resolved = (binding, fresh) };
        fresh
    } else {
        MemoryView::NONE
    };
    // SAFETY: as above; guest code the callee ran may have made the activations frames,
    // whose registers the context then names.
    let r = unsafe {
        (*cx).memory = memory;
        (*cx).registers
    };
    r.set(i.a, value);
    let view = Carried { memory, integers: crate::value::IntegerEncoding::TAG, acc: Slot::of(Value::UNDEFINED) };
    Some(Resumed::At(r, following(ip), view))
}

/// A Wasm reference call of a JavaScript-backed import, made from the lane beside its
/// activations (`lane_in_place`) through the import's shared conversion path
/// (`Vm::call_compiled_js_wasm_import`), so the Wasm callers stay activations unless the
/// JavaScript callee makes them frames. The caller resumes as `lane_call_compiled`'s does.
/// `None` for a host-backed import, which leaves for the general loop.
#[cold]
#[inline(never)]
fn lane_call_js_import<H: Host>(cx: *mut LaneContext<H>, r: Registers, ip: Ip, memory: MemoryView) -> Option<Resumed> {
    if let Some(resumed) = lane_activate_js_import(cx, r, ip, memory) {
        return Some(resumed);
    }
    // SAFETY: called only from the call handler of the record at `ip`, while the lane runs.
    let i = unsafe { &*ip };
    let vm = unsafe { vm(cx) };
    let callee = r.get(i.b);
    let env = vm.heap.function(callee)?.env;
    let (target, capture, signature) = match vm.heap.get(env) {
        Some(Cell::WasmHostFunction { target, signature }) => match target {
            crate::heap::WasmHostTarget::Js(target) => (*target, None, signature.clone()),
            crate::heap::WasmHostTarget::JsEmscriptenMemcpy(target) => {
                (target.function, Some((target.view_environment, target.view_slot)), signature.clone())
            }
            crate::heap::WasmHostTarget::Host(_) => return None,
        },
        _ => return None,
    };
    // SAFETY: the window is the caller's registers, copied before guest code runs.
    let args: smallvec::SmallVec<[Value; 8]> = unsafe { lane_call_window(r, i) }.iter().copied().collect();
    if args.len() != signature.params.len() {
        return None;
    }
    let program = unsafe { (*cx).program };
    let Some(called) = lane_in_place(cx, ip, |vm, code| {
        vm.call_compiled_js_wasm_import(
            program, code, callee, target, capture, &signature, Value::UNDEFINED, &args,
            Value::UNDEFINED, std::ptr::null_mut(),
        )
    }) else {
        return Some(Resumed::Exit(exit(ip)));
    };
    let (binding, resolved) = unsafe { (*cx).resolved };
    let memory = if memory.bytes == resolved.bytes {
        let fresh = memory_view(vm, binding);
        unsafe { (*cx).resolved = (binding, fresh) };
        fresh
    } else {
        MemoryView::NONE
    };
    // SAFETY: guest code the callee ran may have made the activations frames, whose
    // registers the context then names.
    let r = unsafe {
        (*cx).memory = memory;
        (*cx).registers
    };
    r.set(i.a, called);
    let view = Carried { memory, integers: crate::value::IntegerEncoding::TAG, acc: Slot::of(Value::UNDEFINED) };
    Some(Resumed::At(r, following(ip), view))
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
    let wrapper = r.get(i.b);
    let plan = match vm.lane_stack.js_import {
        Some(plan) if plan.wrapper == wrapper => plan,
        _ => {
            let plan = js_import_plan(vm, wrapper)?;
            vm.lane_stack.js_import = Some(plan);
            plan
        }
    };
    if plan.realm != vm.realm.globals {
        return None;
    }
    let (callee, result) = (plan.callee, plan.result);
    // SAFETY: the window is the caller's registers.
    let window = unsafe { lane_call_window(r, i) };
    if window.len() != usize::from(plan.argc) {
        return None;
    }
    let mut args = [Value::UNDEFINED; JS_IMPORT_ARGUMENTS];
    for ((slot, &value), &kind) in args.iter_mut().zip(window).zip(&plan.kinds) {
        *slot = match kind {
            JS_IMPORT_I32 => Value::number(f64::from(value.as_int()?)),
            JS_IMPORT_F32 => Value::number(f64::from(f32::from_bits(value.as_int()? as u32))),
            _ => Value::number(f64::from_bits(vm.decode_bits64(value)?)),
        };
    }
    let argc = window.len();
    // The caller's view is of the run's resolved binding, which its return reads again.
    let (binding, resolved) = unsafe { (*cx).resolved };
    let memory_binding = if memory.bytes.is_null() {
        None
    } else if memory.bytes == resolved.bytes {
        Some(binding)
    } else {
        return None;
    };
    if lane_constructing(vm, callee) || vm.with_scope_entered {
        return None;
    }
    let target = match vm.cross_call_entry(callee) {
        // SAFETY: as `Vm::script_call_entry`; the entry is copied before anything resolves.
        Some(entry) => unsafe { *entry },
        None => vm.resolve_script_call_target(unsafe { &*(*cx).code }, callee)?,
    };
    let argc16 = argc as u16;
    if target.program() == unsafe { (*cx).program }
        || !target.entry.activates_across()
        || target.forwards()
        || !(argc16 <= target.params || target.drops_extra_arguments())
        || !lane_activation_room(vm, &target)
    {
        return None;
    }
    let this = lane_call_this(vm, &target, Value::UNDEFINED)?;
    let instance = unsafe { (*cx).instance };
    let (registers, start) = lane_push_passed::<H, true, false>(cx, r, ip, &target, args.as_ptr(), argc16.min(target.params), this, false);
    vm.lane_stack.recomplete_top(Completion::ToWasm);
    vm.lane_stack.to_wasm.push(ToWasmCall { instance, memory: memory_binding, result });
    unsafe {
        (*cx).inline_calls = false;
        lane_set_script_calls(cx, true);
        (*cx).instance = Value::DELETED;
    }
    let view = Carried::script(cx);
    unsafe { (*cx).memory = view.memory };
    Some(Resumed::At(registers, start, view))
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
