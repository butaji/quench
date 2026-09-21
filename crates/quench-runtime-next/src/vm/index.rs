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
        if let Some(index) = key.as_int().filter(|index| *index >= 0)
            && let Some(Cell::Array { elements, .. }) = self.heap.get(object)
            && let Some(value) = elements.get(index as usize).copied()
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
        if let Some(index) = key.as_number().filter(|x| *x >= 0.0 && x.fract() == 0.0) {
            if let Some(Cell::Array { elements, .. }) = self.heap.get(object) {
                let index = index as usize;
                let dense = elements.get(index).copied();
                let sparse = dense
                    .is_none()
                    .then(|| self.heap.sparse_get(object, index))
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
        let key = self.to_string(p, key)?;
        let atom = self.intern_atom(&key);
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
            let replaces = matches!(
                self.heap.get(object),
                Some(Cell::Array { elements, .. }) if index < elements.len()
            );
            if replaces {
                let Some(Cell::Array { elements, .. }) = self.heap.get_mut(object) else {
                    unreachable!()
                };
                #[cfg(feature = "profile-aggregate")]
                self.profile
                    .array_write_ownership(Rc::strong_count(elements) != 1);
                mutable_array_elements(elements)[index] = value;
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
        let key = self.to_string(p, key)?;
        let atom = self.intern_atom(&key);
        self.set_property(object, atom, value)
    }

    pub(super) fn set_array_element(&mut self, object: Value, index: usize, value: Value) -> bool {
        let Some(Cell::Array { elements, .. }) = self.heap.get(object) else {
            return false;
        };
        let dense_len = elements.len();
        if index < dense_len {
            let Some(Cell::Array { elements, .. }) = self.heap.get_mut(object) else {
                unreachable!()
            };
            #[cfg(feature = "profile-aggregate")]
            self.profile
                .array_write_ownership(Rc::strong_count(elements) != 1);
            mutable_array_elements(elements)[index] = value;
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
            elements.resize(index + 1, Value::UNDEFINED);
            elements[index] = value;
        }
        true
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
