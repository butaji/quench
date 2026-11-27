use super::*;

impl<'a, 'f> Lowering<'a, 'f> {
    /// The callee of a self tail call (DESIGN.md 4.4): the returning call at `pc` of the function's own body, whose one owned
    /// callee is the function itself, is a tail site of the body (`Function::tail_site_permitted`), and whose callee plan is an
    /// ordinary body's (`CallPlan::tail_callee`). It passes a register-held closure, no more arguments than the parameters,
    /// and its receiver is the body's `this` or `undefined`. Anything else keeps the call's own lowering.
    pub(crate) fn self_tail_callee(&self, pc: usize, record: u32, site: &Invocation) -> Option<&'a DirectCallee> {
        let function = self.program.function_get(self.function_id as usize)?;
        let [callee] = self.direct.get(&pc)?.as_slice() else { return None };
        // SAFETY: the body owns its callees for as long as its code runs (`Lowering::call`).
        let callee: &'a DirectCallee = unsafe { &**callee };
        let admitted = self.completion == Completion::Body
            && matches!(self.coverage, Coverage::Prefix | Coverage::Full)
            && site.returns && !site.constructs() && !site.forwards
            && matches!(site.callee, quench_runtime::staged::forms::Callee::Register(_))
            && matches!(site.receiver, quench_runtime::staged::forms::Receiver::Undefined
                | quench_runtime::staged::forms::Receiver::This { path: None })
            && matches!(site.arguments, Arguments::Window { count, .. } if usize::from(count) <= self.params)
            && callee.target.function == self.function_id && callee.forwarder.is_none()
            && function.tail_site_permitted(record)
            && self.program.function_get(callee.target.function as usize).is_some_and(|body| body.call_plan(self.program).tail_callee)
            && self.vm.ahead.and_then(|ahead| ahead.calls.get(&pc)).is_none_or(Vec::is_empty);
        admitted.then_some(callee)
    }

    /// The self tail call `self_tail_callee` admits, lowered as a jump to the function's entry: the parameters take the
    /// arguments (read before any parameter is written, as the window overlaps them), the registers the entry holds live
    /// beyond the parameters are undefined as a fresh call's are, and the entry is entered as a back edge of the body, which
    /// polls as one does (`back_edge_check`). The body's own closure, captures and receiver are the entry's words, so the
    /// jump is taken only where the callee register holds the closure the owned callee names, the captures are the body's,
    /// and the receiver is the one the call passes; otherwise the guards leave at the call (`missed`), which makes the call.
    pub(crate) fn self_tail_call(&mut self, state: &mut State, pc: usize, missed: ir::Block, site: &Invocation,
        callee: &DirectCallee) -> bool {
        let (quench_runtime::staged::forms::Callee::Register(register), Arguments::Window { first, count }) = (site.callee, site.arguments) else {
            return false;
        };
        let entry = self.coverage.entry();
        let identity = self.tagged(state, register);
        let same_closure = self.raw_compare_imm(Relation::Equal, identity, callee.callee.to_bits() as i64);
        crate::compiled::emit_js_guard(&mut self.builder, same_closure, missed);
        let captures = self.reloc(RelocKind::CaptureWords, callee.target.function, callee.capture_word());
        let entry_captures = self.entry_words.1;
        let same_captures = self.raw_compare(Relation::Equal, entry_captures, captures);
        crate::compiled::emit_js_guard(&mut self.builder, same_captures, missed);
        if matches!(site.receiver, quench_runtime::staged::forms::Receiver::Undefined) {
            let undefined = self.raw_const(Lane::I64, Value::UNDEFINED.to_bits() as i64);
            let entry_this = self.entry_words.0;
            let same_this = self.raw_compare(Relation::Equal, entry_this, undefined);
            crate::compiled::emit_js_guard(&mut self.builder, same_this, missed);
        }
        // The back edge polls before the parameters are written, so an interrupt leaves at the call with its state.
        self.backedge = true;
        self.count_trip();
        self.back_edge_check(state, pc, entry);
        let arguments: Vec<ir::Value> = (0..count).map(|index| self.tagged(state, first + index)).collect();
        let undefined = self.raw_const(Lane::I64, Value::UNDEFINED.to_bits() as i64);
        for index in 0..self.params {
            let value = arguments.get(index).copied().unwrap_or(undefined);
            self.set_tagged(state, index as Register, value);
        }
        for index in self.params..usize::from(self.unit.caller_registers) {
            if self.is_live(entry, index as Register) { self.set_tagged(state, index as Register, undefined); }
        }
        self.continue_at(state, entry);
        false
    }
}
