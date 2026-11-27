//! Raw slot-word copies between objects whose slots encode alike (`ValueArena::encodings_match`):
//! the words of a source's values become the target's words, with no decode and no recode. The
//! write barrier and the storage replacement are those of `initialize_object_properties`.
use super::*;

impl Heap {
    /// The epoch under which slot encodings are current (`ValueArena::owner_epoch`).
    #[inline(always)]
    pub(crate) fn slot_encoding_epoch(&self) -> u64 {
        self.properties.owner_epoch()
    }

    /// Whether the first `len` slots of shapes `source` and `target` encode alike
    /// (`ValueArena::encodings_match`).
    pub(crate) fn slot_encodings_match(&self, source: u32, target: u32, len: usize) -> bool {
        self.properties.encodings_match(source, target, len)
    }

    /// Gives `target`, an empty object, the `shape` values of `source` as one arena run of its own,
    /// copying the words as they are. `source` must be an object outside dictionary storage whose
    /// `shape` lists `len` slots in arena words, and the caller proved the encodings match. Tracking
    /// needs no join: the words are the source's, whose owners hold their types, and the target's
    /// owners hold the same types (`encodings_match`). `false`, before any effect, for a source
    /// whose values are not one compact wide run.
    pub(crate) fn copy_slot_words(&mut self, target: Value, source: Value, shape: u32) -> bool {
        let Some(source_vector) = self.object_ref(source).map(|object| object.record().properties()) else {
            return false;
        };
        let len = self.properties.shape_length(shape);
        if len == 0
            || self.properties.len(source_vector) != len
            || !source_vector.has_compact_range()
            || source_vector.is_narrow()
            || self.object_ref(target).is_none()
        {
            return false;
        }
        let Some(vector) = self.properties.copy_run(source_vector, len, shape) else {
            return false;
        };
        let (mut object, arena) = self.object_and_arena_mut_ref(target).expect("property owner is an object");
        let previous = object.record().replace_property_storage(vector, [Value::UNDEFINED; INLINE_PROPERTY_COUNT], arena);
        arena.release(previous);
        true
    }
}
