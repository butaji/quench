//! What each record family keeps alive, copied out before a barriered write.
use super::*;

/// A record's children as the reference-count barrier copies them (`RecordSpace::rc_note`):
/// what the mark would push for it, with no shape roots.
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

impl records::RecordChildren for Object {
    fn children(&self, properties: &ValueArena, out: &mut Vec<Value>) {
        snapshot_object(self, properties, out);
    }
}

impl records::RecordChildren for ArrayObject {
    fn children(&self, properties: &ValueArena, out: &mut Vec<Value>) {
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

impl records::RecordChildren for FunctionRecord {
    fn children(&self, properties: &ValueArena, out: &mut Vec<Value>) {
        snapshot_object(&self.object, properties, out);
        out.extend([self.realm, self.env]);
    }
}

impl records::RecordChildren for PromiseCell {
    fn children(&self, properties: &ValueArena, out: &mut Vec<Value>) {
        snapshot_object(&self.object, properties, out);
        self.record.append_roots(out);
    }
}

impl records::RecordChildren for IteratorRecord {
    fn children(&self, properties: &ValueArena, out: &mut Vec<Value>) {
        snapshot_object(&self.object, properties, out);
        Heap::iterator_edges(self, out);
    }
}

impl records::RecordChildren for AwaitState {
    fn children(&self, _properties: &ValueArena, out: &mut Vec<Value>) {
        self.extend_roots(out);
    }
}
