//! By-name loads that read the global scope (`Vm::lane_global_reads`).
use super::*;
use crate::vm::global_facts::{GlobalRead, GlobalSlot};
use crate::staged::GlobalBinding;

impl Lowering<'_, '_> {
    /// `LoadName` or `LoadNameCall` at `pc` of a name the global scope resolves: an immutable
    /// global's constant (`builtins::immutable_globals`); a global cell's value, held as a
    /// Stable fact while the realm compiled against is the active one (`hold_global`); else
    /// the lane's own read, which leaves where it misses.
    pub(super) fn global_read(&mut self, state: &mut State, pc: usize, instruction: WideInstruction) -> bool {
        let register = instruction.result_register();
        let (binding, held) = self.global_binding(pc);
        let exit = (!matches!(binding, GlobalBinding::Constant(_))).then(|| self.guard_exit(state, pc as u32));
        if let (Some((fact, global, realm_offset)), Some(exit)) = (held, exit) {
            self.hold_global(state, exit, fact, (global, realm_offset));
        }
        if let (GlobalBinding::Typed { .. }, Some(&GlobalRead::Slot(slot)), Some(exit)) = (binding, self.global_reads.get(&pc), exit) {
            self.hold_global_slot(state, exit, slot);
        }
        let proof = super::EmitProof::GlobalRead { miss: exit };
        let edges = exit.map_or(frame::Edges::Value, |miss| frame::Edges::ValueGuarded { miss });
        self.step(state, edges, crate::staged::Operation::GlobalRead { destination: register,
            atom: instruction.atom_index(), site: instruction.cache_site_index(), binding, proof });
        // A call's receiver is `undefined` for a binding of the global scope.
        if instruction.op().is_load_name_call() {
            self.define_immediate(state, instruction.register_b(), Value::UNDEFINED);
        }
        true
    }

    /// The binding `pc`'s read takes, and the fact that holds it: what the read's own binding
    /// time (`GlobalBinding::time`) leaves this lowering's division to fold, else the lane's read.
    fn global_binding(&self, pc: usize) -> (GlobalBinding, Option<(crate::heap::GlobalFact, Value, i32)>) {
        let (binding, held) = match self.global_reads[&pc] {
            GlobalRead::Constant(value) => (GlobalBinding::Constant(value), None),
            GlobalRead::Stable { value, ty, fact, global, realm_offset } =>
                (GlobalBinding::Held { value, ty }, Some((fact, global, realm_offset))),
            GlobalRead::Frozen { value, ty } => (GlobalBinding::Frozen { value, ty }, None),
            GlobalRead::Slot(slot) if slot.dependency.is_some() => (GlobalBinding::Typed { ty: slot.ty }, None),
            GlobalRead::Slot(_) | GlobalRead::Dynamic | GlobalRead::NativeMember { .. } => (GlobalBinding::Dynamic, None),
        };
        if self.division().folds(binding.time()) { (binding, held) } else { (GlobalBinding::Dynamic, None) }
    }

    /// The global slot a `StoreName` or `LoadNameTypeof` at `pc` holds directly
    /// (`Vm::stable_global_slot`), after taking its dependencies: the layout and the slot's
    /// type, and the realm that names the global object (`hold_global_slot`); a store also holds
    /// that no method cache names its atom (`emit_store_atom_admitted`).
    pub(super) fn global_cell(&mut self, state: &mut State, pc: usize, row: &crate::staged::Operation<ir::Value, EmitProof>, exit: ir::Block)
        -> Option<GlobalSlot> {
        let GlobalRead::Slot(slot) = *self.global_reads.get(&pc)? else { return None };
        if !self.division().folds(BindingTime::Stable) { return None; }
        self.hold_global_slot(state, exit, slot);
        if let crate::staged::Operation::GlobalWrite { atom, .. } = *row {
            let admitted = self.emit_store_atom_admitted(atom);
            emit_js_guard(&mut self.builder, admitted, exit);
        }
        Some(slot)
    }

    /// Defines `register` as the immediate `value`, of its own type.
    fn define_immediate(&mut self, state: &mut State, register: Register, value: Value) {
        let bits = self.ins().iconst(types::I64, value.to_bits() as i64);
        self.define(state, register, bits, RegisterFact::definition(Type::immediate(value), Representation::Tagged));
    }

    /// Before loop `header`, the proofs its blocks would otherwise make each iteration: the
    /// realm its Stable global reads were compiled against (`prove_realm`; a call returns to
    /// its caller's realm), and, where its body changes no Stable fact's subject
    /// (`keeps_stable_facts`), the invalidation word (`hold_stable`). A write elsewhere retires
    /// the code then, and the next entry leaves; a body that may write reads the word again
    /// after each such instruction (`State::call_effect`).
    pub(super) fn hoist_stable(&mut self, state: &mut State, header: usize) {
        if !self.division().folds(BindingTime::Stable) { return; }
        let Some(end) = self.loop_end(header) else { return };
        let global = self.loop_realm(header, end);
        let folds = global.is_some() || self.unit.instructions[header..=end].iter().any(|instruction| instruction.op().is_get_field());
        let stable = folds && (header..=end).all(|pc| self.keeps_stable_facts(pc, self.unit.instructions[pc]));
        if !(stable || global.is_some()) || !self.enters_only_at_header(header, end) { return; }
        let exit = self.guard_exit(state, header as u32);
        if stable && !state.proofs.stable { self.read_invalidation(state, exit); }
        if let Some(realm) = global.filter(|_| !state.proofs.realm) { self.test_realm(state, exit, realm); }
        self.hold_in_loop(header, end, stable, global.is_some());
    }

    /// The last backedge of the loop at `header`, if one closes there.
    fn loop_end(&self, header: usize) -> Option<usize> {
        self.unit.instructions.iter().rposition(|instruction|
            instruction.op().is_jump() && instruction.jump_target() as usize == header).filter(|end| *end >= header)
    }

    /// The realm a Stable global read in `header..=end` was compiled against, if any.
    fn loop_realm(&self, header: usize, end: usize) -> Option<(Value, i32)> {
        self.global_reads.range(header..=end).find_map(|(_, read)| match *read {
            GlobalRead::Stable { global, realm_offset, .. } => Some((global, realm_offset)),
            _ => None,
        })
    }
}
