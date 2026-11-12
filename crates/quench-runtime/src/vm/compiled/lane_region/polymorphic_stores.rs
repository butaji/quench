//! Stores whose site met several shapes, completed through the VM's polymorphic entries
//! where no shape the site proved applies.

use super::*;
use crate::staged::{Stage, add::{Edge, site_add}};

impl Lowering<'_, '_> {
    /// `finish_store` of a store through a shape switch. In a lane body, a shape no case
    /// names, or a value its slot's type does not hold, stores through the VM's polymorphic
    /// entries (`field_add`), which complete it rather than leave; it may reshape the object
    /// or move the arena, so every object proof retires past the store.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn finish_shape_switch_store(&mut self, state: &mut State, object_bits: ir::Value, atom: Atom, site: u16,
        value: ir::Value, ty: Type, exit: ir::Block, header: Option<ObjectHeader>, general: ir::Block,
        widens: &[ir::Block], done: ir::Block) {
        if !self.completion.status_and_pc() {
            return self.finish_store(state, object_bits, atom, site, value, ty, exit, header, general, widens, done);
        }
        for block in std::iter::once(general).chain(widens.iter().copied()) {
            self.builder.switch_to_block(block);
            self.field_add(state, object_bits, (atom, site), value, exit);
            self.ins().jump(done, &[]);
        }
        self.builder.switch_to_block(done);
        self.reshaped(state);
        self.reload_receiver();
    }

    /// The store of `value` to `atom` of `object` that adds the property: the staged add row where the
    /// site recorded a transition (`add_row`), its miss and any other site the VM's own polymorphic
    /// and adding entries (`compiled::lane_field_add`); a miss of those leaves through `exit`.
    pub(super) fn field_add(&mut self, state: &mut State, object: ir::Value, (atom, site): (Atom, u16), value: ir::Value, exit: ir::Block) {
        let Some((slow, done)) = self.add_row(state, object, (atom, site), value, exit) else {
            return self.field_add_helper(object, (atom, site), value, exit);
        };
        self.builder.switch_to_block(slow);
        self.field_add_helper(object, (atom, site), value, exit);
        self.ins().jump(done, &[]);
        self.builder.switch_to_block(done);
    }

    /// The staged add (`staged::add::site_add`) of `value` to `object` by the site's recorded transition, a
    /// hit continuing at the returned `done` block and a miss at `slow`, where the caller completes it. The row
    /// holds, as Stable facts, that no method cache names `atom` and that the transition's prototype chain
    /// intercepts no store (`hold_add`), and tests only what varies per receiver: its shape, link, integrity,
    /// storage and the store gate. `None` (nothing emitted) where the site recorded none, or a cache names `atom`.
    fn add_row(&mut self, state: &mut State, object: ir::Value, (atom, site): (Atom, u16), value: ir::Value,
        exit: ir::Block) -> Option<(ir::Block, ir::Block)> {
        let online = self.vm.online.filter(|online| !online.names(atom))?;
        let adds = online.adds?;
        let add = adds(site, atom, true)?;
        let (transition, form, link) = (add.transition, add.transition.form()?, add.link?);
        self.hold_add(state, exit, atom, transition.before);
        let (slow, done) = (self.builder.create_block(), self.builder.create_block());
        self.builder.set_cold_block(slow);
        let records = self.object_table_base();
        let properties = self.builder.use_var(self.entry.as_ref().expect("entry facts").receiver.properties);
        let mut stage = super::object_slots::Emit { builder: &mut self.builder, property_values: Some(properties), gate: Some(self.payload) };
        let edge = Edge { before: stage.constant(u64::from(transition.before)), after: stage.constant(u64::from(transition.after)),
            slot: stage.constant(u64::from(transition.slot)) };
        let link = stage.constant(u64::from(link));
        site_add(&mut stage, records, object, (edge, Some(link)), form, value, slow)?;
        self.ins().jump(done, &[]);
        Some((slow, done))
    }

    /// The VM's own polymorphic and adding entries, the slow arm of an add and the whole of one the site never recorded.
    fn field_add_helper(&mut self, object: ir::Value, (atom, site): (Atom, u16), value: ir::Value, exit: ir::Block) {
        let write = self.atom_site_word(atom, site);
        let payload = self.payload;
        let stored = self.helper_call(LaneHelper::FieldAdd, &[payload, object, write, value]);
        self.forget_append_rooms(self.at);
        let hit = self.ins().icmp_imm_u(IntCC::NotEqual, stored, 0);
        emit_js_guard(&mut self.builder, hit, exit);
    }
}
