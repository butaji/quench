//! The snapshot barrier: the copies a running cycle takes of the values its mutator overwrites
//! (`satb_note`), and the children each record kind contributes to a snapshot.

use super::*;

/// The snapshot barrier's copy of a record's children (`RecordSpace::satb_note`): what the
/// mark would push for it, with no shape roots (pushed per shape when a cycle starts) and
/// every string left to the cycle's mark.
fn snapshot_object(object: &Object, properties: &ValueArena, out: &mut Vec<Value>) {
    Heap::object_children(
        Value::UNDEFINED,
        object,
        properties,
        out,
        &mut |_, _, _, _, _| {},
        GcTraceKind::Collection,
        &mut PushedProtos::default(),
    );
}

impl records::SatbSnapshot for Object {
    fn snapshot(&self, properties: &ValueArena, out: &mut Vec<Value>) {
        snapshot_object(self, properties, out);
    }
}

impl records::SatbSnapshot for ArrayObject {
    fn snapshot(&self, properties: &ValueArena, out: &mut Vec<Value>) {
        Heap::array_children(
            Value::UNDEFINED,
            self,
            properties,
            |_| false,
            out,
            &mut |_, _, _, _, _| {},
            GcTraceKind::Collection,
            &mut PushedProtos::default(),
        );
    }
}

impl records::SatbSnapshot for FunctionRecord {
    fn snapshot(&self, properties: &ValueArena, out: &mut Vec<Value>) {
        snapshot_object(&self.object, properties, out);
        out.extend([self.realm, self.env]);
    }
}

impl records::SatbSnapshot for PromiseCell {
    fn snapshot(&self, properties: &ValueArena, out: &mut Vec<Value>) {
        snapshot_object(&self.object, properties, out);
        self.record.append_roots(out);
    }
}

impl records::SatbSnapshot for IteratorRecord {
    fn snapshot(&self, properties: &ValueArena, out: &mut Vec<Value>) {
        snapshot_object(&self.object, properties, out);
        Heap::iterator_edges(self, out);
    }
}

impl records::SatbSnapshot for AwaitState {
    fn snapshot(&self, _properties: &ValueArena, out: &mut Vec<Value>) {
        self.extend_roots(out);
    }
}

impl Heap {
    /// Turns the snapshot barrier on or off in every record family, as a concurrent cycle's
    /// start and remark will; the children it copies are taken with `take_satb_values`.
    pub(in crate::heap) fn set_satb(&mut self, active: bool) {
        self.satb_active = active;
        for (family, _) in self.record_families_mut() {
            if active { family.begin_satb() } else { family.end_satb() }
        }
    }

    /// Allocation is black while a cycle marks: a cell made now is marked at once, newer than
    /// the snapshot, so the cycle neither traces nor frees it.
    #[cold]
    #[inline(never)]
    pub(in crate::heap) fn blacken_slot(&mut self, index: usize) {
        let (word, bit) = slot_bit(index);
        *self.marks[word].get_mut() |= bit;
    }

    /// The children the snapshot barrier copied in every family since the last call.
    pub(in crate::heap) fn take_satb_values(&mut self) -> Vec<Value> {
        let mut values = Vec::new();
        for (family, _) in self.record_families_mut() {
            values.append(&mut family.take_satb_values());
        }
        values
    }
}
