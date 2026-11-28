use super::*;
#[allow(unused_imports)] use quench_runtime::staged::glue_prelude::*;

impl CallFrame<Emit<'_, '_>> for EmitFrame<'_> {
    /// A callee's bound `this` is the callee's own fact (`dispatch_fast::receiver_trusted`): the call guards nothing.
    fn call_this_bound(&mut self, _stage: &mut Emit<'_, '_>, _this: ir::Value, _miss: ir::Block) -> Option<()> {
        Some(())
    }

    fn call_entry(&self, site: &Invocation) -> CallEntry {
        self.proven_arguments.set(false);
        self.rebinds.set(false);
        let Some(CallSite { callee: Some(callee), .. }) = self.call_site() else { return CallEntry::Helper };
        // An indirect callee's window is read at run time (`call_registers_dynamic`), so the plan's is unused for it.
        let registers = callee.registers().unwrap_or(0);
        self.proven_arguments.set(self.arguments_proven(site, callee));
        let kind = if site.constructs() { CalleeKind::Class } else { CalleeKind::Script };
        let (identity, sloppy) = match callee {
            DirectSite::Owned(owned) => {
                // A callee register the state already holds as this very function (a Stable binding's
                // read, or an identity an earlier guard proved) needs no check: the identity is folded.
                let word = owned.callee.to_bits();
                let proven = matches!(site.callee, quench_runtime::staged::forms::Callee::Register(register)
                    if self.state.values[usize::from(register)].value.ty.function_identity().map(|identity| identity.to_bits()) == Some(word));
                let identity = if proven { Identity::Folded(word) } else { Identity::Guarded(word) };
                // A guarded call of a JIT plan reads the closure it calls and its captures from the bound words, which
                // its helper call rebinds to another closure of the function (`DirectCallee::bound`).
                self.rebinds.set(!proven && !self.helpers.links() && !site.constructs() && owned.forwarder.is_none());
                if self.rebinds.get() { owned.bound.read.set(true); }
                (identity, owned.sloppy.map(Value::to_bits))
            }
            // The identity and the global are the record's words (`call_identity`, `call_global`): the plan's
            // constants stand for them and are never emitted. The record's identity is checked against a register
            // only, so a call of any other form takes the VM's (its callee is not the record's to check).
            DirectSite::Proven(proven) => {
                if !matches!(site.callee, quench_runtime::staged::forms::Callee::Register(_)) { return CallEntry::Helper; }
                (Identity::Guarded(crate::aot_image::unbound_callee()), proven.sloppy.then(crate::aot_image::unbound_callee))
            }
            // An indirect callee's identity is its record's, which the closed-set guard checked: the register's identity is
            // guarded again here as a proven callee's is, and its sloppiness and window are its record's at run time.
            DirectSite::Indirect { .. } => {
                if !matches!(site.callee, quench_runtime::staged::forms::Callee::Register(_)) { return CallEntry::Helper; }
                (Identity::Guarded(crate::aot_image::unbound_callee()), None)
            }
        };
        let plan = DirectPlan { kind, identity, sloppy, registers, returned: LANE_BODY_RETURNED, bound: None };
        // The callee is the one the site linked, which the call guards: a Site fact, never inlined by the plan.
        plan_entry(Some(Resolved { plan, binding: BindingTime::Site }), false)
    }

    fn call_registers(&mut self, _stage: &mut Emit<'_, '_>) -> Option<ir::Value> {
        Some(self.call_site()?.registers)
    }

    /// The receiver the constructor's plan allocates (`LaneHelper::ConstructReceiver`); a due
    /// collection answers the deleted marker. An initializing construction (`CallSite::initializing`)
    /// is allocated finished from its arguments in the caller's window (`LaneHelper::Construct`), as
    /// `lane_body_construction` makes it: the plan's fields, or the initializer's cached adds. Its
    /// body is not entered (`call_enter`), so the helper is the whole construction.
    fn call_receiver(&mut self, stage: &mut Emit<'_, '_>, constructor: ir::Value) -> Option<ir::Value> {
        if let Some(CallSite { initializing: Some((first, count)), registers, .. }) = self.call_site() {
            let arguments = stage.add_imm(registers, quench_runtime::staged::word_bytes(first));
            let argc = stage.constant(u64::from(count));
            let helper = self.helpers.target(&mut stage.builder, LaneHelper::Construct);
            return Some(emit_lane_helper_call(stage.builder, self.payload, helper, &[self.payload, constructor, arguments, argc]));
        }
        let helper = self.helpers.target(&mut stage.builder, LaneHelper::ConstructReceiver);
        Some(emit_lane_helper_call(stage.builder, self.payload, helper, &[self.payload, constructor]))
    }

    /// An indirect callee's window is its record's words, read at run time.
    fn call_registers_dynamic(&mut self, stage: &mut Emit<'_, '_>) -> Option<ir::Value> {
        match self.call_site()?.callee? {
            callee @ DirectSite::Indirect { .. } => self.callee_word(stage, callee, std::mem::offset_of!(CalleeRecord, registers)),
            DirectSite::Owned(_) | DirectSite::Proven(_) => None,
        }
    }

    /// An indirect callee's sloppiness is its record's word, read at run time.
    fn call_sloppy_dynamic(&mut self, stage: &mut Emit<'_, '_>) -> Option<ir::Value> {
        match self.call_site()?.callee? {
            callee @ DirectSite::Indirect { .. } => self.callee_word(stage, callee, std::mem::offset_of!(CalleeRecord, sloppy)),
            DirectSite::Owned(_) | DirectSite::Proven(_) => None,
        }
    }

    fn call_room(&mut self, stage: &mut Emit<'_, '_>, miss: ir::Block) -> Option<Room<ir::Value>> {
        let callee = self.call_site()?.callee?;
        let cell = match callee {
            DirectSite::Owned(owned) => address(self.helpers, stage.builder, RelocKind::EntryCell, owned.target.function, owned.cell as u64),
            other => self.callee_word(stage, other, std::mem::offset_of!(CalleeRecord, cell))?,
        };
        self.admit_stacks(stage, callee, cell, miss)
    }

    /// The identity an image's proven call checks its callee register against: the record's word, which the binding
    /// makes the callee's identity (`BoundCalls::bind`) and which no callee holds while the record is unbound.
    /// A body's own callee has its identity as the plan's constant, so the frame reads none.
    fn call_identity(&mut self, stage: &mut Emit<'_, '_>) -> Option<ir::Value> {
        match self.call_site()?.callee? {
            DirectSite::Owned(owned) => self.rebinds.get().then(|| self.bound_word(stage, owned, std::mem::offset_of!(BoundClosure, callee))),
            other => self.callee_word(stage, other, std::mem::offset_of!(CalleeRecord, callee)),
        }
    }

    /// The global object a proven sloppy callee binds, from the record's `global_this` (`BoundCalls::bind`).
    fn call_global(&mut self, stage: &mut Emit<'_, '_>) -> Option<ir::Value> {
        match self.call_site()?.callee? {
            DirectSite::Owned(_) => None,
            other => self.callee_word(stage, other, std::mem::offset_of!(CalleeRecord, global_this)),
        }
    }

    /// The call of the entry the callee's cell holds, with the lane context, the receiver, the
    /// callee's captures and its window; both tops stay pushed, which a completion pops and a leave keeps
    /// for the lane to link.
    fn call_enter(&mut self, stage: &mut Emit<'_, '_>, this: ir::Value, room: Room<ir::Value>) -> Option<Entered<ir::Value, ir::Block>> {
        let call = self.call_site()?;
        let callee = call.callee?;
        // An initializing construction's receiver is finished (`call_receiver`): the body is not entered,
        // so no frame is pushed and no entry is called. The status is the returned one as a constant, which
        // `complete_call` takes as the construction's receiver; `call_settle` pops no header for it.
        if call.initializing.is_some() {
            let returned = stage.constant(u64::from(LANE_BODY_RETURNED));
            let unfinished = stage.builder.create_block();
            stage.builder.set_cold_block(unfinished);
            return Some(Entered { status: returned, word: this, unfinished });
        }
        let entry = stage.entry_target(crate::compiled::slow_paths::EngineEntry::CompiledEntry, crate::compiled::regions::flags(ENTRY_CELL), room.cell, field(std::mem::offset_of!(EntryCell, entry)));
        let entry = self.proven_entry(stage, callee, room.cell, entry);
        let captures = match callee {
            DirectSite::Owned(owned) if self.rebinds.get() => self.bound_word(stage, owned, std::mem::offset_of!(BoundClosure, captures)),
            DirectSite::Owned(owned) => address(self.helpers, stage.builder, RelocKind::CaptureWords, owned.target.function, owned.capture_word()),
            other => self.callee_word(stage, other, std::mem::offset_of!(CalleeRecord, captures))?,
        };
        // A sibling tail call of an owned body takes the running frame over (`tail_branch`), unless its callee is the stub.
        if let (Some(arguments), DirectSite::Owned(owned)) = (call.tail, callee) {
            self.tail_branch(stage, owned, this, room, captures, entry, arguments)?;
        }
        self.push_native(stage, callee, this, call.registers, room)?;
        let signature = crate::compiled::lane_body_abi::body_shim_signature(types::I64);
        let signature = stage.builder.import_signature(signature);
        let shim = stage.entry_target(crate::compiled::slow_paths::EngineEntry::CompiledEntry, crate::compiled::regions::flags(ENTRY_CELL), call.context,
            field(std::mem::offset_of!(CallContext, head.body_shim)));
        let (status, word) = stage.call_entry(signature, shim, &[call.context, this, captures, room.top, entry.word()]);
        // The status test's fall-through is the returned path (`complete_call`); every other status is cold.
        let unfinished = stage.builder.create_block();
        stage.builder.set_cold_block(unfinished);
        Some(Entered { status, word, unfinished })
    }

    /// A returned callee's value is the call's result, its frame popped, and the call continues at
    /// `completed`. A callee that never entered (the cell held the stub) takes the VM's call; one
    /// that ended by an exception (`LANE_BODY_THREW`, in `Vm::lane_raised`) has its frame popped and
    /// is counted in its cell, and the call continues at `raised`; any other status is a leave in
    /// the callee, which stops the caller with it (`dispatch_fast::body_left_in_lane`).
    fn call_settle(&mut self, stage: &mut Emit<'_, '_>, site: &Invocation, entered: Entered<ir::Value, ir::Block>, room: Room<ir::Value>,
        value: ir::Value) -> Option<()> {
        let call = self.call_site()?;
        let helper = self.helper_block()?;
        // An initializing construction pushed no header (`call_enter`): there is none to pop.
        if call.initializing.is_none() { self.pop_native(stage, room); }
        stage.store_u64(FRAME, call.registers, register_offset(site.result), value);
        match call.returned {
            Some(returned) => stage.jump(returned, &[value]),
            None => stage.jump(call.completed, &[]),
        }
        let (left, missed, threw, raised) = (stage.builder.create_block(), stage.builder.create_block(), stage.builder.create_block(), stage.builder.create_block());
        for block in [left, missed, threw, raised] {
            stage.builder.set_cold_block(block);
        }
        stage.builder.switch_to_block(entered.unfinished);
        let not_entered = stage.raw_compare_imm(Relation::Equal, entered.status, i64::from(LaneStatus::NotEntered as u32));
        stage.branch(not_entered, (missed, &[]), (threw, &[]));
        stage.builder.switch_to_block(missed);
        self.pop_native(stage, room);
        stage.jump(helper, &[]);
        stage.builder.switch_to_block(threw);
        let did_raise = stage.raw_compare_imm(Relation::Equal, entered.status, i64::from(LaneStatus::Threw as u32));
        stage.branch(did_raise, (raised, &[]), (left, &[]));
        stage.builder.switch_to_block(raised);
        self.pop_native(stage, room);
        stage.jump(call.raised, &[]);
        stage.builder.switch_to_block(left);
        call.leave.with(stage.builder, entered.status, Some(entered.word));
        Some(())
    }

    /// The call by the lane's helper of the record: its completion continues at `completed`, a
    /// miss leaves at `missed`, and a leave in a callee, or a termination, reaches `stop`. A call
    /// that emitted no direct entry jumps here; one that did has ended its blocks. The result
    /// is in the call's register, which the lowering reloads.
    fn call_helper(&mut self, stage: &mut Emit<'_, '_>, site: &Invocation, direct: Option<ir::Value>) -> Option<ir::Value> {
        let call = self.call_site()?;
        let helper = self.helper_block()?;
        // Where a direct entry was emitted, the VM's call is reached only from a guard's miss, so its
        // blocks are cold and the completed path stays the fall-through of the direct call.
        let direct_entry = direct.is_some();
        if !direct_entry {
            stage.jump(helper, &[]);
        }
        stage.builder.switch_to_block(helper);
        if direct_entry { stage.builder.set_cold_block(helper); }
        let which = if site.constructs() { LaneHelper::ConstructCall } else { LaneHelper::Call };
        let context = emit_js_payload_word(stage.builder, self.payload, std::mem::offset_of!(JsStepDispatch, head.lane_context));
        let record = stage.raw_const(Lane::I64, i64::from(call.record));
        let address = self.helpers.target(&mut stage.builder, which);
        let status = emit_lane_helper_call(stage.builder, self.payload, address, &[context, record]);
        // The status dispatch, generated from the shared outcome table: each test but the first
        // is cold where a direct entry was emitted; the last miss leaves with the status.
        let outcomes = quench_runtime::staged::protocol::HELPER_OUTCOMES;
        for (index, (code, outcome)) in outcomes.into_iter().enumerate() {
            let target = match outcome {
                quench_runtime::staged::protocol::HelperOutcome::Completed => call.completed,
                quench_runtime::staged::protocol::HelperOutcome::Missed => call.missed,
                quench_runtime::staged::protocol::HelperOutcome::Raised => call.raised,
            };
            let hit = stage.raw_compare_imm(Relation::Equal, status, code as i64);
            if index + 1 == outcomes.len() {
                stage.branch(hit, (target, &[]), (call.stop, &[status]));
            } else {
                let other = stage.builder.create_block();
                stage.branch(hit, (target, &[]), (other, &[]));
                stage.builder.switch_to_block(other);
                if direct_entry { stage.builder.set_cold_block(other); }
            }
        }
        stage.builder.switch_to_block(call.completed);
        Some(stage.constant(0))
    }
}
