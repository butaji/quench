use super::*;

impl<'a, 'f> Lowering<'a, 'f> {
    /// `leave` naming the lowering line that decided the leave, which a diagnostics run reports
    /// (`JsStepDispatch::exit_line`).
    #[track_caller]
    pub(super) fn exit(&mut self, state: &mut State, pc: u32) {
        if self.vm.diagnostics && !self.completion.numeric() {
            let line = self.raw_const(Lane::I64, i64::from(std::panic::Location::caller().line()));
            self.store_word(regions::dispatch(), line, self.payload, std::mem::offset_of!(JsStepDispatch, exit_line) as i32);
        }
        self.leave_at(state, pc);
    }

    /// Leave at `pc` with the interpreter's state: every written register back in the lane's file, but those it already holds.
    pub(super) fn leave_at(&mut self, state: &mut State, pc: u32) {
        if self.completion.numeric() {
            self.return_at(0);
            return;
        }
        // A pure callee miss restarts its call. A nested marker names the
        // parent's call PC, so recovery keeps any already-performed parent effects.
        let mut pc = pc as usize;
        while let Some(super::lane_unit::Origin::Inlined { frame, .. }) = self.unit.origins.get(pc).copied() {
            if self.unit.frames[frame].recovery != super::lane_unit::CallRecovery::RestartCall { break; }
            pc = self.unit.markers[frame];
        }
        // A kept receiver the interpreter reads is an object again (`virtual_objects`).
        if !state.virtuals.is_empty() || !state.virtual_arrays.is_empty() { self.materialize_virtuals(state, pc); }
        let stored = self.stored_at(state, pc);
        self.materialize_inlined(state, pc);
        // A caller-frame leave of a body differs from another storing the same registers only
        // by its exit word: one block stores them for all, the word its parameter.
        let origin = self.unit.origins.get(pc).copied().unwrap_or(super::lane_unit::Origin::Caller(self.unit.end));
        if let (Completion::Body, super::lane_unit::Origin::Caller(record)) = (self.completion, origin) {
            let block = self.shared_exit_block(stored);
            let word = self.raw_const(Lane::I64, LaneExit::At(record as usize).word() as i64);
            self.goto(block, &[word]);
            return;
        }
        let block = self.exit_block(pc, stored);
        self.goto(block, &[]);
    }

    /// Lowers into `block` from here, sealed where every edge into it is lowered.
    pub(super) fn switch_to(&mut self, block: ir::Block, sealed: bool) {
        self.builder.switch_to_block(block);
        if sealed { self.builder.seal_block(block); }
    }

    pub(super) fn continue_at(&mut self, state: &mut State, pc: usize) {
        if !self.coverage.lowered(pc) {
            self.decline_kept(state, pc);
            self.exit(state, pc as u32);
            return;
        }
        // A block before this one that no edge entered was not lowered.
        let lowered = pc > self.at || self.entered.contains(pc);
        match self.blocks.get(pc).copied().flatten().filter(|_| lowered) {
            // An edge the forward analysis did not see reaches a sealed block: decline.
            Some(_) if pc <= self.at && self.sealable.contains(pc) => { self.valid = false; declined("an unseen edge reaches a sealed block", pc); }
            Some(block) => {
                self.entered.insert(pc);
                self.finishes |= pc <= self.at;
                if self.at < pc { self.hoist_entry(state, pc); }
                self.conform(state, pc);
                self.carry_kept(state, pc);
                if self.at < pc { self.arrive(state, pc); }
                self.goto(block, &[]);
            }
            None => {
                self.decline_kept(state, pc);
                self.exit(state, pc as u32);
            }
        }
    }

    /// Record `state` arriving at block leader `pc` by one of its forward edges.
    pub(super) fn arrive(&mut self, state: &State, pc: usize) {
        if self.loop_carry.contains_key(&pc) { self.carried_typed.entry(pc).or_default().push(state.proofs.typed.clone()); }
        if self.forward.get(pc).copied().flatten().is_some() {
            self.arrived.entry(pc).or_default().push(state.proofs.clone());
        }
    }

    /// The typed views every forward edge into the loop block `pc` arrived with (`loop_carry`), kept where no PC of the
    /// loop (`pc` to its last entering PC) retires one: each is a typed-only access, keeps array views, and writes no
    /// register a view names. The views were proved before the loop, in blocks over all its forward edges.
    pub(super) fn carry_typed_views(&mut self, state: &mut State, pc: usize) {
        let Some(&(edges, last)) = self.loop_carry.get(&pc) else { return };
        let Some(arrived) = self.carried_typed.remove(&pc) else { return };
        let Some((first, rest)) = arrived.split_first().filter(|_| arrived.len() == edges) else { return };
        let mut kept: smallvec::SmallVec<[TypedProof; INLINE_ARRAY_VIEWS]> = first.iter().copied()
            .filter(|view| rest.iter().all(|other| other.contains(view))).collect();
        if kept.is_empty() { return; }
        for at in pc..=last.min(self.unit.instructions.len() - 1) {
            let instruction = self.unit.instructions[at];
            let typed_write = matches!(crate::compiled::performed(instruction), Some(quench_runtime::staged::Operation::ElementWrite { .. }));
            if self.unit.markers.contains(&at) || !retains_array_views(&self.program.constants, instruction) || (typed_write && !self.typed_only_at(at)) { return; }
                kept.retain(|view| instruction.written_registers().all(|register| register != view.register));
        }
        state.proofs.typed = kept;
    }

    /// The local proofs every forward edge into `pc` arrived with, as the same values, for
    /// a block no other edge enters: each was made in a block over all of them, so over this.
    pub(super) fn keep_shared_proofs(&mut self, state: &mut State, pc: usize) {
        let Some(edges) = self.forward.get(pc).copied().flatten() else { return };
        let Some(arrived) = self.arrived.remove(&pc) else { return };
        let Some((first, rest)) = arrived.split_first().filter(|_| arrived.len() == edges) else { return };
        // A header every arrival holds for the same word is the join's, read where any arrival read it.
        state.proofs.headers = first.headers.iter().filter(|(word, header, _)| rest.iter()
            .all(|other| other.headers.iter().any(|(known, proof, _)| known == word && proof == header)))
            .map(|(word, header, _)| (*word, *header, arrived.iter()
                .any(|other| other.headers.iter().any(|(known, _, readable)| known == word && *readable))))
            .collect();
        state.proofs.records = first.records.iter().copied().filter(|record| rest.iter()
            .all(|other| other.records.contains(record))).collect();
        state.proofs.fresh = first.fresh.iter().copied().filter(|word| rest.iter()
            .all(|other| other.fresh.contains(word))).collect();
        state.proofs.layouts = first.layouts.iter().filter(|(key, projection)| rest.iter()
            .all(|other| other.layouts.get(key) == Some(projection))).map(|(key, projection)| (*key, *projection)).collect();
        state.proofs.receivers = first.receivers.iter().filter(|register| rest.iter()
            .all(|other| other.receivers.contains(register))).copied().collect();
        state.proofs.fields = first.fields.iter().copied().filter(|field| rest.iter()
            .all(|other| other.fields.contains(field))).collect();
        state.proofs.int32_projections = first.int32_projections.iter().copied().filter(|projection| rest.iter()
            .all(|other| other.int32_projections.contains(projection))).collect();
        state.proofs.numbers = first.numbers.iter().copied().filter(|projection| rest.iter()
            .all(|other| other.numbers.contains(projection))).collect();
        state.proofs.admitted_captures = first.admitted_captures.iter().copied().filter(|view| rest.iter()
            .all(|other| other.admitted_captures.contains(view))).collect();
        state.proofs.arrays = first.arrays.iter().copied().filter(|view| rest.iter()
            .all(|other| other.arrays.contains(view))).collect();
        state.proofs.typed = first.typed.iter().copied().filter_map(|view| rest.iter().try_fold(view, |joined, other| other.typed.iter()
            .find(|proof| TypedProof { kept: joined.kept, ..**proof } == joined).map(|proof| TypedProof { kept: joined.kept | proof.kept, ..joined }))).collect();
        state.proofs.stable = first.stable && rest.iter().all(|other| other.stable);
        state.proofs.realm = first.realm && rest.iter().all(|other| other.realm);
        state.proofs.specialized = first.specialized && rest.iter().all(|other| other.specialized);
        // A block a rotated latch also enters has an edge `rest` does not hold: only a sealable block keeps the homes.
        if self.sealable.contains(pc) {
            state.proofs.homed.clone_from(&first.homed);
            for other in rest { state.proofs.meet_homed(&other.homed); }
        }
    }

    /// Whether unit `register` is live at unit `pc`. Without unit masks: the frame that owns
    /// it is in the chain of frames `pc` runs in, and its function's masks keep it live at
    /// the frame's own PC there (the PC itself for the innermost, its call's for each frame
    /// around it).
    pub(super) fn is_live(&self, pc: usize, register: Register) -> bool {
        // An inlined frame bound by its function (a closed-world call, `CalleeBinding::Function`) names
        // no closure: a leave inside it rebuilds the activation from the closure its callee register
        // holds (`body_inlined_activation_in_lane`), so that register lives through the frame.
        let mut open = match self.unit.origins.get(pc).copied() { Some(super::lane_unit::Origin::Inlined { frame, .. }) => Some(frame), _ => None };
        while let Some(index) = open {
            let inlined = self.unit.frames[index];
            if inlined.target.identity().is_none() && inlined.callee_register == register { return true; }
            open = inlined.parent;
        }
        let (caller, frames) = match &self.live {
            Liveness::Unit(masks) => return masks.as_deref().is_none_or(|live| live.is_live(pc, register)),
            Liveness::Frames { caller, frames } => (caller, frames),
        };
        let (mut frame, mut at) = match self.unit.origins.get(pc).copied() {
            Some(super::lane_unit::Origin::Inlined { frame, pc }) => (Some(frame), pc),
            Some(super::lane_unit::Origin::Caller(pc)) => (None, pc),
            None => (None, self.unit.end),
        };
        while let Some(index) = frame {
            let inlined = self.unit.frames[index];
            if (inlined.base..inlined.base + inlined.registers).contains(&register) {
                return frames[index].is_none_or(|live| live.is_live(at as usize, register - inlined.base));
            }
            (frame, at) = (inlined.parent, inlined.call);
        }
        register < self.unit.caller_registers && caller.is_none_or(|live| live.is_live(at as usize, register))
    }
}
