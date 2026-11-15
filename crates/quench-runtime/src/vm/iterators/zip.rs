//! `Iterator.zip` and `Iterator.zipKeyed`: the inputs they open, their padding, and the zipping helper.
use super::*;

impl<H: Host> Vm<H> {
    pub(super) fn iterator_zip(
        &mut self,
        p: &ResidualProgram,
        keyed: bool,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let input = args.first().copied().unwrap_or(Value::UNDEFINED);
        if !self.is_object_like(input) {
            return Err(self.type_error(p, "Iterator.zip requires an object".into()));
        }
        let options = args.get(1).copied().unwrap_or(Value::UNDEFINED);
        let (mode, padding_object) = self.iterator_zip_options(p, options)?;
        let (iterators, keys) = if keyed {
            self.iterator_zip_keyed_inputs(p, input)?
        } else {
            (self.iterator_zip_inputs(p, input)?, None)
        };
        let iterator_roots = iterators
            .iter()
            .copied()
            .map(|value| self.heap.root(value))
            .collect::<Vec<_>>();
        let key_roots = keys
            .iter()
            .flatten()
            .copied()
            .map(|value| self.heap.root(value))
            .collect::<Vec<_>>();
        let mut padding_roots = Vec::new();
        let result = (|| {
            let padding = match self.iterator_zip_padding(
                p,
                padding_object,
                keys.as_deref(),
                iterators.len(),
            ) {
                Ok(padding) => padding,
                Err(error) => {
                    let _ = self.iterator_close_all(p, &iterators, None);
                    return Err(error);
                }
            };
            let iterators = iterator_roots
                .iter()
                .filter_map(|root| self.heap.root_value(*root))
                .collect::<Vec<_>>();
            let keys = keys.as_ref().map(|_| {
                key_roots
                    .iter()
                    .filter_map(|root| self.heap.root_value(*root))
                    .collect::<Vec<_>>()
            });
            padding_roots.extend(padding.iter().copied().map(|value| self.heap.root(value)));
            let opened = vec![true; iterators.len()];
            self.iterator_helper(
                p,
                Value::UNDEFINED,
                IteratorKind::Zip,
                IteratorHelper::Zip {
                    iterators,
                    padding,
                    mode,
                    keys,
                    opened,
                    done: false,
                },
            )
        })();
        for root in iterator_roots.into_iter().chain(key_roots).chain(padding_roots) {
            self.heap.release_root(root);
        }
        result
    }

    pub(super) fn iterator_zip_options(
        &mut self,
        p: &ResidualProgram,
        options: Value,
    ) -> Result<(IteratorZipMode, Value), JsError> {
        if options.is_undefined() {
            return Ok((IteratorZipMode::Shortest, Value::UNDEFINED));
        }
        if !self.is_object_like(options) {
            return Err(self.type_error(p, "Iterator.zip options must be an object".into()));
        }
        let mode_atom = self.intern_atom("mode");
        let mode_value = self.get_property(p, options, mode_atom)?;
        let mode = match self.heap.get(mode_value) {
            _ if let Some(value) = self.heap.string(mode_value) && value.host_string() == "shortest" => {
                IteratorZipMode::Shortest
            }
            _ if let Some(value) = self.heap.string(mode_value) && value.host_string() == "longest" => {
                IteratorZipMode::Longest
            }
            _ if let Some(value) = self.heap.string(mode_value) && value.host_string() == "strict" => IteratorZipMode::Strict,
            _ if mode_value.is_undefined() => IteratorZipMode::Shortest,
            _ => return Err(self.type_error(p, "Iterator.zip mode is invalid".into())),
        };
        let padding = if matches!(mode, IteratorZipMode::Longest) {
            let padding_atom = self.intern_atom("padding");
            self.get_property(p, options, padding_atom)?
        } else {
            Value::UNDEFINED
        };
        if !padding.is_undefined() && !self.is_object_like(padding) {
            return Err(self.type_error(p, "Iterator.zip padding must be an object".into()));
        }
        Ok((mode, padding))
    }

    pub(super) fn iterator_zip_inputs(
        &mut self,
        p: &ResidualProgram,
        input: Value,
    ) -> Result<Vec<Value>, JsError> {
        let outer = self
            .get_iterator(p, input)
            .and_then(|iterator| self.iterator_get_direct(p, iterator))?;
        let outer_root = self.heap.root(outer);
        let mut roots = Vec::new();
        let mut iterators = Vec::new();
        loop {
            let outer = self.heap.root_value(outer_root).unwrap_or(outer);
            let next = self.iterator_helper_step(p, outer, &[]);
            let (value, value_root) = match next {
                Ok(Some(value)) => (value, self.heap.root(value)),
                Ok(None) => {
                    let iterators = roots
                        .iter()
                        .filter_map(|root| self.heap.root_value(*root))
                        .collect::<Vec<_>>();
                    roots.into_iter().for_each(|root| {
                        self.heap.release_root(root);
                    });
                    self.heap.release_root(outer_root);
                    return Ok(iterators);
                }
                Err(error) => {
                    let iterators = roots
                        .iter()
                        .filter_map(|root| self.heap.root_value(*root))
                        .collect::<Vec<_>>();
                    let _ = self.iterator_close_all(p, &iterators, None);
                    roots.into_iter().for_each(|root| {
                        self.heap.release_root(root);
                    });
                    self.heap.release_root(outer_root);
                    return Err(error);
                }
            };
            match self
                .iterator_flattenable_source(p, value, IteratorPrimitiveHandling::Reject)
                .and_then(|iterator| self.iterator_get_direct(p, iterator))
            {
                Ok(iterator) => {
                    roots.push(self.heap.root(iterator));
                    iterators.push(iterator);
                    self.heap.release_root(value_root);
                }
                Err(error) => {
                    self.heap.release_root(value_root);
                    let iterators = roots
                        .iter()
                        .filter_map(|root| self.heap.root_value(*root))
                        .collect::<Vec<_>>();
                    let _ = self.iterator_close_all(p, &iterators, Some(outer));
                    roots.into_iter().for_each(|root| {
                        self.heap.release_root(root);
                    });
                    self.heap.release_root(outer_root);
                    return Err(error);
                }
            }
            iterators.clear();
            iterators.extend(roots.iter().filter_map(|root| self.heap.root_value(*root)));
        }
    }

    pub(super) fn iterator_zip_keyed_inputs(
        &mut self,
        p: &ResidualProgram,
        input: Value,
    ) -> Result<(Vec<Value>, Option<Vec<Value>>), JsError> {
        let keys_value = self.object_own_keys(p, input)?;
        let keys_root = self.heap.root(keys_value);
        let keys = self
            .heap
            .array_elements(keys_value)
            .map_or_else(Vec::new, |elements| elements.to_vec());
        let mut iterators = Vec::new();
        let mut output_keys = Vec::new();
        let mut roots = Vec::new();
        let result = (|| {
            for key in keys {
                let property_key = self.to_property_key(p, key)?;
                let property_key_root = self.heap.root(property_key);
                let atom = if matches!(self.heap.get(property_key), Some(Cell::Symbol(_))) {
                    None
                } else {
                    let name = self.coerce_js_string(p, property_key)?;
                    Some(self.intern_atom(name.host_string()))
                };
                let enumerable = if self.heap.array(input).is_some()
                    || matches!(
                        self.heap.get(input),
                        Some(Cell::Proxy { .. } | Cell::TypedArray { .. })
                    ) {
                    let descriptor =
                        self.object_get_own_property_descriptor(p, &[input, property_key])?;
                    !descriptor.is_undefined() && self.descriptor_flag(descriptor, self.runtime_atoms.enumerable)
                } else {
                    let key =
                        atom.map_or_else(|| PropertyKey::symbol(property_key), PropertyKey::string);
                    self.property_attributes(input, key)
                        .unwrap_or(DEFAULT_PROPERTY_ATTRIBUTES)
                        .enumerable
                };
                if !enumerable {
                    self.heap.release_root(property_key_root);
                    continue;
                }
                let value = match atom {
                    Some(atom) => self.get_property(p, input, atom),
                    None => self.get_index(p, input, property_key),
                };
                let value = match value {
                    Ok(value) => value,
                    Err(error) => {
                        self.heap.release_root(property_key_root);
                        return Err(error);
                    }
                };
                if value.is_undefined() {
                    self.heap.release_root(property_key_root);
                    continue;
                }
                let value_root = self.heap.root(value);
                match self
                    .iterator_flattenable_source(p, value, IteratorPrimitiveHandling::Reject)
                    .and_then(|iterator| self.iterator_get_direct(p, iterator))
                {
                    Ok(iterator) => {
                        iterators.push(iterator);
                        roots.push(self.heap.root(iterator));
                        output_keys.push(
                            self.heap
                                .root_value(property_key_root)
                                .unwrap_or(property_key),
                        );
                        roots.push(property_key_root);
                        self.heap.release_root(value_root);
                    }
                    Err(error) => {
                        self.heap.release_root(value_root);
                        self.heap.release_root(property_key_root);
                        return Err(error);
                    }
                }
            }
            let iterators = roots
                .iter()
                .filter_map(|root| self.heap.root_value(*root))
                .step_by(2)
                .collect::<Vec<_>>();
            let output_keys = roots
                .iter()
                .filter_map(|root| self.heap.root_value(*root))
                .skip(1)
                .step_by(2)
                .collect::<Vec<_>>();
            Ok((iterators, Some(output_keys)))
        })();
        let result = match result {
            Ok(result) => Ok(result),
            Err(error) => {
                let iterators = roots
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .filter_map(|[iterator, _]| self.heap.root_value(*iterator))
                    .collect::<Vec<_>>();
                let _ = self.iterator_close_all(p, &iterators, None);
                Err(error)
            }
        };
        roots.into_iter().for_each(|root| {
            self.heap.release_root(root);
        });
        self.heap.release_root(keys_root);
        result
    }

    pub(super) fn iterator_zip_padding(
        &mut self,
        p: &ResidualProgram,
        padding: Value,
        keys: Option<&[Value]>,
        count: usize,
    ) -> Result<Vec<Value>, JsError> {
        if padding.is_undefined() {
            return Ok(vec![Value::UNDEFINED; count]);
        }
        if let Some(keys) = keys {
            let mut values = Vec::with_capacity(keys.len());
            for key in keys.iter().copied() {
                let value = match self.heap.get(key) {
                    _ if let Some(name) = self.heap.string(key)  => {
                        let name = name.clone();
                        let atom = self.intern_js_atom(&name);
                        self.get_property(p, padding, atom)?
                    }
                    Some(Cell::Symbol(_)) => self.get_index(p, padding, key)?,
                    _ => return Err(JsError::validation("invalid zipKeyed property key".into())),
                };
                values.push(value);
            }
            return Ok(values);
        }
        let iterator = self
            .get_iterator(p, padding)
            .and_then(|iterator| self.iterator_get_direct(p, iterator))?;
        let iterator_root = self.heap.root(iterator);
        let mut values = Vec::with_capacity(count);
        let mut value_roots = Vec::with_capacity(count);
        while values.len() < count {
            let iterator = self.heap.root_value(iterator_root).unwrap_or(iterator);
            match self.iterator_helper_step(p, iterator, &[]) {
                Ok(Some(value)) => {
                    values.push(value);
                    value_roots.push(self.heap.root(value));
                }
                Ok(None) => {
                    values = value_roots
                        .iter()
                        .filter_map(|root| self.heap.root_value(*root))
                        .collect();
                    values.resize(count, Value::UNDEFINED);
                    value_roots.into_iter().for_each(|root| {
                        self.heap.release_root(root);
                    });
                    self.heap.release_root(iterator_root);
                    return Ok(values);
                }
                Err(error) => {
                    value_roots.into_iter().for_each(|root| {
                        self.heap.release_root(root);
                    });
                    self.heap.release_root(iterator_root);
                    return Err(error);
                }
            };
        }
        let iterator = self.heap.root_value(iterator_root).unwrap_or(iterator);
        self.iterator_close(p, iterator)?;
        values = value_roots
            .iter()
            .filter_map(|root| self.heap.root_value(*root))
            .collect();
        value_roots.into_iter().for_each(|root| {
            self.heap.release_root(root);
        });
        self.heap.release_root(iterator_root);
        Ok(values)
    }

}
