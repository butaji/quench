use super::property_key::PropertyKey;
use super::*;

const MAX_DENSE_ARRAY_HOLE_GAP: usize = 1024;

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
            Some(Cell::String(_)) => "String",
            Some(Cell::Symbol(_)) => "Symbol",
            Some(Cell::BigInt(_)) => "BigInt",
            _ if value.as_bool().is_some() => "Boolean",
            _ if value.as_number().is_some() => "Number",
            _ => return None,
        };
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
        if let Some(index) = key.as_int().filter(|index| *index >= 0)
            && let Some(value) = self.typed_array_get(object, index as usize)
        {
            return Ok(value);
        }
        if let Some(index) = key.as_int().filter(|index| *index >= 0)
            && self
                .array_descriptor(object, index as usize)
                .is_some_and(|attributes| attributes.accessor)
        {
            let atom = self.intern_atom(&index.to_string());
            return self.get_property(p, object, atom);
        }
        if let Some(index) = key.as_int().filter(|index| *index >= 0)
            && let Some(Cell::Array { elements, .. }) = self.heap.get(object)
            && let Some(value) = elements
                .get(index as usize)
                .copied()
                .filter(|value| !value.is_deleted())
        {
            #[cfg(feature = "profile-aggregate")]
            self.profile.index_get(0);
            return Ok(value);
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
        if matches!(self.heap.get(key), Some(Cell::Symbol(_))) {
            return self.get_symbol_property_with_receiver(p, object, key, object);
        }
        if let Some(index) = key.as_number().filter(|x| {
            *x >= 0.0
                && x.fract() == 0.0
                && (!matches!(self.heap.get(object), Some(Cell::Array { .. }))
                    || *x < u32::MAX as f64)
        }) {
            if let Some(value) = self.typed_array_get(object, index as usize) {
                return Ok(value);
            }
            if let Some(Cell::Array { elements, .. }) = self.heap.get(object) {
                let index = index as usize;
                let dense = elements
                    .get(index)
                    .copied()
                    .filter(|value| !value.is_deleted());
                let sparse = dense
                    .is_none()
                    .then(|| {
                        self.heap
                            .sparse_get(object, index)
                            .filter(|value| !value.is_deleted())
                    })
                    .flatten();
                #[cfg(feature = "profile-aggregate")]
                self.profile.index_get(
                    usize::from(key.as_int().is_none()) * 3
                        + if dense.is_some() {
                            0
                        } else if sparse.is_some() {
                            1
                        } else {
                            2
                        },
                );
                if let Some(value) = dense.or(sparse) {
                    return Ok(value);
                }
            }
            #[cfg(feature = "profile-aggregate")]
            self.profile.index_get(6);
        }
        #[cfg(feature = "profile-aggregate")]
        if key.as_number().is_none_or(|x| x < 0.0 || x.fract() != 0.0) {
            self.profile.index_get(7);
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
        if let Some(index) = key.as_int().filter(|index| *index >= 0) {
            let index = index as usize;
            if let Some(attributes) = self.array_descriptor(object, index)
                && attributes.accessor
            {
                if strict && attributes.setter.is_none() {
                    return Err(self.type_error(p, "array index has no setter".into()));
                }
                let atom = self.intern_atom(&index.to_string());
                return self.set_property_with_program(p, object, atom, value);
            }
            if !self.has_own_array_index(object, index)
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
                    return Err(JsError("cannot write sealed or frozen array".into()));
                }
            }
            if self.typed_array_set(p, object, index, value)? {
                return Ok(());
            }
            let replaces = matches!(
                self.heap.get(object),
                Some(Cell::Array { elements, .. })
                    if elements.get(index).is_some_and(|value| !value.is_deleted())
            );
            if replaces {
                let Some(Cell::Array { elements, .. }) = self.heap.get_mut(object) else {
                    unreachable!()
                };
                #[cfg(feature = "profile-aggregate")]
                self.profile
                    .array_write_ownership(Rc::strong_count(elements) != 1);
                mutable_array_elements(elements)[index] = value;
                self.sync_mapped_argument(object, index, value);
                #[cfg(feature = "profile-aggregate")]
                self.profile.index_set(0);
                return Ok(());
            }
        }
        self.set_index_slow(p, object, key, value, strict)
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
        if matches!(self.heap.get(key), Some(Cell::Symbol(_))) {
            if let Some(Cell::Proxy {
                target, handler, ..
            }) = self.heap.get(object).cloned()
            {
                return self.proxy_set_symbol(p, target, handler, object, key, value);
            }
            if let Some(attributes) = self.property_attributes(object, PropertyKey::symbol(key))
                && attributes.accessor
            {
                if let Some(setter) = attributes.setter {
                    self.call_value(p, setter, object, &[value])?;
                } else if strict {
                    return Err(self.type_error(p, "symbol property has no setter".into()));
                }
                return Ok(());
            }
            return match self.set_symbol_property(object, key, value) {
                Ok(()) => Ok(()),
                Err(_) if !strict => Ok(()),
                Err(error) => Err(error),
            };
        }
        if matches!(self.heap.get(object), Some(Cell::Array { .. }))
            && self.prototype_chain_contains_proxy(object)
        {
            let key = self.coerce_js_string(p, key)?;
            let atom = self.intern_js_atom(&key);
            return self.set_property_with_program(p, object, atom, value);
        }
        if let Some(index) = key.as_number().filter(|x| {
            *x >= 0.0
                && x.fract() == 0.0
                && (!matches!(self.heap.get(object), Some(Cell::Array { .. }))
                    || *x < u32::MAX as f64)
        }) {
            let index = index as usize;
            if let Some(attributes) = self.array_descriptor(object, index) {
                if attributes.accessor {
                    if let Some(setter) = attributes.setter {
                        self.call_value(p, setter, object, &[value])?;
                    } else if strict {
                        return Err(self.type_error(p, "array index has no setter".into()));
                    }
                    return Ok(());
                }
                if !attributes.writable {
                    return if strict {
                        Err(self.type_error(p, "array index is not writable".into()))
                    } else {
                        Ok(())
                    };
                }
            }
            if self.set_inherited_index_accessor(p, object, index, value, strict)? {
                return Ok(());
            }
            #[cfg(feature = "profile-aggregate")]
            let kind = self.heap.get(object).and_then(|cell| match cell {
                Cell::Array { elements, .. } => {
                    let sparse =
                        self.array_index_uses_sparse_storage(object, index, elements.len());
                    Some(if sparse {
                        2
                    } else {
                        usize::from(index >= elements.len())
                    })
                }
                _ => None,
            });
            if self.set_array_element(object, index, value) {
                #[cfg(feature = "profile-aggregate")]
                self.profile
                    .index_set(usize::from(key.as_int().is_none()) * 3 + kind.unwrap());
                return Ok(());
            }
            #[cfg(feature = "profile-aggregate")]
            self.profile.index_set(6);
        }
        #[cfg(feature = "profile-aggregate")]
        if key.as_number().is_none_or(|x| x < 0.0 || x.fract() != 0.0) {
            self.profile.index_set(7);
        }
        let key = self.coerce_js_string(p, key)?;
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
        self.set_property_with_program(p, object, atom, value)
    }

    fn set_inherited_index_accessor(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        index: usize,
        value: Value,
        strict: bool,
    ) -> Result<bool, JsError> {
        let atom = self.intern_atom(&index.to_string());
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
