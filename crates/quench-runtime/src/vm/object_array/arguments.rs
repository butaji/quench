//! The reads of an arguments object's own `length` and elements, past its parameter mapping.
use super::*;

impl<H: Host> Vm<H> {
    /// The own data `length` of an arguments object that keeps the layout it was made with
    /// (a realm's `ArgumentsObjectTemplate` shape, which fixes the property, its attributes
    /// and its slot); `None` for any other value or layout.
    pub(in crate::vm) fn arguments_object_length(&self, target: Value) -> Option<Value> {
        let object = self.heap.object(target)?;
        if !self.heap.array(target)?.is_arguments_object() {
            return None;
        }
        let shape = object.shape();
        [true, false].into_iter().find_map(|expose_callee| {
            let template =
                self.realm.intrinsics.arguments_objects.get(&(self.realm.globals, expose_callee))?;
            (self.heap.object(template.anchor)?.shape() == shape)
                .then(|| self.heap.property_get(object, template.length_slot))
                .flatten()
        })
    }

    /// Own data element `index` of an arguments object, where no parameter mapping reaches
    /// it and no descriptor stands for it: its stored element, as `[[Get]]` finds it before
    /// any prototype. `None` for a mapped, absent, accessor or redefined element, or any other
    /// value.
    pub(in crate::vm) fn plain_arguments_element(&self, target: Value, index: usize) -> Option<Value> {
        if self.heap.object(target)?.mapped_argument_slot(index).is_some() {
            return None;
        }
        self.stored_arguments_element(target, index)
    }

    /// `plain_arguments_element` of an element a parameter may map: the stored element, which
    /// every write to the parameter reaches while the mapping holds, as the general path's
    /// stores do (`Vm::mapped_argument_store`).
    pub(in crate::vm) fn stored_arguments_element(&self, target: Value, index: usize) -> Option<Value> {
        let object = self.heap.object(target)?;
        if !object.is_arguments_object() || object.has_indexed_descriptors() {
            return None;
        }
        let value = self.heap.array_elements(target)?.get(index)?;
        (!value.is_deleted()).then_some(value)
    }

    /// The element of `arguments` that parameter `slot` aliases, while the mapping holds: its
    /// stored element, which writes to the parameter update (`stored_arguments_element`).
    pub(in crate::vm) fn mapped_window_element(&self, arguments: Value, slot: usize) -> Option<Value> {
        (self.heap.object(arguments)?.mapped_argument_slot(slot) == Some(slot))
            .then(|| self.stored_arguments_element(arguments, slot))
            .flatten()
    }
}
