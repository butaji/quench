//! Immutable creation names share the shape's lifetime and property transitions.
use super::*;

pub(super) fn shape_names(shapes: &[Shape], shape: u32) -> (Option<Atom>, Option<(Value, bool)>) {
    let (mut function, mut constructed) = (None, None);
    let mut current = Some(shape);
    while let Some(id) = current {
        let shape = &shapes[id as usize];
        match shape.transition {
            ShapeTransition::FunctionName { name } if function.is_none() => function = Some(name),
            ShapeTransition::Constructed {
                constructor,
                new_target_is_base,
            } if constructed.is_none() => constructed = Some((constructor, new_target_is_base)),
            _ => {}
        }
        current = shape.parent();
    }
    (function, constructed)
}

impl<H: Host> Vm<H> {
    pub(super) fn capture_function_name(&mut self, function: Value, name: Value) {
        let Some(Cell::String(name)) = self.heap.get(name) else {
            return;
        };
        let name = name.clone();
        let name = self.intern_js_atom(&name);
        if self.original_function_name(function) == Some(name) {
            return;
        }
        let Some(object) = self.object_data(function) else {
            return;
        };
        let shape = object.shape();
        let shape =
            self.append_cached_shape_transition(shape, ShapeTransition::FunctionName { name });
        self.object_data_mut(function)
            .expect("function object")
            .set_shape(shape);
    }

    pub(super) fn original_function_name(&self, function: Value) -> Option<Atom> {
        let object = self.object_data(function)?;
        shape_names(&self.shapes, object.shape()).0
    }

    /// V8's debug name reads data properties but never invokes a name accessor.
    pub(super) fn debug_function_name(&self, function: Value) -> Option<JsString> {
        let name_key = self.find_atom(Self::atom_hash_str("name"), |atom| {
            self.atom_units_equal_str(atom, "name")
        });
        if let Some(name_key) = name_key {
            let mut current = function;
            while let Some(object) = self.object_data(current) {
                if matches!(self.heap.get(current), Some(Cell::Proxy { .. })) {
                    break;
                }
                if let Some(attributes) =
                    self.property_attributes(current, PropertyKey::string(name_key))
                {
                    if !attributes.accessor {
                        if let Some(Cell::String(name)) = self
                            .own_property(current, name_key)
                            .and_then(|value| self.heap.get(value))
                        {
                            return Some(name.clone());
                        }
                    }
                    break;
                }
                current = object.proto();
            }
        }
        self.original_function_name(function)
            .map(|name| self.atom_value(name))
    }

    fn constructed_shape(
        &mut self,
        shape: u32,
        constructor: Value,
        new_target_is_base: bool,
    ) -> u32 {
        if self.original_function_name(constructor).is_none()
            || shape_names(&self.shapes, shape).1 == Some((constructor, new_target_is_base))
        {
            return shape;
        }
        self.append_cached_shape_transition(
            shape,
            ShapeTransition::Constructed {
                constructor,
                new_target_is_base,
            },
        )
    }

    pub(super) fn capture_constructed_name(
        &mut self,
        object: &mut Object,
        constructor: Value,
        new_target_is_base: bool,
    ) {
        object.set_shape(self.constructed_shape(object.shape(), constructor, new_target_is_base));
    }

    pub(super) fn capture_constructed_value(
        &mut self,
        value: Value,
        constructor: Value,
        new_target_is_base: bool,
    ) {
        let Some(shape) = self.object_data(value).map(Object::shape) else {
            return;
        };
        let shape = self.constructed_shape(shape, constructor, new_target_is_base);
        self.object_data_mut(value)
            .expect("constructed object")
            .set_shape(shape);
    }

    pub(crate) fn embedding_original_function_name(
        &self,
        root: RootId,
    ) -> Result<Option<JsString>, JsError> {
        let value = self.embedding_value(root)?;
        if !matches!(self.heap.get(value), Some(Cell::Function { kind, .. }) if !matches!(kind, FunctionKind::Native(Native::FunctionBoundCall)))
        {
            return Ok(None);
        }
        Ok(self
            .original_function_name(value)
            .map(|name| self.atom_value(name)))
    }

    pub(crate) fn embedding_constructed_name(
        &self,
        root: RootId,
    ) -> Result<Option<JsString>, JsError> {
        let value = self.embedding_value(root)?;
        let Some(object) = self.object_data(value) else {
            return Ok(None);
        };
        if object.was_used_as_prototype() {
            return Ok(None);
        }
        Ok(shape_names(&self.shapes, object.shape())
            .1
            .filter(|(_, new_target_is_base)| *new_target_is_base)
            .and_then(|(constructor, _)| self.debug_function_name(constructor)))
    }
}
