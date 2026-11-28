//! The extras-borne reads and writes of an ordinary object's view (`ObjectRef`, `ObjectMut`). A record answers from its
//! own extras; a cell answers from the extras its header names (`ExtrasSpace`), and a setter made them first with
//! `Heap::ensure_cell_extras`. Each is the body of the `Object` method of the same meaning.

use super::*;

impl<'a> ObjectRef<'a> {
    // The extras-borne reads. A record answers from its own extras, a cell from its extras by id (`ExtrasSpace`); an
    // absent extras entry answers as an object with no such field.

    /// The boxed primitive of a wrapper object (`Object::boxed_primitive`).
    #[inline(always)]
    pub fn boxed_primitive(self) -> Option<Value> {
        match self {
            ObjectRef::Record(object) => object.boxed_primitive(),
            ObjectRef::Cell(cell) => cell.extras.and_then(|extras| extras.boxed_primitive()),
        }
    }

    /// The stack data of an error object (`Object::stack_data`).
    #[inline(always)]
    pub fn stack_data(self) -> Option<&'a StackData> {
        match self {
            ObjectRef::Record(object) => object.stack_data(),
            ObjectRef::Cell(cell) => cell.extras.and_then(|extras| extras.stack_data()),
        }
    }

    /// The arguments mapping of an arguments object (`Object::arguments_map`).
    #[inline(always)]
    pub fn arguments_map(self) -> Option<&'a [u16]> {
        match self {
            ObjectRef::Record(object) => object.arguments_map(),
            ObjectRef::Cell(cell) => cell.extras.and_then(|extras| extras.arguments_map()),
        }
    }

    /// The parameter slot an arguments object's index maps to (`Object::mapped_argument_slot`).
    #[inline(always)]
    pub fn mapped_argument_slot(self, index: usize) -> Option<usize> {
        self.arguments_map()?
            .get(index)
            .copied()
            .filter(|slot| *slot != u16::MAX)
            .map(usize::from)
    }

    /// Whether the object is an arguments object (`Object::is_arguments_object`).
    #[inline(always)]
    pub fn is_arguments_object(self) -> bool {
        match self {
            ObjectRef::Record(object) => object.is_arguments_object(),
            ObjectRef::Cell(cell) => cell.extras.is_some_and(|extras| extras.is_arguments()),
        }
    }

    /// Whether the object is a raw JSON object (`Object::is_raw_json`).
    #[inline(always)]
    pub fn is_raw_json(self) -> bool {
        match self {
            ObjectRef::Record(object) => object.is_raw_json(),
            ObjectRef::Cell(cell) => cell.extras.is_some_and(|extras| extras.is_raw_json()),
        }
    }

    /// Whether the object carries [[ErrorData]] (`Object::has_error_data`).
    #[inline(always)]
    pub fn has_error_data(self) -> bool {
        match self {
            ObjectRef::Record(object) => object.has_error_data(),
            ObjectRef::Cell(cell) => cell.extras.is_some_and(|extras| extras.has_error_data()),
        }
    }

    /// Whether structured transfer of the object is forbidden (`Object::is_structured_transfer_forbidden`).
    #[inline(always)]
    pub fn is_structured_transfer_forbidden(self) -> bool {
        match self {
            ObjectRef::Record(object) => object.is_structured_transfer_forbidden(),
            ObjectRef::Cell(cell) => cell.extras.is_some_and(|extras| extras.structured_transfer_forbidden()),
        }
    }

    /// How structured serialization treats the object (`Object::structured_serialization`).
    #[inline(always)]
    pub fn structured_serialization(self) -> StructuredSerialization {
        match self {
            ObjectRef::Record(object) => object.structured_serialization(),
            ObjectRef::Cell(cell) => cell.extras.map_or(StructuredSerialization::Ordinary, |extras| extras.structured_serialization()),
        }
    }

    /// The bindings of a module namespace (`Object::module_bindings`).
    #[inline(always)]
    pub fn module_bindings(self) -> &'a [(Atom, ProgramId, u16)] {
        match self {
            ObjectRef::Record(object) => object.module_bindings(),
            ObjectRef::Cell(cell) => cell.extras.map_or(&[], |extras| extras.module_bindings()),
        }
    }

    /// The module a deferred namespace loads on first use (`Object::deferred_module`).
    #[inline(always)]
    pub fn deferred_module(self) -> Option<&'a crate::ModuleSource> {
        match self {
            ObjectRef::Record(object) => object.deferred_module(),
            ObjectRef::Cell(cell) => cell.extras.and_then(|extras| extras.deferred_module()),
        }
    }

    /// The parent program and slot of the module binding `atom` (`Object::module_binding`).
    #[inline(always)]
    pub fn module_binding(self, atom: Atom) -> Option<(ProgramId, u16)> {
        self.module_bindings()
            .iter()
            .find_map(|(name, program, slot)| (*name == atom).then_some((*program, *slot)))
    }

    /// The external address of a host object (`Object::external_address`).
    #[inline(always)]
    pub fn external_address(self) -> Option<usize> {
        match self {
            ObjectRef::Record(object) => object.external_address(),
            ObjectRef::Cell(cell) => cell.extras.and_then(|extras| extras.external_address()),
        }
    }

    /// The activation depth of an arguments object's window (`Object::window_depth`).
    #[inline(always)]
    pub fn window_depth(self) -> Option<usize> {
        match self {
            ObjectRef::Record(object) => object.window_depth(),
            ObjectRef::Cell(cell) => cell.extras.and_then(|extras| extras.window_depth()),
        }
    }

    /// A function's creation realm when it differs from the main realm (`Object::function_realm_override`).
    #[inline(always)]
    pub fn function_realm_override(self) -> Option<Value> {
        match self {
            ObjectRef::Record(object) => object.function_realm_override(),
            ObjectRef::Cell(cell) => cell.extras.and_then(|extras| extras.function_realm()),
        }
    }

    /// The table of the values code read from this global object by name (`Object::global_values`).
    #[inline(always)]
    pub fn global_values(self) -> Option<u32> {
        match self {
            ObjectRef::Record(object) => object.global_values(),
            ObjectRef::Cell(cell) => cell.extras.and_then(|extras| extras.global_values()),
        }
    }

    /// Whether the object's prototype lookup is ready (`Object::prototype_lookup_ready`); a cell's is in its extras.
    #[inline(always)]
    pub fn prototype_lookup_ready(self) -> bool {
        match self {
            ObjectRef::Record(object) => object.prototype_lookup_ready(),
            ObjectRef::Cell(cell) => cell.extras.is_some_and(ObjectExtras::prototype_ready),
        }
    }

    /// Whether the object holds a `length` descriptor (`Object::has_length_descriptor`); a cell's fact is in its extras.
    #[inline(always)]
    pub fn has_length_descriptor(self) -> bool {
        match self {
            ObjectRef::Record(object) => object.has_length_descriptor(),
            ObjectRef::Cell(cell) => cell.extras.is_some_and(|extras| extras.element_facts().has(ElementFacts::LENGTH_DESCRIPTOR)),
        }
    }
}

impl<'a> ObjectMut<'a> {
    // Extras setters (S6a). A record writes its own extras (`Object::edit_extras`); a cell writes the extras its
    // header names. The caller made them first with `Heap::ensure_cell_extras`, so a cell's extras are present here.

    /// Edits the object's extras: the record's, made on first need, or the cell's, which `ensure_cell_extras` made.
    #[inline(always)]
    fn edit_extras(&mut self, edit: impl FnOnce(&mut ObjectExtras)) {
        match self {
            ObjectMut::Record(object) => object.edit_extras(edit),
            ObjectMut::Cell(cell) => edit(cell.extras.as_deref_mut().expect("a cell's extras are made before a write to them")),
        }
    }

    /// `Object::set_boxed_primitive`.
    #[inline(always)]
    pub fn set_boxed_primitive(&mut self, value: Value) {
        self.edit_extras(|extras| extras.set_boxed_primitive(value));
    }

    /// `Object::set_stack_data`.
    #[inline(always)]
    pub fn set_stack_data(&mut self, data: StackData) {
        self.edit_extras(|extras| extras.set_stack_data(data));
    }

    /// `Object::set_arguments_map`.
    #[inline(always)]
    pub fn set_arguments_map(&mut self, mapping: Vec<u16>) {
        crate::heap::array_proofs::retire_array_storage_proofs();
        self.edit_extras(|extras| extras.set_arguments_map(mapping));
    }

    /// `Object::arguments_map_mut`: the mapping, to unmap an index in place.
    #[inline(always)]
    pub fn arguments_map_mut(&mut self) -> Option<&mut Vec<u16>> {
        match self {
            ObjectMut::Record(object) => object.arguments_map_mut(),
            ObjectMut::Cell(cell) => cell.extras.as_deref_mut()?.arguments_map_mut(),
        }
    }

    /// `Object::set_arguments_object`: the arguments brand, and the exotic-access bit in the class word.
    #[inline(always)]
    pub fn set_arguments_object(&mut self) {
        match self {
            ObjectMut::Record(object) => object.set_arguments_object(),
            ObjectMut::Cell(cell) => {
                cell.edit_class(|object| object.set_exotic_access());
                self.edit_extras(|extras| extras.set_brand(ObjectBrand::Arguments));
            }
        }
    }

    /// `Object::set_raw_json`.
    #[inline(always)]
    pub fn set_raw_json(&mut self) {
        self.edit_extras(|extras| extras.set_brand(ObjectBrand::RawJson));
    }

    /// `Object::set_module_namespace`: the namespace brand, and the exotic-access bit in the class word.
    #[inline(always)]
    pub fn set_module_namespace(&mut self) {
        match self {
            ObjectMut::Record(object) => object.set_module_namespace(),
            ObjectMut::Cell(cell) => {
                cell.edit_class(|object| object.set_exotic_access());
                self.edit_extras(|extras| extras.set_brand(ObjectBrand::ModuleNamespace));
            }
        }
    }

    /// `Object::set_module_bindings`.
    #[inline(always)]
    pub fn set_module_bindings(&mut self, bindings: Vec<(Atom, ProgramId, u16)>) {
        self.edit_extras(|extras| extras.set_module_bindings(bindings));
    }

    /// `Object::set_deferred_module`.
    #[inline(always)]
    pub fn set_deferred_module(&mut self, module: Option<crate::ModuleSource>) {
        self.edit_extras(|extras| extras.set_deferred_module(module));
    }

    /// `Object::set_structured_serialization`.
    #[inline(always)]
    pub fn set_structured_serialization(&mut self, policy: StructuredSerialization) {
        self.edit_extras(|extras| extras.set_structured_serialization(policy));
    }

    /// `Object::forbid_structured_transfer`.
    #[inline(always)]
    pub fn forbid_structured_transfer(&mut self) {
        self.edit_extras(|extras| extras.forbid_structured_transfer());
    }

    /// `Object::set_external_address`.
    #[inline(always)]
    pub fn set_external_address(&mut self, address: usize) {
        self.edit_extras(|extras| extras.set_external_address(address));
    }

    /// `Object::set_function_realm_override`.
    #[inline(always)]
    pub fn set_function_realm_override(&mut self, realm: Value) {
        self.edit_extras(|extras| extras.set_function_realm(realm));
    }

    /// `Object::set_window_depth`.
    #[inline(always)]
    pub fn set_window_depth(&mut self, depth: usize) {
        self.edit_extras(|extras| extras.set_window_depth(depth));
    }

    /// `Object::global_values_table`: this global object's table, made on first use.
    #[inline(always)]
    pub fn global_values_table(&mut self) -> u32 {
        match self {
            ObjectMut::Record(object) => object.global_values_table(),
            ObjectMut::Cell(cell) => cell.extras.as_deref_mut().expect("a cell's extras are made before a global table").global_values_table(),
        }
    }
}
