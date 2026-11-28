//! In-place fast paths for `Object.keys` and `Object.assign` on plain objects: each completes
//! the call with no guest code and no raised error, or answers `None` so the general path runs.
use super::super::object_symbols::{PLAIN_COPY_INLINE_PROPERTIES, PropertyCopyKind};
use super::super::*;

impl Vm {
    /// `Object.keys(object)` of a plain object whose shape lists its enumerable string keys
    /// (`shape_enumerable_slots`), as V8's enum cache answers it: the shape's key strings
    /// (`Vm::own_enumerable_key_values_cached`, made once per shape and collection) are copied
    /// into a new array in one allocation. `None` leaves every other object to
    /// `plain_enumerable_own_properties`.
    #[inline]
    pub(in crate::vm) fn plain_object_keys(&mut self, object: Value) -> Option<Value> {
        let shape = self.enum_cache_shape(object)?;
        let keys = self.own_enumerable_key_values_of_shape(object, shape)?;
        Some(self.array_of(&keys[..]))
    }

    /// The shape of `object` where it alone decides `object`'s enumerable string keys, as
    /// `shape_enumerable_slots` requires: an ordinary object record (no array, typed array or
    /// Proxy, which live elsewhere) without exotic access (arguments object, module namespace),
    /// side-table descriptors or a boxed primitive (a String wrapper's indices), whose shape
    /// is no dictionary. A memo entry for that shape was made from such an object, so it
    /// answers for this one too.
    #[inline(always)]
    fn enum_cache_shape(&self, object: Value) -> Option<u32> {
        self.enum_cache_shape_of(self.heap.direct_object_view(object)?)
    }

    /// `enum_cache_shape` of the ordinary object record `data`, for a caller that holds it.
    #[inline(always)]
    fn enum_cache_shape_of(&self, data: ObjectRef<'_>) -> Option<u32> {
        if data.has_exotic_access() || data.has_side_descriptors() || data.boxed_primitive().is_some() {
            return None;
        }
        let shape = data.shape();
        (!self.shape_is_dictionary(shape)).then_some(shape)
    }

    /// `Object.assign(target, source)` of one plain source into a fresh empty `target`, where
    /// the shape memo answers the copy (`plain_assign_shapes`): `target` takes the shape the
    /// source's shape reached and the source's slot values in one initialization
    /// (`initialize_plain_copy`), with one probe per memo. `false`, before any effect, leaves the
    /// copy to `copy_plain_properties`.
    fn copy_into_fresh_empty(&mut self, target: Value, source: Value) -> bool {
        let Some(target_data) = self.heap.direct_object_view(target) else {
            return false;
        };
        let (target_shape, target_proto) = (target_data.shape(), target_data.proto());
        if target_proto != self.object_proto || !self.is_fresh_empty(target_data) {
            return false;
        }
        let Some(source_data) = self.heap.direct_object_view(source) else {
            return false;
        };
        let Some(source_shape) = self.enum_cache_shape_of(source_data) else {
            return false;
        };
        let memoized = self
            .plain_assign_shapes
            .get(target_shape, (source_shape, target_proto))
            .filter(|entry| entry.cell.holds());
        let (Some(entry), Some(slots)) = (memoized, self.enumerable_keys.get(source_shape)) else {
            return false;
        };
        // The memo's word proof makes the source's contiguous words the target's: one run copy.
        // `target` is fresh (`is_fresh_empty`): no prototype, so the adds name no method cache's
        // chain and need no `note_plain_copy_atoms`.
        if entry.words == Some(self.heap.slot_encoding_epoch()) && self.heap.copy_slot_words(target, source, entry.shape) {
            return true;
        }
        let Some(source_data) = self.heap.direct_object_view(source) else {
            return false;
        };
        let mut values = smallvec::SmallVec::<[Value; PLAIN_COPY_INLINE_PROPERTIES]>::from_elem(Value::UNDEFINED, slots.len());
        if !self.read_shaped_values(source_data, &slots, &mut values) {
            return false;
        }
        self.heap.initialize_object_properties(target, entry.shape, &values);
        true
    }

    /// Whether `data` is an extensible ordinary object with no storage, side descriptors, dictionary
    /// shape or prototype role: an empty object whose first copy may take its shape at once.
    /// `plain_object_assign` has already excluded exotic access.
    fn is_fresh_empty(&self, data: ObjectRef<'_>) -> bool {
        data.is_extensible()
            && !data.has_side_descriptors()
            && !data.was_used_as_prototype()
            && !self.shape_is_dictionary(data.shape())
            && self.shapes[data.shape() as usize].storage_len() == 0
    }

    /// `Object.assign(target, ...sources)` where every copy is a plain one
    /// (`copy_plain_properties` with `Set`): `target` is an ordinary object record without
    /// exotic access, so ToObject leaves it as it is, and each source is null, undefined, or an
    /// ordinary object record whose own enumerable properties are string-keyed data properties
    /// its shape lists. A single source into a fresh empty `target` takes the memoized copy first
    /// (`copy_into_fresh_empty`). Otherwise an empty target reaching a shape an earlier copy of
    /// the source's shape reached takes that shape and the source's slot values at once
    /// (`plain_assign_shapes`); any other target takes each key by the plain property store.
    /// `None` for anything else.
    ///
    /// A source that declines comes after the earlier sources were copied, and the general
    /// native then copies every source again. Repeating those copies is unobservable: each read
    /// a data property of an ordinary object and wrote a writable data property of `target`,
    /// with no guest code, so the general path reads the same values and writes them again.
    pub fn plain_object_assign(&mut self, args: &[Value]) -> Option<Value> {
        let (&target, sources) = args.split_first()?;
        if self.heap.direct_object_view(target)?.has_exotic_access() {
            return None;
        }
        if let [source] = sources && self.copy_into_fresh_empty(target, *source) {
            return Some(target);
        }
        for &source in sources {
            if source.is_null() || source.is_undefined() {
                continue;
            }
            let copied = self.heap.direct_object_view(source).is_some()
                && self.copy_plain_properties(target, source, &[], PropertyCopyKind::Set);
            if !copied {
                return None;
            }
        }
        Some(target)
    }
}
