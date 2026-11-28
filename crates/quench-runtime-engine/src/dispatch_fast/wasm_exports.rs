//! JavaScript calls of exported Wasm functions in the lane: the lane's primitives of
//! `wasm_activation::WasmExportCall`, whose rules order them.
#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;

/// The export a lane call resolved, as its program's code; the rest of the call stays in
/// `LaneExportCall::call`, moved no further.
type LaneExport = Rc<ResidualProgram>;

/// A lane call's export as `Vm::wasm_export_frame_call` resolves it: its program, function,
/// instance, encoded arguments and result type.
pub(super) type ExportCall = Option<(ProgramId, u32, Value, smallvec::SmallVec<[Value; 8]>, Option<quench_runtime::WasmType>)>;

/// The lane's primitives of a JavaScript call of an exported Wasm function and of its frame's
/// return (`wasm_activation::WasmExportCall`); `ip` and `i` are the call's record.
pub(super) struct LaneExportCall {
    pub(super) cx: *mut LaneContext,
    pub(super) r: Registers,
    pub(super) ip: Ip,
    pub(super) i: *const LaneInstruction,
    /// The call's export, which `export` fills.
    pub(super) call: ExportCall,
}

impl quench_runtime::staged::wasm_activation::WasmExportCall for LaneExportCall {
    type Resume = Resumed;
    type Export = LaneExport;
    type Pending = PendingGeneralCall;
    type Value = Value;
    #[inline(always)]
    fn admits(&mut self) -> bool {
        // SAFETY: a call's primitives run with its record.
        let (cx, i) = (self.cx, unsafe { &*self.i });
        let vm = unsafe { vm(cx) };
        let returns = i.a & quench_runtime::bytecode::RETURN_REGISTER != 0;
        (unsafe { (*cx).script_calls }) && (!returns || returning_call_admitted(cx)) && vm.execution_mode != ExecutionMode::Compiled
    }
    #[inline(always)]
    fn export(&mut self) -> Option<LaneExport> {
        // SAFETY: a call's primitives run with its record.
        let (cx, r, i) = (self.cx, self.r, unsafe { &*self.i });
        let vm = unsafe { vm(cx) };
        // SAFETY: the call's window is registers of the running function, read before any
        // activation becomes a frame.
        let args = unsafe { call_window_in_lane(r, i) };
        self.call = vm.wasm_export_frame_call(r.get(i.b), args);
        let &Some((program, function, ..)) = &self.call else { return None };
        let callee_code = vm.programs.get(program)?;
        if !vm.admits_in_lane(program, &callee_code) || !vm.call_may_derive_in_lane(program, function) {
            return None;
        }
        Some(callee_code)
    }
    /// The call as a frameless Wasm activation of the export (`ActivationKind::WasmEntry`) over its
    /// instance environment, pushed as a Wasm call's activation is (`wasm_invoke::step_call_known_of`)
    /// beside the caller's activations, which stay activations. The run goes on as a Wasm run
    /// (`LaneContext::inline_calls`) until the activation returns. `None`, which changes nothing,
    /// where the callee has no lane view yet or the lane no room for it.
    #[inline(always)]
    fn export_activation(&mut self, _: &LaneExport) -> Option<Resumed> {
        let (cx, r, ip) = (self.cx, self.r, self.ip);
        let Some((program, function, instance, ref encoded, result)) = self.call else { return None };
        let vm = unsafe { vm(cx) };
        let view_in_lane = vm
            .programs
            .views_in_lane(program)?
            .get(function as usize)
            .and_then(quench_runtime::vm::program_store::derived_lane_view)?;
        let shape = view_in_lane.activation?;
        let base = view_in_lane.records();
        let stack: &mut LaneStack = &mut vm.lane_stack;
        let count = usize::from(shape.registers);
        let frame_storage_bytes = count.saturating_mul(std::mem::size_of::<Value>());
        if frame_storage_bytes
            > quench_runtime::stack::GuestCallGuard::remaining_storage_bytes().saturating_sub(stack.wasm_storage_bytes)
            || !stack.admits_activations(1)
            || vm.frames.len() + stack.activation_count() >= quench_runtime::vm::dispatch_frame::WASM_ACTIVATION_LIMIT
            || stack.values.room() < fill_room::<ACTIVATION_FILL_BLOCK>(count)
        {
            return None;
        }
        let window = stack.values.len();
        let passed = encoded.len().min(usize::from(shape.params));
        // SAFETY: capacity holds the window; each slot is written before the length covers it,
        // and no activation's window moves.
        let callee_registers = unsafe {
            let registers = stack.values.as_mut_ptr().add(window);
            fill_window(registers, usize::from(shape.fill), shape.initial);
            for (offset, &value) in encoded.as_slice()[..passed].iter().enumerate() {
                registers.add(usize::from(shape.parameters) + offset).write(value);
            }
            stack.values.set_len(window + count + lane_values::CONTEXT);
            // A Wasm activation runs with its caller's receiver and environment (`ActivationState::of`).
            Registers(registers).set_this(r.this());
            Registers(registers).set_env(r.env());
            Registers(registers).set_capture(r.capture());
            Registers(registers)
        };
        let (caller_program, caller_base) = unsafe { ((*cx).program, (*cx).base) };
        // SAFETY: capacity holds the activation.
        unsafe {
            let at = stack.values.as_mut_ptr().wrapping_add(window);
            stack.push_activation(Activation::of_parts(
                function, ip, caller_base, r, caller_program,
                ActivationKind::WasmEntry { instance, result, frame_storage_bytes, program, pc: 0 },
            ), at);
        }
        enter_program_in_lane(cx, program);
        // A fresh activation's memory register holds no memory until the function's instance
        // binding loads it.
        unsafe {
            (*cx).registers = callee_registers;
            (*cx).base = base;
            (*cx).memory = MemoryView::NONE;
            (*cx).instance = instance;
            (*cx).inline_calls = true;
            set_script_calls_in_lane(cx, false);
        }
        let view = Carried { memory: MemoryView::NONE, integers: quench_runtime::value::IntegerEncoding::TAG, acc: Slot::of(Value::UNDEFINED) };
        Some(Resumed::At(callee_registers, base, view))
    }
    #[inline(never)]
    fn export_frame(&mut self, callee_code: LaneExport) -> Resumed {
        // SAFETY: a call's primitives run with its record.
        let (cx, ip, i) = (self.cx, self.ip, unsafe { &*self.i });
        let vm = unsafe { vm(cx) };
        let Some((program, function, env, ref encoded, result)) = self.call else { unreachable!("the call resolved its export") };
        let returns = i.a & quench_runtime::bytecode::RETURN_REGISTER != 0;
        vm.materialize_lane_activations(cx);
        let caller = unsafe { (*cx).frame() };
        // SAFETY: the call record lies in the running view.
        let call_pc = unsafe { record_pc(cx, ip) } as usize;
        vm.frames[caller].pc = call_pc + 1;
        vm.frames[caller].pc_is_resume = true;
        // The callee runs in its own program, which its frame belongs to and the loop's executing
        // one sets aside as a frame call into another program does (`push_frame_call_in_lane`).
        let caller_program = unsafe { (*cx).program };
        enter_program_in_lane(cx, program);
        let call_guard = match vm.push_wasm_frame_over(&callee_code, function, env, encoded) {
            Ok(guard) => guard,
            Err(error) => {
                // An exhausted guest stack traps, which reaches the caller as a `RuntimeError`.
                enter_program_in_lane(cx, caller_program);
                // SAFETY: the context's program, the caller's again, outlives the lane run.
                let error = vm.trap_raised_in_javascript(unsafe { &*(*cx).code }, error); vm.lane_completion.raise(error);
                return Resumed::Exit(exit(ip));
            }
        };
        // SAFETY: the general loop's executing program outlives the lane run.
        let executing = unsafe { &mut *(*cx).executing };
        let saved = quench_runtime::vm::dispatch_frame::SavedProgram {
            id: executing.id,
            retained: std::mem::replace(&mut executing.retained, Some(callee_code)),
        };
        executing.id = program;
        // The run goes on as a Wasm run (`LaneContext::inline_calls`) until the frame returns,
        // which completes on the general path, as it completes any call into another program
        // (`return_frame_in_lane`); the caller's lane run starts afresh there.
        unsafe {
            (*cx).inline_calls = true;
            set_script_calls_in_lane(cx, false);
        }
        // SAFETY: the general loop's pending calls outlive the lane run.
        unsafe { &mut *(*cx).pending }.push(PendingGeneralCall {
            caller,
            caller_cursor: None,
            call_pc: call_pc as u32,
            destination: i.a & quench_runtime::bytecode::REGISTER_MASK,
            completion: CallCompletion::WasmExport(result),
            returns,
            call_guard: Some(call_guard),
            caller_program: Some(saved),
        });
        // The pushed frame's guard narrows the activations the thread admits.
        vm.lane_stack.refresh_activation_limit();
        enter_frame_in_lane(cx, caller + 1, 0)
    }
    #[inline(always)]
    fn retire_frame(&mut self) -> PendingGeneralCall {
        let (cx, vm) = (self.cx, unsafe { vm(self.cx) });
        // SAFETY: the general loop's pending calls outlive the lane run.
        let mut call = unsafe { &mut *(*cx).pending }.pop().expect("the export's frame has its pending call");
        vm.retire_wasm_frame();
        let saved = call.caller_program.take().expect("an export's frame runs its own program");
        // SAFETY: the general loop's executing program outlives the lane run.
        let executing = unsafe { &mut *(*cx).executing };
        (executing.id, executing.retained) = (saved.id, saved.retained);
        enter_program_in_lane(cx, saved.id);
        unsafe { (*cx).inline_calls = false; set_script_calls_in_lane(cx, true) };
        drop(call.call_guard.take());
        vm.active_registers = vm.frames[call.caller].registers.as_mut_ptr();
        call
    }
    #[inline(always)]
    fn export_value(&mut self, call: &PendingGeneralCall, value: Value) -> Value {
        let CallCompletion::WasmExport(result) = call.completion else { unreachable!("the caller tested the completion") };
        unsafe { vm(self.cx) }.wasm_export_result(value, result)
    }
    #[inline(always)]
    fn caller_returns(call: &PendingGeneralCall) -> bool { call.returns }
    #[inline(always)]
    fn complete_caller(&mut self, call: PendingGeneralCall, value: Value) -> Resumed {
        // The caller completes too, at its call record, as a returning frame call's does
        // (`return_script_frame_in_lane`).
        let (_, call_ip, _) = frame_state_in_lane(self.cx, call.caller, call.call_pc as usize);
        Resumed::Exit(return_script_frame_in_lane(self.cx, call_ip, value, true))
    }
    #[inline(always)]
    fn resume_caller(&mut self, call: PendingGeneralCall, value: Value) -> Resumed {
        unsafe { vm(self.cx) }.write(call.caller, call.destination, value);
        enter_script_frame_in_lane(self.cx, call.caller)
    }
}
