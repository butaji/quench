//! The zipping of `Iterator.zip` and `Iterator.zipKeyed` as one iterator helper, and the closing of the inputs it has opened.
use super::*;

impl<H: Host> Vm<H> {
    pub(super) fn iterator_close_all(
        &mut self,
        p: &ResidualProgram,
        iterators: &[Value],
        final_: Option<Value>,
    ) -> Result<(), JsError> {
        let mut abrupt = None;
        for iterator in iterators.iter().rev().copied().chain(final_) {
            if let Err(error) = self.iterator_close(p, iterator)
                && abrupt.is_none()
            {
                abrupt = Some(error);
            }
        }
        abrupt.map_or(Ok(()), Err)
    }

    pub(super) fn iterator_helper_zip(
        &mut self,
        p: &ResidualProgram,
        helper: Value,
        iterators: Vec<Value>,
        padding: Vec<Value>,
        mode: IteratorZipMode,
        keys: Option<Vec<Value>>,
        mut opened: Vec<bool>,
        done: bool,
    ) -> Result<Value, JsError> {
        let helper_root = self.heap.root(helper);
        let iterator_roots = iterators
            .iter()
            .copied()
            .map(|value| self.heap.root(value))
            .collect::<Vec<_>>();
        let padding_roots = padding
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
        let result = (|| {
            if done {
                return self.iterator_result(Value::UNDEFINED, true);
            }
            if iterators.is_empty() {
                self.mark_iterator_done(self.heap.root_value(helper_root).unwrap_or(helper));
                return self.iterator_result(Value::UNDEFINED, true);
            }
            let mut values = Vec::with_capacity(iterators.len());
            let mut value_roots = Vec::new();
            let mut ended_count = 0;
            for (index, iterator_root) in iterator_roots.iter().copied().enumerate() {
                let padding_value = padding_roots
                    .get(index)
                    .and_then(|root| self.heap.root_value(*root))
                    .unwrap_or(Value::UNDEFINED);
                if !opened.get(index).copied().unwrap_or(false) {
                    values.push(padding_value);
                    value_roots.push(self.heap.root(padding_value));
                    continue;
                }
                let iterator = self
                    .heap
                    .root_value(iterator_root)
                    .unwrap_or(Value::UNDEFINED);
                match self.iterator_helper_step(p, iterator, &[]) {
                    Ok(Some(value)) => {
                        if matches!(mode, IteratorZipMode::Strict) && ended_count > 0 {
                            let helper = self.heap.root_value(helper_root).unwrap_or(helper);
                            self.mark_iterator_done(helper);
                            let active = iterator_roots
                                .iter()
                                .copied()
                                .zip(opened.iter().copied())
                                .filter_map(|(root, is_open)| {
                                    is_open.then(|| self.heap.root_value(root)).flatten()
                                })
                                .collect::<Vec<_>>();
                            let _ = self.iterator_close_all(p, &active, None);
                            value_roots.into_iter().for_each(|root| {
                                self.heap.release_root(root);
                            });
                            return Err(self.type_error(
                                p,
                                "Iterator.zip iterators have different lengths".into(),
                            ));
                        }
                        values.push(value);
                        value_roots.push(self.heap.root(value));
                    }
                    Ok(None) => {
                        let padding_value = padding_roots
                            .get(index)
                            .and_then(|root| self.heap.root_value(*root))
                            .unwrap_or(Value::UNDEFINED);
                        values.push(padding_value);
                        value_roots.push(self.heap.root(padding_value));
                        opened[index] = false;
                        ended_count += 1;
                        if matches!(mode, IteratorZipMode::Shortest) {
                            break;
                        }
                        if matches!(mode, IteratorZipMode::Strict) && index > 0 && ended_count == 1
                        {
                            let helper = self.heap.root_value(helper_root).unwrap_or(helper);
                            self.mark_iterator_done(helper);
                            let active = iterator_roots
                                .iter()
                                .copied()
                                .zip(opened.iter().copied())
                                .filter_map(|(root, is_open)| {
                                    is_open.then(|| self.heap.root_value(root)).flatten()
                                })
                                .collect::<Vec<_>>();
                            let _ = self.iterator_close_all(p, &active, None);
                            value_roots.into_iter().for_each(|root| {
                                self.heap.release_root(root);
                            });
                            return Err(self.type_error(
                                p,
                                "Iterator.zip iterators have different lengths".into(),
                            ));
                        }
                    }
                    Err(error) => {
                        opened[index] = false;
                        let helper = self.heap.root_value(helper_root).unwrap_or(helper);
                        self.mark_iterator_done(helper);
                        let remaining = iterator_roots
                            .iter()
                            .copied()
                            .zip(opened.iter().copied())
                            .filter_map(|(root, is_open)| {
                                is_open.then(|| self.heap.root_value(root)).flatten()
                            })
                            .collect::<Vec<_>>();
                        let _ = self.iterator_close_all(p, &remaining, None);
                        value_roots.into_iter().for_each(|root| {
                            self.heap.release_root(root);
                        });
                        return Err(error);
                    }
                }
            }
            if ended_count == iterators.len() {
                let helper = self.heap.root_value(helper_root).unwrap_or(helper);
                self.mark_iterator_done(helper);
                value_roots.into_iter().for_each(|root| {
                    self.heap.release_root(root);
                });
                return self.iterator_result(Value::UNDEFINED, true);
            }
            if matches!(mode, IteratorZipMode::Strict) && ended_count > 0 {
                let helper = self.heap.root_value(helper_root).unwrap_or(helper);
                self.mark_iterator_done(helper);
                let active = iterator_roots
                    .iter()
                    .copied()
                    .zip(opened.iter().copied())
                    .filter_map(|(root, is_open)| {
                        is_open.then(|| self.heap.root_value(root)).flatten()
                    })
                    .collect::<Vec<_>>();
                let _ = self.iterator_close_all(p, &active, None);
                value_roots.into_iter().for_each(|root| {
                    self.heap.release_root(root);
                });
                return Err(
                    self.type_error(p, "Iterator.zip iterators have different lengths".into())
                );
            }
            let iterators = iterator_roots
                .iter()
                .filter_map(|root| self.heap.root_value(*root))
                .collect::<Vec<_>>();
            let padding = padding_roots
                .iter()
                .filter_map(|root| self.heap.root_value(*root))
                .collect::<Vec<_>>();
            let keys = keys.as_ref().map(|_| {
                key_roots
                    .iter()
                    .filter_map(|root| self.heap.root_value(*root))
                    .collect::<Vec<_>>()
            });
            let helper = self.heap.root_value(helper_root).unwrap_or(helper);
            if opened.iter().all(|is_open| !is_open) {
                self.mark_iterator_done(helper);
                self.update_iterator_helper(
                    helper,
                    IteratorHelper::Zip {
                        iterators,
                        padding,
                        mode,
                        keys,
                        opened,
                        done: true,
                    },
                );
                value_roots.into_iter().for_each(|root| {
                    self.heap.release_root(root);
                });
                return self.iterator_result(Value::UNDEFINED, true);
            }
            if ended_count > 0
                && matches!(mode, IteratorZipMode::Shortest | IteratorZipMode::Strict)
            {
                self.mark_iterator_done(helper);
                let active = iterator_roots
                    .iter()
                    .copied()
                    .zip(opened.iter().copied())
                    .filter_map(|(root, is_open)| {
                        is_open.then(|| self.heap.root_value(root)).flatten()
                    })
                    .collect::<Vec<_>>();
                let close_result = self.iterator_close_all(p, &active, None);
                value_roots.into_iter().for_each(|root| {
                    self.heap.release_root(root);
                });
                close_result?;
                if matches!(mode, IteratorZipMode::Strict) {
                    return Err(
                        self.type_error(p, "Iterator.zip iterators have different lengths".into())
                    );
                }
                return self.iterator_result(Value::UNDEFINED, true);
            }
            let values = value_roots
                .iter()
                .filter_map(|root| self.heap.root_value(*root))
                .collect::<Vec<_>>();
            self.update_iterator_helper(
                helper,
                IteratorHelper::Zip {
                    iterators,
                    padding,
                    mode,
                    keys: keys.clone(),
                    opened,
                    done: false,
                },
            );
            let result = if let Some(keys) = keys {
                let object = self
                    .heap
                    .alloc(Cell::Object(Self::empty_object(Value::NULL)));
                for (key, value) in keys.into_iter().zip(values) {
                    self.set_index(p, object, key, value)?;
                }
                object
            } else {
                self.new_array(values)
            };
            value_roots.into_iter().for_each(|root| {
                self.heap.release_root(root);
            });
            self.iterator_result(result, false)
        })();
        iterator_roots.into_iter().for_each(|root| {
            self.heap.release_root(root);
        });
        padding_roots.into_iter().for_each(|root| {
            self.heap.release_root(root);
        });
        key_roots.into_iter().for_each(|root| {
            self.heap.release_root(root);
        });
        self.heap.release_root(helper_root);
        result
    }
}
