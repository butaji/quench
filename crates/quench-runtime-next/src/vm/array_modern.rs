use super::*;
use crate::heap::{ArrayFromAsyncAwait, ArrayFromAsyncState};

#[derive(Clone, Copy, PartialEq, Eq)]
enum ArrayFromTarget {
    Array,
    TypedArray,
}

impl<H: Host> Vm<H> {
    pub(super) fn array_modern_native(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        match native {
            Native::ArrayToReversed | Native::ArrayToSpliced => {
                self.array_copy_native(p, native, this, args)
            }
            Native::ArraySort => self.array_sort_native(p, this, args, true),
            Native::ArrayToSorted => self.array_sort_native(p, this, args, false),
            Native::ArraySpecies => Ok(this),
            Native::ArrayToString => self.array_to_string_native(p, this),
            Native::ArrayToLocaleString => self.array_to_locale_string_native(p, this, args),
            Native::ArrayFrom => self.array_from_native(p, this, args, ArrayFromTarget::Array),
            Native::TypedArrayFrom => {
                self.array_from_native(p, this, args, ArrayFromTarget::TypedArray)
            }
            Native::ArrayFromAsync => self.array_from_async_native(p, this, args),
            Native::ArrayOf => self.array_of_native(p, this, args),
            Native::TypedArrayOf => self.typed_array_of_native(p, this, args),
            _ => unreachable!("non-modern native routed to modern array dispatch"),
        }
    }

    pub(super) fn array_values(&self, this: Value) -> Result<Vec<Value>, JsError> {
        let Some(Cell::Array { elements, .. }) = self.heap.get(this) else {
            return Err(JsError("modern array method receiver is not array".into()));
        };
        let length = self.heap.sparse_length(this).unwrap_or(elements.len());
        Ok((0..length)
            .map(|index| self.array_value_at(this, index))
            .collect())
    }

    pub(super) fn new_array(&mut self, values: Vec<Value>) -> Value {
        self.new_array_with_prototype(values, self.array_proto)
    }

    pub(super) fn new_array_with_prototype(
        &mut self,
        values: Vec<Value>,
        prototype: Value,
    ) -> Value {
        self.heap.alloc(Cell::Array {
            object: Self::empty_object(prototype),
            elements: Rc::new(values),
        })
    }

    fn array_to_string_native(
        &mut self,
        p: &ResidualProgram,
        this: Value,
    ) -> Result<Value, JsError> {
        self.require_object_coercible(p, this)?;
        let join = self.intern_atom("join");
        let method = self.get_property(p, this, join)?;
        if self.is_function(method) {
            self.call_value(p, method, this, &[])
        } else {
            self.object_prototype_to_string(p, this)
        }
    }

    pub(super) fn array_to_locale_string_native(
        &mut self,
        p: &ResidualProgram,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let object = self.box_object_or_type_error(p, this)?;
        let length = self.array_like_length(p, object)?;
        self.array_to_locale_string_with_length(p, object, length, args)
    }

    pub(super) fn array_to_locale_string_with_length(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        length: usize,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let to_locale_string = self.intern_atom("toLocaleString");
        let invoke_arguments = [
            args.first().copied().unwrap_or(Value::UNDEFINED),
            args.get(1).copied().unwrap_or(Value::UNDEFINED),
        ];
        let mut result = String::new();
        for index in 0..length {
            if index != 0 {
                result.push(',');
            }
            let value = self.get_index(p, object, Value::number(index as f64))?;
            if value.is_null() || value.is_undefined() {
                continue;
            }
            let method = self.get_property(p, value, to_locale_string)?;
            if !self.is_function(method) {
                return Err(self.type_error(p, "toLocaleString is not callable".into()));
            }
            let element = self.call_value(p, method, value, &invoke_arguments)?;
            result.push_str(&self.to_string(p, element)?);
        }
        Ok(self.heap.alloc(Cell::String(result.into())))
    }

    fn array_of_native(
        &mut self,
        p: &ResidualProgram,
        constructor: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let target = if self.is_constructable(p, constructor) {
            self.construct_value(p, constructor, &[Value::number(args.len() as f64)])?
        } else {
            self.array_create(p, args.len())?
        };
        let root = self.heap.root(target);
        let outcome = (|| {
            for (index, value) in args.iter().copied().enumerate() {
                let target = self.heap.root_value(root).unwrap();
                self.create_data_property_or_throw(p, target, index, value)?;
            }
            let target = self.heap.root_value(root).unwrap();
            self.set_array_like_length(p, target, args.len())?;
            Ok(target)
        })();
        self.heap.release_root(root);
        outcome
    }

    fn typed_array_of_native(
        &mut self,
        p: &ResidualProgram,
        constructor: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        if !self.is_constructable(p, constructor) {
            return Err(self.type_error(p, "typed array of receiver is not a constructor".into()));
        }
        let target = self.array_from_target(
            p,
            constructor,
            Some(args.len()),
            ArrayFromTarget::TypedArray,
        )?;
        let target_root = self.heap.root(target);
        let outcome = (|| {
            for (index, value) in args.iter().copied().enumerate() {
                self.typed_array_set(p, target, index, value)?;
            }
            Ok(self.heap.root_value(target_root).unwrap())
        })();
        self.heap.release_root(target_root);
        outcome
    }

    pub(super) fn array_create(&mut self, p: &ResidualProgram, length: usize) -> Result<Value, JsError> {
        if length > MAX_ARRAY_LENGTH {
            return Err(self.range_error(p, "invalid array length".into()));
        }
        let array_atom = self.intern_atom("Array");
        let array_constructor = self.get_property(p, self.realm.globals, array_atom)?;
        let prototype_atom = self.intern_atom("prototype");
        let prototype = self.get_property(p, array_constructor, prototype_atom)?;
        let prototype = if self.object_data(prototype).is_some() {
            prototype
        } else {
            self.array_proto
        };
        let array = self.heap.alloc(Cell::Array {
            object: Self::empty_object(prototype),
            elements: Rc::new(Vec::new()),
        });
        self.heap.sparse_set_length(array, length);
        Ok(array)
    }

    fn array_from_target(
        &mut self,
        p: &ResidualProgram,
        constructor: Value,
        length: Option<usize>,
        target: ArrayFromTarget,
    ) -> Result<Value, JsError> {
        if self.is_constructable(p, constructor) {
            let args = length
                .map(|length| vec![Value::number(length as f64)])
                .unwrap_or_default();
            let result = self.construct_value(p, constructor, &args)?;
            if target == ArrayFromTarget::TypedArray && self.typed_array_kind(result).is_none() {
                return Err(
                    self.type_error(p, "typed array constructor returned invalid result".into())
                );
            }
            if target == ArrayFromTarget::TypedArray {
                self.validate_typed_array_result(
                    p,
                    result,
                    length.unwrap_or_default(),
                    true,
                )?;
            }
            Ok(result)
        } else if target == ArrayFromTarget::TypedArray {
            Err(self.type_error(p, "typed array from receiver is not a constructor".into()))
        } else {
            self.array_create(p, length.unwrap_or(0))
        }
    }

    fn array_from_native(
        &mut self,
        p: &ResidualProgram,
        constructor: Value,
        args: &[Value],
        target_kind: ArrayFromTarget,
    ) -> Result<Value, JsError> {
        if target_kind == ArrayFromTarget::TypedArray && !self.is_constructable(p, constructor) {
            return Err(self.type_error(p, "typed array from receiver is not a constructor".into()));
        }
        let source = args.first().copied().unwrap_or(Value::UNDEFINED);
        let mapfn = args.get(1).copied().filter(|value| !value.is_undefined());
        if mapfn.is_some_and(|mapfn| !self.is_function(mapfn)) {
            return Err(self.type_error(p, "Array.from map function is not callable".into()));
        }
        let map_this = args.get(2).copied().unwrap_or(Value::UNDEFINED);
        let iterator_symbol = self.well_known_symbols.get("iterator").copied();
        let iterator_method = iterator_symbol
            .map(|symbol| self.get_index(p, source, symbol))
            .transpose()?
            .unwrap_or(Value::UNDEFINED);
        if !iterator_method.is_undefined() && !iterator_method.is_null() {
            if !self.is_function(iterator_method) {
                return Err(self.type_error(p, "iterator method is not callable".into()));
            }
            if target_kind == ArrayFromTarget::TypedArray {
                return self.typed_array_from_iterable(
                    p,
                    constructor,
                    source,
                    iterator_method,
                    mapfn,
                    map_this,
                );
            }
            let target = self.array_from_target(p, constructor, None, target_kind)?;
            let root = self.heap.root(target);
            let outcome = (|| {
                let iterator = self.call_value(p, iterator_method, source, &[])?;
                if !self.is_object_like(iterator) {
                    return Err(
                        self.type_error(p, "iterator method did not return an object".into())
                    );
                }
                let iterator_root = self.heap.root(iterator);
                let outcome = (|| {
                    let mut index = 0;
                    loop {
                        let iterator = self.heap.root_value(iterator_root).unwrap_or(iterator);
                        let Some(mut value) = self.iterator_step_value(p, iterator)? else {
                            break;
                        };
                        if let Some(mapfn) = mapfn {
                            let key = Value::number(index as f64);
                            value = match self.call_value(p, mapfn, map_this, &[value, key]) {
                                Ok(value) => value,
                                Err(error) => {
                                    return Err(self.iterator_abrupt(p, iterator, error));
                                }
                            };
                        }
                        let target = self.heap.root_value(root).unwrap();
                        if let Err(error) =
                            self.create_data_property_or_throw(p, target, index, value)
                        {
                            return Err(self.iterator_abrupt(p, iterator, error));
                        }
                        index += 1;
                    }
                    let target = self.heap.root_value(root).unwrap();
                    self.set_array_like_length(p, target, index)
                        .map(|()| target)
                })();
                self.heap.release_root(iterator_root);
                outcome
            })();
            self.heap.release_root(root);
            return outcome;
        }

        let length_atom = self.intern_atom("length");
        let length_value = self.get_property(p, source, length_atom)?;
        let length = self.to_number(p, length_value)?;
        let length = if !length.is_finite() || length <= 0.0 {
            if length.is_infinite() && length.is_sign_positive() {
                return Err(JsError("Array.from length is too large".into()));
            }
            0
        } else {
            length.floor().min(usize::MAX as f64) as usize
        };
        let target = self.array_from_target(p, constructor, Some(length), target_kind)?;
        let root = self.heap.root(target);
        let outcome = (|| {
            for index in 0..length {
                let mut value = self.get_index(p, source, Value::number(index as f64))?;
                if let Some(mapfn) = mapfn {
                    value =
                        self.call_value(p, mapfn, map_this, &[value, Value::number(index as f64)])?;
                }
                let target = self.heap.root_value(root).unwrap();
                if target_kind == ArrayFromTarget::TypedArray {
                    self.typed_array_set(p, target, index, value)?;
                } else {
                    self.create_data_property_or_throw(p, target, index, value)?;
                }
            }
            Ok(self.heap.root_value(root).unwrap())
        })();
        self.heap.release_root(root);
        outcome
    }

    fn typed_array_from_iterable(
        &mut self,
        p: &ResidualProgram,
        constructor: Value,
        source: Value,
        iterator_method: Value,
        mapfn: Option<Value>,
        map_this: Value,
    ) -> Result<Value, JsError> {
        let done_atom = self.intern_atom("done");
        let value_atom = self.intern_atom("value");
        let iterator = self.call_value(p, iterator_method, source, &[])?;
        if !self.is_object_like(iterator) {
            return Err(self.type_error(p, "iterator method did not return an object".into()));
        }
        let iterator_root = self.heap.root(iterator);
        let mut values = Vec::new();
        let outcome = (|| {
            let next_atom = self.intern_atom("next");
            let next_method = self.get_property(p, iterator, next_atom)?;
            if !self.is_function(next_method) {
                return Err(self.type_error(p, "iterator next is not callable".into()));
            }
            loop {
                let iterator = self.heap.root_value(iterator_root).unwrap_or(iterator);
                let step = match self.call_value(p, next_method, iterator, &[]) {
                    Ok(step) if self.is_object_like(step) => step,
                    Ok(_) => {
                        let error = self.type_error(p, "iterator next result is not an object".into());
                        return Err(self.iterator_abrupt(p, iterator, error));
                    }
                    Err(error) => return Err(self.iterator_abrupt(p, iterator, error)),
                };
                let done = match self.get_property(p, step, done_atom) {
                    Ok(done) => done,
                    Err(error) => return Err(self.iterator_abrupt(p, iterator, error)),
                };
                if self.truthy(done) {
                    break;
                }
                let value = match self.get_property(p, step, value_atom) {
                    Ok(value) => value,
                    Err(error) => return Err(self.iterator_abrupt(p, iterator, error)),
                };
                values.push(self.heap.root(value));
            }
            let target = self.array_from_target(
                p,
                constructor,
                Some(values.len()),
                ArrayFromTarget::TypedArray,
            )?;
            let target_root = self.heap.root(target);
            let result = (|| {
                for (index, value) in values.iter().enumerate() {
                    let value = self.heap.root_value(*value).unwrap();
                    let value = if let Some(mapfn) = mapfn {
                        self.call_value(
                            p,
                            mapfn,
                            map_this,
                            &[value, Value::number(index as f64)],
                        )?
                    } else {
                        value
                    };
                    self.typed_array_set(p, target, index, value)?;
                }
                Ok(self.heap.root_value(target_root).unwrap())
            })();
            self.heap.release_root(target_root);
            result
        })();
        self.heap.release_root(iterator_root);
        for value in values {
            self.heap.release_root(value);
        }
        outcome
    }

    fn array_from_async_native(
        &mut self,
        p: &ResidualProgram,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let promise = self.promise_object();
        if let Err(error) = self.array_from_async_start(p, promise, this, args) {
            let reason = self.thrown_value_for(p, error);
            self.promise_settle(p, promise, super::promise::PromiseState::Rejected, reason)?;
        }
        Ok(promise)
    }

    fn array_from_async_start(
        &mut self,
        p: &ResidualProgram,
        output: Value,
        constructor: Value,
        args: &[Value],
    ) -> Result<(), JsError> {
        let source = args.first().copied().unwrap_or(Value::UNDEFINED);
        self.require_object_coercible(p, source)?;
        let mapper = args.get(1).copied().filter(|value| !value.is_undefined());
        if mapper.is_some_and(|mapper| !self.is_function(mapper)) {
            return Err(self.type_error(p, "Array.fromAsync mapper is not callable".into()));
        }
        let iterator = match self.get_async_iterator(p, source) {
            Ok(iterator) => Some(iterator),
            Err(error) if error.to_string() == "value is not iterable" => None,
            Err(error) => return Err(error),
        };
        let this_arg = args.get(2).copied().unwrap_or(Value::UNDEFINED);
        let constructor = if self.is_constructable(p, constructor) {
            constructor
        } else {
            self.native_value(Native::Array)
        };
        if let Some(iterator) = iterator {
            let iterator_root = self.heap.root(iterator);
            let result = self.construct_value(p, constructor, &[]);
            let iterator = self
                .heap
                .root_value(iterator_root)
                .expect("rooted Array.fromAsync iterator remains live");
            self.heap.release_root(iterator_root);
            let result = result?;
            let state = self.new_array_from_async_state(ArrayFromAsyncState {
                output,
                iterator: Some(iterator),
                result,
                mapper,
                this_arg,
                index: 0,
                array_like: None,
                awaiting: ArrayFromAsyncAwait::IteratorStep,
            });
            return self.array_from_async_next(p, state);
        }

        {
            let length_atom = self.intern_atom("length");
            let length_value = self.get_property(p, source, length_atom)?;
            let number = self.to_number(p, length_value)?;
            let length = if number.is_nan() || number <= 0.0 {
                0
            } else if number.is_infinite() {
                MAX_SAFE_INTEGER as usize
            } else {
                number.floor().min(MAX_SAFE_INTEGER) as usize
            };
            if length > u32::MAX as usize {
                return Err(self.range_error(
                    p,
                    "Array.fromAsync array-like length is out of range".into(),
                ));
            }
            let result = self.construct_value(p, constructor, &[Value::number(length as f64)])?;
            let state = self.new_array_from_async_state(ArrayFromAsyncState {
                output,
                iterator: None,
                result,
                mapper,
                this_arg,
                index: 0,
                array_like: Some((source, length)),
                awaiting: ArrayFromAsyncAwait::ArrayLikeValue,
            });
            self.array_from_async_array_like(p, state)
        }
    }

    fn new_array_from_async_state(&mut self, state: ArrayFromAsyncState) -> Value {
        let mut roots = vec![
            self.heap.root(state.output),
            self.heap.root(state.result),
            self.heap.root(state.this_arg),
        ];
        roots.extend(state.iterator.map(|value| self.heap.root(value)));
        roots.extend(state.mapper.map(|value| self.heap.root(value)));
        roots.extend(state.array_like.map(|(value, _)| self.heap.root(value)));
        let state_value = self.heap.alloc(Cell::ArrayFromAsyncState(state));
        for root in roots {
            self.heap.release_root(root);
        }
        state_value
    }

    fn array_from_async_next(
        &mut self,
        p: &ResidualProgram,
        state_value: Value,
    ) -> Result<(), JsError> {
        let state_root = self.heap.root(state_value);
        let result = (|| {
            let state = self.array_from_async_state(state_value)?;
            let Some(iterator) = state.iterator else {
                return self.array_from_async_array_like(p, state_value);
            };
            let next = self.iterator_next(p, iterator)?;
            let state_value = self
                .heap
                .root_value(state_root)
                .expect("rooted Array.fromAsync state remains live");
            self.array_from_async_await(p, state_value, next, ArrayFromAsyncAwait::IteratorStep)
        })();
        self.heap.release_root(state_root);
        result
    }

    fn array_from_async_array_like(
        &mut self,
        p: &ResidualProgram,
        state_value: Value,
    ) -> Result<(), JsError> {
        let state_root = self.heap.root(state_value);
        let result = (|| {
            let state = self.array_from_async_state(state_value)?;
            let Some((source, length)) = state.array_like else {
                return Err(JsError("Array.fromAsync state has no input source".into()));
            };
            if state.index >= length {
                self.set_array_from_async_length(p, state.result, length)?;
                return self.promise_resolve_value(p, state.output, state.result);
            }
            let value = self.get_index(p, source, Value::number(state.index as f64))?;
            let state_value = self
                .heap
                .root_value(state_root)
                .expect("rooted Array.fromAsync state remains live");
            self.array_from_async_await(p, state_value, value, ArrayFromAsyncAwait::ArrayLikeValue)
        })();
        self.heap.release_root(state_root);
        result
    }

    fn array_from_async_await(
        &mut self,
        p: &ResidualProgram,
        state_value: Value,
        value: Value,
        awaiting: ArrayFromAsyncAwait,
    ) -> Result<(), JsError> {
        let state_root = self.heap.root(state_value);
        if let Some(Cell::ArrayFromAsyncState(state)) = self.heap.get_mut(state_value) {
            state.awaiting = awaiting;
        }
        let result = (|| {
            let promise = self.promise_object();
            self.promise_resolve_value(p, promise, value)?;
            let state_value = self
                .heap
                .root_value(state_root)
                .expect("rooted Array.fromAsync continuation remains live");
            let fulfilled = self.native_with_env(Native::ArrayFromAsyncFulfilled, state_value);
            let rejected = self.native_with_env(Native::ArrayFromAsyncRejected, state_value);
            self.promise_then(p, promise, fulfilled, rejected)?;
            Ok(())
        })();
        self.heap.release_root(state_root);
        result
    }

    fn array_from_async_state(&self, state: Value) -> Result<ArrayFromAsyncState, JsError> {
        match self.heap.get(state) {
            Some(Cell::ArrayFromAsyncState(state)) => Ok(*state),
            _ => Err(JsError(
                "Array.fromAsync continuation state is invalid".into(),
            )),
        }
    }

    pub(super) fn array_from_async_reaction(
        &mut self,
        p: &ResidualProgram,
        fulfilled: bool,
        value: Value,
    ) -> Result<Value, JsError> {
        let state_value = self
            .active_native_env()
            .ok_or_else(|| JsError("Array.fromAsync job has no state".into()))?;
        let state = self.array_from_async_state(state_value)?;
        if !fulfilled {
            self.array_from_async_reject(p, state, value);
            return Ok(Value::UNDEFINED);
        }
        if let Err(error) = self.array_from_async_fulfill(p, state_value, state, value) {
            let reason = self.thrown_value_for(p, error);
            self.array_from_async_reject(p, state, reason);
        }
        Ok(Value::UNDEFINED)
    }

    fn array_from_async_fulfill(
        &mut self,
        p: &ResidualProgram,
        state_value: Value,
        state: ArrayFromAsyncState,
        value: Value,
    ) -> Result<(), JsError> {
        match state.awaiting {
            ArrayFromAsyncAwait::IteratorStep => {
                if !self.is_object_like(value) {
                    return Err(self.type_error(p, "iterator result is not an object".into()));
                }
                let done_atom = self.intern_atom("done");
                let done = self.get_property(p, value, done_atom)?;
                if self.truthy(done) {
                    self.set_array_from_async_length(p, state.result, state.index)?;
                    return self.promise_resolve_value(p, state.output, state.result);
                }
                let value_atom = self.intern_atom("value");
                let item = self.get_property(p, value, value_atom)?;
                self.array_from_async_map_or_store(p, state_value, state, item)
            }
            ArrayFromAsyncAwait::ArrayLikeValue => {
                self.array_from_async_map_or_store(p, state_value, state, value)
            }
            ArrayFromAsyncAwait::MapperResult => {
                self.create_data_property_or_throw(p, state.result, state.index, value)?;
                self.array_from_async_advance(p, state_value, state)
            }
        }
    }

    fn array_from_async_map_or_store(
        &mut self,
        p: &ResidualProgram,
        state_value: Value,
        state: ArrayFromAsyncState,
        value: Value,
    ) -> Result<(), JsError> {
        if let Some(mapper) = state.mapper {
            let mapped = self.call_value(
                p,
                mapper,
                state.this_arg,
                &[value, Value::number(state.index as f64)],
            )?;
            return self.array_from_async_await(
                p,
                state_value,
                mapped,
                ArrayFromAsyncAwait::MapperResult,
            );
        }
        self.create_data_property_or_throw(p, state.result, state.index, value)?;
        self.array_from_async_advance(p, state_value, state)
    }

    fn array_from_async_advance(
        &mut self,
        p: &ResidualProgram,
        state_value: Value,
        mut state: ArrayFromAsyncState,
    ) -> Result<(), JsError> {
        state.index += 1;
        if let Some(Cell::ArrayFromAsyncState(current)) = self.heap.get_mut(state_value) {
            *current = state;
        }
        if state.iterator.is_some() {
            self.array_from_async_next(p, state_value)
        } else {
            self.array_from_async_array_like(p, state_value)
        }
    }

    fn array_from_async_reject(
        &mut self,
        p: &ResidualProgram,
        state: ArrayFromAsyncState,
        reason: Value,
    ) {
        if let Some(iterator) = state.iterator {
            let _ = self.iterator_close(p, iterator);
        }
        let _ = self.promise_settle(
            p,
            state.output,
            super::promise::PromiseState::Rejected,
            reason,
        );
    }

    fn set_array_from_async_length(
        &mut self,
        p: &ResidualProgram,
        target: Value,
        length: usize,
    ) -> Result<(), JsError> {
        if matches!(self.heap.get(target), Some(Cell::Array { .. })) {
            return Ok(());
        }
        let length_atom = self.intern_atom("length");
        let succeeded = self.set_property_with_receiver(
            p,
            target,
            length_atom,
            Value::number(length as f64),
            target,
        )?;
        if succeeded {
            Ok(())
        } else {
            Err(self.type_error(p, "cannot set result length".into()))
        }
    }

    pub(super) fn create_data_property_or_throw(
        &mut self,
        p: &ResidualProgram,
        target: Value,
        index: usize,
        value: Value,
    ) -> Result<(), JsError> {
        let descriptor = self
            .heap
            .alloc(Cell::Object(Self::empty_object(self.object_proto)));
        for (name, field) in [
            ("value", value),
            ("writable", Value::TRUE),
            ("enumerable", Value::TRUE),
            ("configurable", Value::TRUE),
        ] {
            let atom = self.intern_atom(name);
            self.set_property(descriptor, atom, field)?;
        }
        self.object_define_property(p, &[target, Value::number(index as f64), descriptor])?;
        Ok(())
    }

    pub(super) fn iterator_abrupt(
        &mut self,
        p: &ResidualProgram,
        iterator: Value,
        error: JsError,
    ) -> JsError {
        let _ = self.iterator_close(p, iterator);
        error
    }

    fn array_sort_native(
        &mut self,
        p: &ResidualProgram,
        this: Value,
        args: &[Value],
        mutate: bool,
    ) -> Result<Value, JsError> {
        let comparator = args.first().copied().filter(|value| !value.is_undefined());
        if let Some(value) = comparator
            && !self.is_function(value)
        {
            return Err(self.type_error(p, "sort comparator is not callable".into()));
        }
        let comparator = comparator.map(|value| self.heap.root(value));
        let mut object_root = None;
        let mut values = Vec::new();
        let outcome = (|| {
            let object = self.box_object_or_type_error(p, this)?;
            let root = self.heap.root(object);
            object_root = Some(root);
            let length = self.array_like_length(p, object)?;
            if !mutate && length > MAX_ARRAY_LENGTH {
                return Err(self.range_error(p, "invalid array length".into()));
            }
            let mut undefined_count = 0;
            for index in 0..length {
                let key = Value::number(index as f64);
                let object = self.heap.root_value(root).unwrap();
                if !mutate || self.has_property(p, object, key)? {
                    let object = self.heap.root_value(root).unwrap();
                    let value = self.get_index(p, object, key)?;
                    if value.is_undefined() {
                        undefined_count += 1;
                    } else {
                        values.push(self.heap.root(value));
                    }
                }
            }
            super::sort::try_stable_sort_by(&mut values, |left, right| {
                Ok(match self.sort_compare(p, comparator, *left, *right)? {
                    value if value < 0.0 => std::cmp::Ordering::Less,
                    value if value > 0.0 => std::cmp::Ordering::Greater,
                    _ => std::cmp::Ordering::Equal,
                })
            })?;
            if mutate {
                let sorted_count = values.len();
                for (index, value) in values.iter().enumerate() {
                    let object = self.heap.root_value(root).unwrap();
                    let value = self.heap.root_value(*value).unwrap();
                    self.set_index_mode(p, object, Value::number(index as f64), value, true)?;
                }
                for index in sorted_count..sorted_count + undefined_count {
                    let object = self.heap.root_value(root).unwrap();
                    self.set_index_mode(
                        p,
                        object,
                        Value::number(index as f64),
                        Value::UNDEFINED,
                        true,
                    )?;
                }
                for index in sorted_count + undefined_count..length {
                    let object = self.heap.root_value(root).unwrap();
                    let deleted =
                        self.object_delete_property(p, &[object, Value::number(index as f64)])?;
                    if !self.truthy(deleted) {
                        return Err(self.type_error(p, "cannot delete array-like element".into()));
                    }
                }
                Ok(self.heap.root_value(root).unwrap())
            } else {
                let mut result: Vec<_> = values
                    .iter()
                    .map(|root| self.heap.root_value(*root).unwrap())
                    .collect();
                result.resize(length, Value::UNDEFINED);
                Ok(self.new_array(result))
            }
        })();
        for value in values {
            self.heap.release_root(value);
        }
        if let Some(root) = object_root {
            self.heap.release_root(root);
        }
        if let Some(root) = comparator {
            self.heap.release_root(root);
        }
        outcome
    }

    // Undefined values are counted outside the sortable snapshot.
    fn sort_compare(
        &mut self,
        p: &ResidualProgram,
        comparator: Option<RootId>,
        left: RootId,
        right: RootId,
    ) -> Result<f64, JsError> {
        if let Some(comparator) = comparator {
            let comparator = self.heap.root_value(comparator).unwrap();
            let left = self.heap.root_value(left).unwrap();
            let right = self.heap.root_value(right).unwrap();
            let result = self.call_value(p, comparator, Value::UNDEFINED, &[left, right])?;
            let result = self.heap.root(result);
            let value = self.heap.root_value(result).unwrap();
            let number = self.to_number(p, value);
            self.heap.release_root(result);
            let number = number?;
            return Ok(if number.is_nan() { 0.0 } else { number });
        }
        let left = self.heap.root_value(left).unwrap();
        let left = self.to_string(p, left)?;
        let right = self.heap.root_value(right).unwrap();
        let right = self.to_string(p, right)?;
        Ok(match left.cmp(&right) {
            std::cmp::Ordering::Less => -1.0,
            std::cmp::Ordering::Equal => 0.0,
            std::cmp::Ordering::Greater => 1.0,
        })
    }
}
