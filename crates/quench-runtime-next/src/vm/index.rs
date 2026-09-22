use super::property_key::PropertyKey;
use super::*;

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
    #[inline(always)]
    pub(super) fn get_index(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        key: Value,
    ) -> Result<Value, JsError> {
        if self.typed_array_out_of_bounds(object) {
            return Err(self.type_error(
                p,
                "cannot access typed array with an out-of-bounds backing buffer".into(),
            ));
        }
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
        if self.typed_array_out_of_bounds(object) {
            return Err(self.type_error(
                p,
                "cannot access typed array with an out-of-bounds backing buffer".into(),
            ));
        }
        if matches!(self.heap.get(key), Some(Cell::Symbol(_))) {
            if let Some(Cell::Proxy {
                target, handler, ..
            }) = self.heap.get(object).cloned()
            {
                return self.proxy_get_symbol(p, target, handler, object, key);
            }
            let mut owner = object;
            loop {
                if let Some(value) = self.symbol_property(owner, key) {
                    let attributes = self
                        .descriptors
                        .get(&(owner, PropertyKey::symbol(key)))
                        .copied()
                        .unwrap_or(DEFAULT_PROPERTY_ATTRIBUTES);
                    if attributes.accessor {
                        return attributes
                            .getter
                            .filter(|getter| !getter.is_undefined())
                            .map_or(Ok(Value::UNDEFINED), |getter| {
                                self.call_value(p, getter, object, &[])
                            });
                    }
                    return Ok(value);
                }
                let Some(data) = self.object_data(owner) else {
                    return Ok(Value::UNDEFINED);
                };
                owner = data.proto;
                if owner.is_null() {
                    return Ok(Value::UNDEFINED);
                }
            }
        }
        if let Some(index) = key.as_number().filter(|x| *x >= 0.0 && x.fract() == 0.0) {
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
                return Ok(dense.or(sparse).unwrap_or(Value::UNDEFINED));
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
        if let Some(index) = key.as_int().filter(|index| *index >= 0) {
            let index = index as usize;
            if self
                .array_descriptor(object, index)
                .is_some_and(|attributes| attributes.accessor)
            {
                let atom = self.intern_atom(&index.to_string());
                return self.set_property_with_program(p, object, atom, value);
            }
            if self
                .array_descriptor(object, index)
                .is_some_and(|attributes| !attributes.writable)
            {
                return Ok(());
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
        self.set_index_slow(p, object, key, value)
    }

    #[inline(never)]
    fn set_index_slow(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        key: Value,
        value: Value,
    ) -> Result<(), JsError> {
        if matches!(self.heap.get(key), Some(Cell::Symbol(_))) {
            if let Some(Cell::Proxy {
                target, handler, ..
            }) = self.heap.get(object).cloned()
            {
                return self.proxy_set_symbol(p, target, handler, object, key, value);
            }
            if let Some(attributes) = self.descriptors.get(&(object, PropertyKey::symbol(key)))
                && attributes.accessor
            {
                if let Some(setter) = attributes.setter {
                    self.call_value(p, setter, object, &[value])?;
                }
                return Ok(());
            }
            return self.set_symbol_property(object, key, value);
        }
        if let Some(index) = key.as_number().filter(|x| *x >= 0.0 && x.fract() == 0.0) {
            #[cfg(feature = "profile-aggregate")]
            let kind = self.heap.get(object).and_then(|cell| match cell {
                Cell::Array { elements, .. } => {
                    let index = index as usize;
                    let sparse = self.heap.sparse_length(object).is_some()
                        || index > 1024 && index > elements.len().saturating_mul(4).max(16);
                    Some(if sparse {
                        2
                    } else {
                        usize::from(index >= elements.len())
                    })
                }
                _ => None,
            });
            if self.set_array_element(object, index as usize, value) {
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
                .is_some_and(|attributes| !attributes.writable)
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
        let sparse = self.heap.sparse_length(object).is_some()
            || index > 1024 && index > dense_len.saturating_mul(4).max(16);
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

    pub(super) fn check_array_mutation(
        &self,
        object: Value,
        writes: bool,
        adds: bool,
        removes: bool,
    ) -> Result<(), JsError> {
        if self.object_data(object).is_some_and(Object::is_frozen) && (writes || adds || removes) {
            return Err(JsError("cannot mutate frozen array".into()));
        }
        if self
            .object_data(object)
            .is_some_and(|object| !object.is_extensible())
            && (adds || removes)
        {
            return Err(JsError("cannot change sealed array length".into()));
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
