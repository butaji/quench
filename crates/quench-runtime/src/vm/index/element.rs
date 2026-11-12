//! `get_index` at an array-index key: the reads that answer from the element storage, a Proxy
//! trap, a typed array, a string or an accessor index before the named-key walk.

use super::*;

impl<H: Host> Vm<H> {
    /// `get_index` of `object` at the array index `index` (the key `key`): the value or error
    /// of the read where the element storage, a Proxy, a typed array, a string or an accessor
    /// index decides it; `None` leaves the read to the named-key walk.
    #[inline(always)]
    pub(super) fn indexed_element_get(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        key: Value,
        index: usize,
    ) -> Option<Result<Value, JsError>> {
        // Accessor indices are stored as holes, so a present dense element is a data property.
        if let Some(array_object) = self.heap.array(object)
            && let Some(value) = array_object
                .elements()
                .get(index)
                .copied()
                .filter(|value| !value.is_deleted())
        {
            #[cfg(feature = "profile-aggregate")]
            self.profile.index_get(ARRAY_INDEX_GET_DENSE);
            return Some(Ok(value));
        }
        if let Some(read) = self.proxy_index_get(p, object, key) {
            return Some(read);
        }
        if let Some(value) = self.absent_array_element(object, key) {
            return Some(Ok(value));
        }
        if let Some(value) = self.typed_array_get(object, index) {
            return Some(Ok(value));
        }
        if let Some(unit) = self.string_index_value(object, index) {
            return Some(Ok(unit));
        }
        self.indexed_array_entry_get(p, object, index)
    }

    /// `get_index` of an array at an index held as an accessor descriptor or a sparse element;
    /// `None` for any other read.
    fn indexed_array_entry_get(&mut self, p: &ResidualProgram, object: Value, index: usize) -> Option<Result<Value, JsError>> {
        if let Some((atom, attributes)) = self.array_descriptor_entry(object, index)
            && attributes.accessor
        {
            return Some(self.get_property(p, object, atom));
        }
        if self.heap.array(object).is_some()
            && let Some(value) = self
                .heap
                .sparse_get(object, index)
                .filter(|value| !value.is_deleted())
        {
            #[cfg(feature = "profile-aggregate")]
            self.profile.index_get(ARRAY_INDEX_GET_SPARSE);
            return Some(Ok(value));
        }
        None
    }

    /// `get_index` of a Proxy: its trap called with the key as a string; `None` for any other base.
    fn proxy_index_get(&mut self, p: &ResidualProgram, object: Value, key: Value) -> Option<Result<Value, JsError>> {
        let (target, handler) = self.proxy_parts(object)?;
        Some(self.to_property_key(p, key).and_then(|key| self.proxy_get_key_value(p, target, handler, object, key)))
    }
}
