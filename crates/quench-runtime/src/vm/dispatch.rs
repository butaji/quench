// What `Vm::step` runs an opcode as: its one arm is the wildcard, `step_unlisted`, which runs each opcode as its
// catalog runner (`RUNNERS`, decided once per opcode by `runner_of`). `step` keeps
// `#[quench_macros::operation_semantics]`: `resolve_step_operation` (compiled.rs) selects each opcode's semantic
// from it, and the attribute takes only a `match` on the opcode, so that match holds the wildcard alone. (The
// attribute rejects match guards.) The runners, in `runner_of`'s priority order:
// - `NO_STEP` (Nop; the wide marker is never dispatched).
// - `REGISTER_ROW` (Move, LoadLocalPlain, StoreLocalPlain, Jump, JumpFalse, JumpTrue): `register_row_step`.
// - A call-family record (`staged::invoke::general_call_form`: Call, CallArray, CallSpread, CallForwardingArguments,
//   CallKnown, CallMethod, CallThisMethod, Construct, CallFirstChunk): its staged row, then the call's general form
//   on the frame (`call_forms.rs`, `InterpreterFrame::call_row`).
// - `SUSPENDS` (Await, Yield, YieldStar): `staged_suspend`.
// - `ROW_ONLY`: a staged row that decides its opcode on the frame alone, which never declines: GetField, GetIndex,
//   SetField, SetFieldStrict, SetIndex, Unary, JumpBinaryFalse, WasmBranchTable, IteratorCleanupPush,
//   IteratorCleanupPop, ForInNext, GetAsyncIterator, StringCaseIndex, CacheTemplateObject, LoadCachedTemplateObject,
//   ToNumeric, IncDec, Return, Throw, SetThisField, SetThisFieldStrict. A read or store its plan leaves is answered by
//   the frame's `CallOut::LoadField`, `LoadElement`, `StoreField` or `StoreElement`; a read that writes `this` stores
//   its value through `store_field_sink`. A branch table's edge is an entry (`Edge::Entry`), which `run_row` takes.
// - `ARITHMETIC_BINARY` (Binary, NumericAdd, NumericMultiply): the staged row, then the record's general `Binary` on
//   a miss (`run_row_completed`). The Number relation of `==` or `===` is that miss, which `Binary` answers.
// - `OWNER_ONLY` (LoadEnvLocal, StoreEnvLocal, StoreName): `owner_step` alone.
// - An owned step (`Op::owned_step`) whose catalog marks a row (InitializeTdz): its row first, then `owner_step` on a
//   miss; an owned step with no row: `owner_step` alone.
// - `Decoder::Literal` (`decode_literal`, run completed on the frame), the Names, Generic, Property and Value
//   call-outs (`staged_value_step`), `Decoder::Flow` (`LoadConst`, over the active program's constants), and any
//   other opcode: a Wasm step (`step_wasm`).

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

    /// Whether the dispatch loop may not push a call made by frame `f` as an activation of its
    /// own: an eval is in progress, `p` is a Wasm program (its calls keep Wasm's own activation
    /// and exhaustion accounting), `f` is not the top frame, `f` is the root activation, or the
    /// call is a strict terminal one that the host makes a proper tail call (`call_forms`).
    #[inline(always)]
    fn inline_call_blocked(&self, p: &ResidualProgram, f: usize, i: WideInstruction) -> bool {
        self.direct_eval
            || self.parameter_eval
            || p.kind == crate::bytecode::ProgramKind::Wasm
            || f + 1 != self.frames.len()
            || self.frames[f].function == super::ROOT_FUNCTION_ID
            || (self.host.proper_tail_calls()
                && i.returns_from_frame()
                && p.function(self.frames[f].function as usize).strict)
    }

    /// Whether the call at `pc` made by frame `f` is a JavaScript proper tail call: the host
    /// permits them, the call returns from its frame, and the call site permits it.
    #[inline(always)]
    pub(super) fn js_tail_call_allowed(&self, p: &ResidualProgram, f: usize, i: WideInstruction, pc: usize) -> bool {
        self.host.proper_tail_calls()
            && i.returns_from_frame()
            && p.function(self.frames[f].function as usize).permits_js_tail_call_at(pc.saturating_sub(1) as u32)
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
        if !crate::staged::calls_ordinary(i.op()) || i.direct_eval() || self.inline_call_blocked(p, f, i) {
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
        if i.is_super_construct() {
            return self.try_inline_super_construct(p, f, i, pc);
        }
        let crate::bytecode::ConstructArguments::Registers(window) = i.construct_arguments() else { return None };
        let callee = self.read(f, i.register_b());
        let arguments: *const [Value] = self.register_window(f, window);
        if let Some((program, id, env, this)) = self.inline_construct_target(p, f, callee) {
            // SAFETY: see `push_general_user_frame`; the window outlives callee setup.
            return self.push_in_loop_activation(p, f, pc, (program, id, env), callee, this, unsafe { &*arguments },
                i.result_register(), CallCompletion::Constructor(this));
        }
        // A derived class constructor: its frame binds `this` at `super(...)`, and its pending
        // call applies the derived return rules, as `construct_value` would around a nested run.
        let (program, id, env) = self.inline_derived_construct_target(p, f, callee)?;
        // SAFETY: as above.
        self.push_in_loop_activation(p, f, pc, (program, id, env), callee, Value::UNDEFINED, unsafe { &*arguments },
            i.result_register(), CallCompletion::DerivedConstructor)
    }

    /// `super(...)` of the running constructor pushed in place (`inline_super_construct_target`):
    /// the parent's [[Construct]] with the constructor's `new.target`, a base parent's over the
    /// receiver made from it (`CallCompletion::Constructor`), a derived parent's binding its own
    /// (`CallCompletion::DerivedConstructor`). The construction's value lands in the record's
    /// register, which `SuperCallCheck` and `InitializeThis` then bind, as after
    /// `construct_super_value`. Applied arguments (an implicit constructor's rest list) are
    /// passed where a dense array holds them (`dense_super_arguments`); arguments that reading
    /// would run guest code for, and any other parent, keep that path.
    fn try_inline_super_construct(&mut self, p: &ResidualProgram, f: usize, i: WideInstruction, pc: usize)
        -> Option<Result<StepResult, JsError>> {
        let parent = self.read(f, i.register_b());
        let construction = self.inline_super_construct_target(p, f, parent)?;
        let listed;
        let arguments: *const [Value] = match i.construct_arguments() {
            crate::bytecode::ConstructArguments::Registers(window) => std::ptr::from_ref(self.register_window(f, window)),
            crate::bytecode::ConstructArguments::Array(register) => {
                listed = self.dense_super_arguments(self.read(f, register))?;
                std::ptr::from_ref(listed.as_slice())
            }
        };
        // SAFETY: see `push_general_user_frame`; the window (or list) outlives callee setup.
        self.push_in_loop_construction(p, f, pc, construction.target, (parent, Some(construction.new_target)), construction.this,
            unsafe { &*arguments }, i.result_register(), construction.completion)
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
        let new_target = matches!(completion, CallCompletion::Constructor(_) | CallCompletion::DerivedConstructor).then_some(callee);
        self.push_in_loop_construction(p, f, pc, (program, id, env), (callee, new_target), this, arguments, destination, completion)
    }

    /// `push_in_loop_activation` with [[Construct]]'s `new.target` given: `Some` binds the
    /// construction target (`construct_target`) for the callee's activation to take, as a
    /// `super(...)` pushed in place passes its constructor's `new.target` to the parent's
    /// (`try_inline_super_construct`).
    #[allow(clippy::too_many_arguments)]
    pub(super) fn push_in_loop_construction(
        &mut self,
        p: &ResidualProgram,
        f: usize,
        pc: usize,
        (program, id, env): (ProgramId, u32, Value),
        (callee, new_target): (Value, Option<Value>),
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
        let constructs = new_target.is_some();
        if let Some(new_target) = new_target { self.set_construct_target(Some(ConstructionTarget { callee, new_target })); }
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

    /// The cached target of `callee` for `new` as well as for a call (`InLoopCallTarget`): the
    /// entry of `callee`'s slot, resolved and cached on a miss. Its `push` may be
    /// `InLoopPush::Unpushed`, and its `construct` classifies a construction.
    #[inline(always)]
    pub(super) fn in_loop_entry(&mut self, p: &ResidualProgram, callee: Value) -> Option<super::InLoopCallTarget> {
        let heap_index = callee.heap_index()?;
        let index = heap_index as usize & (super::CALL_TARGET_CACHE_SIZE - 1);
        let cached = self.call_targets[index];
        if cached.callee == callee {
            return Some(cached);
        }
        self.resolve_in_loop_call_target(p, callee, index)
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
        if cached.callee == callee && cached.push != super::InLoopPush::Unpushed {
            #[cfg(feature = "profile-lane-exits")]
            super::lane_exits::call_eligibility(super::lane_exits::CallEligibility::PositiveCacheHit);
            return Some(cached);
        }
        let target = self
            .resolve_in_loop_call_target(p, callee, index)
            .filter(|target| target.push != super::InLoopPush::Unpushed);
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
            super::InLoopPush::Unpushed
        };
        // A call-only closure (`Value::known_closure`) has no heap record and is never constructed.
        let construct = if self.heap.function(callee).is_some() {
            Self::construct_kind(body)
        } else {
            super::ConstructKind::No
        };
        let target = super::InLoopCallTarget {
            callee,
            program,
            function,
            env,
            push,
            construct,
        };
        // A callee that neither a call nor a construction may push is not cached, so plain calls
        // of it leave the slots they had.
        if push != super::InLoopPush::Unpushed || construct != super::ConstructKind::No {
            self.call_targets[index] = target;
        }
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
        if !crate::staged::calls_positional(i.op()) || self.inline_call_blocked(p, f, i) {
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
                    bound_arguments = super::function::bound_then(&bound, window);
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
            _ => return self.step_unlisted(p, f, i, pc, allow_inline_calls),
        }
    }

    /// Runs `operation`, decoded from `i` at resume PC `pc`, as its staged row on the running frame
    /// `f` (`InterpreterFrame::step_row`). A taken jump sets `pc`, and a back-edge jump publishes its
    /// PC and checks for a due collection.
    #[inline(never)]
    fn run_row(&mut self, p: &ResidualProgram, f: usize, i: WideInstruction, pc: &mut usize, allow_inline_calls: bool,
        operation: crate::staged::Operation<u64, ()>) -> RowRun {
        use crate::staged::{Edge, Operation};
        if !self.row_admitted(p, f, operation) {
            return RowRun::Declined;
        }
        let run = InterpreterFrame::step_row(self as *mut Self, p, f, *pc, allow_inline_calls, i, operation);
        // A branch table's entry: `pc` names the table's first entry, and the entry its selector picks lies that far on
        // (`branch_table_case`).
        if let RowRun::Done(Edge::Entry(entry)) = run {
            *pc += entry as usize;
        }
        if matches!(run, RowRun::Done(Edge::Target)) {
            *pc = i.jump_target() as usize;
            if matches!(operation, Operation::Jump) {
                // The target is the next instruction to run, so a collection scans the roots
                // live before it: a resume PC would name the instruction before the target,
                // an unrelated one whose liveness can drop registers the target reads.
                self.frames[f].pc = *pc;
                self.frames[f].pc_is_resume = false;
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
        if i.writes_current_this() && !i.op().traits().has(crate::bytecode::Traits::FIELD_LEAF) {
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
        // Every decoder the row takes (`decode_row`: the general one, a name load, the lane's), over the record without its flags.
        let (plain, flags) = crate::staged::Operation::<u64, ()>::completion_split(i)?;
        Self::decode_row(plain, context).map(|operation| (operation, flags))
    }

    /// Runs `operation` as its staged row, then completes the frame as the record asks: a return of the
    /// value the row wrote to its result register, or the numeric local the result writes.
    #[inline(always)]
    fn run_row_completed(&mut self, p: &ResidualProgram, f: usize, i: WideInstruction, pc: &mut usize, allow_inline_calls: bool,
        operation: crate::staged::Operation<u64, ()>, completes: crate::bytecode::Register) -> RowRun {
        // A numeric local target names a local slot, not a register: the plain row writes its result to the
        // register of that index, which holds another value (a hoisted closure in TypeScript's checker), so the
        // register is put back once the local has the result.
        let kept = i.numeric_local_target().map(|local| self.read(f, local));
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
            if let Some(kept) = kept { self.write(f, local, kept); }
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
            // Decoded over the program's tables: a row-only record that names a site or constant (a template, a method
            // site) decodes only with them.
            Runner::RowOnly => match self.step_row_of(p, f, i, pc, allow_inline_calls, crate::staged::DecodeContext::of(p)) {
                RowRun::Done(_) => Ok(StepResult::Continue),
                RowRun::Stop(stop) => stop,
                // A unary's row leaves where its words decide nothing the frame proves (an immediate no tag class
                // names, a Number row's miss); the VM's general unary (`Vm::unary`) answers every operand.
                RowRun::Declined if i.op() == crate::bytecode::Op::Unary => {
                    let value = self.unary(p, i.unary_operator(), self.read(f, i.register_b()))?;
                    self.write(f, i.result_register(), value);
                    if i.result_flags() & crate::bytecode::RETURN_REGISTER != 0 {
                        return Ok(StepResult::Return(value));
                    }
                    Ok(StepResult::Continue)
                }
                // A named read whose row leaves (a form its decode or plan does not take) is the VM's named read
                // (`load_field_step`), completed as the record asks: a `this` write, a return.
                RowRun::Declined if i.op() == crate::bytecode::Op::GetField => {
                    let lookup = i.field_lookup().expect("a named read names its lookup");
                    let value = match self.load_field_step(p, f, *pc, lookup, Some(i.result_register()), allow_inline_calls && !i.writes_current_this())? {
                        super::call_out::Loaded::Value(value) => value,
                        super::call_out::Loaded::Pushed(step) => return Ok(step),
                    };
                    self.write(f, i.result_register(), value);
                    if i.writes_current_this() {
                        self.store_field_sink(p, f, i)?;
                    }
                    if i.result_flags() & crate::bytecode::RETURN_REGISTER != 0 {
                        return Ok(StepResult::Return(value));
                    }
                    Ok(StepResult::Continue)
                }
                RowRun::Declined => unreachable!("a row-only opcode's staged row decides it: {:?} flags {:#x} register a {:?} at pc {}",
                    i.op(), i.result_flags(), self.frames[f].registers.get(usize::from(i.a())).copied(), *pc),
            },
            // An arithmetic binary: its staged row first, then the record's general `Binary` on a miss. The Number
            // relation of `==` or `===` leaves a pair that is no Number, and that relation names both operators, so
            // the general `Binary` of the record (`CallOut::Binary`) answers it.
            Runner::RowThenGeneralBinary => match self.step_row_of(p, f, i, pc, allow_inline_calls, crate::staged::DecodeContext::EMPTY) {
                RowRun::Done(_) => Ok(StepResult::Continue),
                RowRun::Stop(stop) => stop,
                RowRun::Declined => {
                    let operation = crate::staged::Operation::<u64, ()>::Binary {
                        destination: i.result_register(), left: i.operand_b(), right: i.operand_c(), operator: i.binary_operator(),
                    };
                    let completes = i.result_flags() & crate::bytecode::RETURN_REGISTER;
                    match self.run_row_completed(p, f, i, pc, allow_inline_calls, operation, completes) {
                        RowRun::Done(_) => Ok(StepResult::Continue),
                        RowRun::Stop(stop) => stop,
                        RowRun::Declined => unreachable!("the interpreter frame answers the general binary row"),
                    }
                }
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
                    RowRun::Declined => unreachable!("the interpreter frame answers every literal row: {:?}", i.op()),
                }
            }
            // A call-out row the interpreter's stage answers in place (`staged_value_step`). A record
            // that returns its result (`CallRest` ending an arrow's expression body) completes the
            // frame with it: the compiler emits no `Return` after it.
            Runner::Value => {
                let site = super::call_out::CallSite { owner: VarOwner::Frame(f), function: self.frames[f].function, resume: *pc };
                let (plain, returning) = i.split_return();
                self.staged_value_step(p, site, plain)?;
                if returning {
                    return Ok(StepResult::Return(self.read(f, plain.result_register())));
                }
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
    /// An arithmetic binary (`ARITHMETIC_BINARY`): its staged row, then the record's general `Binary` on a miss.
    RowThenGeneralBinary,
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
    if traits.has(crate::bytecode::Traits::REGISTER_ROW) {
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
    if traits.has(crate::bytecode::Traits::ARITHMETIC_BINARY) {
        return Runner::RowThenGeneralBinary;
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
