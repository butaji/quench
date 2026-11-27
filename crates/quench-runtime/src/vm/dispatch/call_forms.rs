//! The general forms of the call-family records (`staged::CallRecord::has_general_form`), which the call row
//! runner runs on the interpreter's frame where the staged row declines (`InterpreterFrame::call_row`): a plain,
//! forwarding, spread-array or spread-iterable call, a call of a known function, the two method calls, a
//! construction (plain, applied or `super(...)`) and a first-call stub. Each is the body its opcode's arm had in
//! `Vm::step`, over the operands the record names; `pc` is the record's resume PC.

use super::*;
use crate::staged::CallRecord;

impl Vm {
    /// The general form `form` of the call-family record `i`, run by frame `f` at resume PC `pc`. The form's
    /// completion is `Continue` once its result is written, `Return` where the call returns from the frame, or
    /// the activation or tail call it pushed.
    pub(in crate::vm) fn call_form_step(&mut self, p: &ResidualProgram, f: usize, form: CallRecord, i: WideInstruction,
        pc: usize, allow_inline_calls: bool) -> Result<StepResult, JsError> {
        match form {
            CallRecord::Plain | CallRecord::Forwarding | CallRecord::Array | CallRecord::Spread =>
                self.call_window_form(p, f, form, i, pc, allow_inline_calls),
            CallRecord::Known => self.call_known_form(p, f, i, pc, allow_inline_calls),
            CallRecord::Method => self.call_method_form(p, f, i, pc),
            CallRecord::ThisMethod => self.call_this_method_form(p, f, i, pc),
            CallRecord::Construct | CallRecord::Super => self.construct_form(p, f, i, pc, allow_inline_calls),
            CallRecord::FirstChunk => self.call_first_chunk(p, f, i, pc).map(|()| StepResult::Continue),
            CallRecord::Rest => unreachable!("a rest view's call has no general form (`CallRecord::has_general_form`)"),
        }
    }

    /// The calls through registers: a plain call (direct eval included), a forwarding call that passes the
    /// activation's `arguments`, and the calls of a spread array or iterable. The callee and receiver are
    /// registers, and the arguments are the window, the array's elements or the iterable's values.
    fn call_window_form(&mut self, p: &ResidualProgram, f: usize, form: CallRecord, i: WideInstruction, pc: usize,
        allow_inline_calls: bool) -> Result<StepResult, JsError> {
        if form == CallRecord::Forwarding
            && let Some(result) = self.forward_activation_arguments(p, f, i, pc, allow_inline_calls)
        {
            return result;
        }
        self.profile.call_source(CallSource::Dynamic);
        let window = i.call_window();
        // A spread argument array is read before any call state changes, since
        // reading it can fail; register windows are copied only on the paths that
        // need an owned argument list.
        if form == CallRecord::Spread {
            // ArgumentListEvaluation of the one spread: its values, as the argument
            // array a `CallArray` reads, in the window it reads them from.
            let iterable = self.read(f, window.base);
            let array = self.hole_array(0);
            self.write(f, window.base, array);
            self.spread_into_array(p, array, iterable)?;
        }
        let spread_arguments = if matches!(form, CallRecord::Array | CallRecord::Spread) {
            let array = self.read(f, window.base);
            Some(CallArguments::from_values(self.array_value_list(array)?))
        } else {
            None
        };
        let this = self.read(f, i.register_c());
        let callee = self.read(f, i.register_b());
        let caller = &mut *self.frames[f];
        caller.pc = pc;
        caller.pc_is_resume = true;
        debug_assert_eq!(caller.program, self.active_program);
        let direct_eval = i.direct_eval() && self.calls_direct_eval(callee);
        // The one raise of a function whose parameters' `eval` declares `arguments`
        // (`Function::parameter_eval_arguments_error`): its activations run as any
        // other's until that call, which the lane leaves to this step.
        if direct_eval
            && i.parameter_eval()
            && p.function(self.frames[f].function as usize).parameter_eval_arguments_error
        {
            return Err(self
                .syntax_error_result(p, "arguments binding is not allowed in function parameters")
                .expect_err("syntax_error_result must throw"));
        }
        let previous_binding_site_pc = if direct_eval {
            Some(self.frames[f].binding_site_pc.replace(pc as u32))
        } else {
            None
        };
        let parameter_eval = direct_eval && i.parameter_eval();
        let previous_direct_eval = self.direct_eval;
        let previous_parameter_eval = self.parameter_eval;
        let previous_this = (direct_eval && !this.is_undefined())
            .then(|| std::mem::replace(&mut self.frames[f].this, this));
        // The dispatch loop offers every plain call to `try_inline_user_call` first.
        // Compilation flags every call that falls into a return of its result.
        self.direct_eval = direct_eval;
        self.parameter_eval = parameter_eval;
        let arguments = spread_arguments
            .unwrap_or_else(|| CallArguments::from_slice(self.register_window(f, window)));
        let args = arguments.as_slice();
        if p.kind == crate::bytecode::ProgramKind::Wasm
            && i.returns_from_frame()
            && let Some(CallTarget::User(program_id, id, env)) =
                self.call_target(callee).ok()
        {
            let program = self.programs.get(program_id).ok_or_else(|| {
                JsError::validation("missing Wasm tail-call program".into())
            })?;
            let realm = self
                .heap
                .function(callee)
                .map_or(self.realm.globals, |function| self.record_realm(function));
            let result = self.with_call_roots(
                [callee, this].into_iter().chain(args.iter().copied()),
                |vm| {
                    vm.set_active_program(program_id);
                    vm.switch_realm_global(realm);
                    vm.prepare_user_tail(
                        &program,
                        f,
                        id,
                        env,
                        this,
                        args,
                        CallContext::Function(callee),
                    )
                },
            );
            self.direct_eval = previous_direct_eval;
            self.parameter_eval = previous_parameter_eval;
            result?;
            self.profile.terminal_call(TerminalCall::Function);
            return Ok(StepResult::TailCall);
        }
        if p.kind != crate::bytecode::ProgramKind::Wasm
            && self.js_tail_call_allowed(p, f, i, pc)
            && !matches!(self.heap.get(callee), Some(Cell::Proxy { .. }))
            && let Some(CallTarget::User(program_id, id, env)) =
                self.call_target(callee).ok()
            && program_id == self.frames[f].program
            && !p.function(id as usize).is_async
            && !p.function(id as usize).is_generator
        {
            self.with_call_roots(
                [callee, this].into_iter().chain(args.iter().copied()),
                |vm| {
                    vm.prepare_user_tail(
                        p,
                        f,
                        id,
                        env,
                        this,
                        args,
                        CallContext::Function(callee),
                    )
                },
            )?;
            self.direct_eval = previous_direct_eval;
            self.parameter_eval = previous_parameter_eval;
            self.profile.terminal_call(TerminalCall::Function);
            return Ok(StepResult::TailCall);
        }
        let discarded_wasm_frame =
            p.kind == crate::bytecode::ProgramKind::Wasm && i.returns_from_frame();
        if discarded_wasm_frame {
            // Native/host calls need argument roots, not the discarded guest activation.
            self.with_stack.truncate(self.frames[f].with_base);
            let frame = &mut self.frames[f];
            frame.context = CallContext::Internal;
            frame.reflected_parameters = 0;
            frame.original_arguments.clear();
            frame.locals.fill(Value::UNDEFINED);
            frame.registers.fill(Value::UNDEFINED);
            frame.dynamic_bindings.clear();
            frame.active_iterators.clear();
            frame.env = Value::NULL;
            frame.this = Value::UNDEFINED;
            frame.fixed_this = false;
            frame.captured = false;
        }
        if allow_inline_calls
            && !direct_eval
            && let Some(result) = self.try_inline_call_apply(p, f, i, pc)
        {
            self.direct_eval = previous_direct_eval;
            self.parameter_eval = previous_parameter_eval;
            return result;
        }
        let called = if discarded_wasm_frame {
            self.call_value(p, callee, this, args)
        } else {
            self.call_value_from_frame(p, callee, this, args)
        };
        if let Some(binding_site_pc) = previous_binding_site_pc {
            self.frames[f].binding_site_pc = binding_site_pc;
        }
        let value = match called {
            Ok(value) => value,
            Err(error) => {
                if let Some(previous_this) = previous_this {
                    self.frames[f].this = previous_this;
                }
                self.direct_eval = previous_direct_eval;
                self.parameter_eval = previous_parameter_eval;
                return Err(error);
            }
        };
        if let Some(previous_this) = previous_this {
            self.frames[f].this = previous_this;
        }
        self.direct_eval = previous_direct_eval;
        self.parameter_eval = previous_parameter_eval;
        if i.returns_from_frame() {
            self.profile.terminal_call(TerminalCall::Function);
            return Ok(StepResult::Return(value));
        }
        self.write(f, i.result_register(), value);
        Ok(StepResult::Continue)
    }

    /// A call of the function the record names (`Callee::Known`), with no receiver: a wasm program's call
    /// compiled or pushed as its frame, else the function's JIT entry, else the user call (or its tail call).
    fn call_known_form(&mut self, p: &ResidualProgram, f: usize, i: WideInstruction, pc: usize,
        allow_inline_calls: bool) -> Result<StepResult, JsError> {
        if allow_inline_calls
            && p.kind == crate::bytecode::ProgramKind::Wasm
            && !i.returns_from_frame()
        {
            self.profile.call_source(CallSource::Known);
            // The native entry the lane makes first, by the same rule (`wasm_activation::enter_native`).
            let mut call = GeneralWasmCall { vm: self, f, i, offered: None, raised: None };
            if let crate::staged::wasm_activation::NativeEntry::Ran(resumed) = crate::staged::wasm_activation::enter_native(&mut call, u32::from(i.known_function_index())) {
                return resumed;
            }
            self.frames[f].pc = pc;
            self.frames[f].pc_is_resume = true;
            let call_guard = self.push_wasm_frame(
                p,
                f,
                u32::from(i.known_function_index()),
                i.call_window(),
            )?;
            return Ok(StepResult::PushFrame {
                destination: i.result_register(),
                call_guard,
                completion: CallCompletion::Value,
            });
        }
        self.profile.call_source(CallSource::Known);
        let window = i.call_window();
        let function_index = i.known_function_index();
        let argument_start = usize::from(window.base);
        let argument_end = argument_start + usize::from(window.count);
        let call_arguments: *const [Value] =
            &self.frames[f].registers[argument_start..argument_end];
        // SAFETY: the window lies in the caller frame, which the tier's direct call
        // only reads.
        if let Some(value) = self.js_jit_compiled_call(
            p,
            (self.active_program, u32::from(function_index)),
            i.returns_from_frame(),
            unsafe { &*call_arguments },
        ) {
            self.profile.call_target(1, usize::from(window.count));
            self.write(f, i.result_register(), value);
            return Ok(StepResult::Continue);
        }
        let register_arguments: *const [Value] = self.register_window(f, window);
        let parent = self.owner_capture_env(VarOwner::Frame(f), 0).unwrap_or(self.frames[f].env);
        self.profile.call_target(1, usize::from(window.count));
        self.frames[f].pc = pc;
        self.frames[f].pc_is_resume = true;
        let tail_call = if p.kind == crate::bytecode::ProgramKind::Wasm {
            i.returns_from_frame()
        } else {
            self.js_tail_call_allowed(p, f, i, pc)
        };
        let proper_tail = tail_call
            && !p.function(function_index as usize).is_async
            && !p.function(function_index as usize).is_generator;
        // A proper tail call recycles the caller's register buffer before it binds the
        // arguments, so only that path copies the window. Every other call borrows it, as the
        // tier's direct call does: the caller's registers are not written during the call.
        // SAFETY: the window lies in the caller frame, whose registers are not written while
        // the callee's setup and call run.
        let copied = proper_tail.then(|| CallArguments::from_slice(unsafe { &*register_arguments }));
        let args: &[Value] = match &copied {
            Some(arguments) => arguments.as_slice(),
            None => unsafe { &*register_arguments },
        };
        if proper_tail {
            self.prepare_user_tail(
                p,
                f,
                u32::from(function_index),
                parent,
                Value::UNDEFINED,
                args,
                CallContext::Internal,
            )?;
            self.profile.terminal_call(TerminalCall::Function);
            return Ok(StepResult::TailCall);
        }
        let value = self.call_user_maybe_async(
            p,
            u32::from(function_index),
            parent,
            Value::UNDEFINED,
            args,
            CallContext::Internal,
        )?;
        if i.returns_from_frame() {
            self.profile.terminal_call(TerminalCall::Function);
            return Ok(StepResult::Return(value));
        }
        self.write(f, i.result_register(), value);
        Ok(StepResult::Continue)
    }

    /// A method call through its receiver register (`CallMethod`): the method site loads the method.
    fn call_method_form(&mut self, p: &ResidualProgram, f: usize, i: WideInstruction, pc: usize) -> Result<StepResult, JsError> {
        self.profile.call_source(CallSource::Method);
        let this = self.read(f, i.register_b());
        self.frames[f].pc = pc;
        self.frames[f].pc_is_resume = true;
        let value = self.call_method_site_safe(p, f, i.method_site_index(), this)?;
        if i.returns_from_frame() {
            self.profile.terminal_call(TerminalCall::Method);
            return Ok(StepResult::Return(value));
        }
        self.write(f, i.result_register(), value);
        Ok(StepResult::Continue)
    }

    /// A method call on the activation's `this` (`CallThisMethod`), its receiver read through the site's path.
    fn call_this_method_form(&mut self, p: &ResidualProgram, f: usize, i: WideInstruction, pc: usize) -> Result<StepResult, JsError> {
        self.profile.call_source(CallSource::ThisMethod);
        let method_site = i.method_site_index();
        let path = p.method_sites[method_site].receiver_path.get();
        let this = if let Some((atom, cache)) = path {
            self.get_field_cached(p, self.frames[f].this, atom, cache)?
        } else {
            self.frames[f].this
        };
        self.frames[f].pc = pc;
        self.frames[f].pc_is_resume = true;
        let value = self.call_method_site_safe(p, f, method_site, this)?;
        if i.returns_from_frame() {
            self.profile.terminal_call(TerminalCall::ThisMethod);
            return Ok(StepResult::Return(value));
        }
        self.write(f, i.result_register(), value);
        Ok(StepResult::Continue)
    }

    /// A construction: a plain one with its arguments in registers or an array, which a plain user function
    /// may be pushed for in place, or `super(...)`, which constructs the running activation's parent.
    fn construct_form(&mut self, p: &ResidualProgram, f: usize, i: WideInstruction, pc: usize,
        allow_inline_calls: bool) -> Result<StepResult, JsError> {
        self.profile.call_source(CallSource::Construct);
        // A returned construction is pushed too; its caller returns it (`PendingGeneralCall::returns`).
        if allow_inline_calls && let Some(step) = self.try_inline_construct(p, f, i, pc) {
            return step;
        }
        let array_arguments;
        let window_arguments;
        let args: &[Value] = match i.construct_arguments() {
            crate::bytecode::ConstructArguments::Array(register) => {
                let array = self.read(f, register);
                if !self.heap.array(array).is_some() {
                    return Err(JsError("constructor arguments are not an array".into()));
                }
                array_arguments = self.call_argument_list(p, array, false)?;
                array_arguments.as_slice()
            }
            crate::bytecode::ConstructArguments::Registers(window) => {
                window_arguments = CallArguments::from_slice(self.register_window(f, window));
                window_arguments.as_slice()
            }
        };
        self.frames[f].pc = pc;
        self.frames[f].pc_is_resume = true;
        let callee = self.read(f, i.register_b());
        let v = if i.is_super_construct() {
            self.construct_super_value(p, callee, args)?
        } else {
            self.construct_value(p, callee, args)?
        };
        if i.returns_from_frame() {
            return Ok(StepResult::Return(v));
        }
        self.write(f, i.result_register(), v);
        Ok(StepResult::Continue)
    }
}

/// A Wasm known call of the general loop, as the native entry rule (`wasm_activation::enter_native`) sees
/// it: frame `f`'s record `i`, the code the tiers offered for its callee, and what a native run raised,
/// which `left` raises.
struct GeneralWasmCall<'a> {
    vm: &'a mut Vm,
    f: usize,
    i: WideInstruction,
    offered: Option<std::rc::Rc<crate::vm::engine_types::CompiledProgram>>,
    raised: Option<JsError>,
}

impl crate::staged::wasm_activation::WasmNativeEntry for GeneralWasmCall<'_> {
    type Resume = Result<StepResult, JsError>;

    #[inline(always)]
    fn native_mode(&mut self) -> bool {
        self.vm.execution_mode == crate::ExecutionMode::Compiled
    }

    /// The tiers' answer to this entry (`Vm::offer_compiled`, which runs `offer_entry` over the
    /// general loop's tiers and makes the compiles the entry makes due in place, a compile the
    /// lane left for included); never `Leave`.
    #[inline(always)]
    fn offer(&mut self, function: u32) -> crate::staged::wasm_activation::EntryOffer<()> {
        use crate::staged::wasm_activation::EntryOffer;
        self.vm.offer_compiled(self.f, function).map_or(EntryOffer::Interpret, EntryOffer::Held).keep(&mut self.offered)
    }

    #[inline(always)]
    fn enter_compiled(&mut self, function: u32) -> crate::staged::wasm_activation::NativeCall {
        use crate::staged::wasm_activation::NativeCall;
        let offered = self.offered.take().expect("a native entry runs the code its tiers offered");
        match self.vm.run_offered(offered, self.f, function, self.i.call_window()) {
            None => NativeCall::Declined,
            Some(Ok(value)) => NativeCall::Ran(value.to_bits()),
            Some(Err(error)) => {
                self.raised = Some(error);
                NativeCall::Left
            }
        }
    }

    #[inline(always)]
    fn left(&mut self) -> Self::Resume {
        Err(self.raised.take().expect("a native run that left raised"))
    }

    #[inline(always)]
    fn complete_call(&mut self, word: u64) -> Self::Resume {
        self.vm.write(self.f, self.i.result_register(), Value::from_bits(word));
        Ok(StepResult::Continue)
    }
}
