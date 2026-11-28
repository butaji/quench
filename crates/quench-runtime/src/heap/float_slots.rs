//! The words of slots whose owners hold raw doubles (`ValueArena::float_slots`). A word is written in
//! the encoding its owner's type has when it is written (`encode_slot`), and read as any owner's
//! (`decode_slot`); an owner that enters or leaves the Number type (`ValueArena::take_widened`) makes
//! its slots' words wait in the old encoding, which the heap recodes here, before the next
//! store or compile relies on one encoding. A recoding is idempotent (`ValueArena::recode_owned_run`),
//! so the order of recodings and stores does not matter.
use super::*;

/// The arena run of `object`'s tracked slots: its start, exact shape and length, where the object
/// keeps its values in the arena outside dictionary storage and exotic access.
pub(super) fn arena_run_of(properties: &ValueArena, object: &Object) -> Option<(usize, u32, usize)> {
    let shape = object.exact_shape();
    if shape != object.shape() { return None; }
    let start = match object.exact_storage() {
        ExactStorage::Plain(start) => start,
        ExactStorage::Extended { inline: false } => object.extras_storage_start()?,
        // Packed and narrow halves hold narrow values, whose words no owner's type recodes.
        ExactStorage::PackedInline | ExactStorage::Extended { inline: true } | ExactStorage::Narrow => return None,
    };
    Some((start, shape, properties.shape_length(shape)))
}

impl Heap {
    /// Visits every live object-bearing record and legacy cell with the property arena.
    pub(super) fn visit_live_objects(&mut self, mut visit: impl FnMut(&Object, &mut ValueArena)) {
        let properties = &mut self.properties;
        self.object_records.for_each_live(|_, object| visit(object, properties));
        self.array_records.for_each_live(|_, array| visit(array, properties));
        self.function_records.for_each_live(|_, function| visit(&function.object, properties));
        self.promise_records.for_each_live(|_, promise| visit(&promise.object, properties));
        self.iterator_records.for_each_live(|_, iterator| visit(&iterator.object, properties));
        self.collection_records.for_each_live(|_, collection| visit(&collection.object, properties));
        for slot in self.slots.iter() {
            if let Some(object) = slot.cell.as_ref().and_then(Cell::object) {
                visit(object, properties);
            }
        }
    }

    /// The arena runs of every live object's tracked slots (`arena_run_of`).
    pub(super) fn tracked_runs(&self) -> Vec<(usize, u32, usize)> {
        let properties = &self.properties;
        let mut runs = Vec::new();
        let mut visit = |object: &Object| runs.extend(arena_run_of(properties, object));
        self.object_records.for_each_live(|_, object| visit(object));
        self.array_records.for_each_live(|_, array| visit(array));
        self.function_records.for_each_live(|_, function| visit(&function.object));
        self.promise_records.for_each_live(|_, promise| visit(&promise.object));
        self.iterator_records.for_each_live(|_, iterator| visit(&iterator.object));
        self.collection_records.for_each_live(|_, collection| visit(&collection.object));
        for slot in self.slots.iter() {
            if let Some(object) = slot.cell.as_ref().and_then(Cell::object) { visit(object); }
        }
        runs
    }

    /// Recodes the words of the slots whose owners entered or left the Number type, in every live
    /// object: a store that widened an owner, or a join of new values, does it before it returns.
    #[inline(always)]
    pub(super) fn settle_widened(&mut self) {
        if self.properties.has_widened() { self.recode_widened_slots(); }
    }

    #[inline(never)]
    fn recode_widened_slots(&mut self) {
        let owners = self.properties.take_widened();
        self.visit_live_objects(|object, properties| {
            if let Some(run) = arena_run_of(properties, object) { properties.recode_owned_run(run, &owners); }
        });
    }

    /// Recodes every tracked slot to its owner's encoding, once the owners' types are joined anew
    /// from the values the objects hold (`register_shape_layouts`).
    pub(super) fn recode_all_slots(&mut self) {
        if !self.properties.float_slots() { return; }
        self.visit_live_objects(|object, properties| {
            if let Some((start, shape, len)) = arena_run_of(properties, object) { properties.recode_run(start, shape, len); }
        });
        drop(self.properties.take_widened());
    }

    /// Recodes a new object's words, made as tagged values, once its values are joined into their
    /// owners' types: the object is no live record yet, so `settle_widened` does not reach it.
    pub(super) fn recode_new_object(&mut self, object: &Object) {
        self.settle_widened();
        if let Some((start, shape, len)) = arena_run_of(&self.properties, object) {
            self.properties.recode_run(start, shape, len);
        }
    }
}
