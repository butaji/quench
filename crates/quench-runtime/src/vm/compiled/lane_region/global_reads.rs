//! By-name loads that read the global scope (`Vm::lane_global_reads`).
use super::*;
use crate::vm::global_facts::GlobalRead;

impl Lowering<'_, '_> {
    /// `LoadName` or `LoadNameCall` at `pc` of a name the global scope resolves: an immutable
    /// global's constant (`builtins::immutable_globals`); a global cell's value, held as a
    /// Stable fact while the realm compiled against is the active one (`hold_global`); else
    /// the lane's own read, which leaves where it misses.
    pub(super) fn global_read(&mut self, state: &mut State, pc: usize, instruction: WideInstruction) -> bool {
        let register = instruction.result_register();
        match self.global_reads[&pc] {
            GlobalRead::Constant(constant) => {
                self.define_immediate(state, register, constant);
            }
            GlobalRead::Stable { value, ty, fact, global, realm_offset } if self.division().folds(BindingTime::Stable) => {
                let exit = self.guard_exit(state, pc as u32);
                self.hold_global(state, exit, fact, (global, realm_offset));
                self.define_dependent(state, register, value, ty);
            }
            GlobalRead::Frozen { value, ty } => {
                // Whole-program fact: read the cell once here and require the value the
                // closed program gives it. A changed binding fails the guard and leaves
                // to the shared body; the loop then runs the checked constant.
                let exit = self.guard_exit(state, pc as u32);
                let actual = self.global_get(instruction);
                let same = self.ins().icmp_imm_u(IntCC::Equal, actual, value.to_bits() as i64);
                emit_js_guard(&mut self.builder, same, exit);
                self.define_dependent(state, register, value, ty);
            }
            GlobalRead::Stable { .. } | GlobalRead::Dynamic => {
                let exit = self.guard_exit(state, pc as u32);
                let value = self.global_get(instruction);
                let hit = self.ins().icmp_imm_u(IntCC::NotEqual, value, Value::DELETED.to_bits() as i64);
                emit_js_guard(&mut self.builder, hit, exit);
                self.set_tagged(state, register, value);
            }
        }
        // A call's receiver is `undefined` for a binding of the global scope.
        if instruction.op() == Op::LoadNameCall {
            self.define_immediate(state, instruction.register_b(), Value::UNDEFINED);
        }
        true
    }

    /// Defines `register` as the immediate `value`, of its own type.
    fn define_immediate(&mut self, state: &mut State, register: Register, value: Value) {
        let bits = self.ins().iconst(types::I64, value.to_bits() as i64);
        self.define(state, register, bits, RegisterFact::definition(Type::immediate(value), Representation::Tagged));
    }

    /// The lane's own read of the global `instruction` names (`LaneHelper::GlobalGet`):
    /// its value, or `Value::DELETED` where the read misses.
    fn global_get(&mut self, instruction: WideInstruction) -> ir::Value {
        let atom = self.ins().iconst(types::I64, i64::from(instruction.atom_index()));
        let site = self.ins().iconst(types::I64, i64::from(instruction.cache_site_index()));
        let global_get = self.helpers.address(self.builder.ins(), LaneHelper::GlobalGet);
        emit_lane_helper_call(&mut self.builder, self.payload, global_get, &[atom, site])
    }

    /// Defines `register` as `value`, of type `ty`, held as a Stable fact by its dependency.
    fn define_dependent(&mut self, state: &mut State, register: Register, value: Value, ty: Type) {
        let constant = self.ins().iconst(types::I64, value.to_bits() as i64);
        let held = RegisterFact::admit(Fact { ty, representation: Representation::Tagged,
            binding: BindingTime::Stable, proof: Proof::Dependency, projection: Projection::Exact })
            .expect("a Stable fact's dependency proves any type");
        self.define(state, register, constant, held);
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
        let folds = global.is_some() || self.unit.instructions[header..=end].iter().any(|instruction| instruction.op() == Op::GetField);
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
            instruction.op() == Op::Jump && instruction.jump_target() as usize == header).filter(|end| *end >= header)
    }

    /// The realm a Stable global read in `header..=end` was compiled against, if any.
    fn loop_realm(&self, header: usize, end: usize) -> Option<(Value, i32)> {
        self.global_reads.range(header..=end).find_map(|(_, read)| match *read {
            GlobalRead::Stable { global, realm_offset, .. } => Some((global, realm_offset)),
            _ => None,
        })
    }
}
