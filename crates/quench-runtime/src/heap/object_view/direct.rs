//! The view of an ordinary object named by a direct `Value` or a prototype link's granule. Every ordinary object is a
//! class cell (S7f: no object record exists), so the view of a granule is its live cell.

use super::*;

impl Heap {
    /// The view of the direct object `value` names: a live class cell, or the record of a non-cell object. Every
    /// other value (an array, a function, a promise, a primitive) is `None`, as `object_record` makes it, so a
    /// caller that means ordinary objects only keeps that meaning.
    #[inline(always)]
    pub fn direct_object_view(&self, value: Value) -> Option<ObjectRef<'_>> {
        value.direct_object_index()?;
        self.object(value)
    }

    /// The view of the ordinary object a prototype link names by `granule` (`ProtoLink::object_granule`): a live
    /// class cell, its own view at its granule.
    #[inline(always)]
    pub(crate) fn object_view_by_granule(&self, granule: u32) -> ObjectRef<'_> {
        self.live_cell(granule)
    }

    /// `property_get_shaped` of a view: a record's shaped read, or a cell's word at `slot` (a shaped read tests no
    /// deletion; the caller proves the slot exists).
    ///
    /// # Safety
    /// As for `property_get_shaped`, for a record view.
    #[inline(always)]
    pub(crate) unsafe fn property_get_shaped_view(&self, object: ObjectRef<'_>, slot: usize) -> Option<Value> {
        match object {
            // SAFETY: as the caller guarantees.
            ObjectRef::Record(record) => unsafe { self.property_get_shaped(record, slot) },
            ObjectRef::Cell(cell) => cell.word(slot),
        }
    }

    /// `property_get_unchecked` of a view: a record's, or a cell's word, which the caller proves exists.
    ///
    /// # Safety
    /// As for `property_get_unchecked`, for a record view.
    #[inline(always)]
    pub(crate) unsafe fn property_get_unchecked_view(&self, object: ObjectRef<'_>, slot: usize) -> Value {
        match object {
            // SAFETY: as the caller guarantees.
            ObjectRef::Record(record) => unsafe { self.property_get_unchecked(record, slot) },
            ObjectRef::Cell(cell) => cell.word(slot).expect("the caller proves that the property slot exists"),
        }
    }
}
