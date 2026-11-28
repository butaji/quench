//! The object record's state: its brand, structured-serialization policy, and boxed and stack data.

use super::*;

/// The values an object's extras hold, which its sweep and marking trace: the boxed primitive, the function realm
/// and the captured stack's roots. A record traces its extras through `Object::visit_extras_roots`; a live class
/// cell's extras trace through `ExtrasSpace::trace`.
#[inline(never)]
pub(crate) fn trace_extras(extras: &ObjectExtras, mut visit: impl FnMut(Value)) {
    let Some(extras) = extras.rare() else { return };
    if let Some(value) = extras.boxed_primitive {
        visit(value);
    }
    if let Some(realm) = extras.function_realm {
        visit(realm);
    }
    match extras.stack_data.as_ref() {
        Some(StackData::Captured { records, .. }) => {
            for record in records {
                record.visit_roots(&mut visit);
            }
        }
        Some(StackData::CallSite(record)) => {
            record.visit_roots(&mut visit);
        }
        Some(StackData::Formatted(value)) => visit(*value),
        None => {}
    }
}

impl Object {
    /// Presence of [[ErrorData]] is the unforgeable Error brand.
    pub(crate) fn error(proto: Value) -> Self {
        let mut object = Self::new(proto);
        object.extras_mut().rare_mut().brand = ObjectBrand::Error;
        object
    }

    /// How structured serialization treats this object.
    pub(crate) fn set_structured_serialization(&mut self, policy: StructuredSerialization) {
        self.extras_mut().rare_mut().structured_serialization = policy;
    }

    pub(crate) fn structured_serialization(&self) -> StructuredSerialization {
        self.extras().and_then(ObjectExtras::rare).map_or(StructuredSerialization::Ordinary, |rare| rare.structured_serialization)
    }

    pub(crate) fn forbid_structured_transfer(&mut self) {
        self.extras_mut().rare_mut().structured_transfer_forbidden = true;
    }

    pub(crate) fn is_structured_transfer_forbidden(&self) -> bool {
        self.extras()
            .and_then(ObjectExtras::rare).is_some_and(|rare| rare.structured_transfer_forbidden)
    }

    pub(crate) fn has_error_data(&self) -> bool {
        self.brand() == ObjectBrand::Error
    }

    /// Genuine primitive-wrapper data; guest properties cannot supply this slot.
    #[inline]
    pub(crate) fn boxed_primitive(&self) -> Option<Value> {
        self.extras()?.boxed_primitive()
    }

    pub(crate) fn set_boxed_primitive(&mut self, value: Value) {
        self.extras_mut().rare_mut().boxed_primitive = Some(value);
    }

    /// A function's creation realm when it is not the main realm (`FunctionRecord::realm_override`).
    pub(crate) fn function_realm_override(&self) -> Option<Value> {
        self.extras()?.rare()?.function_realm
    }

    pub(crate) fn set_function_realm_override(&mut self, realm: Value) {
        self.extras_mut().rare_mut().function_realm = Some(realm);
    }

    pub(crate) fn stack_data(&self) -> Option<&StackData> {
        self.extras()?.rare()?.stack_data.as_ref()
    }

    pub(crate) fn set_stack_data(&mut self, data: StackData) {
        self.extras_mut().rare_mut().stack_data = Some(data);
    }

    /// Visit internal primitive data, private-name homes and captured stack roots.
    #[inline(always)] pub(crate) fn visit_extras_roots(&self, visit: impl FnMut(Value)) {
        if let Some(extras) = self.extras() {
            trace_extras(extras, visit);
        }
    }

    #[quench_macros::test_visible]
    #[inline] pub(crate) fn was_used_as_prototype(&self) -> bool {
        self.extras().is_some_and(ObjectExtras::prototype_marked)
    }

    #[quench_macros::test_visible]
    pub(crate) fn mark_as_prototype(&mut self) {
        if !self.was_used_as_prototype() {
            // A prototype's elements answer its dependents' reads: no proof replaces them.
            super::array_proofs::retire_array_storage_proofs();
            self.note_global_layout_change();
            self.extras_mut().set_prototype_setup();
        }
    }

    pub(crate) fn prototype_lookup_ready(&self) -> bool {
        self.extras().is_some_and(ObjectExtras::prototype_ready)
    }

    pub(crate) fn prepare_prototype_lookup(&mut self) {
        if self.was_used_as_prototype() {
            self.extras_mut().prepare_prototype();
        }
    }

    #[inline] pub fn shape(&self) -> u32 {
        ValueVec::from_parts(0, self.aux()).auxiliary()
    }
    /// The shape with its dictionary and exotic bits (`ValueVec::exact_auxiliary`): a cache
    /// entry, which records only shapes, matches it exactly when it matches the shape of an
    /// ordinary object outside dictionary storage.
    #[inline(always)]
    pub fn exact_shape(&self) -> u32 {
        ValueVec::from_parts(0, self.aux()).exact_auxiliary()
    }
    /// The shape with its exotic bit and without its dictionary bit: a recorded shape
    /// matches it exactly when it matches the shape of an ordinary object, in whatever
    /// storage (`ValueVec::EXOTIC_ACCESS`).
    #[inline(always)]
    pub(crate) fn ordinary_shape(&self) -> u32 {
        self.aux() & !(crate::value_vec::DICTIONARY_STORAGE | crate::value_vec::STORAGE_FORM)
    }

    #[inline(always)]
    pub(crate) fn set_shape(&mut self, shape: u32, arena: &mut ValueArena) {
        arena.note_shape_transition(self.shape(), shape);
        self.note_prototype_layout_change();
        self.note_global_layout_change();
        self.renumber_shape(shape);
    }
    #[inline] pub(crate) fn is_extensible(&self) -> bool {
        self.properties().is_extensible()
    }
    /// Whether a cached add transition (`FieldAddCache`) applies to this object as to the
    /// ordinary object that recorded it: it is extensible, and no arguments object or module
    /// namespace, whose exotic own properties its shape does not describe.
    #[inline(always)]
    pub(crate) fn admits_cached_add(&self) -> bool {
        self.is_extensible() && !self.has_exotic_access()
    }
    /// Whether the object's property access is not the ordinary one its shape describes: an
    /// arguments object, a module namespace or a proxy's own object. A header bit, so fast
    /// paths need not read the extras (`ObjectBrand`).
    #[inline(always)]
    pub(crate) fn has_exotic_access(&self) -> bool {
        self.aux() & crate::value_vec::EXOTIC_ACCESS != 0
    }
    #[inline(always)]
    pub(super) fn brand(&self) -> ObjectBrand {
        self.extras().map_or(ObjectBrand::Ordinary, ObjectExtras::brand)
    }
    #[inline(always)]
    pub(super) fn element_facts(&self) -> ElementFacts {
        self.extras().map_or(ElementFacts(0), ObjectExtras::element_facts)
    }
    pub(super) fn mark_element_fact(&mut self, bit: u8) {
        self.extras_mut().mark_element_fact(bit);
    }
    pub(crate) fn set_extensible(&mut self, value: bool) {
        let mut properties = ValueVec::from_parts(self.meta() & VALUE_VEC_INTEGRITY_BITS, 0);
        properties.set_extensible(value);
        self.set_meta((self.meta() & !VALUE_VEC_INTEGRITY_BITS) | properties.into_parts().0);
    }
    #[inline]
    pub(crate) fn is_frozen(&self) -> bool {
        self.properties().is_frozen()
    }
    pub(crate) fn set_frozen(&mut self, value: bool) {
        self.note_prototype_layout_change();
        self.note_global_layout_change();
        super::array_proofs::retire_array_storage_proofs();
        let mut properties = ValueVec::from_parts(self.meta() & VALUE_VEC_INTEGRITY_BITS, 0);
        properties.set_frozen(value);
        self.set_meta((self.meta() & !VALUE_VEC_INTEGRITY_BITS) | properties.into_parts().0);
    }
    #[inline]
    pub(crate) fn arguments_map(&self) -> Option<&[u16]> {
        self.extras()?.rare()?.arguments_map.as_deref()
    }
    pub(crate) fn arguments_map_mut(&mut self) -> Option<&mut Vec<u16>> {
        self.extras_mut_existing()?.rare_mut_existing()?.arguments_map.as_mut()
    }
    pub(crate) fn window_depth(&self) -> Option<usize> {
        self.extras()?.rare()?.window_depth.map(|depth| depth as usize)
    }
    pub fn set_window_depth(&mut self, depth: usize) {
        if let Some(rare) = self.extras_mut_existing().and_then(ObjectExtras::rare_mut_existing) {
            rare.window_depth = u32::try_from(depth).ok();
        }
    }
    pub(crate) fn mapped_argument_slot(&self, index: usize) -> Option<usize> {
        self.arguments_map()?
            .get(index)
            .copied()
            .filter(|slot| *slot != u16::MAX)
            .map(usize::from)
    }
    pub(crate) fn set_arguments_map(&mut self, mapping: Vec<u16>) {
        super::array_proofs::retire_array_storage_proofs();
        self.extras_mut().rare_mut().arguments_map = Some(mapping);
    }
    pub(crate) fn set_arguments_object(&mut self) {
        self.extras_mut().rare_mut().brand = ObjectBrand::Arguments;
        self.set_aux(self.aux() | crate::value_vec::EXOTIC_ACCESS);
    }
    /// Whether the object's elements are ordinary and all dense, as `dense_array_length`
    /// needs them: it is extensible and not frozen, and holds no indexed or `length`
    /// descriptor, no arguments mapping and never held sparse elements
    /// (`mark_sparse_elements`, which every sparse write makes). Read from the header bits
    /// and one mask of the extras' element facts.
    #[inline(always)]
    pub(crate) fn holds_ordinary_elements(&self) -> bool {
        self.meta() & VALUE_VEC_INTEGRITY_BITS == 0
            && !self.has_exotic_access()
            && !self.element_facts().has(ElementFacts::NOT_ORDINARY)
    }
    #[inline] pub fn is_arguments_object(&self) -> bool {
        self.brand() == ObjectBrand::Arguments
    }
    #[inline]
    pub(crate) fn has_indexed_descriptors(&self) -> bool {
        self.element_facts().has(ElementFacts::INDEXED_DESCRIPTORS)
    }
    pub(crate) fn mark_indexed_descriptors(&mut self) {
        self.note_prototype_layout_change();
        super::array_proofs::retire_array_storage_proofs();
        self.mark_element_fact(ElementFacts::INDEXED_DESCRIPTORS);
    }
    /// Whether the object ever held sparse elements (`mark_sparse_elements`).
    #[inline(always)]
    pub(crate) fn has_sparse_elements(&self) -> bool {
        self.element_facts().has(ElementFacts::SPARSE_ELEMENTS)
    }
    pub(crate) fn mark_sparse_elements(&mut self) {
        self.mark_element_fact(ElementFacts::SPARSE_ELEMENTS);
    }
    pub(crate) fn has_length_descriptor(&self) -> bool {
        self.element_facts().has(ElementFacts::LENGTH_DESCRIPTOR)
    }
    pub(crate) fn mark_length_descriptor(&mut self) {
        self.mark_element_fact(ElementFacts::LENGTH_DESCRIPTOR);
    }
    /// Whether the VM's descriptor table may hold an entry for this object: lookups of an
    /// object without one need not probe the table.
    #[inline(always)]
    pub(crate) fn has_side_descriptors(&self) -> bool {
        self.element_facts().has(ElementFacts::SIDE_DESCRIPTORS)
    }
    /// A prototype's side descriptors can name keys its shape lacks, which retires the
    /// proofs (its subject's, `ChainCell`) that found them absent.
    pub(crate) fn mark_side_descriptors(&mut self) {
        self.note_prototype_layout_change();
        self.mark_element_fact(ElementFacts::SIDE_DESCRIPTORS);
    }
    pub(crate) fn is_raw_json(&self) -> bool {
        self.brand() == ObjectBrand::RawJson
    }
    pub(crate) fn set_raw_json(&mut self) {
        self.extras_mut().rare_mut().brand = ObjectBrand::RawJson;
    }

    pub(crate) fn external_address(&self) -> Option<usize> {
        self.extras()?.rare()?.external_address
    }
    pub(crate) fn set_external_address(&mut self, address: usize) {
        let extras = self.extras_mut().rare_mut();
        extras.external_address = Some(address);
        extras.structured_serialization = StructuredSerialization::Forbidden;
    }

    /// A namespace is exotic (`set_module_namespace`), so the flag in the object's own word
    /// rules out every other object without reading its extras.
    #[inline(always)]
    pub fn is_module_namespace(&self) -> bool {
        self.has_exotic_access() && self.brand() == ObjectBrand::ModuleNamespace
    }
    /// A proxy's own object: the handler answers every access, so no shape a cache records
    /// may match it.
    pub(crate) fn proxy(proto: Value) -> Self {
        let mut object = Self::new(proto);
        object.set_aux(object.aux() | crate::value_vec::EXOTIC_ACCESS);
        object
    }
    pub(crate) fn set_module_namespace(&mut self) {
        self.extras_mut().rare_mut().brand = ObjectBrand::ModuleNamespace;
        self.set_aux(self.aux() | crate::value_vec::EXOTIC_ACCESS);
    }
    pub(crate) fn module_bindings(&self) -> &[(Atom, ProgramId, u16)] {
        self.extras().and_then(ObjectExtras::rare).map_or(&[], |rare| &rare.module_bindings)
    }
    pub(crate) fn set_module_bindings(&mut self, bindings: Vec<(Atom, ProgramId, u16)>) {
        self.extras_mut().rare_mut().module_bindings = bindings;
    }
    #[inline]
    pub(crate) fn deferred_module(&self) -> Option<&crate::ModuleSource> {
        self.extras()?.rare()?.deferred_module.as_ref()
    }
    pub(crate) fn set_deferred_module(&mut self, module: Option<crate::ModuleSource>) {
        self.extras_mut().rare_mut().deferred_module = module;
    }
    pub(crate) fn allocated_extra_bytes(&self) -> usize {
        self.extras().map_or(0, ObjectExtras::allocated_bytes)
    }

    pub(crate) fn module_binding(&self, atom: Atom) -> Option<(ProgramId, u16)> {
        self.module_bindings()
            .iter()
            .find_map(|(name, program, slot)| (*name == atom).then_some((*program, *slot)))
    }
}
