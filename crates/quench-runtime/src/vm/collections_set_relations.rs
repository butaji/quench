enum SetRecord {
    Native {
        set: Value,
        root: RootId,
    },
    Like {
        size: f64,
        receiver_root: RootId,
        has_root: RootId,
        keys_root: RootId,
    },
}

enum SetRecordStep<T> {
    Continue(T),
    Stop(T),
}

enum SetRelationResult {
    Set,
    Boolean(bool),
}

impl SetRecord {
    fn size<H: Host>(&self, vm: &Vm<H>) -> f64 {
        match self {
            Self::Native { set, .. } => match vm.heap.get(*set) {
                Some(Cell::Set { entries, .. }) => entries.len() as f64,
                _ => 0.0,
            },
            Self::Like { size, .. } => *size,
        }
    }

    fn release<H: Host>(self, vm: &mut Vm<H>) {
        match self {
            Self::Native { root, .. } => {
                vm.heap.release_root(root);
            }
            Self::Like {
                receiver_root,
                has_root,
                keys_root,
                ..
            } => {
                for root in [receiver_root, has_root, keys_root] {
                    vm.heap.release_root(root);
                }
            }
        }
    }
}

impl<H: Host> Vm<H> {
    pub(super) fn set_relation(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        receiver: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let receiver_root = self.heap.root(receiver);
        let outcome = (|| {
            if !matches!(self.heap.get(receiver), Some(Cell::Set { .. })) {
                return Err(self.type_error(p, "Set method called on incompatible receiver".into()));
            }
            let other = self.get_set_record(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
            let mut values = Vec::new();
            let mut value_roots = Vec::new();
            let result = self.apply_set_relation(
                p,
                native,
                receiver,
                &other,
                &mut values,
                &mut value_roots,
            );
            other.release(self);
            let result = match result {
                Ok(SetRelationResult::Boolean(value)) => {
                    Ok(if value { Value::TRUE } else { Value::FALSE })
                }
                Ok(SetRelationResult::Set) => {
                    let set = self.heap.alloc(Cell::Set {
                        object: Self::empty_object(self.set_proto),
                        entries: values,
                    });
                    Ok(set)
                }
                Err(error) => Err(error),
            };
            for root in value_roots {
                self.heap.release_root(root);
            }
            result
        })();
        self.heap.release_root(receiver_root);
        outcome
    }

    fn get_set_record(&mut self, p: &ResidualProgram, value: Value) -> Result<SetRecord, JsError> {
        let receiver_root = self.heap.root(value);
        if matches!(self.heap.get(value), Some(Cell::Set { .. })) {
            return Ok(SetRecord::Native {
                set: value,
                root: receiver_root,
            });
        }
        let size_atom = self.intern_atom("size");
        let size = match self
            .get_property(p, value, size_atom)
            .and_then(|value| self.to_number(p, value))
        {
            Ok(size) if !size.is_nan() && size >= 0.0 => size,
            Ok(_) => {
                self.heap.release_root(receiver_root);
                return Err(self.type_error(p, "Set-like size is invalid".into()));
            }
            Err(error) => {
                self.heap.release_root(receiver_root);
                return Err(error);
            }
        };
        let receiver = self.heap.root_value(receiver_root).unwrap_or(value);
        let has_atom = self.intern_atom("has");
        let has = match self.get_property(p, receiver, has_atom) {
            Ok(has) => has,
            Err(error) => {
                self.heap.release_root(receiver_root);
                return Err(error);
            }
        };
        if !self.is_function(has) {
            self.heap.release_root(receiver_root);
            return Err(self.type_error(p, "Set-like has is not callable".into()));
        }
        let has_root = self.heap.root(has);
        let receiver = self.heap.root_value(receiver_root).unwrap_or(value);
        let keys_atom = self.intern_atom("keys");
        let keys = match self.get_property(p, receiver, keys_atom) {
            Ok(keys) => keys,
            Err(error) => {
                self.heap.release_root(has_root);
                self.heap.release_root(receiver_root);
                return Err(error);
            }
        };
        let keys_root = self.heap.root(keys);
        if !self.is_function(keys) {
            self.heap.release_root(keys_root);
            self.heap.release_root(has_root);
            self.heap.release_root(receiver_root);
            return Err(self.type_error(p, "Set-like keys is not callable".into()));
        }
        Ok(SetRecord::Like {
            size,
            receiver_root,
            has_root,
            keys_root,
        })
    }

    fn set_record_has(
        &mut self,
        p: &ResidualProgram,
        other: &SetRecord,
        value: Value,
    ) -> Result<bool, JsError> {
        match other {
            SetRecord::Native { set, .. } => Ok(self.set_entry_index(*set, value).is_some()),
            SetRecord::Like {
                receiver_root,
                has_root,
                ..
            } => {
                let receiver = self.heap.root_value(*receiver_root).unwrap_or(Value::UNDEFINED);
                let has = self.heap.root_value(*has_root).unwrap_or(Value::UNDEFINED);
                let result = self.call_value(p, has, receiver, &[value])?;
                Ok(self.truthy(result))
            }
        }
    }

    fn for_each_set_snapshot<F>(
        &mut self,
        p: &ResidualProgram,
        set: Value,
        mut visit: F,
    ) -> Result<(), JsError>
    where
        F: FnMut(&mut Self, Value) -> Result<bool, JsError>,
    {
        let values = match self.heap.get(set) {
            Some(Cell::Set { entries, .. }) => entries.clone(),
            _ => return Err(self.type_error(p, "Set method called on incompatible receiver".into())),
        };
        for value in values {
            if !visit(self, value)? {
                break;
            }
        }
        Ok(())
    }

    fn for_each_live_set<F>(
        &mut self,
        p: &ResidualProgram,
        set: Value,
        mut visit: F,
    ) -> Result<(), JsError>
    where
        F: FnMut(&mut Self, Value) -> Result<bool, JsError>,
    {
        let mut index = 0;
        loop {
            let Some(value) = self.heap.get(set).and_then(|cell| match cell {
                Cell::Set { entries, .. } => entries.get(index).copied(),
                _ => None,
            }) else {
                break;
            };
            if !visit(self, value)? {
                break;
            }
            let still_at_index = self
                .heap
                .get(set)
                .and_then(|cell| match cell {
                    Cell::Set { entries, .. } => entries.get(index),
                    _ => None,
                })
                .is_some_and(|current| self.same_value_zero(*current, value));
            if still_at_index {
                index += 1;
            }
        }
        let _ = p;
        Ok(())
    }

    fn fold_set_record<T, I, F>(
        &mut self,
        p: &ResidualProgram,
        other: &SetRecord,
        initialize: I,
        mut step: F,
    ) -> Result<T, JsError>
    where
        I: FnOnce(&mut Self) -> Result<T, JsError>,
        F: FnMut(&mut Self, T, Value) -> Result<SetRecordStep<T>, JsError>,
    {
        match other {
            SetRecord::Native { set, .. } => {
                let values = match self.heap.get(*set) {
                    Some(Cell::Set { entries, .. }) => entries.clone(),
                    _ => Vec::new(),
                };
                let mut acc = initialize(self)?;
                for value in values {
                    acc = match step(self, acc, value)? {
                        SetRecordStep::Continue(acc) => acc,
                        SetRecordStep::Stop(acc) => return Ok(acc),
                    };
                }
                Ok(acc)
            }
            SetRecord::Like {
                receiver_root,
                keys_root,
                ..
            } => {
                let receiver = self.heap.root_value(*receiver_root).unwrap_or(Value::UNDEFINED);
                let keys = self.heap.root_value(*keys_root).unwrap_or(Value::UNDEFINED);
                let iterator = match self.call_value(p, keys, receiver, &[]) {
                    Ok(iterator) if self.is_object_like(iterator) => iterator,
                    Ok(_) => return Err(self.type_error(p, "Set-like keys did not return an object".into())),
                    Err(error) => return Err(error),
                };
                let iterator_root = self.heap.root(iterator);
                let next_atom = self.intern_atom("next");
                let next_method = match self.get_property(p, iterator, next_atom) {
                    Ok(method) if self.is_function(method) => method,
                    Ok(_) => {
                        self.heap.release_root(iterator_root);
                        return Err(self.type_error(p, "iterator next method is not callable".into()));
                    }
                    Err(error) => {
                        self.heap.release_root(iterator_root);
                        return Err(error);
                    }
                };
                let next_root = self.heap.root(next_method);
                let done_atom = self.intern_atom("done");
                let value_atom = self.intern_atom("value");
                let mut acc = initialize(self)?;
                loop {
                    let iterator = self.heap.root_value(iterator_root).unwrap_or(iterator);
                    let next_method = self.heap.root_value(next_root).unwrap_or(next_method);
                    let result = match self.iterator_next_with_cached_method(
                        p,
                        iterator,
                        next_method,
                        &[],
                    ) {
                        Ok(result) => result,
                        Err(error) => {
                            let error = self.iterator_abrupt(p, iterator, error);
                            self.heap.release_root(iterator_root);
                            self.heap.release_root(next_root);
                            return Err(error);
                        }
                    };
                    let result_root = self.heap.root(result);
                    let done = match self.get_property(p, result, done_atom) {
                        Ok(done) => done,
                        Err(error) => {
                            self.heap.release_root(result_root);
                            let iterator = self.heap.root_value(iterator_root).unwrap_or(iterator);
                            let error = self.iterator_abrupt(p, iterator, error);
                            self.heap.release_root(iterator_root);
                            self.heap.release_root(next_root);
                            return Err(error);
                        }
                    };
                    if self.truthy(done) {
                        self.heap.release_root(result_root);
                        self.heap.release_root(iterator_root);
                        self.heap.release_root(next_root);
                        return Ok(acc);
                    }
                    let value = match self.get_property(p, result, value_atom) {
                        Ok(value) => value,
                        Err(error) => {
                            self.heap.release_root(result_root);
                            let iterator = self.heap.root_value(iterator_root).unwrap_or(iterator);
                            let error = self.iterator_abrupt(p, iterator, error);
                            self.heap.release_root(iterator_root);
                            self.heap.release_root(next_root);
                            return Err(error);
                        }
                    };
                    self.heap.release_root(result_root);
                    match step(self, acc, value) {
                        Ok(SetRecordStep::Continue(next)) => acc = next,
                        Ok(SetRecordStep::Stop(done)) => {
                            let iterator = self.heap.root_value(iterator_root).unwrap_or(iterator);
                            let close = self.iterator_close(p, iterator);
                            self.heap.release_root(iterator_root);
                            self.heap.release_root(next_root);
                            close?;
                            return Ok(done);
                        }
                        Err(error) => {
                            let iterator = self.heap.root_value(iterator_root).unwrap_or(iterator);
                            let error = self.iterator_abrupt(p, iterator, error);
                            self.heap.release_root(iterator_root);
                            self.heap.release_root(next_root);
                            return Err(error);
                        }
                    }
                }
            }
        }
    }

    fn push_set_result(
        &mut self,
        values: &mut Vec<Value>,
        roots: &mut Vec<RootId>,
        value: Value,
    ) {
        if values
            .iter()
            .any(|candidate| self.same_value_zero(*candidate, value))
        {
            return;
        }
        let value = if value.as_number().is_some_and(|number| number == 0.0) {
            Value::number(0.0)
        } else {
            value
        };
        roots.push(self.heap.root(value));
        values.push(value);
    }

    fn apply_set_relation(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        own: Value,
        other: &SetRecord,
        values: &mut Vec<Value>,
        roots: &mut Vec<RootId>,
    ) -> Result<SetRelationResult, JsError> {
        let own_size = match self.heap.get(own) {
            Some(Cell::Set { entries, .. }) => entries.len() as f64,
            _ => return Err(self.type_error(p, "Set method called on incompatible receiver".into())),
        };
        match native {
            Native::SetDifference if own_size <= other.size(self) => {
                self.for_each_set_snapshot(p, own, |vm, value| {
                    if !vm.set_record_has(p, other, value)? {
                        vm.push_set_result(values, roots, value);
                    }
                    Ok(true)
                })?;
            }
            Native::SetDifference => {
                let current = match self.heap.get(own) {
                    Some(Cell::Set { entries, .. }) => entries.clone(),
                    _ => Vec::new(),
                };
                for value in current {
                    self.push_set_result(values, roots, value);
                }
                self.fold_set_record(p, other, |_| Ok(()), |vm, (), value| {
                    values.retain(|candidate| !vm.same_value_zero(*candidate, value));
                    Ok(SetRecordStep::Continue(()))
                })?;
            }
            Native::SetIntersection if own_size <= other.size(self) => {
                self.for_each_live_set(p, own, |vm, value| {
                    if vm.set_record_has(p, other, value)? {
                        vm.push_set_result(values, roots, value);
                    }
                    Ok(true)
                })?;
            }
            Native::SetIntersection => {
                self.fold_set_record(p, other, |_| Ok(()), |vm, (), value| {
                    if vm.set_entry_index(own, value).is_some() {
                        vm.push_set_result(values, roots, value);
                    }
                    Ok(SetRecordStep::Continue(()))
                })?;
            }
            Native::SetSymmetricDifference | Native::SetUnion => {
                let (result, result_roots) = self.fold_set_record(
                    p,
                    other,
                    |vm| {
                        let mut result = Vec::new();
                        let mut result_roots = Vec::new();
                        let current = match vm.heap.get(own) {
                            Some(Cell::Set { entries, .. }) => entries.clone(),
                            _ => Vec::new(),
                        };
                        for value in current {
                            vm.push_set_result(&mut result, &mut result_roots, value);
                        }
                        Ok((result, result_roots))
                    },
                    |vm, (mut result, mut result_roots), value| {
                        let in_own = vm.set_entry_index(own, value).is_some();
                        if native == Native::SetSymmetricDifference && in_own {
                            result.retain(|candidate| !vm.same_value_zero(*candidate, value));
                        } else if !in_own {
                            vm.push_set_result(&mut result, &mut result_roots, value);
                        }
                        Ok(SetRecordStep::Continue((result, result_roots)))
                    },
                )?;
                values.extend(result);
                roots.extend(result_roots);
            }
            Native::SetIsDisjointFrom if own_size <= other.size(self) => {
                let mut disjoint = true;
                self.for_each_live_set(p, own, |vm, value| {
                    if vm.set_record_has(p, other, value)? {
                        disjoint = false;
                        return Ok(false);
                    }
                    Ok(true)
                })?;
                return Ok(SetRelationResult::Boolean(disjoint));
            }
            Native::SetIsDisjointFrom => {
                let found = self.fold_set_record(p, other, |_| Ok(false), |vm, _, value| {
                    Ok(if vm.set_entry_index(own, value).is_some() {
                        SetRecordStep::Stop(true)
                    } else {
                        SetRecordStep::Continue(false)
                    })
                })?;
                return Ok(SetRelationResult::Boolean(!found));
            }
            Native::SetIsSubsetOf => {
                if own_size > other.size(self) {
                    return Ok(SetRelationResult::Boolean(false));
                }
                let mut is_subset = true;
                self.for_each_live_set(p, own, |vm, value| {
                    if !vm.set_record_has(p, other, value)? {
                        is_subset = false;
                        return Ok(false);
                    }
                    Ok(true)
                })?;
                return Ok(SetRelationResult::Boolean(is_subset));
            }
            Native::SetIsSupersetOf => {
                if own_size < other.size(self) {
                    return Ok(SetRelationResult::Boolean(false));
                }
                let missing = self.fold_set_record(p, other, |_| Ok(false), |vm, _, value| {
                    Ok(if vm.set_entry_index(own, value).is_some() {
                        SetRecordStep::Continue(false)
                    } else {
                        SetRecordStep::Stop(true)
                    })
                })?;
                return Ok(SetRelationResult::Boolean(!missing));
            }
            _ => return Err(JsError("invalid Set relation native".into())),
        }
        Ok(SetRelationResult::Set)
    }
}
