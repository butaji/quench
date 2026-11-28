use super::*;

impl Lowering<'_, '_> {
    /// A call of a register callee that the closed world proves is one of a closed set of functions (`arms`, an image's
    /// method read whose name the program stores each of them under). The receiver's layout id is the class the call
    /// reaches (`Symbol::ClassLayout`): with few arms whose plan inlines them, a chain of compares of that id, each arm's
    /// direct call (`closed_set_by_layout`); otherwise the indirect call through the class's method vector
    /// (`closed_set_indirect`). A set no arm of which has a proven layout is not closed for this lowering: the VM's call alone.
    pub(super) fn closed_set_call(&mut self, state: &mut State, site: quench_runtime::staged::Invocation, record: u32, register: Register, pc: usize,
        arms: &[ProvenCall], (completed, missed, raised): (ir::Block, ir::Block, ir::Block)) {
        if quench_runtime::trace::Channel::AotLanes.on() {
            eprintln!("aot-lanes: function {} closed-set call at record {record}: arms {:?}", self.function_id, arms.iter().map(|arm| arm.function).collect::<Vec<_>>());
        }
        // A fixed class's receiver has no record layout: its class id names the method vector, which the indirect form reads
        // (`closed_set_indirect`), and no layout compare decides the arm.
        if self.fixed_receiver_class(pc, site.receiver).is_some() {
            return self.closed_set_indirect(state, site, record, register, pc, arms, (completed, missed, raised));
        }
        // Every arm's receiver layout is proven, and the receiver is a register or `this`: the layouts decide the arm.
        let keyed: Option<Vec<(ProvenCall, u32)>> = arms.iter().map(|&arm| arm.layout.map(|key| (arm, key))).collect();
        let by_layout = keyed.filter(|_| match site.receiver {
            quench_runtime::staged::Receiver::Register(_) => true,
            quench_runtime::staged::Receiver::This { path: None } => self.entry.is_some(),
            _ => false,
        });
        let Some(keyed) = by_layout else {
            return self.closed_set_step(state, site, record, None, (completed, missed, raised));
        };
        if keyed.iter().all(|(arm, _)| arm.inline) {
            return self.closed_set_by_layout(state, site, record, pc, &keyed, (completed, missed, raised));
        }
        self.closed_set_indirect(state, site, record, register, pc, arms, (completed, missed, raised));
    }

    /// `closed_set_call` as an indirect call through the receiver class's method vector: the receiver's layout id is the
    /// class id (`layouts::class_methods`), whose vector holds each method's `CalleeRecord` address at the method's selector
    /// (`SymbolTable::selector`), and the call reads its callee's words from that record (`DirectSite::Indirect`), while its
    /// own words come from the site's record, which every arm shares. Every arm
    /// names the one method, so they share the selector. Until the loader fills the vectors, each call is guarded: a
    /// receiver whose class has no vector, or no record for the method, or whose record's callee is not the callee
    /// register's, takes the VM's call. The guard stays at the miss edge only once the fill binds every listed record.
    fn closed_set_indirect(&mut self, state: &mut State, site: quench_runtime::staged::Invocation, record: u32, register: Register,
        pc: usize, arms: &[ProvenCall], (completed, missed, raised): (ir::Block, ir::Block, ir::Block)) {
        let Some(&site_arm) = arms.first() else {
            return self.closed_set_step(state, site, record, None, (completed, missed, raised));
        };
        let Some(selector) = site_arm.selector else {
            return self.closed_set_step(state, site, record, None, (completed, missed, raised));
        };
        let Some(selector_at) = i32::try_from(u64::from(selector) * std::mem::size_of::<usize>() as u64).ok() else {
            return self.closed_set_step(state, site, record, None, (completed, missed, raised));
        };
        let record_flags = regions::flags(quench_runtime::staged::Region::EntryCell.mutable());
        let loaded = self.receiver_layout(state, site.receiver, pc, missed);
        // The class's vector: entry `class` of the table at `class * size_of::<usize>()` past its base (`Symbol::ClassMethods`).
        let base = crate::symbol_words::symbol_address(&mut self.builder, RelocKind::ClassMethods as u32, 0);
        let offset = self.raw_binary_imm(Binary::Mul, loaded, std::mem::size_of::<usize>() as i64);
        let entry = self.raw_binary(Binary::Add, base, offset);
        let vector = self.raw_load(Lane::I64, record_flags, entry, 0);
        let (present, check, taken, vm) = (self.builder.create_block(), self.builder.create_block(), self.builder.create_block(), self.builder.create_block());
        let has_vector = self.raw_compare_imm(Relation::NotEqual, vector, 0);
        self.branch_if(has_vector, (present, &[]), (vm, &[]));
        self.builder.switch_to_block(present);
        let at = self.raw_load(Lane::I64, record_flags, vector, selector_at);
        let has_record = self.raw_compare_imm(Relation::NotEqual, at, 0);
        self.branch_if(has_record, (check, &[]), (vm, &[]));
        self.builder.switch_to_block(check);
        // The identity guard: the callee record's callee is the callee register's.
        let word = self.raw_load(Lane::I64, regions::frame(), self.registers_base, register_offset(register));
        let identity = self.raw_load(Lane::I64, record_flags, at, std::mem::offset_of!(quench_bjit::call_record::CalleeRecord, callee) as i32);
        let matched = self.raw_compare(Relation::Equal, word, identity);
        self.branch_if(matched, (taken, &[]), (vm, &[]));
        self.builder.switch_to_block(taken);
        self.closed_set_step(state, site, record, Some(DirectSite::Indirect { site: site_arm, callee: at }), (completed, missed, raised));
        self.builder.switch_to_block(vm);
        self.closed_set_step(state, site, record, None, (completed, missed, raised));
    }

    /// `closed_set_call` where every arm's receiver layout is proven: the receiver's layout id is compared with each arm's
    /// `Symbol::ClassLayout` key, and the first arm it matches takes its direct call. The last arm takes no compare where
    /// the plan has no miss (`closed`); otherwise a receiver of no arm's layout takes the VM's call alone.
    fn closed_set_by_layout(&mut self, state: &mut State, site: quench_runtime::staged::Invocation, record: u32, pc: usize,
        keyed: &[(ProvenCall, u32)], (completed, missed, raised): (ir::Block, ir::Block, ir::Block)) {
        let Some((&(last, last_key), tested)) = keyed.split_last() else { return };
        let loaded = self.receiver_layout(state, site.receiver, pc, missed);
        for &(arm, key) in tested {
            let layout = crate::symbol_words::symbol_address(&mut self.builder, RelocKind::ClassLayout as u32, key);
            let matched = self.raw_compare(Relation::Equal, loaded, layout);
            let (taken, next) = (self.builder.create_block(), self.builder.create_block());
            self.branch_if(matched, (taken, &[]), (next, &[]));
            self.builder.switch_to_block(taken);
            self.closed_set_step(state, site, record, Some(DirectSite::Proven(arm)), (completed, missed, raised));
            self.builder.switch_to_block(next);
        }
        if last.closed {
            self.closed_set_step(state, site, record, Some(DirectSite::Proven(last)), (completed, missed, raised));
            return;
        }
        let layout = crate::symbol_words::symbol_address(&mut self.builder, RelocKind::ClassLayout as u32, last_key);
        let matched = self.raw_compare(Relation::Equal, loaded, layout);
        let (taken, vm) = (self.builder.create_block(), self.builder.create_block());
        self.branch_if(matched, (taken, &[]), (vm, &[]));
        self.builder.switch_to_block(taken);
        self.closed_set_step(state, site, record, Some(DirectSite::Proven(last)), (completed, missed, raised));
        self.builder.switch_to_block(vm);
        self.closed_set_step(state, site, record, None, (completed, missed, raised));
    }

    /// One step of a closed-set call: the direct call of `callee`, or the VM's call alone (`None`), whose miss the step fills.
    fn closed_set_step(&mut self, state: &mut State, site: quench_runtime::staged::Invocation, record: u32, callee: Option<DirectSite<'_>>,
        (completed, missed, raised): (ir::Block, ir::Block, ir::Block)) {
        use quench_runtime::staged::{CallOperation, Operation};
        let (stop, leave, context, registers) = (self.call_out_stop(), self.leave_site(), self.context, self.registers_base);
        let miss = cold_block(&mut self.builder);
        let call = CallSite { callee, record, context, registers, completed, returned: None, missed, raised, stop, leave, initializing: None, tail: None, construct: None };
        self.step_with(state, frame::Edges::ValueGuarded { miss }, Operation::Call(CallOperation::Site(site)), StepWith { site: Some(Site::Call(call)), ..StepWith::default() });
    }

    /// The layout id (`I64`) of the object `receiver` names: its class cell's class word (`cell_class` of `cell_of`), the id
    /// the method vector is keyed by. A fixed class's receiver names its class id (`fixed_receiver_class`) without a read.
    /// A receiver that is no direct object leaves at `missed`.
    fn receiver_layout(&mut self, state: &mut State, receiver: quench_runtime::staged::Receiver, pc: usize, missed: ir::Block) -> ir::Value {
        if let Some(class) = self.fixed_receiver_class(pc, receiver) {
            return self.raw_const(Lane::I64, i64::from(class));
        }
        let (bits, known) = match receiver {
            quench_runtime::staged::Receiver::Register(register) => (self.tagged(state, register),
                Some(state.values[usize::from(register)].value).filter(|fact| tag_proofs::proves_object(fact.ty))),
            _ => {
                let this = self.entry.as_ref().expect("entry facts").this;
                (headers::this_word(&mut self.builder, this), None)
            }
        };
        // The register's solver type proves its family where it names a direct object: no family test.
        if known.is_none() {
            let object = quench_runtime::staged::value::is_direct_object(&mut super::object_slots::emit(&mut self.builder), bits);
            emit_js_guard(&mut self.builder, object, missed);
        }
        let mut stage = super::object_slots::emit(&mut self.builder);
        let cell = quench_runtime::staged::object::cell_of(&mut stage, bits);
        let class = quench_runtime::staged::object::cell_class(&mut stage, cell);
        stage.raw_unary(Unary::ZeroExtend(Lane::I64), class)
    }

    /// The activation completes with `register`, by the staged return (`Frame::ret`) at the
    /// record that completes it (`LaneExit::Returned`). Never falls through.
    pub(super) fn return_register(&mut self, state: &mut State, record: u32, register: Register) -> bool {
        let site = Site::Return(ReturnSite { record, leave: self.leave_site() });
        self.step_with(state, frame::Edges::Value, quench_runtime::staged::Operation::Return { source: register }, StepWith { site: Some(site), ..StepWith::default() });
        self.finishes = true;
        false
    }

    /// The activation completes with `register`, which the record at `pc` names. Only a caller
    /// record can: an inlined callee's `Return` is the unit's move and jump.
    pub(super) fn complete(&mut self, state: &mut State, pc: usize, register: Register) -> bool {
        let Some(super::lane_unit::Origin::Caller(record)) = self.unit.origins.get(pc).copied() else {
            self.exit(state, pc as u32);
            return false;
        };
        self.native += 1;
        self.return_register(state, record, register)
    }

    /// Stops with the activation's completion: `value` in the result word, `record` the
    /// record that completes it (`LANE_BODY_RETURNED`).
    pub(super) fn return_value(&mut self, record: u32, value: ir::Value) {
        self.finishes = true;
        let leave = self.leave_site();
        ReturnSite { record, leave }.complete(&mut self.builder, self.payload, value);
    }

    /// Where this run's completion goes (`Leave`).
    pub(super) fn leave_site(&self) -> Leave {
        Leave { block: self.leave }
    }

    /// Leaves with `status`, and with `word` as the result word where a run publishes one.
    pub(crate) fn leave_with(&mut self, status: ir::Value, word: Option<ir::Value>) {
        self.leave_site().with(&mut self.builder, status, word);
    }

    pub(super) fn continue_branch(&mut self, state: &mut State, pc: usize, target: usize,
        taken_block: ir::Block, fall_block: ir::Block) {
        let back = target <= pc && target == self.coverage.entry();
        self.backedge |= back;
        let refinements = std::mem::take(&mut state.refinements);
        self.switch_to(taken_block, true);
        if back { self.count_trip(); }
        let mut taken_state = state.clone();
        taken_state.narrow(quench_runtime::staged::Edge::Target, &refinements);
        self.continue_at(&mut taken_state, target);
        self.switch_to(fall_block, true);
        let mut fall_state = state.clone();
        fall_state.narrow(quench_runtime::staged::Edge::Following, &refinements);
        self.continue_at(&mut fall_state, pc + 1);
    }

    /// Completes the activation with `destination`, the result of the returning operator at the
    /// caller record `pc` (`RETURN_REGISTER`). The completion names the function's terminal
    /// `Return` where it has one, whose register the lane reads it from, else the operator's own
    /// record.
    pub(super) fn complete_result(&mut self, state: &mut State, pc: usize, destination: Register) {
        let terminal = self.unit.instructions.len().checked_sub(1)
            .filter(|terminal| matches!(crate::compiled::decoded(self.unit.instructions[*terminal]), Some(quench_runtime::staged::Operation::Return { .. })))
            .and_then(|terminal| match self.unit.origins.get(terminal) {
                Some(super::lane_unit::Origin::Caller(record)) => Some((*record, self.unit.instructions[terminal].register_a())),
                _ => None,
            });
        let Some(super::lane_unit::Origin::Caller(own)) = self.unit.origins.get(pc).copied() else { unreachable!("a returning operator is a caller record's") };
        let bits = self.tagged(state, destination);
        // The activation's `Return` register holds it too, as the record it names reads.
        if let Some((_, register)) = terminal {
            let base = self.registers_base;
            self.store_word(regions::frame(), bits, base, register_offset(register));
        }
        self.return_value(terminal.map_or(own, |(record, _)| record), bits);
    }
}
