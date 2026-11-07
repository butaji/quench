//! By-name loads that read the global scope (`Vm::lane_global_reads`).
use super::*;

impl Lowering<'_, '_> {
    /// `LoadName` or `LoadNameCall` at `pc` of a name the global scope resolves: an immutable
    /// global's constant (`builtins::immutable_globals`), else the lane's own read, which
    /// leaves where it misses.
    pub(super) fn global_read(&mut self, state: &mut State, pc: usize, instruction: WideInstruction) -> bool {
        match self.global_reads[&pc] {
            Some(constant) => {
                let value = self.ins().iconst(types::I64, constant.to_bits() as i64);
                self.define(state, instruction.result_register(), value,
                    RegisterFact::definition(Type::immediate(constant), Representation::Tagged));
            }
            None => {
                let exit = self.guard_exit(state, pc as u32);
                let atom = self.ins().iconst(types::I64, i64::from(instruction.atom_index()));
                let site = self.ins().iconst(types::I64, i64::from(instruction.cache_site_index()));
                let global_get = self.helpers.address(self.builder.ins(), LaneHelper::GlobalGet);
                let value = emit_lane_helper_call(&mut self.builder, self.payload, global_get, &[atom, site]);
                let hit = self.ins().icmp_imm_u(IntCC::NotEqual, value, Value::DELETED.to_bits() as i64);
                emit_js_guard(&mut self.builder, hit, exit);
                self.set_tagged(state, instruction.result_register(), value);
            }
        }
        // A call's receiver is `undefined` for a binding of the global scope.
        if instruction.op() == Op::LoadNameCall {
            let undefined = self.ins().iconst(types::I64, Value::UNDEFINED.to_bits() as i64);
            self.define(state, instruction.register_b(), undefined,
                RegisterFact::definition(Type::immediate(Value::UNDEFINED), Representation::Tagged));
        }
        true
    }
}
