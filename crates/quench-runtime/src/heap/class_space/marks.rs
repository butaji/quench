//! The collector's view of the cell regions (a split of `heap::class_space`): the mark bits of cells, the parallel
//! mark views the workers read through, the sweep that frees dead cells with their spill runs and dictionary tables,
//! and the cell regions as record families. The cell layout and the region addressing stay in `class_space`.

use super::{CELL_REGIONS, ClassCell1, ClassCell2, ClassCell3, ClassCell4, ClassCell5, ClassSpace, locate};
use crate::heap::extras_space::ExtrasSpace;
use crate::heap::pacing::PromotionAge;
use crate::heap::records::{ParallelRecordMarks, RecordFamily};
use crate::heap::spill::{SpillSpace, dispose_dead_runs};
use crate::value_vec::ValueArena;

/// The cell regions' parallel mark views, one per region `1..CAPACITY_CLASSES` (`ParallelRecordMarks`).
#[derive(Clone, Copy)]
pub(in crate::heap) struct ClassCellMarks<'space> {
    regions: (
        ParallelRecordMarks<'space, ClassCell1>,
        ParallelRecordMarks<'space, ClassCell2>,
        ParallelRecordMarks<'space, ClassCell3>,
        ParallelRecordMarks<'space, ClassCell4>,
        ParallelRecordMarks<'space, ClassCell5>,
    ),
}

impl ClassCellMarks<'_> {
    /// Marks cell `g` for a worker if it is new (`ParallelRecordMarks::mark_if_new`). A stub (region 0) has no
    /// mark of its own, as `ClassSpace::mark_cell`: it answers false.
    pub(in crate::heap) fn mark_if_new(&self, g: u32) -> bool {
        let (region, local) = locate(g);
        let regions = &self.regions;
        match region {
            0 => false,
            1 => regions.0.mark_if_new(local),
            2 => regions.1.mark_if_new(local),
            3 => regions.2.mark_if_new(local),
            4 => regions.3.mark_if_new(local),
            5 => regions.4.mark_if_new(local),
            _ => unreachable!("a parallel mark of a granule past the cell regions"),
        }
    }
}

impl ClassSpace {
    /// Marks granule `g`'s cell in its region (`RecordSpace::mark`); whether it was newly marked. A stub (region
    /// 0) has no mark of its own, so there this does nothing and its record is marked instead. A class cell
    /// in a cell region is marked with its Value: its granule stays live while the Value does.
    pub(in crate::heap) fn mark_cell(&mut self, g: u32) -> bool {
        let (region, local) = locate(g);
        if region == 0 {
            return false;
        }
        with_region_mut!(self.regions, region, |space| space.mark(local))
    }

    /// Whether cell `g` is marked in its region (`RecordSpace::is_marked`).
    pub(in crate::heap) fn is_marked(&self, g: u32) -> bool {
        let (region, local) = locate(g);
        with_region!(self.regions, region, |space| space.is_marked(local))
    }

    pub(in crate::heap) fn is_live(&self, g: u32) -> bool {
        let (region, local) = locate(g);
        with_region!(self.regions, region, |space| space.is_live(local))
    }

    /// Sweeps the cell regions (`RecordSpace::sweep`): an unmarked cell is freed, a dead cell that spilled frees its
    /// run, a dead dictionary cell its table in the value arena `values`, and a dead cell its extras in `extras`
    /// (`dispose_dead_runs`). `spill_of` gives
    /// the spill slot of a class over the arena's shape lengths (`layouts::class_spill`). Region 0's stubs die with
    /// their records and are not swept here.
    pub(in crate::heap) fn sweep_cells(
        &mut self,
        values: &mut ValueArena,
        extras: &mut ExtrasSpace,
        full: bool,
        age: PromotionAge,
        spill_of: impl Fn(&ValueArena, u32) -> Option<u32>,
    ) -> (usize, usize) {
        let ClassSpace { regions, spill, base, .. } = self;
        let base = *base;
        let sweeps = [
            regions.1.sweep_disposing(full, age, false, dispose_dead_runs::<ClassCell1, _>(&mut *spill, &mut *values, &mut *extras, base, 1, &spill_of), |_| {}),
            regions.2.sweep_disposing(full, age, false, dispose_dead_runs::<ClassCell2, _>(&mut *spill, &mut *values, &mut *extras, base, 2, &spill_of), |_| {}),
            regions.3.sweep_disposing(full, age, false, dispose_dead_runs::<ClassCell3, _>(&mut *spill, &mut *values, &mut *extras, base, 3, &spill_of), |_| {}),
            regions.4.sweep_disposing(full, age, false, dispose_dead_runs::<ClassCell4, _>(&mut *spill, &mut *values, &mut *extras, base, 4, &spill_of), |_| {}),
            regions.5.sweep_disposing(full, age, false, dispose_dead_runs::<ClassCell5, _>(&mut *spill, &mut *values, &mut *extras, base, 5, &spill_of), |_| {}),
        ];
        sweeps.into_iter().fold((0, 0), |(live, freed), (swept_live, swept_freed)| (live + swept_live, freed + swept_freed))
    }

    /// The cell regions as record families, which `Heap::record_families` lists after the record spaces.
    pub(in crate::heap) fn cell_families(&self) -> [&dyn RecordFamily; CELL_REGIONS] {
        [
            &self.regions.1 as &dyn RecordFamily,
            &self.regions.2 as &dyn RecordFamily,
            &self.regions.3 as &dyn RecordFamily,
            &self.regions.4 as &dyn RecordFamily,
            &self.regions.5 as &dyn RecordFamily,
        ]
    }

    pub(in crate::heap) fn cell_families_mut(&mut self) -> [&mut dyn RecordFamily; CELL_REGIONS] {
        let regions = &mut self.regions;
        [
            &mut regions.1 as &mut dyn RecordFamily,
            &mut regions.2 as &mut dyn RecordFamily,
            &mut regions.3 as &mut dyn RecordFamily,
            &mut regions.4 as &mut dyn RecordFamily,
            &mut regions.5 as &mut dyn RecordFamily,
        ]
    }

    /// The parallel mark's views of the cell regions, taken with the record families' (`ParallelMarkView::new`).
    pub(in crate::heap) fn parallel_marks(&mut self) -> (ClassCellMarks<'_>, &SpillSpace) {
        let regions = &mut self.regions;
        let marks = ClassCellMarks {
            regions: (
                regions.1.parallel_marks(),
                regions.2.parallel_marks(),
                regions.3.parallel_marks(),
                regions.4.parallel_marks(),
                regions.5.parallel_marks(),
            ),
        };
        (marks, &self.spill)
    }
}
