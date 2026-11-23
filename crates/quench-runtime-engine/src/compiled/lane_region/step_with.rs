//! `Lowering::step_with` (`lane_region`), alone in its module and so in its own codegen unit.
#[allow(unused_imports)]
use super::*;

impl Lowering<'_, '_> {
    /// `step` with what the operation's frame needs beyond its edges (`StepWith`).
    pub(super) fn step_with(&mut self, state: &mut State, edges: frame::Edges, operation: quench_runtime::staged::Operation<ir::Value, EmitProof>,
        StepWith { access, sites, site }: StepWith<'_>) -> quench_runtime::staged::Step<ir::Value, ()> {
        let locals = !self.completion.numeric();
        let makes_environment = operation.makes_environment();
        let property_values = access.and_then(|_| self.entry.as_ref()).map(|entry| self.builder.use_var(entry.receiver.properties));
        let gate = access.map(|_| self.payload);
        let mut stage = Emit { builder: &mut self.builder, property_values, gate, payload: Some(self.payload) };
        let fields = access.map(|(plans, held)| FieldFacts { plans, entry: self.entry.as_ref(), native_calls: self.native_calls,
            held,
            stable: StableHold { holders: &mut self.stable.holders, shapes: &mut self.stable.shapes, loops: &self.stable.stable_loops, online: self.vm.online, at: self.at } });
        let mut frame = EmitFrame { registers: &mut self.registers, written: &mut self.written,
            state, edges, constants: &self.program.constants, this: self.entry.as_ref().map(|entry| entry.this),
            payload: self.payload, helpers: self.helpers, locals, polls: &mut self.polls,
            roots: &mut self.stable.roots, number_returns: &mut self.number_returns, fields,
            call_out: sites.call_out, capture: sites.capture, html_dda: self.vm.html_dda, tiers: self.vm.tiers,
            prototype: sites.prototype, table_entry: self.entry.as_ref(),
            held: self.held_view.take(), hole_leaves: std::mem::take(&mut self.hole_leaves), arrays_fixed: self.arrays_fixed, site, proven_arguments: Default::default() };
        super::record_boxed_numbers();
        let step = quench_runtime::staged::step(&mut stage, &mut frame, operation);
        // A Number the step boxed (a raw-double slot's read) reads back as the word it boxed.
        for (bits, number) in super::take_boxed_numbers() {
            let bits = self.builder.func.dfg.resolve_aliases(bits);
            state.proofs.note_number(bits, number);
        }
        debug_assert!(!matches!(step, quench_runtime::staged::Step::Unresolved), "the lane specializer decides every truthiness");
        // A step that ends in a branch makes no environment: only a closure's allocation, a value step, does.
        if makes_environment && matches!(edges, frame::Edges::Value | frame::Edges::ValueGuarded { .. }) { self.reload_environment(); }
        step
    }
}
