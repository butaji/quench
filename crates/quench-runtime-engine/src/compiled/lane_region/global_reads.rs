//! By-name loads that read the global scope (`Vm::global_reads_in_lane`).
#[allow(unused_imports)] use quench_runtime::staged::{OperationNamesExt};
#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;
use quench_runtime::vm::global_facts::{GlobalRead, GlobalSlot};
use quench_runtime::staged::GlobalBinding;
#[allow(unused_imports)] use quench_runtime::staged::glue_prelude::*;

impl Lowering<'_, '_> {
    /// `LoadName` or `LoadNameCall` at `pc` of a name the global scope resolves: an immutable
    /// global's constant (`builtins::immutable_globals`); a global cell's value, held as a
    /// Stable fact while the realm compiled against is the active one (`hold_global`); a global
    /// lexical binding no code folds, read live from its word under the global layout
    /// (`hold_lexical`); else the lane's own read, which leaves where it misses.
    pub(crate) fn global_read(&mut self, state: &mut State, pc: usize, instruction: WideInstruction) -> bool {
        let (binding, held) = self.global_binding(pc);
        let exit = (!matches!(binding, GlobalBinding::Constant(_) | GlobalBinding::Frozen { .. })).then(|| self.guard_exit_for(state, pc as u32, ExitFact::Global));
        if let (Some((fact, global, realm_offset)), Some(exit)) = (held, exit) {
            self.hold_global(state, exit, fact, (global, realm_offset));
        }
        let mut held_slot = None;
        if let (GlobalBinding::Typed { .. }, Some(&(GlobalRead::Slot(slot) | GlobalRead::NativeSlot { slot, .. })), Some(exit)) = (binding, self.global_reads.get(&pc), exit) {
            held_slot = Some((slot, self.hold_global_slot(state, exit, slot)));
        }
        if let (GlobalBinding::Lexical { .. }, Some(&GlobalRead::Lexical(slot)), Some(exit)) = (binding, self.global_reads.get(&pc), exit) {
            self.hold_lexical(state, exit, slot);
        }
        let proof = super::EmitProof::GlobalRead { miss: exit, binding, slot: held_slot };
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
        let mut held_slot = None;
        if let (GlobalBinding::Typed { .. }, Some(&(GlobalRead::Slot(slot) | GlobalRead::NativeSlot { slot, .. })), Some(exit)) = (binding, self.global_reads.get(&pc), exit) {
            held_slot = Some((slot, self.hold_global_slot(state, exit, slot)));
        }
        let proof = super::EmitProof::GlobalRead { miss: exit, binding, slot: held_slot };
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
            // A slot read live under the layout: of its observed type where a dependency holds it, else of any.
            GlobalRead::Slot(slot) | GlobalRead::NativeSlot { slot, .. } => (GlobalBinding::Typed { ty: if slot.dependency.is_some() { slot.ty } else { Type::ANY } }, None),
            GlobalRead::Held(_) | GlobalRead::Dynamic | GlobalRead::NativeMember { .. } => (GlobalBinding::Dynamic, None),
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
        // Online, the admission is a block proof the loop's entering edge may make once (`online_store_held`).
        if let quench_runtime::staged::Operation::GlobalWrite { atom, .. } = *row && self.online_store_held(state, atom, exit).is_none() {
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
        // A store of a held global slot (`global_cell`) writes a `Mutable` cell that no code folds, of a global object
        // no object inherits from: it neither stores nor reads a fact a field store could retire.
        let slots = self.loop_global_slots(header, end);
        let reads = matches!(global, Some((_, false))) || self.unit.instructions[header..=end].iter().any(|instruction| matches!(crate::compiled::performed(instruction), Some(quench_runtime::staged::Operation::GetField { .. })));
        // A store may write a prototype's field that a read's folded holder value names, which the inline
        // store does not retire; a loop that both stores and reads a Stable fact holds none.
        let folds = reads || stores || slots;
        let stable = folds && !(stores && reads) && (header..=end).all(|pc| self.keeps_stable_facts(pc, self.unit.instructions[pc]));
        if !(stable || global.is_some()) || !self.enters_only_at_header(header, end) { return; }
        let exit = self.guard_exit_for(state, header as u32, ExitFact::Global);
        if stable && !state.proofs.stable { self.read_invalidation(state, exit); }
        // A loop whose stores all leave on a miss runs no store that could unset `stores_specialized`.
        let specialized = stable && (stores || slots);
        if specialized { self.hold_specialized(state, exit); }
        if let Some((realm, _)) = global.filter(|_| !state.proofs.realm) { self.test_realm(state, exit, realm); }
        self.hold_in_loop(header, end, stable, global.is_some(), specialized);
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

    /// The realm a Stable global read, or a held global slot or lexical binding, in `header..=end` was compiled
    /// against, if any, and whether only slots and bindings name it (no Stable read).
    fn loop_realm(&self, header: usize, end: usize) -> Option<((Value, i32), bool)> {
        let folds = self.division().folds(BindingTime::Stable);
        let mut held = None;
        for (_, read) in self.global_reads.range(header..=end) {
            match *read {
                GlobalRead::Stable { global, realm_offset, .. } => return Some(((global, realm_offset), false)),
                GlobalRead::Slot(slot) | GlobalRead::NativeSlot { slot, .. } if folds => { held.get_or_insert(((slot.global, slot.realm_offset), true)); }
                GlobalRead::Lexical(slot) if folds => { held.get_or_insert(((slot.global, slot.realm_offset), true)); }
                _ => {}
            }
        }
        held
    }

    /// Whether loop `header..=end` stores a held global slot (`global_cell`).
    fn loop_global_slots(&self, header: usize, end: usize) -> bool {
        (header..=end).any(|pc| self.stores_global_slot(pc, self.unit.instructions[pc]))
    }

    /// Whether `instruction` at `pc` is a `StoreName` of a global slot this lowering holds (`global_cell`).
    pub(crate) fn stores_global_slot(&self, pc: usize, instruction: WideInstruction) -> bool {
        matches!(crate::compiled::decoded(instruction), Some(quench_runtime::staged::Operation::GlobalWrite { initialize: false, .. }))
            && matches!(self.global_reads.get(&pc), Some(GlobalRead::Slot(_)))
    }
}
