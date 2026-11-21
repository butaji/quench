//! Stores into dense arrays no guest code reaches yet: the arrays builtins build.

use super::{Value, Vm};

impl Vm {
    /// Append an own data element to an unexposed dense array. No guest code runs.
    pub(super) fn append_fresh_array_element(&mut self, array: Value, value: Value) -> usize {
        let Some(length) = self.heap.grow_array_elements(array, |elements| {
            elements.push(value);
            elements.len()
        }) else {
            unreachable!("fresh array owner remains an array");
        };
        length
    }

    /// Store element `index` of a dense array no guest code reaches, at most one past its
    /// end: its own data property, every element a default one. The store passes the write
    /// barrier, since the array may have been promoted while guest code ran.
    ///
    /// An element inside the array's storage is one store (`UniqueElements::set`), inlined into
    /// each step; only the growth past the end (`grow_fresh_array_element`) is out of line.
    #[inline(always)]
    pub(super) fn store_fresh_array_element(&mut self, array: Value, index: usize, value: Value) {
        let Some(object) = self.heap.array_store_mut(array, value) else {
            unreachable!("fresh array owner remains an array");
        };
        let elements = super::index::mutable_array_elements(object.elements_mut());
        if index < elements.len() {
            elements.set(index, value);
        } else {
            self.grow_fresh_array_element(array, index, value);
        }
    }

    /// `store_fresh_array_element` of `index`, the element one past a fresh array's end: the
    /// array grows by it, and the growth is charged to the heap.
    #[cold]
    #[inline(never)]
    fn grow_fresh_array_element(&mut self, array: Value, index: usize, value: Value) {
        let Some(object) = self.heap.array_store_mut(array, value) else {
            unreachable!("fresh array owner remains an array");
        };
        let elements = super::index::mutable_array_elements(object.elements_mut());
        debug_assert_eq!(index, elements.len(), "a fresh array grows by one element at a time");
        let capacity = elements.capacity();
        elements.push(value);
        let grown = elements.capacity() - capacity;
        self.heap.charge_payload_growth(grown * size_of::<Value>());
    }
}
