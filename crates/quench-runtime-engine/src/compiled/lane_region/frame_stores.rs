//! The lane frame's store callout (`staged::PropertyFrame`): the write through the slot a loop's entry
//! projected. The `SetField` row decides which store a site makes. An own-field store that no loop slot or
//! class arm takes is the VM's host store (`PropertyFrame::field_store_host`).

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;
use super::super::object_slots::emit;
use quench_runtime::staged::Region;
use quench_runtime::staged::flow::FlowStage;

impl EmitFrame<'_> {
    /// The int32 `value` stored through the slot a loop's entry projected (`LoopStore`): a
    /// packed object takes the narrow word, an arena object the full one, both without the
    /// store barrier a verified integer never needs. A value past the packed range leaves at `miss`.
    pub(crate) fn loop_slot_store(&mut self, stage: &mut Emit<'_, '_>, site: u16, value: ir::Value, miss: ir::Block) -> Option<()> {
        let facts = self.fields.as_ref()?;
        let (slot, property) = (facts.held.slot?, regions::property_flags(facts.property));
        #[cfg(test)] LOOP_SLOT_STORES.with(|count| count.set(count.get() + 1));
        let builder = &mut *stage.builder;
        if super::number_domains::is_double(slot.domain) {
            // The entry admitted the slot's raw doubles and left a packed record: the word goes in as it is.
            emit(builder).raw_store(Lane::F64, property, (value, false), slot.address, 0);
            return Some(());
        }
        let Some(inline) = slot.header.inline else {
            // A fixed class's cell holds its union slots flat, with no storage form to test. A verified integer needs no barrier.
            emit(builder).raw_store(Lane::I64, property, (value, true), slot.address, 0);
            return Some(());
        };
        let (packed, arena, done) = (builder.create_block(), builder.create_block(), builder.create_block());
        emit(builder).branch(inline, (packed, &[]), (arena, &[]));
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
        let mut writer = emit(builder);
        let narrow = writer.uextend32(narrow);
        writer.store_private_u32(Region::Payload.mutable(), slot.address, 0, narrow);
        emit(builder).jump(done, &[]);
        builder.switch_to_block(arena);
        // Verified integers neither widen an arena slot nor create a heap edge.
        emit(builder).raw_store(Lane::I64, property, (value, true), slot.address, 0);
        emit(builder).jump(done, &[]);
        builder.switch_to_block(done);
        Some(())
    }
}
