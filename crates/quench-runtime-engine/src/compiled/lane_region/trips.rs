//! The loop tier's pricing input: the iterations a region run completed natively
//! (`LoopEntry::backedge_count`, read by `LoopSite::note_run`). The hot loop only advances a
//! register; the record is written once, where the run leaves.

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;

impl Lowering<'_, '_> {
    /// Starts the trip count of a loop region at zero. A lane body (`Coverage::Prefix`) counts nothing.
    pub(crate) fn begin_trips(&mut self) {
        if !matches!(self.coverage, Coverage::Loop { .. }) { return; }
        let trips = self.builder.declare_var(types::I64);
        let zero = self.raw_const(Lane::I64, 0);
        self.builder.def_var(trips, zero);
        self.trips = Some(trips);
    }

    /// Counts one taken execution of the region's backward edge.
    pub(crate) fn count_trip(&mut self) {
        let Some(trips) = self.trips else { return };
        let count = self.builder.use_var(trips);
        let next = self.raw_binary_imm(Binary::Add, count, 1);
        self.builder.def_var(trips, next);
    }

    /// Adds the run's trips to its loop entry record, in the block the run leaves through.
    pub(crate) fn flush_trips(&mut self) {
        let Some(trips) = self.trips else { return };
        let offset = i32::try_from(std::mem::offset_of!(super::super::JsStepDispatch, head.loop_entry))
            .expect("dispatch offset fits i32");
        let payload = self.payload;
        let entry = self.raw_load(Lane::I64, regions::dispatch(), payload, offset);
        let absent = self.raw_compare_imm(Relation::Equal, entry, 0);
        let (skip, count) = (self.builder.create_block(), self.builder.create_block());
        self.builder.set_cold_block(count);
        self.branch_if(absent, (skip, &[]), (count, &[]));
        self.builder.switch_to_block(count);
        let run = self.builder.use_var(trips);
        let word = self.raw_load(Lane::I64, regions::miss_counter(), entry, 0);
        // The record's word is the budget left, so the run's trips are debited from it.
        let total = self.raw_binary(Binary::Sub, word, run);
        self.store_word(regions::miss_counter(), total, entry, 0);
        self.goto(skip, &[]);
        self.builder.switch_to_block(skip);
    }
}
