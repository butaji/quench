//! The array record: its construction, element storage and the traits an array object carries.

use super::*;

impl ArrayObject {
    /// A new array of `prototype` with no properties: `new` of `Object::new(prototype)`,
    /// made with its arena storage at once.
    #[inline(always)]
    pub(crate) fn fresh(prototype: Value, elements: super::ArrayElements) -> Self {
        // The layout depends on the prototype alone: the last one's is kept, so a run of new
        // arrays (every literal of one realm) interns nothing.
        #[thread_local]
        static mut LAST: [(u32, u32); 16] = [(u32::MAX, 0); 16];
        let meta = Object::encode_array_proto(prototype) | OBJECT_ARRAY;
        let at = (meta.wrapping_mul(0x9e37_79b9) >> 28) as usize;
        // SAFETY: this thread's own memo; no reference to it outlives the read.
        let (key, layout) = unsafe { (*std::ptr::addr_of!(LAST))[at] };
        let object = if key == meta {
            Object { head: ObjectHead { payload: u64::from(EMPTY_POSITION), layout } }
        } else {
            let mut object = Object::from_header(meta, 0, 0);
            object.initialize_properties(ValueVec::new());
            // SAFETY: as above.
            unsafe { (*std::ptr::addr_of_mut!(LAST))[at] = (meta, object.layout) };
            object
        };
        let mut object = object;
        debug_assert!(object.proto() == prototype);
        debug_assert!(object.extras_pointer().is_none() && object.payload == u64::from(EMPTY_POSITION));
        // No extras and no property store: the payload word takes the backing.
        object.payload = elements.into_raw();
        Self { object }
    }

    pub(crate) fn new(mut object: Object, elements: impl Into<super::ArrayElements>) -> Self {
        let elements = elements.into();
        // Without extras before or after, the array form is a function of the object's layout
        // alone (its payload ends as the empty position): the last ones are kept.
        #[thread_local]
        static mut LAST: [(u32, u32); 16] = [(u32::MAX, 0); 16];
        let input = object.layout;
        let at = (input.wrapping_mul(0x9e37_79b9) >> 28) as usize;
        let plain = object.extras_pointer().is_none();
        // SAFETY: this thread's own memo; no reference to it outlives the read.
        let (key, layout) = unsafe { (*std::ptr::addr_of!(LAST))[at] };
        if plain && key == input {
            debug_assert!(object.inline_properties().is_none_or(|values| values.iter().all(|value| value.is_undefined())));
            debug_assert!(object.inline_properties().is_some() || object.payload == u64::from(EMPTY_POSITION));
            object.layout = layout;
            object.payload = u64::from(EMPTY_POSITION);
            return Self::attach(object, elements);
        }
        let object = Self::array_form(object);
        if plain && object.extras_pointer().is_none() {
            debug_assert_eq!({ object.payload }, u64::from(EMPTY_POSITION));
            // SAFETY: as above.
            unsafe { (*std::ptr::addr_of_mut!(LAST))[at] = (input, object.layout) };
        }
        Self::attach(object, elements)
    }

    /// `object` in the array form `new` gives it.
    pub(super) fn array_form(mut object: Object) -> Object {
        // Read as a plain object until its storage takes the array form below.
        object.set_meta(object.meta() & !OBJECT_ARRAY);
        // Array objects keep named properties in the arena; a fresh object has none inline.
        let current = object.properties();
        if current.has_inline_property_storage() {
            debug_assert!(
                object
                    .inline_properties()
                    .is_some_and(|values| values.iter().all(|value| value.is_undefined()))
            );
            let mut properties = ValueVec::new();
            properties.set_auxiliary(current.auxiliary());
            properties.preserve_integrity_from(current);
            object.initialize_properties(properties);
        }
        // An array without extras holds no property store (`set_storage_position`).
        if object.extras_pointer().is_none() && object.array_store_needs_extras(object.payload as u32) {
            object.extras_mut();
        }
        object.set_meta(object.meta() | OBJECT_ARRAY);
        object
    }

    /// The array of `object`, an array's object in its array form (`new`), and `elements`:
    /// they go to its extras, or to its payload word where it has none.
    pub(super) fn attach(mut object: Object, elements: super::ArrayElements) -> Self {
        debug_assert!(object.is_array());
        match object.extras_mut_existing() {
            Some(extras) => extras.elements = elements,
            None => {
                debug_assert!(object.payload == u64::from(EMPTY_POSITION));
                object.payload = elements.into_raw();
            }
        }
        Self { object }
    }

    #[inline(always)]
    pub(crate) fn elements(&self) -> &super::ArrayElements {
        match self.object.extras() {
            Some(extras) => &extras.elements,
            // SAFETY: without extras, the payload word is the backing this array owns.
            None => unsafe { super::ArrayElements::from_raw_ref(std::ptr::addr_of!(self.object.payload)) },
        }
    }

    /// The elements for any change, including their backing's: the running lane's array
    /// storage proofs retire first (`retire_array_storage_proofs`).
    #[inline(always)]
    pub(crate) fn elements_mut(&mut self) -> &mut super::ArrayElements {
        self.announce_shrinking();
        self.elements_raw_mut()
    }

    /// The elements for a change no proof observes (`elements_mut` announces one first).
    #[inline(always)]
    pub(super) fn elements_raw_mut(&mut self) -> &mut super::ArrayElements {
        match self.object.extras_pointer() {
            // SAFETY: owned by this array's object, and `&mut self` makes the borrow unique.
            Some(extras) => unsafe { &mut (*extras).head.elements },
            // SAFETY: as in `elements`, with `&mut self` making the borrow unique.
            None => unsafe { super::ArrayElements::from_raw_mut(std::ptr::addr_of_mut!(self.object.payload)) },
        }
    }

    /// What a change to this array's elements that a proof may not bound retires: the proofs
    /// that read a prototype's elements, and the running lane's array storage proofs. A change
    /// the staged `array_pop` makes in place announces itself here, as `elements_mut` does.
    #[inline(always)]
    pub(crate) fn announce_shrinking(&self) {
        // A prototype's elements answer index reads of the objects it is the prototype of.
        self.object.note_prototype_elements_change();
        super::array_proofs::retire_array_storage_proofs();
    }

    /// Whether it owns nothing at all: no extras, so no property store, and the shared empty
    /// header as its elements, one compare of its payload word (`[]` until it grows).
    #[inline(always)]
    pub(crate) fn owns_nothing(&self) -> bool {
        self.object.payload == super::array_elements::empty_header_word()
    }

    /// Whether a dead array owns anything its drop frees: extras, or an element allocation
    /// it counts among its backing's owners. Young runs and the empty header need none.
    #[inline] pub(crate) fn needs_drop(&self) -> bool {
        self.object.extras_pointer().is_some() || self.elements().owner_count() != 0
    }

    /// Copies a young run of elements to an allocation of the array's own
    /// (`ArrayElements::leave_nursery`): the same values, so no proof or dependent retires.
    pub(crate) fn leave_nursery(&mut self) {
        self.elements_raw_mut().leave_nursery();
    }

    /// `ArrayElements::survive_nursery`: the same values, so no proof or dependent retires.
    pub(crate) fn survive_nursery(&mut self) {
        self.elements_raw_mut().survive_nursery();
    }

    /// `ArrayElements::widen`: the same values, so no dependent retires; a proof of the
    /// narrow backing proves no index (`ArrayStorageProofs::prove`), so none reads it.
    #[inline]
    pub(crate) fn widen_elements(&mut self) {
        if self.elements().is_encoded() {
            self.elements_raw_mut().widen();
        }
    }

    /// `ArrayElements::narrow_in_place`: the same values, so no proof or dependent retires.
    pub(crate) fn narrow_elements(&mut self) -> usize {
        self.elements_raw_mut().narrow_in_place()
    }

    /// Replaces the present element at `index` in place where this array alone owns its
    /// backing (`ArrayElements::replace_in_place`); `false` changes nothing. A replacement
    /// changes no backing, so no array storage proof retires; a prototype's dependents do.
    #[inline(always)]
    pub(crate) fn replace_in_place(&mut self, index: usize, value: Value) -> bool {
        if !self.elements_raw_mut().replace_in_place(index, value) {
            return false;
        }
        self.object.note_prototype_elements_change();
        true
    }

    /// The first value of the backing this array alone owns, which an array storage proof
    /// replaces elements through (`ArrayStorageProofs::replace`); `None` where it is shared.
    #[inline(always)]
    pub(crate) fn unique_values(&mut self) -> Option<std::ptr::NonNull<Value>> {
        self.elements_raw_mut().unique_values()
    }
}

impl Clone for ArrayObject {
    fn clone(&self) -> Self {
        // `Object::clone` copies extras, their backing with them, and leaves the word of an
        // array without extras empty.
        let mut object = self.object.clone();
        if object.extras_pointer().is_none() {
            object.payload = self.elements().clone().into_raw();
        }
        Self { object }
    }
}

impl Drop for ArrayObject {
    fn drop(&mut self) {
        // Extras drop their backing with the object; without them, the payload word owns it.
        if self.object.extras_pointer().is_none() {
            // SAFETY: the payload word is the backing this array owns, taken once here.
            drop(unsafe { super::ArrayElements::from_raw(self.object.payload) });
            self.object.payload = u64::from(EMPTY_POSITION);
        }
    }
}

impl std::ops::Deref for ArrayObject {
    type Target = Object;
    #[inline] fn deref(&self) -> &Object {
        &self.object
    }
}

impl std::ops::DerefMut for ArrayObject {
    fn deref_mut(&mut self) -> &mut Object {
        &mut self.object
    }
}

impl std::fmt::Debug for ArrayObject {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ArrayObject")
            .field("object", &self.object)
            .field("elements", self.elements())
            .finish()
    }
}

impl std::fmt::Debug for Object {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Object")
            .field("proto", &self.proto())
            .field("properties", &self.properties())
            .field("inline_properties", &self.inline_values())
            .field("extras", &self.extras())
            .finish()
    }
}
