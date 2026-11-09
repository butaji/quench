//! Stores whose site met several shapes, completed through the VM's polymorphic entries
//! where no shape the site proved applies.

use super::*;

impl Lowering<'_, '_> {
    /// `finish_store` of a store through a shape switch. In a lane body, a shape no case
    /// names, or a value its slot's type does not hold, stores through the VM's polymorphic
    /// entries (`field_add`), which complete it rather than leave; it may reshape the object
    /// or move the arena, so every object proof retires past the store.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn finish_shape_switch_store(&mut self, state: &mut State, object_bits: ir::Value, atom: Atom, site: u16,
        value: ir::Value, ty: Type, exit: ir::Block, header: Option<ObjectHeader>, general: ir::Block,
        widens: &[ir::Block], done: ir::Block) {
        if self.completion != Completion::StatusAndPc {
            return self.finish_store(state, object_bits, atom, site, value, ty, exit, header, general, widens, done);
        }
        for block in std::iter::once(general).chain(widens.iter().copied()) {
            self.builder.switch_to_block(block);
            self.field_add(object_bits, atom, site, value, exit);
            self.ins().jump(done, &[]);
        }
        self.builder.switch_to_block(done);
        self.reshaped(state);
        self.reload_receiver();
    }

    /// The store of `value` to `atom` of `object` by the VM's own, polymorphic and adding
    /// entries (`compiled::lane_field_add`); a miss leaves through `exit`.
    pub(super) fn field_add(&mut self, object: ir::Value, atom: Atom, site: u16, value: ir::Value, exit: ir::Block) {
        let write = self.ins().iconst(types::I64, (u64::from(atom) << u16::BITS | u64::from(site)) as i64);
        let payload = self.payload;
        let stored = self.helper_call(LaneHelper::FieldAdd, &[payload, object, write, value]);
        let hit = self.ins().icmp_imm_u(IntCC::NotEqual, stored, 0);
        emit_js_guard(&mut self.builder, hit, exit);
    }
}
