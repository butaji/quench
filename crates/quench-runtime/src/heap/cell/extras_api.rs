//! The extras reads and writes that a live class cell's view answers too (`CellView`, `CellViewMut`, in
//! `heap::object_view`). Each is the body of the `Object` accessor of the same meaning over the extras
//! themselves, so a record and a cell whose extras are by id (`ExtrasSpace`) share one definition.

use super::*;

impl ObjectExtras {
    /// Whether a prototype mark is set on these extras (`Object::was_used_as_prototype`).
    #[inline(always)]
    pub(crate) fn prototype_marked(&self) -> bool {
        self.prototype_lookup != PrototypeLookupState::Ordinary
    }

    /// Whether the prototype's lookup is ready (`Object::prototype_lookup_ready`).
    #[inline(always)]
    pub(crate) fn prototype_ready(&self) -> bool {
        self.prototype_lookup == PrototypeLookupState::Ready
    }

    /// Sets the prototype mark to setup mode (`Object::mark_as_prototype`).
    #[inline(always)]
    pub(crate) fn set_prototype_setup(&mut self) {
        self.prototype_lookup = PrototypeLookupState::Setup;
    }

    /// Readies the prototype's lookup, if it is marked (`Object::prepare_prototype_lookup`).
    #[inline(always)]
    pub(crate) fn prepare_prototype(&mut self) {
        if self.prototype_marked() {
            self.prototype_lookup = PrototypeLookupState::Ready;
        }
    }

    /// This prototype's subject in its heap's dependency table (`Object::watched_subject`).
    #[inline(always)]
    pub(crate) fn watched_subject(&self) -> Option<u32> {
        self.watch.subject()
    }

    /// Gives this prototype its subject in its heap's dependency table (`Object::watch_as`).
    #[inline(always)]
    pub(crate) fn set_watch(&mut self, watch: dependencies::Watch) {
        self.watch = watch;
    }

    /// The layout change of a prototype whose class has exact shape `shape`, made before it takes effect
    /// (`Object::note_prototype_layout_change`): the readers of its shape retire, and its watch (or, with no subject,
    /// the global epoch) retires. A prototype is marked, so an unmarked object's change retires nothing.
    pub(crate) fn note_prototype_layout_change(&self, shape: u32) {
        if !self.prototype_marked() {
            return;
        }
        prototype_values::note_layout_change(shape);
        if self.watched_subject().is_some() {
            self.watch.retire();
        } else {
            advance_prototype_layout_epoch();
        }
    }

    /// The brand of these extras (`Object::brand`).
    #[inline(always)]
    pub(crate) fn brand(&self) -> ObjectBrand {
        self.rare().map_or(ObjectBrand::Ordinary, |rare| rare.brand)
    }

    /// The element facts recorded beside the shape (`Object::element_facts`).
    #[inline(always)]
    pub(crate) fn element_facts(&self) -> ElementFacts {
        self.element_facts
    }

    /// Records element fact `bit` (`Object::mark_element_fact`).
    #[inline(always)]
    pub(crate) fn mark_element_fact(&mut self, bit: u8) {
        self.element_facts.0 |= bit;
    }

    /// The boxed primitive these extras hold, if any (`Object::boxed_primitive`).
    #[inline(always)]
    pub(crate) fn boxed_primitive(&self) -> Option<Value> {
        self.rare()?.boxed_primitive
    }

    /// Whether these extras hold data that a cached define must not overwrite past: a boxed primitive, stack or
    /// error data (`ObjectRef::holds_define_extras`).
    pub(crate) fn holds_define_data(&self) -> bool {
        self.boxed_primitive().is_some() || self.rare().is_some_and(|rare| rare.stack_data.is_some()) || self.brand() == ObjectBrand::Error
    }

    /// Sets the brand (`Object::set_brand`, through the brand's own setters).
    #[inline(always)]
    pub(crate) fn set_brand(&mut self, brand: ObjectBrand) {
        self.rare_mut().brand = brand;
    }

    /// Whether the extras hold the arguments brand (`Object::is_arguments_object`).
    #[inline(always)]
    pub(crate) fn is_arguments(&self) -> bool {
        self.brand() == ObjectBrand::Arguments
    }

    /// Whether the extras hold the raw-JSON brand (`Object::is_raw_json`).
    #[inline(always)]
    pub(crate) fn is_raw_json(&self) -> bool {
        self.brand() == ObjectBrand::RawJson
    }

    /// Whether the extras hold the error brand (`Object::has_error_data`).
    #[inline(always)]
    pub(crate) fn has_error_data(&self) -> bool {
        self.brand() == ObjectBrand::Error
    }

    /// Sets the boxed primitive (`Object::set_boxed_primitive`).
    #[inline(always)]
    pub(crate) fn set_boxed_primitive(&mut self, value: Value) {
        self.rare_mut().boxed_primitive = Some(value);
    }

    /// The captured or formatted stack data (`Object::stack_data`).
    #[inline(always)]
    pub(crate) fn stack_data(&self) -> Option<&StackData> {
        self.rare()?.stack_data.as_ref()
    }

    /// Sets the stack data (`Object::set_stack_data`).
    #[inline(always)]
    pub(crate) fn set_stack_data(&mut self, data: StackData) {
        self.rare_mut().stack_data = Some(data);
    }

    /// The arguments mapping (`Object::arguments_map`).
    #[inline(always)]
    pub(crate) fn arguments_map(&self) -> Option<&[u16]> {
        self.rare()?.arguments_map.as_deref()
    }

    /// The arguments mapping, to edit in place (`Object::arguments_map_mut`).
    #[inline(always)]
    pub(crate) fn arguments_map_mut(&mut self) -> Option<&mut Vec<u16>> {
        self.rare_mut_existing()?.arguments_map.as_mut()
    }

    /// Sets the arguments mapping (`Object::set_arguments_map`).
    #[inline(always)]
    pub(crate) fn set_arguments_map(&mut self, mapping: Vec<u16>) {
        self.rare_mut().arguments_map = Some(mapping);
    }

    /// The activation depth recorded for the arguments object's window (`Object::window_depth`).
    #[inline(always)]
    pub(crate) fn window_depth(&self) -> Option<usize> {
        self.rare()?.window_depth.map(|depth| depth as usize)
    }

    /// Records the activation depth of the arguments object's window (`Object::set_window_depth`), where the
    /// extras already hold their rare fields.
    #[inline(always)]
    pub(crate) fn set_window_depth(&mut self, depth: usize) {
        if let Some(rare) = self.rare_mut_existing() {
            rare.window_depth = u32::try_from(depth).ok();
        }
    }

    /// The module namespace's bindings (`Object::module_bindings`).
    #[inline(always)]
    pub(crate) fn module_bindings(&self) -> &[(Atom, ProgramId, u16)] {
        self.rare().map_or(&[], |rare| &rare.module_bindings)
    }

    /// Sets the module namespace's bindings (`Object::set_module_bindings`).
    #[inline(always)]
    pub(crate) fn set_module_bindings(&mut self, bindings: Vec<(Atom, ProgramId, u16)>) {
        self.rare_mut().module_bindings = bindings;
    }

    /// The module a deferred namespace loads on first use (`Object::deferred_module`).
    #[inline(always)]
    pub(crate) fn deferred_module(&self) -> Option<&crate::ModuleSource> {
        self.rare()?.deferred_module.as_ref()
    }

    /// Sets the deferred module (`Object::set_deferred_module`).
    #[inline(always)]
    pub(crate) fn set_deferred_module(&mut self, module: Option<crate::ModuleSource>) {
        self.rare_mut().deferred_module = module;
    }

    /// How structured serialization treats the object (`Object::structured_serialization`).
    #[inline(always)]
    pub(crate) fn structured_serialization(&self) -> StructuredSerialization {
        self.rare().map_or(StructuredSerialization::Ordinary, |rare| rare.structured_serialization)
    }

    /// Sets how structured serialization treats the object (`Object::set_structured_serialization`).
    #[inline(always)]
    pub(crate) fn set_structured_serialization(&mut self, policy: StructuredSerialization) {
        self.rare_mut().structured_serialization = policy;
    }

    /// Whether structured transfer of the object is forbidden (`Object::is_structured_transfer_forbidden`).
    #[inline(always)]
    pub(crate) fn structured_transfer_forbidden(&self) -> bool {
        self.rare().is_some_and(|rare| rare.structured_transfer_forbidden)
    }

    /// Forbids structured transfer of the object (`Object::forbid_structured_transfer`).
    #[inline(always)]
    pub(crate) fn forbid_structured_transfer(&mut self) {
        self.rare_mut().structured_transfer_forbidden = true;
    }

    /// The external address of a host object (`Object::external_address`).
    #[inline(always)]
    pub(crate) fn external_address(&self) -> Option<usize> {
        self.rare()?.external_address
    }

    /// Sets the external address and forbids structured serialization of the host object
    /// (`Object::set_external_address`).
    #[inline(always)]
    pub(crate) fn set_external_address(&mut self, address: usize) {
        let rare = self.rare_mut();
        rare.external_address = Some(address);
        rare.structured_serialization = StructuredSerialization::Forbidden;
    }

    /// A function's creation realm when it differs from the main realm (`Object::function_realm_override`).
    #[inline(always)]
    pub(crate) fn function_realm(&self) -> Option<Value> {
        self.rare()?.function_realm
    }

    /// Sets a function's creation realm (`Object::set_function_realm_override`).
    #[inline(always)]
    pub(crate) fn set_function_realm(&mut self, realm: Value) {
        self.rare_mut().function_realm = Some(realm);
    }
}

impl super::Object {
    /// The record's extras, made on first need: the record half of `ObjectMut::edit_extras`.
    #[inline(always)]
    pub(crate) fn edit_extras<R>(&mut self, edit: impl FnOnce(&mut ObjectExtras) -> R) -> R {
        edit(self.extras_mut())
    }

    /// Sets the exotic-access header bit, which an arguments object and a module namespace carry.
    #[inline(always)]
    pub(crate) fn set_exotic_access(&mut self) {
        self.set_aux(self.aux() | crate::value_vec::EXOTIC_ACCESS);
    }
}
