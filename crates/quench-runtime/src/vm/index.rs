use super::*;

const MAX_DENSE_ARRAY_HOLE_GAP: usize = 1024;
#[cfg(feature = "profile-aggregate")]
const ARRAY_INDEX_GET_DENSE: usize = 0;
#[cfg(feature = "profile-aggregate")]
const ARRAY_INDEX_GET_SPARSE: usize = 1;

#[inline(always)]
pub(super) fn mutable_array_elements(elements: &mut Rc<Vec<Value>>) -> &mut Vec<Value> {
    if Rc::strong_count(elements) == 1 {
        debug_assert_eq!(Rc::weak_count(elements), 0);
        // SAFETY: this module is the only array-storage mutation boundary. A
        // strong count of one proves that no other Rc can observe the vector,
        // the VM creates no Weak handles, and Rc is single-threaded.
        unsafe { &mut *Rc::as_ptr(elements).cast_mut() }
    } else {
        detach_array_elements(elements)
    }
}

#[cold]
#[inline(never)]
fn detach_array_elements(elements: &mut Rc<Vec<Value>>) -> &mut Vec<Value> {
    Rc::make_mut(elements)
}

impl<H: Host> Vm<H> {
    pub(super) fn primitive_prototype(&self, value: Value) -> Option<Value> {
        let name = match self.heap.get(value) {
            Some(Cell::String(_)) => return Some(self.string_proto),
            Some(Cell::Symbol(_)) => "Symbol",
            Some(Cell::BigInt(_)) => "BigInt",
            _ if value.as_bool().is_some() => "Boolean",
            _ if value.as_number().is_some() => "Number",
            _ => return None,
        };
        self.primitive_prototype_named(name)
    }

    pub(super) fn primitive_prototype_named(&self, name: &str) -> Option<Value> {
        let constructor = self
            .lookup_atom(name)
            .and_then(|atom| self.own_property(self.realm.globals, atom))?;
        self.lookup_atom("prototype")
            .and_then(|atom| self.own_property(constructor, atom))
    }

    pub(super) fn require_object_coercible(
        &mut self,
        p: &ResidualProgram,
        value: Value,
    ) -> Result<(), JsError> {
        if value.is_null() || value.is_undefined() {
            return Err(self.type_error(p, "cannot convert null or undefined to object".into()));
        }
        Ok(())
    }

    #[inline(always)]
    pub(super) fn get_index(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        key: Value,
    ) -> Result<Value, JsError> {
        if let Some(index) = self.array_index_key(key) {
            let index = index as usize;
            // Accessor indices are stored as holes, so a present dense element is a data property.
            if let Some(Cell::Array { elements, .. }) = self.heap.get(object)
                && let Some(value) = elements
                    .get(index)
                    .copied()
                    .filter(|value| !value.is_deleted())
            {
                #[cfg(feature = "profile-aggregate")]
                self.profile.index_get(ARRAY_INDEX_GET_DENSE);
                return Ok(value);
            }
            if let Some(value) = self.typed_array_get(object, index) {
                return Ok(value);
            }
            if let Some((atom, attributes)) = self.array_descriptor_entry(object, index)
                && attributes.accessor
            {
                return self.get_property(p, object, atom);
            }
            if matches!(self.heap.get(object), Some(Cell::Array { .. }))
                && let Some(value) = self
                    .heap
                    .sparse_get(object, index)
                    .filter(|value| !value.is_deleted())
            {
                #[cfg(feature = "profile-aggregate")]
                self.profile.index_get(ARRAY_INDEX_GET_SPARSE);
                return Ok(value);
            }
        }
        self.get_index_slow(p, object, key)
    }

    #[inline(never)]
    fn get_index_slow(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        key: Value,
    ) -> Result<Value, JsError> {
        if object.is_null() || object.is_undefined() {
            let atom = self.intern_atom("");
            return self.get_property(p, object, atom);
        }
        let key = match self.heap.get(key) {
            Some(Cell::String(_) | Cell::Symbol(_)) => key,
            _ => self.to_property_key(p, key)?,
        };
        if matches!(self.heap.get(key), Some(Cell::Symbol(_))) {
            return self.get_symbol_property_with_receiver(p, object, key, object);
        }

        let key = self.coerce_js_string(p, key)?;
        let atom = self.intern_js_atom(&key);
        self.get_property(p, object, atom)
    }

    #[inline(never)]
    pub(super) fn set_index(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        key: Value,
        value: Value,
    ) -> Result<(), JsError> {
        self.set_index_mode(p, object, key, value, false)
    }

    pub(super) fn set_index_mode(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        key: Value,
        value: Value,
        strict: bool,
    ) -> Result<(), JsError> {
        if object.is_null() || object.is_undefined() {
            return Err(self.type_error(
                p,
                if object.is_null() {
                    "cannot set properties of null".into()
                } else {
                    "cannot set properties of undefined".into()
                },
            ));
        }
        if let Some(index) = self.array_index_key(key) {
            let index = index as usize;
            // Descriptors, accessors and frozen state are the only things that make replacing a
            // present dense element differ from a plain store, and a plain array has none.
            if let Some(Cell::Array { object: data, elements }) = self.heap.get(object)
                && !data.has_indexed_descriptors()
                && !data.is_frozen()
                && elements.get(index).is_some_and(|value| !value.is_deleted())
            {
                self.replace_dense_element(object, index, value);
                return Ok(());
            }
            if let Some((atom, attributes)) = self.array_descriptor_entry(object, index)
                && attributes.accessor
            {
                if strict && attributes.setter.is_none() {
                    return Err(self.type_error(p, "array index has no setter".into()));
                }
                return self.set_property_with_program(p, object, atom, value);
            }
            if matches!(self.heap.get(object), Some(Cell::Array { .. }))
                && !self.has_own_array_index(object, index)
                && self.set_inherited_index_accessor(p, object, index, value, strict)?
            {
                return Ok(());
            }
            if self
                .array_descriptor(object, index)
                .is_some_and(|attributes| !attributes.writable)
            {
                return if strict {
                    Err(self.type_error(p, "array index is not writable".into()))
                } else {
                    Ok(())
                };
            }
            if let Some(Cell::Array { elements, .. }) = self.heap.get(object) {
                let existing = index < elements.len()
                    && elements.get(index).is_some_and(|value| !value.is_deleted())
                    || self
                        .heap
                        .sparse_get(object, index)
                        .is_some_and(|value| !value.is_deleted());
                let integrity = self.object_data(object);
                if integrity.is_some_and(Object::is_frozen)
                    || integrity.is_some_and(|object| !object.is_extensible()) && !existing
                {
                    let atom = self
                        .lookup_array_index_atom(index)
                        .unwrap_or_else(|| self.intern_atom(&index.to_string()));
                    return self.set_property_with_program_mode(p, object, atom, value, strict);
                }
            }
            if self.typed_array_set(p, object, index, value)? {
                return Ok(());
            }
            if matches!(self.heap.get(object), Some(Cell::Array { .. }))
                && !self.has_own_array_index(object, index)
                && self.lookup_array_index_atom(index).is_none()
                && !self.prototype_chain_has_indexed_set_exotic(object)
                && self.set_array_element(object, index, value)
            {
                return Ok(());
            }
            let replaces = matches!(
                self.heap.get(object),
                Some(Cell::Array { elements, .. })
                    if elements.get(index).is_some_and(|value| !value.is_deleted())
            );
            if replaces {
                self.replace_dense_element(object, index, value);
                return Ok(());
            }
        }
        self.set_index_slow(p, object, key, value, strict)
    }

    /// Overwrites a present dense array element after every guard has passed.
    #[inline(always)]
    fn replace_dense_element(&mut self, object: Value, index: usize, value: Value) {
        let Some(Cell::Array { elements, .. }) = self.heap.get_mut(object) else {
            unreachable!("dense element replacement requires an array")
        };
        #[cfg(feature = "profile-aggregate")]
        self.profile
            .array_write_ownership(Rc::strong_count(elements) != 1);
        mutable_array_elements(elements)[index] = value;
        self.sync_mapped_argument(object, index, value);
        #[cfg(feature = "profile-aggregate")]
        self.profile.index_set(0);
    }

    #[inline(never)]
    fn set_index_slow(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        key: Value,
        value: Value,
        strict: bool,
    ) -> Result<(), JsError> {
        let key = match self.heap.get(key) {
            Some(Cell::String(_) | Cell::Symbol(_)) => key,
            _ => self.to_property_key(p, key)?,
        };
        if matches!(self.heap.get(key), Some(Cell::Symbol(_))) {
            let succeeded =
                self.set_symbol_property_with_receiver(p, object, key, value, object)?;
            return if succeeded || !strict {
                Ok(())
            } else {
                Err(self.type_error(p, "cannot assign symbol property".into()))
            };
        }
        if matches!(self.heap.get(object), Some(Cell::Array { .. }))
            && self.prototype_chain_contains_proxy(object)
        {
            let key = self.coerce_js_string(p, key)?;
            let atom = self.intern_js_atom(&key);
            return self.set_property_with_program(p, object, atom, value);
        }

        let key = self.coerce_js_string(p, key)?;
        if let Some(index) = super::object_static::array_index(key.host_string())
            && matches!(self.heap.get(object), Some(Cell::Array { .. }))
            && !self.has_own_array_index(object, index as usize)
            && self.prototype_chain_has_typed_array(object)
        {
            let atom = self.intern_js_atom(&key);
            return self.set_property_with_program_mode(p, object, atom, value, strict);
        }
        if let Some(index) = super::object_static::array_index(key.host_string())
            && matches!(self.heap.get(object), Some(Cell::Array { .. }))
        {
            if self
                .array_descriptor(object, index as usize)
                .is_some_and(|attributes| attributes.accessor)
            {
                if strict
                    && self
                        .array_descriptor(object, index as usize)
                        .is_some_and(|attributes| attributes.setter.is_none())
                {
                    return Err(self.type_error(p, "array index has no setter".into()));
                }
                let atom = self.intern_js_atom(&key);
                return self.set_property_with_program(p, object, atom, value);
            }
            if self
                .array_descriptor(object, index as usize)
                .is_some_and(|attributes| !attributes.writable)
            {
                return if strict {
                    Err(self.type_error(p, "array index is not writable".into()))
                } else {
                    Ok(())
                };
            }
            if !self.has_own_array_index(object, index as usize)
                && self.set_inherited_index_accessor(p, object, index as usize, value, strict)?
            {
                return Ok(());
            }
            if self.set_array_element(object, index as usize, value) {
                return Ok(());
            }
        }
        let atom = self.intern_js_atom(&key);
        self.set_property_with_program_mode(p, object, atom, value, strict)
    }

    fn set_inherited_index_accessor(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        index: usize,
        value: Value,
        strict: bool,
    ) -> Result<bool, JsError> {
        let Some(atom) = self.lookup_array_index_atom(index) else {
            return Ok(false);
        };
        let Some(attributes) = self.property_accessor(object, atom) else {
            return Ok(false);
        };
        if let Some(setter) = attributes.setter {
            self.call_value(p, setter, object, &[value])?;
        } else if strict {
            return Err(self.type_error(p, "property has no setter".into()));
        }
        Ok(true)
    }

    fn array_index_key(&self, key: Value) -> Option<u32> {
        if let Some(index) = key.as_int() {
            return u32::try_from(index).ok();
        }
        let Some(Cell::String(key)) = self.heap.get(key) else {
            return None;
        };
        super::object_static::array_index(key.host_string())
    }

    pub(super) fn set_array_element(&mut self, object: Value, index: usize, value: Value) -> bool {
        let Some(Cell::Array { elements, .. }) = self.heap.get(object) else {
            return false;
        };
        if self.check_array_element_write(object, index).is_err() {
            return false;
        }
        let dense_len = elements.len();
        if index < dense_len {
            let Some(Cell::Array { elements, .. }) = self.heap.get_mut(object) else {
                unreachable!()
            };
            #[cfg(feature = "profile-aggregate")]
            self.profile
                .array_write_ownership(Rc::strong_count(elements) != 1);
            mutable_array_elements(elements)[index] = value;
            self.sync_mapped_argument(object, index, value);
            return true;
        }
        let sparse = self.array_index_uses_sparse_storage(object, index, dense_len);
        if sparse {
            self.heap.sparse_set(object, index, value);
        } else if let Some(Cell::Array { elements, .. }) = self.heap.get_mut(object) {
            #[cfg(feature = "profile-aggregate")]
            self.profile
                .array_write_ownership(Rc::strong_count(elements) != 1);
            let elements = mutable_array_elements(elements);
            elements.resize(index + 1, Value::DELETED);
            elements[index] = value;
        }
        self.sync_mapped_argument(object, index, value);
        true
    }

    pub(super) fn define_array_literal_element(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        index: usize,
        value: Value,
    ) -> Result<(), JsError> {
        if self.set_array_element(object, index, value) {
            Ok(())
        } else {
            Err(self.type_error(p, "cannot define array literal element".into()))
        }
    }

    fn array_index_uses_sparse_storage(
        &self,
        object: Value,
        index: usize,
        dense_length: usize,
    ) -> bool {
        self.heap.sparse_length(object).is_some()
            || index.saturating_sub(dense_length) > MAX_DENSE_ARRAY_HOLE_GAP
    }

    pub(super) fn check_array_element_write(
        &self,
        object: Value,
        index: usize,
    ) -> Result<(), JsError> {
        let Some(Cell::Array { elements, .. }) = self.heap.get(object) else {
            return Err(JsError("array receiver is not array".into()));
        };
        let existing = index < elements.len()
            && elements.get(index).is_some_and(|value| !value.is_deleted())
            || self
                .heap
                .sparse_get(object, index)
                .is_some_and(|value| !value.is_deleted());
        let integrity = self.object_data(object);
        if integrity.is_some_and(Object::is_frozen)
            || integrity.is_some_and(|object| !object.is_extensible()) && !existing
        {
            return Err(JsError("cannot write sealed or frozen array".into()));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct SilentHost;
    impl Host for SilentHost {
        fn write_line(&mut self, _: &str) {}
        fn clock_millis(&mut self) -> f64 {
            0.0
        }
    }

    #[test]
    fn high_array_index_stays_out_of_dense_storage() {
        let mut vm = Vm::new(SilentHost);
        let array = vm.heap.alloc(Cell::Array {
            object: Vm::<SilentHost>::empty_object(Value::NULL),
            elements: Rc::new(vec![]),
        });
        assert!(vm.set_array_element(array, 1_000_000, Value::TRUE));
        let Some(Cell::Array { elements, .. }) = vm.heap.get(array) else {
            panic!("array cell")
        };
        assert!(elements.is_empty());
        assert_eq!(vm.heap.sparse_length(array), Some(1_000_001));
    }

    #[test]
    fn bounded_holey_array_length_uses_dense_storage() {
        let program = crate::Engine::specialize("", "dense-array-holes.js").unwrap();
        let mut vm = Vm::new(SilentHost);
        let array = vm.array_create(&program, 16_900).unwrap();

        let Some(Cell::Array { elements, .. }) = vm.heap.get(array) else {
            panic!("array cell")
        };
        assert_eq!(elements.len(), 16_900);
        assert!(elements.iter().all(|value| value.is_deleted()));
        assert_eq!(vm.heap.sparse_length(array), None);

        assert!(vm.set_array_element(array, 16_899, Value::TRUE));
        let Some(Cell::Array { elements, .. }) = vm.heap.get(array) else {
            panic!("array cell")
        };
        assert_eq!(elements[16_899], Value::TRUE);
        assert!(elements[16_898].is_deleted());
        assert_eq!(vm.heap.sparse_length(array), None);
    }

    #[test]
    fn bounded_sparse_write_promotes_to_dense_storage() {
        let mut vm = Vm::new(SilentHost);
        let array = vm.heap.alloc(Cell::Array {
            object: Vm::<SilentHost>::empty_object(Value::NULL),
            elements: Rc::new(Vec::new()),
        });

        assert!(vm.set_array_element(array, 2_048, Value::TRUE));

        let Some(Cell::Array { elements, .. }) = vm.heap.get(array) else {
            panic!("array cell")
        };
        assert_eq!(elements.len(), 2_049);
        assert!(elements[..2_048].iter().all(|value| value.is_deleted()));
        assert_eq!(elements[2_048], Value::TRUE);
        assert_eq!(vm.heap.sparse_length(array), None);
    }

    #[test]
    fn shrinking_large_sparse_array_promotes_and_preserves_values() {
        let mut vm = Vm::new(SilentHost);
        let array = vm.heap.alloc(Cell::Array {
            object: Vm::<SilentHost>::empty_object(Value::NULL),
            elements: Rc::new(Vec::new()),
        });
        vm.heap.sparse_set_length(array, 1_000_000);
        vm.heap.sparse_set(array, 3, Value::TRUE);
        vm.heap.sparse_set(array, 999_999, Value::FALSE);

        vm.heap.sparse_set_length(array, 16_900);

        let Some(Cell::Array { elements, .. }) = vm.heap.get(array) else {
            panic!("array cell")
        };
        assert_eq!(elements.len(), 16_900);
        assert_eq!(elements[3], Value::TRUE);
        assert!(elements[4].is_deleted());
        assert_eq!(vm.heap.sparse_length(array), None);
    }

    #[test]
    fn ordinary_indexed_writes_do_not_intern_property_names() {
        let program = crate::Engine::specialize("", "array-index-atoms.js").unwrap();
        let mut vm = Vm::new(SilentHost);
        let array = vm.heap.alloc(Cell::Array {
            object: Vm::<SilentHost>::empty_object(Value::NULL),
            elements: Rc::new(Vec::new()),
        });
        let atom_count = vm.dynamic_atoms.len();
        vm.heap.retain_allocations_for_test();
        let cell_count = vm.heap.occupied_cell_count_for_test();

        for index in 0..1024 {
            vm.set_index(
                &program,
                array,
                Value::integer(index),
                Value::integer(index),
            )
            .unwrap();
        }

        assert_eq!(vm.dynamic_atoms.len(), atom_count);
        assert!(vm.lookup_array_index_atom(1023).is_none());
        assert_eq!(vm.heap.occupied_cell_count_for_test(), cell_count);
    }

    #[test]
    fn deleting_an_uninterned_present_index_does_not_create_an_atom() {
        let mut vm = Vm::new(SilentHost);
        let array = vm.heap.alloc(Cell::Array {
            object: Vm::<SilentHost>::empty_object(Value::NULL),
            elements: Rc::new(vec![Value::FALSE; 1024]),
        });
        let atom_count = vm.dynamic_atoms.len();

        assert_eq!(vm.delete_array_index(array, 1023), Value::TRUE);

        let Some(Cell::Array { elements, .. }) = vm.heap.get(array) else {
            panic!("array cell")
        };
        assert!(elements[1023].is_deleted());
        assert_eq!(vm.dynamic_atoms.len(), atom_count);
        assert!(vm.lookup_array_index_atom(1023).is_none());
    }

    #[test]
    fn array_index_atom_lookup_uses_canonical_decimal_text() {
        let mut vm = Vm::new(SilentHost);
        for index in [0, 9, 10, 99, 100, 1023, u32::MAX as usize, usize::MAX] {
            let expected = vm.intern_atom(&index.to_string());
            assert_eq!(vm.lookup_array_index_atom(index), Some(expected));
        }
    }

    #[test]
    fn indexed_write_still_calls_an_inherited_numeric_setter() {
        let source = r#"
            var observed = -1;
            Object.defineProperty(Array.prototype, "5", {
                configurable: true,
                set: function(value) { observed = value; }
            });
            var array = [];
            array[5] = 17;
            if (observed !== 17 || Object.prototype.hasOwnProperty.call(array, "5")) {
                throw new Error("inherited numeric setter was skipped");
            }
        "#;
        let program = crate::Engine::specialize(source, "array-index-setter.js").unwrap();
        let mut vm = Vm::new(SilentHost);
        vm.execute(&program).unwrap();
    }

    #[test]
    fn indexed_write_keeps_default_inherited_data_semantics_without_atoms() {
        let source = r#"
            var prototype = [41];
            var array = [];
            Object.setPrototypeOf(array, prototype);
            array[0] = 9;
            if (array[0] !== 9 || prototype[0] !== 41 || !array.hasOwnProperty(0)) {
                throw new Error("indexed write did not create an own element");
            }
        "#;
        let program = crate::Engine::specialize(source, "array-index-prototype-data.js").unwrap();
        let mut vm = Vm::new(SilentHost);
        assert!(vm.lookup_array_index_atom(0).is_none());
        vm.execute(&program).unwrap();
    }

    #[test]
    fn unique_array_mutation_keeps_the_allocation() {
        let mut elements = Rc::new(vec![Value::FALSE]);
        let allocation = Rc::as_ptr(&elements);
        mutable_array_elements(&mut elements)[0] = Value::TRUE;
        assert_eq!(Rc::as_ptr(&elements), allocation);
        assert_eq!(elements[0], Value::TRUE);
    }

    #[test]
    fn shared_array_mutation_detaches_the_template() {
        let template = Rc::new(vec![Value::FALSE]);
        let mut elements = Rc::clone(&template);
        mutable_array_elements(&mut elements)[0] = Value::TRUE;
        assert_eq!(template[0], Value::FALSE);
        assert_eq!(elements[0], Value::TRUE);
        assert!(!Rc::ptr_eq(&template, &elements));
    }
}
