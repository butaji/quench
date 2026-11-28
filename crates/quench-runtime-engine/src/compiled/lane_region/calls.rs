use super::*;

impl Lowering<'_, '_> {
    /// The inlined call's receiver admitted where no dominating proof covers it: the body's own
    /// receiver recovery, a guard leaving at `exit`, but for a restartable numeric callee.
    fn guard_inlined_receiver(&mut self, state: &mut State, frame: usize, exit: ir::Block) {
        let inlined = self.unit.frames[frame];
        // A kept receiver is a direct object of its plan's class: the guard is answered without reading it.
        if self.is_kept(state, inlined.receiver) { return; }
        let receiver = self.tagged(state, inlined.receiver);
        let word = self.builder.func.dfg.resolve_aliases(receiver);
        // A numeric body has no entry facts: only a dominating proof admits its receiver.
        let entry = self.entry.as_ref();
        let admitted = entry.is_some_and(|entry| entry.call_receivers.contains(frame))
            || state.proofs.headers.iter().any(|(known, ..)| *known == word)
            || entry.is_some_and(|entry| entry.records[usize::from(inlined.receiver)].is_some()
                || (state.proofs.receivers.contains(&inlined.receiver) && entry.records[state.values.len()].is_some()));
        let callee = self.program.function(inlined.target.function as usize);
        let numeric = inlined.recovery == super::lane_unit::CallRecovery::RestartCall
            && restartable_numeric_function(self.program, callee, callee.instructions());
        if !admitted && !numeric {
            let admitted = self.receiver_recovery(receiver, ReceiverRecovery::for_body(callee));
            emit_js_guard(&mut self.builder, admitted, exit);
        }
    }

    /// The call marker of inlined `frame` (`lane_unit::unit`): its callee is the function
    /// the call's site linked (an identity, compared) or a closure of the statically bound
    /// function (admitted by the VM's binding rule), and its receiver a direct object, which
    /// a plain call binds as `this`; a callee other than this frame's goes to the next
    /// alternative, and anything else leaves at the call, before its effects. The callee's
    /// parameters take the call's arguments and its other registers `undefined`, as its
    /// activation would (`push_passed_in_lane`).
    pub(super) fn inline_call(&mut self, state: &mut State, pc: usize, frame: usize) -> bool {
        let inlined = self.unit.frames[frame];
        let exit = self.guard_exit(state, pc as u32);
        let callee = self.tagged(state, inlined.callee_register);
        let callee_index = usize::from(inlined.callee_register);
        let (same, proved) = match inlined.target.callee {
            // A dominating proof of the identity needs no compare.
            CalleeBinding::Identity(identity) | CalleeBinding::Forwarded { callee: identity, .. } if state.values[callee_index].value.ty.function_identity() == Some(identity)
                || self.entry.as_ref().is_some_and(|entry| entry.calls.contains(frame)) =>
                    (None, Type::function(identity)),
            CalleeBinding::Identity(identity) | CalleeBinding::Forwarded { callee: identity, .. } => (
                Some(self.is_identity(callee, identity)),
                Type::function(identity),
            ),
            CalleeBinding::Function => (
                if state.values[callee_index].value.ty.body_index() == Some(inlined.target.function)
                    || self.entry.as_ref().is_some_and(|entry| entry.calls.contains(frame))
                    || self.plan_call(state, exit, frame) { None }
                    else { Some(self.function_binding(callee, inlined.target.function)) },
                Some(Type::function_body(inlined.target.function)),
            ),
        };
        if let Some(same) = same {
            match inlined.alternative {
                Some(next) => {
                    let (matched, other) = (self.builder.create_block(), self.builder.create_block());
                    self.branch_if(same, (matched, &[]), (other, &[]));
                    self.builder.switch_to_block(other);
                    let mut other_state = state.clone();
                    self.continue_at(&mut other_state, next);
                    self.builder.switch_to_block(matched);
                }
                None => emit_js_guard(&mut self.builder, same, exit),
            }
        }
        if let Some(ty) = proved {
            state.values[callee_index] = RegisterFact::admit(Fact {
                ty, representation: repr(ty),
                binding: BindingTime::Location, proof: Proof::EntryGuard, projection: Projection::Exact,
            }).expect("an admitted callee and its dominating check");
        }
        let callee = self.program.function(inlined.target.function as usize);
        self.guard_inlined_receiver(state, frame, exit);
        for register in 0..inlined.registers {
            let destination = inlined.base + register;
            let undefined = self.raw_const(Lane::I64, Value::UNDEFINED.to_bits() as i64);
            self.set_tagged(state, destination, undefined);
        }
        for (destination, source) in inlined.passed_registers(callee) {
            // A kept receiver passed as an argument becomes an alias of the callee's parameter (`alias_kept`).
            if !self.alias_kept(state, destination, source) { self.copy(state, destination, source); }
        }
        true
    }

    /// Nullish receivers need no boxing only when the body ignores `this`.
    /// All other primitives retain interpreter recovery's boxing semantics.
    pub(super) fn receiver_recovery(&mut self, receiver: ir::Value, recovery: ReceiverRecovery) -> ir::Value {
        let object = quench_runtime::staged::value::is_direct_object(&mut super::object_slots::emit(&mut self.builder), receiver);
        match recovery {
            ReceiverRecovery::Object => object,
            ReceiverRecovery::ObjectOrNullish => {
                let undefined = self.raw_compare_imm(Relation::Equal, receiver, Value::UNDEFINED.to_bits() as i64);
                let null = self.raw_compare_imm(Relation::Equal, receiver, Value::NULL.to_bits() as i64);
                let nullish = self.raw_binary(Binary::Or, undefined, null);
                self.raw_binary(Binary::Or, object, nullish)
            }
        }
    }

    /// A call or a construction the body does not inline, by the staged call (`staged::invoke`):
    /// the body's registers written back, then a direct call of a compiled callee or the VM's
    /// call; its completion continues, a leave in the callee stops the body, and a miss leaves
    /// at `pc`. The callee may allocate, which may move the property arena, whose base and
    /// length the body reloads.
    pub(super) fn call(&mut self, state: &mut State, pc: usize, instruction: WideInstruction) -> bool {
        use quench_runtime::staged::{CallOperation, Operation};
        let decoded = Operation::<ir::Value, EmitProof>::decode(instruction, quench_runtime::staged::DecodeContext::of(self.program));
        // A callee's exception must go where native code takes it (`Lowering::raises_from`): a call
        // it cannot unwind from is made by the lane.
        let origin = self.unit.origins.get(pc).copied().filter(|_| self.raises_from(pc));
        let (Some(super::lane_unit::Origin::Caller(record)), Some(Operation::Call(CallOperation::Site(site)))) = (origin, decoded) else {
            if quench_runtime::trace::Channel::AotLanes.on() {
                eprintln!("aot-lanes: function {} call at unit pc {pc} leaves: origin {:?}, raises from {}, decoded as a call site {}", self.function_id,
                    self.unit.origins.get(pc), self.raises_from(pc), matches!(decoded, Some(Operation::Call(CallOperation::Site(_)))));
            }
            self.exit(state, pc as u32);
            return false;
        };
        let missed = self.guard_exit(state, pc as u32);
        self.home_written(state, pc);
        // A proper tail call of the function's own body is a back edge to its entry (`self_tail_call`), not a call.
        if let Some((callee, miss)) = self.self_tail_callee(pc, record, &site) {
            return self.self_tail_call(state, pc, missed, &site, callee, miss);
        }
        let completed = self.builder.create_block();
        let raised = cold_block(&mut self.builder);
        let entered = self.builder.create_block();
        // The callee's exception block is filled before the call's blocks, from the state the call is made in.
        self.goto(entered, &[]);
        self.raised_call(state, pc, record, raised);
        self.builder.switch_to_block(entered);
        // SAFETY: the body owns its callees for as long as its code runs.
        let callees: Vec<&DirectCallee> = self.direct.get(&pc).map(|sites| sites.iter().map(|callee| unsafe { &**callee }).collect()).unwrap_or_default();
        // An image's call the closed world proves reads its callee's words from the site's record (`ProvenCall`): a
        // construction takes the VM's call, as the proven callee is not a construction's.
        let arms: &[ProvenCall] = self.vm.ahead.and_then(|ahead| ahead.calls.get(&pc)).filter(|_| !site.constructs()).map_or(&[], Vec::as_slice);
        let proven = arms.first().copied().filter(|_| arms.len() == 1);
        // A direct call: its result is the solve's join over the callees it may reach.
        let direct = !callees.is_empty() || proven.is_some();
        // The declared result the call's completion conforms to: the callee's, where every candidate declares the same one.
        let returns = callees.iter().filter(|_| !site.constructs())
            .map(|callee| declared::declared_return(self.program, callee.target.function))
            .reduce(|first, next| if first == next { first } else { None }).flatten();
        // The result the solve proves a direct call defines (`proven_result`): its native return continues at `returned` with
        // the word, and the helper's completion tests it (`typed_completion`). A numeric region leaves at its calls.
        let mut typed = None;
        match site.callee {
            quench_runtime::staged::forms::Callee::Register(register) if callees.len() >= 2 && !site.constructs() =>
                self.polymorphic_call(state, site, record, register, &callees, (completed, missed, raised)),
            quench_runtime::staged::forms::Callee::Register(register) if callees.is_empty() && arms.len() >= 2 =>
                self.closed_set_call(state, site, record, register, pc, arms, (completed, missed, raised)),
            _ => {
                let owned = callees.first().copied();
                let callee = owned.map(DirectSite::Owned).or(proven.map(DirectSite::Proven));
                if callee.is_some() && !site.returns && !site.constructs() && !self.completion.numeric()
                    && let Some(ty) = self.proven_result(pc, site.result).filter(|ty| self.typed_native_result(pc, *ty)) {
                    let returned = self.builder.create_block();
                    self.builder.append_block_param(returned, types::I64);
                    typed = Some((ty, returned));
                }
                let generic = self.builder.create_block();
                // A construction of a stores-only constructor, whose arguments are a window of the caller's
                // registers (`CallSite::initializing`): its receiver is allocated finished and its body not entered.
                let initializing = match (site.arguments, owned) {
                    // A forwarding site's target is the forwarded initializer, whose receiver its forwarder's plan
                    // allocates (`DirectCallee::forwarder`): that path keeps its body.
                    (quench_runtime::staged::Arguments::Window { first, count }, Some(callee))
                        if site.constructs() && callee.forwarder.is_none() && self.initializes_on_construction(callee.target.function) => Some((first, count)),
                    _ => None,
                };
                let returned = typed.map(|(_, returned)| returned);
                // A sibling tail call of the one body the site calls (`sibling_tail_arguments`): its frame is the body's own.
                let tail = owned.filter(|_| proven.is_none() && callees.len() == 1)
                    .and_then(|callee| self.sibling_tail_arguments(pc, record, &site, callee));
                let call = CallSite { callee, record, context: self.context, registers: self.registers_base, completed, returned, missed, raised,
                    stop: self.call_out_stop(), leave: self.leave_site(), initializing, tail, construct: site.constructs().then(|| self.construct_class(pc, match site.callee { quench_runtime::staged::forms::Callee::Register(register) => Some(register), _ => None })).flatten() };
                self.step_with(state, frame::Edges::ValueGuarded { miss: generic }, Operation::Call(CallOperation::Site(site)), StepWith { site: Some(Site::Call(call)), ..StepWith::default() });
            }
        }
        self.reentries.push(completed);
        // A returning call's completion is its activation's: the lane completes it with the
        // result, which the call left in its register (`LaneExit::Returned`).
        if site.returns {
            self.reload_result(state, site.result);
            return self.return_register(state, record, site.result);
        }
        state.call_effect();
        // A direct call whose result the solve proves: the facts assume that type after the call. The helper's completion
        // checks it on its own path; the native return already holds it.
        let typed = typed.map(|(ty, returned)| (ty, self.typed_completion(state, pc, site.result, ty, returned)));
        self.reload_property_arena();
        match typed {
            Some((ty, value)) => self.define_typed_result(state, site.result, value, ty),
            None => self.reload_result(state, site.result),
        }
        if let Some(ty) = returns { self.conform_declared_result(state, pc, site.result, ty); }
        // Any other call's result is checked once where the call completes, whichever path completed it.
        if typed.is_none() && direct && !self.completion.numeric() && let Some(ty) = self.proven_result(pc, site.result) { self.conform_declared_result(state, pc, site.result, ty); }
        true
    }

    /// Whether a direct call's completion takes the result `ty` its callee's summary types without a test on the native
    /// return (`typed_completion`): the resumed point is a completion, and the fact is a Number or a tagged type the
    /// completion's test checks by its tag. An int32 keeps its test, as an integral double is a Number and not its tag.
    fn typed_native_result(&self, pc: usize, ty: Type) -> bool {
        self.resumes_after(pc) && ty.object_shape().is_none() && ty != Type::DIRECT_OBJECT_FAMILY
            && (number_domains::is_double(ty) || number_domains::target_representation(ty) == Representation::Tagged)
    }

    /// Joins a direct call's native return and its helper's completion, both continuing with the call's result in the
    /// representation `ty` takes. The native return's word is the callee's, which its summary types, so it is only
    /// decoded; the helper's completion tests the result on its own path, from a copy of the state. Continues in the join,
    /// whose parameter is the result's value.
    pub(super) fn typed_completion(&mut self, state: &State, pc: usize, result: Register, ty: Type, returned: ir::Block) -> ir::Value {
        let float = number_domains::target_representation(ty) == Representation::Float64;
        let join = self.builder.create_block();
        let param = self.builder.append_block_param(join, if float { types::F64 } else { types::I64 });
        let base = self.registers_base;
        let bits = self.raw_load(Lane::I64, regions::frame(), base, register_offset(result));
        let mut helper = state.clone();
        helper.call_effect();
        self.set_tagged(&mut helper, result, bits);
        self.conform_register(&mut helper, pc + 1, result, ty);
        let helper_value = if float {
            let variable = self.float_var(usize::from(result));
            self.builder.use_var(variable)
        } else { bits };
        self.goto(join, &[helper_value]);
        self.reentries.push(returned);
        self.builder.switch_to_block(returned);
        let word = self.builder.block_params(returned)[0];
        let native = if float { emit_known_number(&mut self.builder, word) } else { word };
        self.goto(join, &[native]);
        self.builder.switch_to_block(join);
        param
    }

    /// Defines a direct call's result `value` as its typed fact `ty` (`typed_completion`).
    fn define_typed_result(&mut self, state: &mut State, result: Register, value: ir::Value, ty: Type) {
        let representation = number_domains::target_representation(ty);
        self.define(state, result, value, RegisterFact::definition(ty, representation));
        state.proofs.homed.insert(usize::from(result));
    }

    /// A call of a register callee that one of `callees` may be (several direct candidates of the
    /// site): a chain of tests on the callee register, each leading to its candidate's direct call,
    /// and the last to a call by the VM alone. A test's candidate is the one its step guards again,
    /// which holds. Each step gets its own miss block, which its VM call fills (`CallFrame::call_helper`
    /// appends the call to the block it is given, so blocks are not shared). The steps share `completed`,
    /// `missed` and `raised`, and the chain ends at `completed`.
    pub(super) fn polymorphic_call(&mut self, state: &mut State, site: quench_runtime::staged::Invocation, record: u32, register: Register,
        callees: &[&DirectCallee], (completed, missed, raised): (ir::Block, ir::Block, ir::Block)) {
        use quench_runtime::staged::{CallOperation, Operation};
        let (stop, leave, context, registers) = (self.call_out_stop(), self.leave_site(), self.context, self.registers_base);
        // The callee register is read once: the steps write nothing before a test runs.
        let word = self.raw_load(Lane::I64, regions::frame(), self.registers_base, register_offset(register));
        let tests: Vec<ir::Block> = callees.iter().map(|_| self.builder.create_block()).collect();
        let fallback = self.builder.create_block();
        self.goto(tests[0], &[]);
        for (index, &callee) in callees.iter().enumerate() {
            self.builder.switch_to_block(tests[index]);
            let arm = self.builder.create_block();
            let next = tests.get(index + 1).copied().unwrap_or(fallback);
            let matched = self.raw_compare_imm(Relation::Equal, word, callee.callee.to_bits() as i64);
            self.branch_if(matched, (arm, &[]), (next, &[]));
            self.builder.switch_to_block(arm);
            let miss = cold_block(&mut self.builder);
            let call = CallSite { callee: Some(DirectSite::Owned(callee)), record, context, registers, completed, returned: None, missed, raised, stop, leave, initializing: None, tail: None, construct: None };
            self.step_with(state, frame::Edges::ValueGuarded { miss }, Operation::Call(CallOperation::Site(site)), StepWith { site: Some(Site::Call(call)), ..StepWith::default() });
        }
        // No candidate's identity: the VM's call alone, as a site with no direct entry makes it.
        self.builder.switch_to_block(fallback);
        let miss = cold_block(&mut self.builder);
        let call = CallSite { callee: None, record, context, registers, completed, returned: None, missed, raised, stop, leave, initializing: None, tail: None, construct: None };
        self.step_with(state, frame::Edges::ValueGuarded { miss }, Operation::Call(CallOperation::Site(site)), StepWith { site: Some(Site::Call(call)), ..StepWith::default() });
    }
}
