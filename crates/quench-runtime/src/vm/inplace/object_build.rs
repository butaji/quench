//! In-place fast paths for `Object.keys` and `Object.assign` on plain objects: each completes
//! the call with no guest code and no raised error, or answers `None` so the general path runs.
use super::super::object_symbols::PropertyCopyKind;
use super::super::*;

impl<H: Host> Vm<H> {
    /// `Object.keys(object)` of a plain object whose shape lists its enumerable string keys
    /// (`shape_enumerable_slots`), as V8's enum cache answers it: the key strings its shape's
    /// memo entry holds (`EnumerableKeyMemo::with_key_strings`) are copied into a new array in
    /// one pass. The first call on a shape, or the first after a collection, makes the strings
    /// and records them. `None` leaves every other object to `plain_enumerable_own_properties`.
    #[inline]
    pub(in crate::vm) fn plain_object_keys(&mut self, object: Value) -> Option<Value> {
        let shape = self.enum_cache_shape(object)?;
        let epoch = self.heap.collection_epoch();
        let array_proto = self.array_proto;
        let heap = &mut self.heap;
        let cached = self.enumerable_keys.with_key_strings(shape, epoch, |strings| {
            heap.alloc_literal_array(array_proto, strings.len(), |index| strings[index])
        });
        if cached.is_some() {
            return cached;
        }
        self.record_object_key_strings(object, shape, epoch)
    }

    /// The shape of `object` where it alone decides `object`'s enumerable string keys, as
    /// `shape_enumerable_slots` requires: an ordinary object record (no array, typed array or
    /// Proxy, which live elsewhere) without exotic access (arguments object, module namespace),
    /// side-table descriptors or a boxed primitive (a String wrapper's indices), whose shape
    /// is no dictionary. A memo entry for that shape was made from such an object, so it
    /// answers for this one too.
    #[inline(always)]
    fn enum_cache_shape(&self, object: Value) -> Option<u32> {
        let data = self.heap.object_record(object)?;
        if data.has_exotic_access() || data.has_side_descriptors() || data.boxed_primitive().is_some() {
            return None;
        }
        let shape = data.shape();
        (!self.shape_is_dictionary(shape)).then_some(shape)
    }

    /// `plain_object_keys` where the memo holds no strings of `shape` made in collection epoch
    /// `epoch`: makes each key's string (`Vm::atom_string`), returns the array of them, and
    /// records them beside the shape's slots. No collection runs before the array holds them.
    #[cold]
    fn record_object_key_strings(&mut self, object: Value, shape: u32, epoch: u64) -> Option<Value> {
        let slots = self.shape_enumerable_slots(object)?;
        let strings = slots.iter().map(|&(atom, _)| self.atom_string(atom)).collect::<Box<[Value]>>();
        let keys = self.array_of(&strings[..]);
        self.enumerable_keys.record_key_strings(shape, epoch, strings);
        Some(keys)
    }

    /// `Object.assign(target, ...sources)` where every copy is a plain one
    /// (`copy_plain_properties` with `Set`): `target` is an ordinary object record without
    /// exotic access, so ToObject leaves it as it is, and each source is null, undefined, or an
    /// ordinary object record whose own enumerable properties are string-keyed data properties
    /// its shape lists. An empty target reaching a shape an earlier copy of the source's shape
    /// reached takes that shape and the source's slot values at once (`plain_assign_shapes`);
    /// any other target takes each key by the plain property store. `None` for anything else.
    ///
    /// A source that declines comes after the earlier sources were copied, and the general
    /// native then copies every source again. Repeating those copies is unobservable: each read
    /// a data property of an ordinary object and wrote a writable data property of `target`,
    /// with no guest code, so the general path reads the same values and writes them again.
    pub(in crate::vm) fn plain_object_assign(&mut self, args: &[Value]) -> Option<Value> {
        let (&target, sources) = args.split_first()?;
        if self.heap.object_record(target)?.has_exotic_access() {
            return None;
        }
        for &source in sources {
            if source.is_null() || source.is_undefined() {
                continue;
            }
            let copied = self.heap.object_record(source).is_some()
                && self.copy_plain_properties(target, source, &[], PropertyCopyKind::Set);
            if !copied {
                return None;
            }
        }
        Some(target)
    }
}
