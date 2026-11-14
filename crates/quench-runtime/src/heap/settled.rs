//! What a mark visit settles as it finds an edge, where pushing the edge would cost a pop to
//! learn there is nothing to visit: a string without parts, which marking visits completely, and
//! a prototype marked already, which most objects of a kind share (an object's prototype is
//! pushed whenever it differs from the last object's, and kinds alternate through a graph).
use super::records::{
    ArrayRecordSpace, FunctionRecordSpace, ObjectRecordSpace, ParallelRecordMarks, RecordId, RecordSpace, StringRecordSpace,
};
use super::{string_parts, Value};
use crate::value::HeapReference;

/// A record family's mark bits, as the serial and the parallel mark read them.
pub(super) trait RecordMarked {
    fn is_marked(&self, id: RecordId) -> bool;
}

impl<T, const CAPACITY: usize> RecordMarked for RecordSpace<T, CAPACITY> {
    #[inline(always)]
    fn is_marked(&self, id: RecordId) -> bool {
        RecordSpace::is_marked(self, id)
    }
}

impl<T> RecordMarked for ParallelRecordMarks<'_, T> {
    #[inline(always)]
    fn is_marked(&self, id: RecordId) -> bool {
        ParallelRecordMarks::is_marked(self, id)
    }
}

/// Whether `value` names an object, array or function record, marked in the given families.
#[inline(always)]
pub(super) fn marked_prototype(value: Value, objects: &impl RecordMarked, arrays: &impl RecordMarked, functions: &impl RecordMarked) -> bool {
    match value.heap_reference() {
        Some(HeapReference::ObjectRecord(id)) => objects.is_marked(id),
        Some(HeapReference::ArrayRecord(id)) => arrays.is_marked(id),
        Some(HeapReference::FunctionRecord(id)) => functions.is_marked(id),
        _ => false,
    }
}

pub(super) trait Settled {
    /// Marks the string `record_id` when it holds no parts, and answers whether it did.
    fn mark_flat_string(&mut self, record_id: RecordId) -> bool;

    /// Whether `value` names an object, array or function record that is marked.
    fn is_marked_prototype(&self, value: Value) -> bool;

    /// Marks `value` where the trace finds it when it is a string without parts. Profiling
    /// builds leave every value to the visit, which counts each.
    #[inline(always)]
    fn leaf(&mut self, value: Value) -> bool {
        !cfg!(feature = "profile-aggregate")
            && value.direct_string_index().is_some_and(|record_id| self.mark_flat_string(record_id))
    }
}

/// Settles nothing: the trace of a cell no record space holds.
pub(super) struct Unsettled;

impl Settled for Unsettled {
    #[inline(always)]
    fn mark_flat_string(&mut self, _: RecordId) -> bool {
        false
    }

    #[inline(always)]
    fn is_marked_prototype(&self, _: Value) -> bool {
        false
    }
}

/// The serial mark's view of the families a visit settles in.
pub(super) struct SerialSettled<'a> {
    pub(super) strings: &'a mut StringRecordSpace,
    pub(super) objects: &'a ObjectRecordSpace,
    pub(super) arrays: &'a ArrayRecordSpace,
    pub(super) functions: &'a FunctionRecordSpace,
}

impl Settled for SerialSettled<'_> {
    #[inline(always)]
    fn mark_flat_string(&mut self, record_id: RecordId) -> bool {
        string_parts::mark_flat(self.strings, record_id)
    }

    #[inline(always)]
    fn is_marked_prototype(&self, value: Value) -> bool {
        marked_prototype(value, self.objects, self.arrays, self.functions)
    }
}
