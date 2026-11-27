//! The reads of an arguments object's own `length` and elements, past its parameter mapping.
use super::*;

impl Vm {
    /// The own data `length` of an arguments object that keeps the layout it was made with
    /// (a realm's `ArgumentsObjectTemplate` shape, which fixes the property, its attributes
    /// and its slot); `None` for any other value or layout.
    pub fn arguments_object_length(&self, target: Value) -> Option<Value> {
        let object = self.heap.object_ref(target)?;
        if !self.heap.array(target)?.is_arguments_object() {
            return None;
        }
        let shape = object.shape();
        [true, false].into_iter().find_map(|expose_callee| {
            let template =
                self.realm.intrinsics.arguments_objects.get(&(self.realm.globals, expose_callee))?;
            (self.heap.object_ref(template.anchor)?.shape() == shape)
                .then(|| self.heap.property_get_ref(object, template.length_slot))
                .flatten()
        })
    }
}
