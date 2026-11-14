//! Strings of parts (`JsString::from_parts`): traced while they are parts, and written out as
//! flat text on first read.
use super::*;
use super::records::{ParallelRecordMarks, StringRecordSpace};
use crate::vm::wtf16::{Part, Parts, Units};

/// The strings the just-marked string `record_id` is made of, pushed onto `work`.
pub(super) fn trace_parts(records: &mut StringRecordSpace, record_id: RecordId, work: &mut Vec<Value>) {
    // SAFETY: the caller just marked the record, so it is occupied.
    let Some(parts) = (unsafe { records.get_live(record_id) }).parts().copied() else { return };
    parts.for_each_text(|part| if !mark_leaf_part(|id| mark_flat(records, id), part) { work.push(Value::direct_string(part)) });
}

/// `trace_parts` through a parallel view.
pub(super) fn trace_parts_if_new(records: &ParallelRecordMarks<'_, JsString>, record_id: RecordId, work: &mut Vec<Value>) {
    // SAFETY: the caller's view just marked the occupied record.
    if let Some(parts) = unsafe { records.get(record_id) }.parts() {
        parts.for_each_text(|part| if !mark_leaf_part(|id| mark_flat_if_new(records, id), part) { work.push(Value::direct_string(part)) });
    }
}

/// Marks the string `record_id` that an object property holds where the trace finds it
/// (`Heap::object_children_marking`), and traces its parts as its visit would: a flat part is
/// marked in place and any other goes to `work`, so the string itself never enters it.
pub(super) fn mark_leaf_string_if_new(records: &ParallelRecordMarks<'_, JsString>, record_id: RecordId, work: &mut Vec<Value>) -> bool {
    if cfg!(feature = "profile-aggregate") {
        return false;
    }
    if records.mark_if_new(record_id) {
        trace_parts_if_new(records, record_id, work);
    }
    true
}

/// Marks a flat part where the trace finds it, as an array's flat string elements are
/// (`Heap::mark_leaf`): a flat string holds no edges, so pushing it would only pop a value
/// already marked or one whose mark is the whole visit (V8-v7 Splay: three parts per leaf
/// string, about two fifths of a full collection's pops). Profiling builds push every part,
/// so each visit is counted.
#[inline(always)]
fn mark_leaf_part(mark_flat: impl FnOnce(RecordId) -> bool, part: u32) -> bool {
    !cfg!(feature = "profile-aggregate") && mark_flat(part)
}

/// Marks the string `record_id` of an array element if it holds no parts, which a leaf visit
/// would skip; answers whether it did.
pub(super) fn mark_flat(records: &mut StringRecordSpace, record_id: RecordId) -> bool {
    // SAFETY: an array holds live strings.
    let flat = unsafe { records.get_live(record_id) }.parts().is_none();
    if flat {
        records.mark(record_id);
    }
    flat
}

/// `mark_flat` through a parallel view.
pub(super) fn mark_flat_if_new(records: &ParallelRecordMarks<'_, JsString>, record_id: RecordId) -> bool {
    // SAFETY: an array holds live strings.
    let flat = unsafe { records.get(record_id) }.parts().is_none();
    if flat {
        records.mark_if_new(record_id);
    }
    flat
}


impl Heap {
    /// The string record `record_id`, written out first if it is parts.
    ///
    /// # Safety
    /// `record_id` names a live string, retained by a reachable Value.
    #[inline(always)]
    pub(crate) unsafe fn written_out_string(&self, record_id: u32) -> &JsString {
        // SAFETY: the caller's contract.
        let string = unsafe { self.string_records.get_live(record_id) };
        if string.parts().is_none() {
            return string;
        }
        // SAFETY: as above.
        unsafe { self.write_out_string(record_id) }
    }

    /// `alloc_string` of the string of `parts`: its record, which carries no payload and nothing
    /// a one-unit string's cache or the materialized-text ledger would add.
    #[inline(always)]
    pub(crate) fn alloc_parts_string(&mut self, parts: Parts) -> Value {
        self.allocated += 1;
        match self.string_records.try_insert(JsString::from_parts(parts)) {
            Ok(record_id) => Value::direct_string(record_id),
            Err(_) => panic!("string record space exhausted"),
        }
    }

    /// The text of `parts`, each part written out first.
    fn parts_text(&self, parts: &Parts) -> JsString {
        let mut numbers = smallvec::SmallVec::<[crate::number_to_string::Short; 1]>::new();
        let mut length = 0;
        for part in parts.each() {
            length += match part {
                // SAFETY: a part is a string the parts string keeps alive.
                Part::Text(part_id) => unsafe { self.written_out_string(part_id) }.len(),
                Part::Number(number) => {
                    let mut text = crate::number_to_string::Short::new();
                    crate::number_to_string::write(number, &mut text);
                    numbers.push(text);
                    numbers.last().map_or(0, |text| text.len())
                }
            };
        }
        let mut numbers = numbers.iter();
        JsString::from_ascii_parts(
            length,
            parts.each().map(|part| match part {
                // SAFETY: as above; written out by the pass before.
                Part::Text(part_id) => match unsafe { self.written_out_string(part_id) }.units() {
                    Units::Ascii(bytes) => bytes,
                    Units::Wide(_) => unreachable!("a part is ASCII (`Vm::string_of_parts`)"),
                },
                Part::Number(_) => numbers.next().expect("a text per number").as_slice(),
            }),
        )
    }

    /// Writes the text of the parts record `record_id` over it, and returns the result.
    ///
    /// # Safety
    /// As `written_out_string`.
    #[cold]
    #[inline(never)]
    unsafe fn write_out_string(&self, record_id: u32) -> &JsString {
        // SAFETY: the caller's contract.
        let parts = *unsafe { self.string_records.get_live(record_id) }
            .parts()
            .expect("the caller found parts");
        let text = self.parts_text(&parts);
        let record = self.string_records.base_ptr().expect("a live record has a space").cast_mut();
        // SAFETY: parts need no drop, no reference to this record is live (a string of parts
        // is read only through `written_out_string`, which writes it out first, and by the
        // collector, which does not run now), and the record is initialized.
        unsafe {
            record.add(record_id as usize).write(text);
            &*record.add(record_id as usize)
        }
    }

    /// Writes out every string of parts, for a snapshot, which holds flat text only.
    pub(crate) fn write_out_all_strings(&self) {
        let mut parts = Vec::new();
        self.string_records.for_each_live(|record_id, string| {
            if string.parts().is_some() {
                parts.push(record_id);
            }
        });
        for record_id in parts {
            // SAFETY: the id was live a moment ago and nothing was freed since.
            unsafe { self.written_out_string(record_id) };
        }
    }
}
