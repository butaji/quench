//! What each record family keeps alive, copied out before a barriered write.
use super::*;
use records::TraceRecord;

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

impl records::TraceRecord for Object {
    fn extend_children(&self, properties: &ValueArena, out: &mut Vec<Value>) {
        snapshot_object(self, properties, out);
    }
}

impl records::TraceRecord for ArrayObject {
    fn extend_children(&self, properties: &ValueArena, out: &mut Vec<Value>) {
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

impl records::TraceRecord for FunctionRecord {
    fn extend_children(&self, properties: &ValueArena, out: &mut Vec<Value>) {
        snapshot_object(&self.object, properties, out);
        out.extend(self.realm_override());
        out.push(self.env);
    }
}

impl records::TraceRecord for PromiseCell {
    fn extend_children(&self, properties: &ValueArena, out: &mut Vec<Value>) {
        snapshot_object(&self.object, properties, out);
        self.record.append_roots(out);
    }
}

impl records::TraceRecord for IteratorRecord {
    fn extend_children(&self, properties: &ValueArena, out: &mut Vec<Value>) {
        snapshot_object(&self.object, properties, out);
        Heap::iterator_edges(self, out);
    }
}

impl records::TraceRecord for CollectionRecord {
    fn extend_children(&self, properties: &ValueArena, out: &mut Vec<Value>) {
        snapshot_object(&self.object, properties, out);
        Heap::collection_edges(&self.entries, out);
    }
}

impl records::TraceRecord for AwaitState {
    fn extend_children(&self, _properties: &ValueArena, out: &mut Vec<Value>) {
        self.extend_roots(out);
    }
}

impl records::TraceRecord for WasmGcRecord {
    fn extend_children(&self, _properties: &ValueArena, out: &mut Vec<Value>) {
        self.trace_children(out);
    }
}

impl records::TraceRecord for SymbolData {
    fn extend_children(&self, _properties: &ValueArena, _out: &mut Vec<Value>) {}
}

impl records::TraceRecord for EnvironmentRecord {
    fn extend_children(&self, _properties: &ValueArena, out: &mut Vec<Value>) {
        Heap::environment_children(self, out, &mut PushedProtos::default());
    }
}

impl Heap {
    /// Appends what the record `value` keeps alive, through its `TraceRecord` impl, for the
    /// reference-count barrier and the counting passes (`Heap::rc_children`). Answers whether
    /// `value` is one of the families that trait covers; a string, a legacy cell or a number is
    /// not, and the caller handles it. Environments, symbols and vectors are left out here as
    /// well: counts never named their edges.
    #[inline(always)]
    pub(super) fn record_children(&self, value: Value, out: &mut Vec<Value>) -> bool {
        let properties = &self.properties;
        // SAFETY: the caller names live records; each arm reads a live record of its family.
        unsafe {
            if let Some(id) = value.direct_object_index() {
                self.object_records.get_live(id).extend_children(properties, out);
            } else if let Some(id) = value.direct_array_index() {
                self.array_records.get_live(id).extend_children(properties, out);
            } else if let Some(id) = value.direct_function_index() {
                self.function_records.get_live(id).extend_children(properties, out);
            } else if let Some(id) = value.direct_promise_index() {
                self.promise_records.get_live(id).extend_children(properties, out);
            } else if let Some(id) = value.direct_await_index() {
                self.await_records.get_live(id).extend_children(properties, out);
            } else if let Some(id) = value.direct_iterator_index() {
                self.iterator_records.get_live(id).extend_children(properties, out);
            } else if let Some(id) = value.direct_collection_index() {
                self.collection_records.get_live(id).extend_children(properties, out);
            } else if let Some(id) = value.direct_wasm_gc_index() {
                self.wasm_gc_records.get_live(id).extend_children(properties, out);
            } else {
                return false;
            }
        }
        true
    }
}
