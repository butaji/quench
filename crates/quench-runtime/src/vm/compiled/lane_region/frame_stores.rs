//! The lane frame's store callouts (`staged::PropertyFrame`): the VM's own-field hit through
//! the slot the entry bound for its site, and the write through the slot a loop's entry
//! projected. The `SetField` row decides which store a site makes; this frame owns the
//! addresses it is proved under, bound before the run.

use super::*;
use crate::vm::compiled::regions;
use crate::staged::property::Owner;
use crate::staged::{store, PropertyFrame, Stage};

impl EmitFrame<'_> {
    /// The VM's own-field store hit of `value`, of type `fact`, through the slot the entry bound
    /// for `site` first where its writes admit the type: the run's admission of the atom and
    /// the slot's, then the shared barrier gate and the full or packed word the object's storage
    /// takes (`staged::store`); anything else, and a site with no bound slot, takes the plain
    /// hit (`PropertyFrame::field_store_host`). The object's record is proved before the
    /// branches, which both use.
    pub(super) fn bound_store(&mut self, stage: &mut Emit<'_, '_>, owner: Owner, object: ir::Value,
        (atom, site): (u32, u16), (value, fact): (ir::Value, Type), miss: ir::Block) -> Option<()> {
        let entry = self.entry();
        let binding = entry.and_then(|entry| entry.fields.iter().find_map(|(known, binding)| (*known == site).then_some(*binding)))
            .filter(|binding| binding.writes.is_some_and(|domain| fact.is_subtype(domain)));
        let (Some(entry), Some(binding)) = (entry, binding) else {
            return self.field_store_host(stage, object, atom, site, value, miss);
        };
        let facts = self.fields.as_ref()?;
        let admitted = facts.held.admitted?;
        let header = self.proof_key(owner).map(|key| headers::object_header(stage.builder, self.registers, self.state,
            (entry, self.payload), key, false, miss));
        let (slow, done, packed) = (stage.builder.create_block(), stage.builder.create_block(), stage.builder.create_block());
        let packed_address = stage.builder.append_block_param(packed, types::I64);
        if let Some(bound) = binding.admitted { emit_js_guard(stage.builder, bound, slow); }
        emit_js_guard(stage.builder, admitted, slow);
        let id = stage.and_imm(object, u64::from(u32::MAX));
        let (address, _) = emit_js_cached_field_slot(stage.builder, self.payload, object,
            site, false, slow, Some(binding), header, Some(packed));
        store::bound_full_store(stage, id, address, (value, fact), slow)?;
        stage.builder.ins().jump(done, &[]);
        stage.builder.switch_to_block(packed);
        store::bound_packed_store(stage, id, packed_address, (value, fact), slow)?;
        stage.builder.ins().jump(done, &[]);
        stage.builder.switch_to_block(slow);
        self.field_store_host(stage, object, atom, site, value, miss)?;
        stage.builder.ins().jump(done, &[]);
        stage.builder.switch_to_block(done);
        Some(())
    }

    /// The int32 `value` stored through the slot a loop's entry projected (`LoopStore`): a
    /// packed object takes the narrow word, an arena object the full one, both without the
    /// store barrier a verified integer never needs. A value past the packed range leaves at `miss`.
    pub(super) fn loop_slot_store(&mut self, stage: &mut Emit<'_, '_>, site: u16, value: ir::Value, miss: ir::Block) -> Option<()> {
        let slot = self.fields.as_ref()?.held.slot?;
        #[cfg(test)] LOOP_SLOT_STORES.with(|count| count.set(count.get() + 1));
        let builder = &mut *stage.builder;
        let (packed, arena, done) = (builder.create_block(), builder.create_block(), builder.create_block());
        builder.ins().brif(slot.header.inline, packed, &[], arena, &[]);
        builder.switch_to_block(packed);
        #[cfg(test)] if slot.packed_range.is_some() { PACKED_RANGE_SITE.with(|watched| {
            if let Some((expected, count)) = watched.get() && expected == site {
                watched.set(Some((expected, count + 1)));
            }
        }); }
        #[cfg(not(test))] let _ = site;
        let narrow = emit_narrow_integer_write(builder, value, miss, slot.packed_range);
        // Ordinary JS own fields are not shared memory. Observation requires
        // a guest/host effect or poll; the backend must prove their absence
        // before deferring this numeric, barrier-free write to an exit.
        let store = builder.ins().store(regions::payload(), narrow, slot.address, 0);
        builder.func.private_stores.push(store);
        builder.ins().jump(done, &[]);
        builder.switch_to_block(arena);
        // Verified integers neither widen an arena slot nor create a heap edge.
        let store = builder.ins().store(regions::object_slots(), value, slot.address, 0);
        builder.func.private_stores.push(store);
        builder.ins().jump(done, &[]);
        builder.switch_to_block(done);
        Some(())
    }
}
