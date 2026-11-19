// Hand arms left: the arms of `Vm::step` that still name an opcode, and why `step_unlisted` does not take them.
// `step` carries `#[quench_macros::operation_semantics]`, which rejects match guards ("semantic dispatch
// guards must live inside the operation"), so no family arm can be written here. `step_unlisted` takes the
// rest by catalog attribute: `NO_STEP` (Nop; the wide marker is never dispatched), `SUSPENDS` (Await, Yield,
// YieldStar: `staged_suspend`), `ROW_ONLY` (a staged row that decides its opcode: IteratorCleanupPush,
// IteratorCleanupPop, ForInNext, GetAsyncIterator, StringCaseIndex, CacheTemplateObject,
// LoadCachedTemplateObject, ToNumeric, IncDec, Return, Throw), `OWNER_ONLY` (LoadEnvLocal, StoreEnvLocal,
// StoreName: `owner_step` alone), an owned step (`Op::owned_step`: its row first where the catalog marks one,
// then `owner_step`), `Decoder::Literal` (`decode_literal` run completed on the frame, whose
// `allocate_value` and element definer answer it), and the Names, Generic, Property and Value call-outs
// (`staged_value_step`).
// What is left, and why:
// - Row, then a per-opcode miss: Binary, NumericAdd, NumericMultiply, Unary, JumpBinaryFalse. Each
//   miss is that opcode's own semantics, not a staged row's. What keeps each from its arm alone:
//   - Binary, NumericAdd, NumericMultiply: `decode_binary` declines an operand that is neither a register nor
//     a constant, and under `stencil_build` every operator that is no Number's.
//   - Unary: `decode_unary` declines the operators it does not stage: a `typeof` or `void` that returns from the
//     frame, and any operator outside the unary family. A non-int32 operand is `CallOut::Unary`.
//   - JumpBinaryFalse: a bitwise test of a constant operand whose word is no int32 has no call-out, since `Decide`
//     takes registers and a `Binary` yields a Number whose truth a word cannot tell (a BigInt's zero is no int32 zero).
// - Field stores to `this`: SetThisField, SetThisFieldStrict. The interpreter frame's property plans are
//   `NoPlans`, so its store row always leaves; the miss must read `this` by `checked_this_binding`, which
//   the named store's call-out (`CallOut::StoreField`, reading register B) does not.
// - Run-only (`Declined` is unreachable): GetField, GetIndex, SetField, SetFieldStrict, SetIndex. Their misses are
//   the call-outs `LoadField`, `LoadElement`, `StoreField` and `StoreElement`, which the interpreter frame answers;
//   a read that writes `this` stores its value through `store_field_sink`.
// - WasmBranchTable: its row's edge is a branch-table entry (`Edge::Entry`), which the interpreter frame's
//   `step_row` refuses, so its case stays the hand body.
// Call-family records (`staged::invoke::general_call_form`: Call, CallArray, CallSpread, CallForwardingArguments,
// CallKnown, CallMethod, CallThisMethod, Construct, CallFirstChunk) are no arms: `step_unlisted` runs their staged
// row, then the form's general body (`call_forms.rs`) on the interpreter frame (`InterpreterFrame::call_row`).

use super::*;
use super::dispatch_frame::{InterpreterFrame, RowRun};

mod call_forms;
mod increment;
pub(super) use increment::number_increment;

impl<H: Host> Vm<H> {
    pub(super) fn module_import_value(
        &self,
        program: ProgramId,
        function: u32,
        slot: usize,
    ) -> Option<Value> {
        if function != super::ROOT_FUNCTION_ID {
            return None;
        }
        let slot = u16::try_from(slot).ok()?;
        match self.programs.module_import(program, slot)? {
            ModuleImport::Value(value) => Some(value),
            ModuleImport::Binding(program, binding, fallback) => {
                let Some(environment) = self.programs.module_environment(program) else {
                    return Some(fallback);
                };
                let value = self
                    .heap
                    .environment_slot(environment, usize::from(binding));
                Some(match value {
                    Some(Value::DELETED) if !fallback.is_undefined() => fallback,
                    None => fallback,
                    Some(value) => value,
                })
            }
        }
    }

    /// Starts a plain or array-spread call of an eligible same-realm user function as an
    /// activation of the running dispatch loop, or returns `None` to leave it to `step`. Such a
    /// call involves no direct eval (its callee is not the eval intrinsic) and neither eval mode
    /// is set before it, so the frame push leaves both untouched.
    #[inline(always)]
    pub(super) fn try_inline_user_call(
        &mut self,
        p: &ResidualProgram,
        f: usize,
        i: WideInstruction,
        pc: usize,
    ) -> Option<Result<StepResult, JsError>> {
        // A sloppy call that falls into a return of its result is pushed like any other; its
        // caller then returns the callee's completion (see `PendingGeneralCall::returns`).
        // Hosts permitting proper tail calls leave strict terminal calls to `step`. Wasm programs run in
        // the loop (as in-loop Wasm frames), and their calls keep Wasm's own activation and
        // exhaustion accounting, so they are never inlined as user calls.
        if !crate::staged::calls_ordinary(i.op())
            || i.direct_eval()
            || self.direct_eval
            || self.parameter_eval
            || p.kind == crate::bytecode::ProgramKind::Wasm
            || f + 1 != self.frames.len()
            || self.frames[f].function == super::ROOT_FUNCTION_ID
            || (self.host.proper_tail_calls()
                && i.returns_from_frame()
                && p.function(self.frames[f].function as usize).strict)
        {
            return None;
        }
        let callee = self.read(f, i.register_b());
        let Some(entry) = self.in_loop_call_entry(p, callee) else { return self.try_forwarded_user_call(p, f, i, pc, callee) };
        let target = (entry.program, entry.function, entry.env);
        if entry.push == super::InLoopPush::Settles {
            // Settling calls consume the ordinary register window and cannot take
            // CallArray's materialized argument vector through this continuation.
            if crate::staged::calls_array(i.op()) {
                return None;
            }
            return self.try_settling_call(p, f, i, pc, callee, target);
        }
        #[cfg(feature = "profile-lane-exits")]
        super::lane_exits::call_target(
            self.active_program.raw(),
            self.frames[f].function,
            pc as u32,
            callee,
            target.1,
        );
        self.profile.call_source(CallSource::Dynamic);
        let window = i.call_window();
        // CallArray's input is an already materialized argument array. Copy its
        // indexed values before pushing the frame, just as `Vm::step` does. The
        // copied Values are rooted through activation setup below because the
        // caller resumes past the temporary array register before its frame is pushed.
        let array_arguments = if crate::staged::calls_array(i.op()) {
            match self.array_values(self.read(f, window.base)) {
                Ok(arguments) => Some(arguments),
                Err(error) => return Some(Err(error)),
            }
        } else {
            None
        };
        // Collection happens only at back edges, tail calls and explicit host safepoints, none
        // of which can run while a frame is pushed; afterwards the new frame itself roots the
        // callee, receiver and arguments.
        let register_arguments: *const [Value] = self.register_window(f, window);
        // SAFETY: the window lies in the caller frame, which the tier's direct call only reads.
        let arguments: &[Value] = array_arguments
            .as_deref()
            .unwrap_or_else(|| unsafe { &*register_arguments });
        if let Some(value) =
            self.js_jit_compiled_call(p, (target.0, target.1), i.returns_from_frame(), arguments)
        {
            self.write(f, i.result_register(), value);
            return Some(Ok(StepResult::Continue));
        }
        let this = self.read(f, i.register_c());
        self.profile.call_target(1, arguments.len());
        // SAFETY: see `push_general_user_frame`; the window outlives callee setup.
        if array_arguments.is_some() {
            let roots = std::iter::once(callee)
                .chain(std::iter::once(this))
                .chain(arguments.iter().copied());
            self.with_call_roots(roots, |vm| {
                vm.push_in_loop_call(
                    p,
                    f,
                    pc,
                    target,
                    callee,
                    this,
                    arguments,
                    i.result_register(),
                )
            })
        } else {
            self.push_in_loop_call(p, f, pc, target, callee, this, arguments, i.result_register())
        }
    }

    /// A plain construction the loop pushes in place (`inline_construct_target`), its receiver bound.
    pub(super) fn try_inline_construct(&mut self, p: &ResidualProgram, f: usize, i: WideInstruction, pc: usize)
        -> Option<Result<StepResult, JsError>> {
        let (crate::bytecode::ConstructArguments::Registers(window), false) = (i.construct_arguments(), i.is_super_construct())
            else { return None };
        let callee = self.read(f, i.register_b());
        let (program, id, env, this) = self.inline_construct_target(p, f, callee)?;
        let arguments: *const [Value] = self.register_window(f, window);
        // SAFETY: see `push_general_user_frame`; the window outlives callee setup.
        self.push_in_loop_activation(p, f, pc, (program, id, env), callee, this, unsafe { &*arguments },
            i.result_register(), CallCompletion::Constructor(this))
    }

    /// A field read's `getter`, a user function the loop may push in place, run as the next
    /// in-loop activation with receiver `this` and no arguments, writing `destination`.
    #[inline(never)]
    pub(super) fn try_inline_getter(
        &mut self,
        p: &ResidualProgram,
        f: usize,
        pc: usize,
        getter: Value,
        this: Value,
        destination: crate::bytecode::Register,
    ) -> Option<Result<StepResult, JsError>> {
        if self.direct_eval
            || self.parameter_eval
            || p.kind == crate::bytecode::ProgramKind::Wasm
            || f + 1 != self.frames.len()
            || self.frames[f].function == super::ROOT_FUNCTION_ID
        {
            return None;
        }
        let target = self.in_loop_call_target(p, getter)?;
        self.push_in_loop_call(p, f, pc, target, getter, this, &[], destination)
    }

    /// Pushes `callee`, resolved by `in_loop_call_target` to `(program, function, env)`, as
    /// the caller `f`'s next in-loop activation. A callee of another program is activated in
    /// its program, which the loop executes from the push (`StepResult::PushFrame`) until the
    /// return restores the caller's.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn push_in_loop_call(
        &mut self,
        p: &ResidualProgram,
        f: usize,
        pc: usize,
        target: (ProgramId, u32, Value),
        callee: Value,
        this: Value,
        arguments: &[Value],
        destination: crate::bytecode::Register,
    ) -> Option<Result<StepResult, JsError>> {
        self.push_in_loop_activation(p, f, pc, target, callee, this, arguments, destination, CallCompletion::Value)
    }

    /// `push_in_loop_call` completing as `completion`: a construction's (`Constructor`) is
    /// pushed with its target bound (`construct_target`), as [[Construct]] enters its body.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn push_in_loop_activation(
        &mut self,
        p: &ResidualProgram,
        f: usize,
        pc: usize,
        (program, id, env): (ProgramId, u32, Value),
        callee: Value,
        this: Value,
        arguments: &[Value],
        destination: crate::bytecode::Register,
        completion: CallCompletion,
    ) -> Option<Result<StepResult, JsError>> {
        let caller_program = self.active_program;
        let callee_residual = if program == caller_program {
            None
        } else {
            Some(self.programs.get(program)?)
        };
        self.frames[f].pc = pc;
        self.frames[f].pc_is_resume = true;
        if callee_residual.is_some() {
            self.set_active_program(program);
        }
        let constructs = matches!(completion, CallCompletion::Constructor(_));
        if constructs { self.set_construct_target(Some(ConstructionTarget { callee, new_target: callee })); }
        let call_guard = self.push_general_user_frame(callee_residual.as_deref().unwrap_or(p), id, env, this, arguments,
            CallContext::user_function(id, callee));
        if constructs { self.set_construct_target(None); }
        if call_guard.is_err() {
            self.set_active_program(caller_program);
        }
        Some(call_guard.map(|call_guard| StepResult::PushFrame { destination, call_guard, completion }))
    }

    /// The program, function and environment of `callee` when it is a user function of a
    /// JavaScript program in the current realm that the loop may push in place: a non-root
    /// ordinary body (`CallPlan::in_loop`). Resolved targets are cached (`InLoopCallTarget`).
    #[inline(always)]
    pub(super) fn in_loop_call_target(
        &mut self,
        p: &ResidualProgram,
        callee: Value,
    ) -> Option<(ProgramId, u32, Value)> {
        self.in_loop_call_entry(p, callee)
            .filter(|target| target.push == super::InLoopPush::Body)
            .map(|target| (target.program, target.function, target.env))
    }

    /// The program, function and environment of `callee` when the loop pushes it as a settling
    /// call (`InLoopPush::Settles`): an async body, which the lane starts as a job
    /// (`Vm::start_async_call`).
    pub(super) fn in_loop_settling_target(&mut self, p: &ResidualProgram, callee: Value) -> Option<(ProgramId, u32, Value)> {
        self.in_loop_call_entry(p, callee)
            .filter(|target| target.push == super::InLoopPush::Settles)
            .map(|target| (target.program, target.function, target.env))
    }

    /// The cached in-loop push of `callee`, of either kind (`InLoopPush`).
    #[inline(always)]
    fn in_loop_call_entry(&mut self, p: &ResidualProgram, callee: Value) -> Option<super::InLoopCallTarget> {
        let Some(heap_index) = callee.heap_index() else {
            #[cfg(feature = "profile-lane-exits")]
            super::lane_exits::call_eligibility(super::lane_exits::CallEligibility::NonHeapCallee);
            return None;
        };
        let index = heap_index as usize & (super::CALL_TARGET_CACHE_SIZE - 1);
        let cached = self.call_targets[index];
        if cached.callee == callee {
            #[cfg(feature = "profile-lane-exits")]
            super::lane_exits::call_eligibility(super::lane_exits::CallEligibility::PositiveCacheHit);
            return Some(cached);
        }
        let target = self.resolve_in_loop_call_target(p, callee, index);
        #[cfg(feature = "profile-lane-exits")]
        if target.is_some() {
            super::lane_exits::call_eligibility(super::lane_exits::CallEligibility::PositiveResolution);
        } else {
            self.record_call_eligibility_miss(p, callee);
        }
        target
    }

    #[cfg(feature = "profile-lane-exits")]
    fn record_call_eligibility_miss(&self, p: &ResidualProgram, callee: Value) {
        let Some(record) = self.heap.function(callee) else {
            super::lane_exits::call_eligibility(super::lane_exits::CallEligibility::NativeOrWrongRealmOrNonCallable);
            return;
        };
        let FunctionKind::User(program, function) = &record.kind else {
            super::lane_exits::call_eligibility(super::lane_exits::CallEligibility::NativeOrWrongRealmOrNonCallable);
            return;
        };
        if self.record_realm(record) != self.realm.globals {
            super::lane_exits::call_eligibility(super::lane_exits::CallEligibility::NativeOrWrongRealmOrNonCallable);
            return;
        }
        let Some(owner) = (if *program == self.active_program {
            Some(p)
        } else {
            self.programs.residual(*program)
        }) else {
            super::lane_exits::call_eligibility(super::lane_exits::CallEligibility::MissingProgramOrFunction);
            return;
        };
        let unsupported = owner.function_get(*function as usize).is_none_or(|body| {
            let plan = body.call_plan(owner);
            !plan.in_loop && !plan.settles_in_loop
        });
        if *function == super::ROOT_FUNCTION_ID
            || owner.kind == crate::bytecode::ProgramKind::Wasm
            || unsupported
        {
            super::lane_exits::call_eligibility(super::lane_exits::CallEligibility::UnsupportedCallPlan);
        } else {
            // Keep this bucket conservative if a new resolver rejection is added.
            super::lane_exits::call_eligibility(super::lane_exits::CallEligibility::MissingProgramOrFunction);
        }
    }

    #[inline(never)]
    fn resolve_in_loop_call_target(
        &mut self,
        p: &ResidualProgram,
        callee: Value,
        index: usize,
    ) -> Option<super::InLoopCallTarget> {
        let (program, function, env) = self.same_realm_user_target(callee)?;
        let owner = if program == self.active_program {
            p
        } else {
            self.programs.residual(program)?
        };
        if function == super::ROOT_FUNCTION_ID || owner.kind == crate::bytecode::ProgramKind::Wasm {
            return None;
        }
        let body = owner.function_get(function as usize)?;
        let plan = body.call_plan(owner);
        let push = if plan.in_loop {
            super::InLoopPush::Body
        } else if plan.settles_in_loop || suspends_in_loop(body) {
            super::InLoopPush::Settles
        } else {
            return None;
        };
        let target = super::InLoopCallTarget {
            callee,
            program,
            function,
            env,
            push,
        };
        self.call_targets[index] = target;
        Some(target)
    }

    /// `f.call(this, ...args)`, `f.apply(this, list)` and `Reflect.apply(f, this, list)` of
    /// the intrinsic natives, and a call of `f.bind(this, ...bound)`, invoke `f` with no
    /// observable step of their own when `list` is a dense array, so when
    /// `try_inline_user_call` declines a call whose callee is one of them and `f` is a user
    /// function the loop may push (`in_loop_call_target`), `f` is pushed directly, in its own
    /// program. It applies `try_inline_user_call`'s conditions to the call; it stays out of
    /// line so ordinary user calls keep their code.
    #[inline(never)]
    pub(super) fn try_inline_call_apply(
        &mut self,
        p: &ResidualProgram,
        f: usize,
        i: WideInstruction,
        pc: usize,
    ) -> Option<Result<StepResult, JsError>> {
        if !crate::staged::calls_positional(i.op())
            || self.direct_eval
            || self.parameter_eval
            || p.kind == crate::bytecode::ProgramKind::Wasm
            || f + 1 != self.frames.len()
            || self.frames[f].function == super::ROOT_FUNCTION_ID
            || (self.host.proper_tail_calls()
                && i.returns_from_frame()
                && p.function(self.frames[f].function as usize).strict)
        {
            return None;
        }
        let callee = self.read(f, i.register_b());
        let mut target = self.read(f, i.register_c());
        let window: *const [Value] = self.register_window(f, i.call_window());
        // SAFETY: see `push_general_user_frame`; the window outlives callee setup.
        let window = unsafe { &*window };
        let mut this = window.first().copied().unwrap_or(Value::UNDEFINED);
        let apply_arguments;
        let bound_arguments;
        // `f.call(...)`, a bound function without bound arguments, or an uncurried builtin
        // (`Vm::forwarded_call`). A forwarded `arguments` reaches here unmaterialized, which
        // only `apply` takes as its list.
        let forwarded = if crate::staged::calls(i.op()) { self.forwarded_call(callee, target, window) } else { None };
        let arguments = match forwarded {
            Some(super::function::Forwarded { callee: forwarded_target, receiver: forwarded_this, args: forwarded_arguments, .. }) => {
                (target, this) = (forwarded_target, forwarded_this);
                forwarded_arguments
            }
            None => match self.heap.function(callee) {
                // A bound function calls its target with its bound `this`, its bound arguments
                // and then the call's, and takes no observable step of its own.
                Some(FunctionRecord {
                    kind: FunctionKind::Native(Native::FunctionBoundCall),
                    env,
                    ..
                }) => {
                    // A forwarded `arguments` reaches here unmaterialized, which only `apply`
                    // takes as its list.
                    if !crate::staged::calls(i.op()) {
                        return None;
                    }
                    let (bound_target, bound_this, bound) = self.bound_function_parts(*env)?;
                    (target, this) = (bound_target, bound_this);
                    bound_arguments = bound.iter().chain(window).copied().collect::<Vec<_>>();
                    bound_arguments.as_slice()
                }
                // `Reflect.apply` takes its target and receiver as arguments and requires a list
                // object, which only a dense array answers here.
                Some(FunctionRecord {
                    kind: FunctionKind::Native(Native::ReflectApply),
                    ..
                }) => {
                    let list = window.get(2).copied().unwrap_or(Value::UNDEFINED);
                    if !crate::staged::calls(i.op()) || self.heap.array(list).is_none() {
                        return None;
                    }
                    target = window.first().copied().unwrap_or(Value::UNDEFINED);
                    this = window.get(1).copied().unwrap_or(Value::UNDEFINED);
                    apply_arguments = self.dense_apply_arguments(list)?;
                    apply_arguments.as_slice()
                }
                Some(FunctionRecord {
                    kind: FunctionKind::Native(Native::FunctionApply),
                    ..
                }) => match window.get(1).copied().unwrap_or(Value::UNDEFINED) {
                    // The caller's unmaterialized `arguments`: the values its frame kept.
                    Value::UNMATERIALIZED_ARGUMENTS => {
                        let kept: *const [Value] = self.frames[f].original_arguments.as_slice();
                        // SAFETY: the caller's frame stays on the stack and is not mutated while
                        // the callee is set up; its buffer does not move when `frames` grows.
                        unsafe { &*kept }
                    }
                    list => {
                        apply_arguments = self.dense_apply_arguments(list)?;
                        apply_arguments.as_slice()
                    }
                },
                _ => return None,
            },
        };
        let resolved = self.in_loop_call_target(p, target)?;
        self.profile.call_source(CallSource::Dynamic);
        self.profile.call_target(1, arguments.len());
        self.push_in_loop_call(p, f, pc, resolved, target, this, arguments, i.result_register())
    }

    /// `Reflect.apply(f, this, list)` of the intrinsic native, tried before the plain user
    /// call: `try_inline_call_apply` pushes `f` in place when `list` is a dense array.
    #[inline(never)]
    pub(super) fn try_inline_reflect_apply(
        &mut self,
        p: &ResidualProgram,
        f: usize,
        i: WideInstruction,
        pc: usize,
    ) -> Option<Result<StepResult, JsError>> {
        let callee = self.read(f, i.register_b());
        if !crate::staged::calls(i.op())
            || !matches!(
                self.heap.function(callee),
                Some(FunctionRecord { kind: FunctionKind::Native(Native::ReflectApply), .. })
            )
        {
            return None;
        }
        self.try_inline_call_apply(p, f, i, pc)
    }

    /// `CallForwardingArguments` whose argument is the caller's unmaterialized `arguments`:
    /// the intrinsic `apply` of an in-loop user function is pushed with the kept values;
    /// any other call first materializes the object, then proceeds as `Call` (`None`).
    #[inline(never)]
    fn forward_activation_arguments(
        &mut self,
        p: &ResidualProgram,
        f: usize,
        i: WideInstruction,
        pc: usize,
        allow_inline_calls: bool,
    ) -> Option<Result<StepResult, JsError>> {
        let last = i.call_window().last()?;
        if self.read(f, last) != Value::UNMATERIALIZED_ARGUMENTS {
            return None;
        }
        if allow_inline_calls && let Some(result) = self.try_inline_call_apply(p, f, i, pc) {
            return Some(result);
        }
        match self.materialize_activation_arguments(p, f) {
            Ok(arguments) => {
                self.write(f, last, arguments);
                None
            }
            Err(error) => Some(Err(error)),
        }
    }

    /// CreateListFromArrayLike for `apply` when it reduces to a copy: null or undefined, a
    /// plain array without holes, or an arguments object whose data `length` covers a
    /// hole-free prefix, neither carrying indexed descriptors or sparse elements. `None`
    /// leaves the list to the generic algorithm, whose reads may run guest code.
    pub(super) fn dense_apply_arguments(&self, list: Value) -> Option<Vec<Value>> {
        if list.is_null() || list.is_undefined() {
            return Some(Vec::new());
        }
        let object = self.heap.array(list)?;
        if object.has_indexed_descriptors() || self.heap.sparse_length(list).is_some() {
            return None;
        }
        let elements = object.elements();
        let length = if object.is_arguments_object() {
            let attributes =
                self.property_attributes(list, PropertyKey::string(self.length_atom))?;
            if attributes.accessor {
                return None;
            }
            usize::try_from(self.own_property(list, self.length_atom)?.as_int()?).ok()?
        } else {
            elements.len()
        };
        let values = elements.range(0..length.min(elements.len()));
        (length <= elements.len() && !elements.range(0..length.min(elements.len())).any(|value| value.is_deleted())).then(|| values.collect())
    }

    /// The length of `list` where CreateListFromArrayLike reads its first elements as they
    /// are stored: a dense array without holes, or an arguments object that keeps its
    /// template's data `length` (`arguments_object_length`), no indexed descriptor and no
    /// hole below it. A mapped parameter's element holds the parameter's value, as its loads
    /// and stores keep it. `None` for any other list, whose reads may run guest code.
    pub(super) fn dense_list_length(&self, list: Value) -> Option<usize> {
        let array = self.heap.array(list)?;
        if self.heap.sparse_length(list).is_some() {
            return None;
        }
        let elements = array.elements();
        let length = if array.is_arguments_object() {
            if array.has_indexed_descriptors() {
                return None;
            }
            usize::try_from(self.arguments_object_length(list)?.as_int()?).ok()?
        } else {
            elements.len()
        };
        (length <= elements.len() && !elements.range(0..length).any(|value| value.is_deleted())).then_some(length)
    }

    #[quench_macros::operation_semantics(resolve_step_operation)]
    #[inline(always)]
    pub(super) fn step(
        &mut self,
        p: &ResidualProgram,
        f: usize,
        i: WideInstruction,
        pc: &mut usize,
        allow_inline_calls: bool,
    ) -> Result<StepResult, JsError> {
        match i.op() {
            Op::GetField => {
                match self.step_row_of(p, f, i, pc, allow_inline_calls, crate::staged::DecodeContext::EMPTY) {
                    RowRun::Done(_) => {}
                    RowRun::Declined => unreachable!("the interpreter frame answers every named read"),
                    RowRun::Stop(stop) => return stop,
                }
            }
            Op::GetIndex => {
                match self.step_row_of(p, f, i, pc, allow_inline_calls, crate::staged::DecodeContext::EMPTY) {
                    RowRun::Done(_) => {}
                    RowRun::Declined => unreachable!("the interpreter frame answers every indexed read"),
                    RowRun::Stop(stop) => return stop,
                }
            }
            Op::SetField | Op::SetFieldStrict => {
                match self.step_row_of(p, f, i, pc, allow_inline_calls, crate::staged::DecodeContext::EMPTY) {
                    RowRun::Done(_) => {}
                    RowRun::Declined => unreachable!("the interpreter frame answers every named store"),
                    RowRun::Stop(stop) => return stop,
                }
            }
            Op::SetThisField | Op::SetThisFieldStrict => {
                let this = self.checked_this_binding(p, f)?;
                self.set_field_cached(
                    p,
                    this,
                    i.atom_index(),
                    self.read(f, i.register_a()),
                    i.cache_site_index(),
                    i.op().field_store().is_some_and(|store| store.strict),
                )?;
            }
            Op::SetIndex => {
                match self.step_row_of(p, f, i, pc, allow_inline_calls, crate::staged::DecodeContext::EMPTY) {
                    RowRun::Done(_) => {}
                    RowRun::Declined => unreachable!("the interpreter frame answers every keyed store"),
                    RowRun::Stop(stop) => return stop,
                }
            }
            Op::Binary | Op::NumericAdd | Op::NumericMultiply => {
                match self.step_row_of(p, f, i, pc, allow_inline_calls, crate::staged::DecodeContext::EMPTY) {
                    RowRun::Done(_) => {}
                    RowRun::Declined => {
                        return self.binary_miss(p, f, i, pc);
                    }
                    RowRun::Stop(stop) => return stop,
                }
            }
            // merge: HEAD's inline Unary, Jump, JumpFalse/JumpTrue arms are dropped; the derivation runs them as staged rows (step_row_of).
            Op::Unary => {
                match self.step_row_of(p, f, i, pc, allow_inline_calls, crate::staged::DecodeContext::EMPTY) {
                    RowRun::Done(_) => {}
                    RowRun::Declined => {
                        return self.unary_miss(p, f, i);
                    }
                    RowRun::Stop(stop) => return stop,
                }
            }
            Op::JumpBinaryFalse => {
                match self.step_row_of(p, f, i, pc, allow_inline_calls, crate::staged::DecodeContext::EMPTY) {
                    RowRun::Done(_) => {}
                    RowRun::Declined => {
                        return self.jump_binary_false_miss(p, f, i, pc);
                    }
                    RowRun::Stop(stop) => return stop,
                }
            }
            Op::WasmBranchTable => *pc += self.branch_table_case(f, i),
            _ => return self.step_unlisted(p, f, i, pc, allow_inline_calls),
        }
        Ok(StepResult::Continue)
    }

    /// Runs `operation`, decoded from `i` at resume PC `pc`, as its staged row on the running frame
    /// `f` (`InterpreterFrame::step_row`). A taken jump sets `pc`, and a back-edge jump publishes its
    /// PC and checks for a due collection.
    #[inline(always)]
    fn run_row(&mut self, p: &ResidualProgram, f: usize, i: WideInstruction, pc: &mut usize, allow_inline_calls: bool,
        operation: crate::staged::Operation<u64, ()>) -> RowRun {
        use crate::staged::{Edge, Operation};
        if !self.row_admitted(p, f, operation) {
            return RowRun::Declined;
        }
        let run = InterpreterFrame::step_row(self as *mut Self, p, f, *pc, allow_inline_calls, i, operation);
        if matches!(run, RowRun::Done(Edge::Target)) {
            *pc = i.jump_target() as usize;
            if matches!(operation, Operation::Jump) {
                self.frames[f].pc = *pc;
                self.frames[f].pc_is_resume = true;
                self.maybe_collect(p);
            }
        }
        run
    }

    /// The staged row `i` decodes to under `context`: a general row, a name load (under no context:
    /// its proof is the frame's), or the lane's suspension and string-case rows.
    #[inline(always)]
    pub(super) fn decode_row(i: WideInstruction, context: crate::staged::DecodeContext<'_>) -> Option<crate::staged::Operation<u64, ()>> {
        crate::staged::Operation::<u64, ()>::decode(i, context)
            .or_else(|| crate::staged::Operation::<u64, ()>::decode_load_name(i, ()))
            .or_else(|| crate::staged::Operation::<u64, ()>::decode_lane(i))
    }

    /// Whether the staged row `operation` may run on frame `f`: the checked local rows need the
    /// frame's slots to be its own (`Function::has_plain_locals`); every other row is admitted.
    #[inline(always)]
    pub(super) fn row_admitted(&self, p: &ResidualProgram, f: usize, operation: crate::staged::Operation<u64, ()>) -> bool {
        match operation {
            crate::staged::Operation::LoadLocalChecked { .. } | crate::staged::Operation::StoreLocalChecked { .. }
            | crate::staged::Operation::InitializeLocal { .. } => {
                let function = self.frames[f].function;
                p.function(function as usize).has_plain_locals(function == super::ROOT_FUNCTION_ID)
            }
            _ => true,
        }
    }

    /// `i` decoded under `context` and run as its staged row (`run_row`). The fast prelude already
    /// decoded this instruction and tried its row where it declined (`pending_row`): the arm runs its
    /// miss path without decoding or trying the row again.
    #[inline(always)]
    fn step_row_of(&mut self, p: &ResidualProgram, f: usize, i: WideInstruction, pc: &mut usize, allow_inline_calls: bool,
        context: crate::staged::DecodeContext<'_>) -> RowRun {
        if self.pending_row.take().is_some() {
            return RowRun::Declined;
        }
        // A record that writes `this` other than a read is the miss path's; a read's store into `this` is `run_row_completed`'s.
        if i.writes_current_this() && i.op() != Op::GetField {
            return RowRun::Declined;
        }
        match Self::decode_row_completed(i, context) {
            Some((operation, completes)) => self.run_row_completed(p, f, i, pc, allow_inline_calls, operation, completes),
            None => RowRun::Declined,
        }
    }

    /// The staged row `i` decodes to under `context`, and the result flags it completes its frame by
    /// (`Operation::decode_completed`). A record whose opcode takes no result flags decodes as `decode_row` does.
    #[inline(always)]
    fn decode_row_completed(i: WideInstruction, context: crate::staged::DecodeContext<'_>)
        -> Option<(crate::staged::Operation<u64, ()>, crate::bytecode::Register)> {
        if i.result_flags() == 0 || i.op().result_layout().allowed_flags() == 0 {
            return Self::decode_row(i, context).map(|operation| (operation, 0));
        }
        crate::staged::Operation::<u64, ()>::decode_completed(i, context)
    }

    /// Runs `operation` as its staged row, then completes the frame as the record asks: a return of the
    /// value the row wrote to its result register, or the numeric local the result writes.
    #[inline(always)]
    fn run_row_completed(&mut self, p: &ResidualProgram, f: usize, i: WideInstruction, pc: &mut usize, allow_inline_calls: bool,
        operation: crate::staged::Operation<u64, ()>, completes: crate::bytecode::Register) -> RowRun {
        // A read that writes `this` pushes no getter: its value is stored to `this` once it is read.
        let run = self.run_row(p, f, i, pc, allow_inline_calls && !i.writes_current_this(), operation);
        if !matches!(run, RowRun::Done(_)) {
            return run;
        }
        if completes & crate::bytecode::SET_THIS_REGISTER != 0 {
            if let Err(error) = self.store_field_sink(p, f, i) {
                return RowRun::Stop(Err(error));
            }
        }
        if completes & crate::bytecode::RETURN_REGISTER != 0 {
            return RowRun::Stop(Ok(StepResult::Return(self.read(f, i.result_register()))));
        }
        if let Some(local) = i.numeric_local_target() {
            let value = self.read(f, i.result_register());
            self.frames[f].locals[local as usize] = value;
            self.profile
                .virtual_opcode(self.frames[f].program.raw(), crate::staged::STORE_LOCAL_PLAIN_ROW as usize);
        }
        run
    }

    /// The value a read `i` wrote to its result register, stored to `this` through the fused field sink of its
    /// nested lookup (the sink's cache site, under the frame's field source).
    fn store_field_sink(&mut self, p: &ResidualProgram, f: usize, i: WideInstruction) -> Result<(), JsError> {
        let lookup = i.field_lookup().ok_or_else(|| JsError::validation("invalid nested field lookup encoding".into()))?;
        let crate::bytecode::FieldLookup::Site(site) = lookup else {
            unreachable!("field sink is only attached to nested field lookup")
        };
        let sink = p.field_sites[site].sink.expect("fused field sink");
        let value = self.read(f, i.result_register());
        let strict = p.function(self.frames[f].function as usize).strict;
        self.with_field_source(f, site, crate::bytecode::FieldPart::Sink, |vm| vm.set_field_cached(
            p, vm.frames[f].this, sink.0, value, sink.1, strict))
    }

    #[inline(never)]
    fn binary_miss(&mut self, p: &ResidualProgram, f: usize, i: WideInstruction, _pc: &mut usize) -> Result<StepResult, JsError> {
        let operator = i.binary_operator();
        let left_operand = i.operand_b();
        let right_operand = i.operand_c();
        self.profile
            .binary(operator as usize, left_operand.0, right_operand.0);
        let left = self.resolve_operand(p, f, left_operand)?;
        let right = self.resolve_operand(p, f, right_operand)?;
        #[cfg(feature = "profile-aggregate")]
        {
            let integer = self.numeric_integer_binary(operator, left, right).is_some();
            self.profile.regional_binary(
                self.frames[f].program.raw(),
                self.frames[f].function,
                (*pc - 1) as u32,
                integer,
            );
            if !crate::staged::binary(i.op()) {
                self.profile.numeric_binary_path(
                    operator as usize,
                    integer,
                    left.as_int().is_some() && right.as_int().is_some(),
                );
            }
        }
        let v = match self.number_binary(operator, left, right) {
            Some(value) => value,
            None => self.binary(p, operator, left, right)?,
        };
        if i.returns_from_frame() {
            return Ok(StepResult::Return(v));
        }
        if let Some(local) = i.numeric_local_target() {
            self.frames[f].locals[local as usize] = v;
            self.profile
                .virtual_opcode(self.frames[f].program.raw(), crate::staged::STORE_LOCAL_PLAIN_ROW as usize);
        } else {
            self.write(f, i.result_register(), v);
        }
        Ok(StepResult::Continue)
    }

    #[inline(never)]
    fn unary_miss(&mut self, p: &ResidualProgram, f: usize, i: WideInstruction) -> Result<StepResult, JsError> {
        let v = self.unary(p, i.unary_operator(), self.read(f, i.register_b()))?;
        if i.returns_from_frame() {
            return Ok(StepResult::Return(v));
        }
        self.write(f, i.result_register(), v);
        Ok(StepResult::Continue)
    }

    #[inline(never)]
    fn jump_binary_false_miss(&mut self, p: &ResidualProgram, f: usize, i: WideInstruction, pc: &mut usize) -> Result<StepResult, JsError> {
        let operator = i.binary_operator_field();
        let left_operand = i.operand_b();
        let right_operand = i.operand_c();
        self.profile
            .binary(operator as usize, left_operand.0, right_operand.0);
        let left = self.resolve_operand(p, f, left_operand)?;
        let right = self.resolve_operand(p, f, right_operand)?;
        if !self.binary_truthy(p, operator, left, right)? {
            *pc = i.jump_target() as usize;
        }
        Ok(StepResult::Continue)
    }

    /// An opcode with no arm of its own in `step`, run as its catalog runner (`RUNNERS`): one table
    /// load, then one match.
    #[inline(never)]
    fn step_unlisted(&mut self, p: &ResidualProgram, f: usize, i: WideInstruction, pc: &mut usize, allow_inline_calls: bool) -> Result<StepResult, JsError> {
        match RUNNERS[i.op() as usize] {
            Runner::Nop => Ok(StepResult::Continue),
            Runner::WideMarker => unreachable!("validated dispatch cannot contain nested wide instruction"),
            Runner::RegisterRow => self.register_row_step(p, f, i, pc, allow_inline_calls),
            // A suspension runs its suspension row on the frame (`staged_suspend`).
            Runner::Suspend => {
                let site = super::call_out::CallSite { owner: VarOwner::Frame(f), function: self.frames[f].function, resume: *pc };
                self.staged_suspend(p, site, i, pc)
            }
            // A row that decides its opcode alone runs as its staged row on the frame, which never declines.
            Runner::RowOnly => match self.step_row_of(p, f, i, pc, allow_inline_calls, crate::staged::DecodeContext::EMPTY) {
                RowRun::Done(_) => Ok(StepResult::Continue),
                RowRun::Stop(stop) => stop,
                RowRun::Declined => unreachable!("a row-only opcode's staged row decides it"),
            },
            // A call-family record: its staged row first, then the call's general form on the running frame (`call_forms`).
            Runner::Call => {
                let form = crate::staged::invoke::general_call_form(i.op()).expect("a call runner names its form");
                match self.step_row_of(p, f, i, pc, allow_inline_calls, crate::staged::DecodeContext::EMPTY) {
                    RowRun::Done(_) => Ok(StepResult::Continue),
                    RowRun::Stop(stop) => stop,
                    RowRun::Declined => match InterpreterFrame::call_row(self as *mut Self, p, f, *pc, allow_inline_calls, i, form) {
                        RowRun::Done(_) => Ok(StepResult::Continue),
                        RowRun::Stop(stop) => stop,
                        RowRun::Declined => unreachable!("the interpreter frame answers every call form"),
                    },
                }
            }
            // An environment local's or a by-name store's owner step, with no row first.
            Runner::Owner => {
                self.owner_step(p, VarOwner::Frame(f), i, *pc)?;
                Ok(StepResult::Continue)
            }
            // An owner's step: its staged row first, then the owner's own step on a miss.
            Runner::OwnedRow => {
                match self.step_row_of(p, f, i, pc, allow_inline_calls, crate::staged::DecodeContext::EMPTY) {
                    RowRun::Done(_) => return Ok(StepResult::Continue),
                    RowRun::Declined => {}
                    RowRun::Stop(stop) => return stop,
                }
                self.owner_step(p, VarOwner::Frame(f), i, *pc)?;
                Ok(StepResult::Continue)
            }
            // A literal's allocation or element definition: its staged row (`decode_literal`), run completed. Its
            // return flags complete the frame as the record asks. A superinstruction's pair reads its four parts
            // from the program's table.
            Runner::Literal => {
                let parts = crate::staged::super_const_array_family(i.op()).then(|| p.superinstructions[i.superinstruction_index()].wide);
                let (operation, completes) = crate::staged::Operation::<u64, ()>::decode_literal_completed_with(i, parts, ())
                    .expect("a literal row decodes");
                match self.run_row_completed(p, f, i, pc, allow_inline_calls, operation, completes) {
                    RowRun::Done(_) => Ok(StepResult::Continue),
                    RowRun::Stop(stop) => stop,
                    RowRun::Declined => unreachable!("the interpreter frame answers every literal row"),
                }
            }
            // A call-out row the interpreter's stage answers in place (`staged_value_step`).
            Runner::Value => {
                let site = super::call_out::CallSite { owner: VarOwner::Frame(f), function: self.frames[f].function, resume: *pc };
                self.staged_value_step(p, site, i)?;
                Ok(StepResult::Continue)
            }
            // A flow row that reads the program's constants (`LoadConst`) decodes over the active program's constants
            // (`DecodeContext::constant_at`), then runs as its row on the frame, which answers every constant load.
            Runner::Flow => {
                let constant_at = |index: usize| self.active_constant(index).map(crate::staged::Constant::untyped);
                let context = crate::staged::DecodeContext { constant_at: Some(&constant_at), ..crate::staged::DecodeContext::EMPTY };
                match crate::staged::Operation::<u64, ()>::decode(i, context) {
                    Some(operation @ crate::staged::Operation::LoadConst { .. }) => {
                        match self.run_row(p, f, i, pc, allow_inline_calls, operation) {
                            RowRun::Done(_) => Ok(StepResult::Continue),
                            RowRun::Stop(stop) => stop,
                            RowRun::Declined => unreachable!("the interpreter frame answers every constant load"),
                        }
                    }
                    Some(_) => self.step_wasm(p, f, i, pc),
                    None => Err(JsError::validation("constant index is outside program".into())),
                }
            }
            Runner::Wasm => self.step_wasm(p, f, i, pc),
        }
    }

    /// A register move, a local slot access or a jump, run as its staged row on the running frame (`run_row`).
    /// The record is decoded once, and that operation is the row run. A jump on a heap value's truthiness is the
    /// VM's, which runs no guest code. A local slot the decode declines (`Operation::of` is `None`) is a Wasm step,
    /// as the general step always took it.
    fn register_row_step(&mut self, p: &ResidualProgram, f: usize, i: WideInstruction, pc: &mut usize, allow_inline_calls: bool) -> Result<StepResult, JsError> {
        let Some(operation @ (crate::staged::Operation::Move { .. } | crate::staged::Operation::LoadLocalPlain { .. }
            | crate::staged::Operation::StoreLocalPlain { .. } | crate::staged::Operation::Jump | crate::staged::Operation::JumpIf { .. }))
            = crate::staged::Operation::<u64, ()>::of(i)
        else {
            return self.step_wasm(p, f, i, pc);
        };
        match self.run_row(p, f, i, pc, allow_inline_calls, operation) {
            RowRun::Done(_) => return Ok(StepResult::Continue),
            RowRun::Stop(stop) => return stop,
            RowRun::Declined => {}
        }
        let crate::staged::Operation::JumpIf { register, taken } = operation else {
            unreachable!("validated bytecode bounds the registers and slots of a register row");
        };
        if self.truthy(self.read(f, register)) == taken {
            *pc = i.jump_target() as usize;
        }
        Ok(StepResult::Continue)
    }

    #[inline(always)]
    pub(super) fn resolve_operand(
        &mut self,
        p: &ResidualProgram,
        frame: usize,
        operand: Operand,
    ) -> Result<Value, JsError> {
        let tag = operand.tag();
        self.profile.operand(tag as usize);
        match operand.kind() {
            Some(crate::bytecode::OperandKind::Register) => {
                Ok(self.read(frame, operand.payload() as Register))
            }
            Some(crate::bytecode::OperandKind::Constant) => {
                debug_assert_eq!(self.frames[frame].program, self.active_program);
                self.active_constant(operand.payload() as usize)
                    .ok_or_else(|| {
                        JsError::validation("constant operand is outside program".into())
                    })
            }
            Some(crate::bytecode::OperandKind::Field) => {
                self.resolve_field(p, frame, usize::from(operand.payload()))
            }
            Some(crate::bytecode::OperandKind::Local) => {
                let slot = operand.payload() as usize;
                // A plain slot is exactly what `LoadLocalPlain` reads.
                if p.function(self.frames[frame].function as usize).plain_local(p, slot) {
                    return Ok(self.frames[frame].locals[slot]);
                }
                self.load_local_binding(p, VarOwner::Frame(frame), slot, None)
            }
            None => unreachable!("two-bit operand tag"),
        }
    }
    #[inline(always)]
    fn with_field_source<T>(
        &mut self,
        frame: usize,
        site: usize,
        part: crate::bytecode::FieldPart,
        run: impl FnOnce(&mut Self) -> Result<T, JsError>,
    ) -> Result<T, JsError> {
        let previous = self.frames[frame].source_field.replace((site, part));
        let result = run(self);
        self.frames[frame].source_field = previous;
        result
    }

    pub(super) fn resolve_field(
        &mut self,
        p: &ResidualProgram,
        frame: usize,
        site: usize,
    ) -> Result<Value, JsError> {
        let site_id = site;
        let site = p.field_sites[site_id];
        let base = self.resolve_field_base(p, frame, site.base)?;
        let value =
            self.with_field_source(frame, site_id, crate::bytecode::FieldPart::First, |vm| {
                vm.get_field_cached(p, base, site.first.0, site.first.1)
            })?;
        match site.second {
            Some((atom, cache)) => {
                self.with_field_source(frame, site_id, crate::bytecode::FieldPart::Second, |vm| {
                    vm.get_field_cached(p, value, atom, cache)
                })
            }
            None => Ok(value),
        }
    }
}

/// What `step_unlisted` runs an opcode as. `runner_of` decides it once per opcode, in the priority order the
/// catalog tests used to run in, and `RUNNERS` holds the result, so a step reads one table entry and matches it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Runner {
    /// `NO_STEP` (`Nop`): nothing to do.
    Nop,
    /// `NO_STEP` on the wide marker, which validated dispatch never runs.
    WideMarker,
    /// A register move, a local slot access or a jump: its staged row on the running frame (`register_row_step`).
    RegisterRow,
    /// A suspension (`Await`, `Yield`, `YieldStar`): its suspension row (`staged_suspend`).
    Suspend,
    /// A row that decides its opcode alone (`ROW_ONLY`): its staged row on the frame, which never declines.
    RowOnly,
    /// A call-family record (`invoke::general_call_form`): its staged row, then the call's general form on the frame.
    Call,
    /// An owner's own step with no row first (`OWNER_ONLY`, or an owned step whose catalog marks no row): `owner_step`.
    Owner,
    /// An owned step whose staged row runs first, then the owner's own step on a miss.
    OwnedRow,
    /// A literal's allocation or element definition (`Decoder::Literal`): its row, run completed.
    Literal,
    /// A call-out row the interpreter's stage answers in place (`Decoder::Names`, `Generic`, `Property`, `Value`).
    Value,
    /// A flow row that reads the program's constants (`LoadConst`): decoded over the active program's constants,
    /// then run as its row on the frame. A flow row that decodes to another row is a Wasm step.
    Flow,
    /// Any other opcode: a Wasm step (`step_wasm`).
    Wasm,
}

/// The runner of `op`: the catalog attributes `step_unlisted` used to test one by one, in that order.
const fn runner_of(op: Op) -> Runner {
    let traits = op.traits();
    if traits.has(crate::bytecode::Traits::NO_STEP) {
        return if op.is_wide_marker() {
            Runner::WideMarker
        } else {
            Runner::Nop
        };
    }
    // Only these opcodes decode to a register row (`Operation::decode_flow`, `decode_local`).
    if matches!(
        op,
        Op::Move | Op::LoadLocalPlain | Op::StoreLocalPlain | Op::Jump | Op::JumpFalse | Op::JumpTrue
    ) {
        return Runner::RegisterRow;
    }
    if crate::staged::invoke::general_call_form(op).is_some() {
        return Runner::Call;
    }
    if traits.has(crate::bytecode::Traits::SUSPENDS) {
        return Runner::Suspend;
    }
    if traits.has(crate::bytecode::Traits::ROW_ONLY) {
        return Runner::RowOnly;
    }
    if traits.has(crate::bytecode::Traits::OWNER_ONLY) {
        return Runner::Owner;
    }
    if op.owned_step().is_some() {
        return if op.staged().is_some() || traits.has(crate::bytecode::Traits::ROW_HANDLER) {
            Runner::OwnedRow
        } else {
            Runner::Owner
        };
    }
    match op.staged() {
        Some(crate::bytecode::Decoder::Literal) => Runner::Literal,
        Some(
            crate::bytecode::Decoder::Names
            | crate::bytecode::Decoder::Generic
            | crate::bytecode::Decoder::Property
            | crate::bytecode::Decoder::Value,
        ) => Runner::Value,
        Some(crate::bytecode::Decoder::Flow) => Runner::Flow,
        _ => Runner::Wasm,
    }
}

/// `runner_of` for every opcode, indexed by the opcode's catalog index.
const RUNNERS: [Runner; Op::COUNT] = {
    let mut table = [Runner::Wasm; Op::COUNT];
    let mut index = 0;
    while index < Op::COUNT {
        table[index] = match Op::from_index(index) {
            Some(op) => runner_of(op),
            None => panic!("catalog index is below the opcode count"),
        };
        index += 1;
    }
    table
};

impl<H: Host> Vm<H> {
    /// The frame call-like instruction `i` of frame `caller` pushes, as its step pushes it:
    /// callee, argument window, receiver and completion. `getter` and `this` are what the step
    /// found running: a getter read's function, and a construction's or getter's receiver.
    /// Frame recovery (`compiled::recover_call`) rebuilds a native activation's frame from it.
    #[cfg(feature = "native-execution")]
    pub(super) fn pushed_frame(&self, caller: usize, i: WideInstruction, getter: Value, this: Value)
        -> (Value, crate::bytecode::RegisterWindow, Value, CallCompletion) {
        if crate::staged::Operation::<u64, ()>::is_construction(i) {
            let crate::bytecode::ConstructArguments::Registers(window) = i.construct_arguments() else {
                unreachable!("a pushed construction passes registers")
            };
            (self.read(caller, i.register_b()), window, this, CallCompletion::Constructor(this))
        } else if crate::staged::Operation::<u64, ()>::is_field_read(i) {
            // A getter's activation: its receiver bound as passed, no arguments (`general_getter`).
            (getter, crate::bytecode::RegisterWindow { base: 0, count: 0 }, this, CallCompletion::Value)
        } else {
            (self.read(caller, i.register_b()), i.call_window(), self.read(caller, i.register_c()), CallCompletion::Value)
        }
    }
}

impl<H: Host> Vm<H> {
    /// The new array of the `count` constants from `start` of `program` (a `MakeConstArray`
    /// record's immediate and element count); `None` for a range outside the program's
    /// constants.
    #[inline]
    pub(super) fn const_array_value(&mut self, program: ProgramId, start: usize, count: usize) -> Option<Value> {
        let elements = self.programs.const_array(program, start, count)?;
        let prototype = self.array_prototype_for_realm(self.realm.globals);
        Some(self.heap.alloc_fresh_array(prototype, elements))
    }
}

/// Whether an async body that may suspend, which `CallPlan::settles_in_loop` leaves out, is
/// pushed in place as a settling call too: the general loop's `Await` of its frame makes the
/// continuation and returns its promise to the caller (`run_general_loop`), as
/// `call_user_maybe_async` does for a call it runs apart. The lane keeps activating only the
/// bodies that never suspend.
fn suspends_in_loop(body: &crate::bytecode::Function) -> bool {
    body.is_async
        && !body.is_generator
        && !body.is_class_constructor
        && !body.derived_constructor
        && body.suspends()
}
