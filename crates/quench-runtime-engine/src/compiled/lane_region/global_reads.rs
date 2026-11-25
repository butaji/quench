//! By-name loads that read the global scope (`Vm::global_reads_in_lane`).
#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;
use quench_runtime::vm::global_facts::{GlobalRead, GlobalSlot};
use quench_runtime::staged::GlobalBinding;

impl Lowering<'_, '_> {
    /// `LoadName` or `LoadNameCall` at `pc` of a name the global scope resolves: an immutable
    /// global's constant (`builtins::immutable_globals`); a global cell's value, held as a
    /// Stable fact while the realm compiled against is the active one (`hold_global`); a global
    /// lexical binding no code folds, read live from its word under the global layout
    /// (`hold_lexical`); else the lane's own read, which leaves where it misses.
    pub(crate) fn global_read(&mut self, state: &mut State, pc: usize, instruction: WideInstruction) -> bool {
        let (binding, held) = self.global_binding(pc);
        let exit = (!matches!(binding, GlobalBinding::Constant(_))).then(|| self.guard_exit_for(state, pc as u32, ExitFact::Global));
        if let (Some((fact, global, realm_offset)), Some(exit)) = (held, exit) {
            self.hold_global(state, exit, fact, (global, realm_offset));
        }
        if let (GlobalBinding::Typed { .. }, Some(&(GlobalRead::Slot(slot) | GlobalRead::NativeSlot { slot, .. })), Some(exit)) = (binding, self.global_reads.get(&pc), exit) {
            self.hold_global_slot(state, exit, slot);
        }
        if let (GlobalBinding::Lexical { .. }, Some(&GlobalRead::Lexical(slot)), Some(exit)) = (binding, self.global_reads.get(&pc), exit) {
            self.hold_lexical(state, exit, slot);
        }
        let proof = super::EmitProof::GlobalRead { miss: exit, binding };
        let edges = exit.map_or(frame::Edges::Value, |miss| frame::Edges::ValueGuarded { miss });
        let row = quench_runtime::staged::Operation::decode_load_name(instruction, proof).expect("a global read is a name read");
        self.step(state, edges, row);
        true
    }

    /// The by-name load at `pc` of a loop's callee, read on the entering edge of the loop at `header` (`hoist_scalar_calls`):
    /// the read `global_read` makes, but a miss leaves at the header, before the loop ran. Its tagged value.
    pub(crate) fn hoist_native_read(&mut self, state: &mut State, header: usize, pc: usize) -> Option<ir::Value> {
        let instruction = self.unit.instructions[pc];
        if !matches!(quench_runtime::staged::Operation::<(), ()>::decode_load_name(instruction, ()), Some(quench_runtime::staged::Operation::LoadName { .. })) { return None; }
        let (binding, held) = self.global_binding(pc);
        let exit = (!matches!(binding, GlobalBinding::Constant(_))).then(|| self.guard_exit_for(state, header as u32, ExitFact::Global));
        if let (Some((fact, global, realm_offset)), Some(exit)) = (held, exit) {
            self.hold_global(state, exit, fact, (global, realm_offset));
        }
        if let (GlobalBinding::Typed { .. }, Some(&(GlobalRead::Slot(slot) | GlobalRead::NativeSlot { slot, .. })), Some(exit)) = (binding, self.global_reads.get(&pc), exit) {
            self.hold_global_slot(state, exit, slot);
        }
        let proof = super::EmitProof::GlobalRead { miss: exit, binding };
        let edges = exit.map_or(frame::Edges::Value, |miss| frame::Edges::ValueGuarded { miss });
        let row = quench_runtime::staged::Operation::decode_load_name(instruction, proof)?;
        self.step(state, edges, row);
        Some(self.tagged(state, instruction.result_register()))
    }

    /// The binding `pc`'s read takes, and the fact that holds it: what the read's own binding
    /// time (`GlobalBinding::time`) leaves this lowering's division to fold, else the lane's read.
    fn global_binding(&self, pc: usize) -> (GlobalBinding, Option<(quench_runtime::tier_surface::GlobalFact, Value, i32)>) {
        let (binding, held) = match self.global_reads[&pc] {
            GlobalRead::Constant(value) => (GlobalBinding::Constant(value), None),
            GlobalRead::Stable { value, ty, fact, global, realm_offset } =>
                (GlobalBinding::Held { value, ty }, Some((fact, global, realm_offset))),
            GlobalRead::Frozen { value, ty } => (GlobalBinding::Frozen { value, ty }, None),
            GlobalRead::Lexical(slot) => (GlobalBinding::Lexical { address: slot.address as u64 }, None),
            GlobalRead::Slot(slot) | GlobalRead::NativeSlot { slot, .. } if slot.dependency.is_some() => (GlobalBinding::Typed { ty: slot.ty }, None),
            GlobalRead::Slot(_) | GlobalRead::Held(_) | GlobalRead::Dynamic | GlobalRead::NativeMember { .. } | GlobalRead::NativeSlot { .. } => (GlobalBinding::Dynamic, None),
        };
        if self.division().folds(binding.time()) { (binding, held) } else { (GlobalBinding::Dynamic, None) }
    }

    /// The global slot a `StoreName` or `LoadNameTypeof` at `pc` holds directly
    /// (`Vm::stable_global_slot`), after taking its dependencies: the layout and the slot's
    /// type, and the realm that names the global object (`hold_global_slot`); a store also holds
    /// that no method cache names its atom (`emit_store_atom_admitted`).
    pub(crate) fn global_cell(&mut self, state: &mut State, pc: usize, row: &quench_runtime::staged::Operation<ir::Value, EmitProof>, exit: ir::Block)
        -> Option<(GlobalSlot, quench_runtime::staged::ShapeFact)> {
        let GlobalRead::Slot(slot) = *self.global_reads.get(&pc)? else { return None };
        if !self.division().folds(BindingTime::Stable) { return None; }
        let fact = self.hold_global_slot(state, exit, slot);
        if let quench_runtime::staged::Operation::GlobalWrite { atom, .. } = *row {
            let admitted = self.emit_store_atom_admitted(atom);
            emit_js_guard(&mut self.builder, admitted, exit);
        }
        Some((slot, fact))
    }

    /// Before loop `header`, the proofs its blocks would otherwise make each iteration: the
    /// realm its Stable global reads were compiled against (`prove_realm`; a call returns to
    /// its caller's realm), and, where its body changes no Stable fact's subject
    /// (`keeps_stable_facts`), the invalidation word (`hold_stable`). A write elsewhere retires
    /// the code then, and the next entry leaves; a body that may write reads the word again
    /// after each such instruction (`State::call_effect`).
    pub(crate) fn hoist_stable(&mut self, state: &mut State, header: usize) {
        if !self.division().folds(BindingTime::Stable) { return; }
        let Some(end) = self.loop_end(header) else { return };
        let global = self.loop_realm(header, end);
        let stores = self.loop_holds_stores(header, end);
        let reads = global.is_some() || self.unit.instructions[header..=end].iter().any(|instruction| matches!(crate::compiled::performed(instruction), Some(quench_runtime::staged::Operation::GetField { .. })));
        // A store may write a prototype's field that a read's folded holder value names, which the inline
        // store does not retire; a loop that both stores and reads a Stable fact holds none.
        let folds = reads || stores;
        let stable = folds && !(stores && reads) && (header..=end).all(|pc| self.keeps_stable_facts(pc, self.unit.instructions[pc]));
        if !(stable || global.is_some()) || !self.enters_only_at_header(header, end) { return; }
        let exit = self.guard_exit_for(state, header as u32, ExitFact::Global);
        if stable && !state.proofs.stable { self.read_invalidation(state, exit); }
        // A loop whose stores all leave on a miss runs no store that could unset `stores_specialized`.
        if stable && stores { self.hold_specialized(state, exit); }
        if let Some(realm) = global.filter(|_| !state.proofs.realm) { self.test_realm(state, exit, realm); }
        self.hold_in_loop(header, end, stable, global.is_some());
    }

    /// Whether loop `header..=end` holds a field store that leaves on a miss (`store_keeps_facts`),
    /// the stores whose Stable proofs `hoist_stable` may hold on the entering edge.
    fn loop_holds_stores(&self, header: usize, end: usize) -> bool {
        (header..=end).any(|pc| {
            let instruction = self.unit.instructions[pc];
            matches!(crate::compiled::decoded(instruction), Some(quench_runtime::staged::Operation::SetField { .. }))
                && self.store_keeps_facts(pc, instruction)
        })
    }

    /// The last backedge of the loop at `header`, if one closes there.
    pub(crate) fn loop_end(&self, header: usize) -> Option<usize> {
        self.unit.instructions.iter().rposition(|instruction|
            matches!(crate::compiled::decoded(instruction), Some(quench_runtime::staged::Operation::Jump)) && instruction.jump_target() as usize == header).filter(|end| *end >= header)
    }

    /// The realm a Stable global read in `header..=end` was compiled against, if any.
    fn loop_realm(&self, header: usize, end: usize) -> Option<(Value, i32)> {
        self.global_reads.range(header..=end).find_map(|(_, read)| match *read {
            GlobalRead::Stable { global, realm_offset, .. } => Some((global, realm_offset)),
            _ => None,
        })
    }
}
