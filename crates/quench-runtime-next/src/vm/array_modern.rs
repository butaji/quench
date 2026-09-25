use super::*;
use crate::heap::{ArrayFromAsyncAwait, ArrayFromAsyncState};

impl<H: Host> Vm<H> {
    pub(super) fn array_modern_native(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        match native {
            Native::ArrayToReversed => self.array_to_reversed_native(p, this),
            Native::ArrayToSpliced => self.array_to_spliced_native(p, this, args),
            Native::ArraySort => self.array_sort_native(p, this, args, true),
            Native::ArrayToSorted => self.array_sort_native(p, this, args, false),
            Native::ArraySpecies => Ok(this),
            Native::ArrayToString => self.array_to_string_native(p, this),
            Native::ArrayToLocaleString => self.array_to_locale_string_native(p, this),
            Native::ArrayFrom => self.array_from_native(p, args),
            Native::ArrayFromAsync => self.array_from_async_native(p, this, args),
            Native::ArrayOf => Ok(self.new_array(args.to_vec())),
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
        self.heap.alloc(Cell::Array {
            object: Self::empty_object(self.array_proto),
            elements: Rc::new(values),
        })
    }

    fn array_to_reversed_native(
        &mut self,
        p: &ResidualProgram,
        this: Value,
    ) -> Result<Value, JsError> {
        let object = self.box_object_or_type_error(p, this)?;
        let length = self.array_like_length(p, object)?;
        if length > u32::MAX as usize {
            return Err(self.range_error(p, "invalid array length".into()));
        }
        let mut values = Vec::with_capacity(length);
        for index in (0..length).rev() {
            values.push(self.get_index(p, object, Value::number(index as f64))?);
        }
        Ok(self.new_array(values))
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

    fn array_to_locale_string_native(
        &mut self,
        p: &ResidualProgram,
        this: Value,
    ) -> Result<Value, JsError> {
        let object = self.box_object_or_type_error(p, this)?;
        let length = self.array_like_length(p, object)?;
        let to_locale_string = self.intern_atom("toLocaleString");
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
            let element = self.call_value(p, method, value, &[])?;
            result.push_str(&self.to_string(p, element)?);
        }
        Ok(self.heap.alloc(Cell::String(result.into())))
    }

    fn array_from_native(&mut self, p: &ResidualProgram, args: &[Value]) -> Result<Value, JsError> {
        let source = args.first().copied().unwrap_or(Value::UNDEFINED);
        let done_atom = self.intern_atom("done");
        let value_atom = self.intern_atom("value");
        let mapfn = args.get(1).copied().filter(|value| !value.is_undefined());
        if let Some(mapfn) = mapfn
            && !matches!(self.heap.get(mapfn), Some(Cell::Function { .. }))
        {
            return Err(JsError("Array.from map function is not callable".into()));
        }
        let map_this = args.get(2).copied().unwrap_or(Value::UNDEFINED);
        let mut values = Vec::new();
        match self.get_iterator(p, source) {
            Ok(iterator) => loop {
                let step = match self.iterator_next(p, iterator) {
                    Ok(step) => step,
                    Err(error) => return Err(self.iterator_abrupt(p, iterator, error)),
                };
                let done = match self.get_property(p, step, done_atom) {
                    Ok(done) => done,
                    Err(error) => return Err(self.iterator_abrupt(p, iterator, error)),
                };
                if self.truthy(done) {
                    break;
                }
                let mut value = match self.get_property(p, step, value_atom) {
                    Ok(value) => value,
                    Err(error) => return Err(self.iterator_abrupt(p, iterator, error)),
                };
                if let Some(mapfn) = mapfn {
                    let index = Value::number(values.len() as f64);
                    value = match self.call_value(p, mapfn, map_this, &[value, index]) {
                        Ok(value) => value,
                        Err(error) => return Err(self.iterator_abrupt(p, iterator, error)),
                    };
                }
                values.push(value);
            },
            Err(error) if error.to_string() == "value is not iterable" => {
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
                values.reserve(length);
                for index in 0..length {
                    let mut value = self.get_index(p, source, Value::number(index as f64))?;
                    if let Some(mapfn) = mapfn {
                        value = self.call_value(
                            p,
                            mapfn,
                            map_this,
                            &[value, Value::number(index as f64)],
                        )?;
                    }
                    values.push(value);
                }
            }
            Err(error) => return Err(error),
        }
        Ok(self.new_array(values))
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
        self.iterator_close(p, iterator).err().unwrap_or(error)
    }

    fn array_to_spliced_native(
        &mut self,
        p: &ResidualProgram,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let object = self.box_object_or_type_error(p, this)?;
        let length = self.array_like_length(p, object)?;
        let start = args
            .first()
            .copied()
            .map(|value| self.array_relative_index(p, value, length))
            .transpose()?
            .unwrap_or(0);
        let remaining = length - start;
        let delete_count = match args.get(1).copied() {
            None if args.is_empty() => 0,
            None => length - start,
            Some(value) => {
                let number = self.to_number(p, value)?;
                if number.is_nan() || number <= 0.0 {
                    0
                } else if number.is_infinite() {
                    remaining
                } else {
                    (number.trunc() as usize).min(remaining)
                }
            }
        };
        let insert_count = args.len().saturating_sub(2);
        let result_length = length - delete_count + insert_count;
        if result_length as f64 > MAX_SAFE_INTEGER {
            return Err(self.type_error(p, "array-like length exceeds safe integer".into()));
        }
        if result_length > u32::MAX as usize {
            return Err(self.range_error(p, "invalid array length".into()));
        }
        let mut updated = Vec::with_capacity(result_length);
        for index in 0..start {
            updated.push(self.get_index(p, object, Value::number(index as f64))?);
        }
        updated.extend(args.iter().copied().skip(2));
        for index in start + delete_count..length {
            updated.push(self.get_index(p, object, Value::number(index as f64))?);
        }
        Ok(self.new_array(updated))
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
            && !matches!(self.heap.get(value), Some(Cell::Function { .. }))
        {
            return Err(self.type_error(p, "sort comparator is not callable".into()));
        }
        let object = self.box_object_or_type_error(p, this)?;
        let length = self.array_like_length(p, object)?;
        if !mutate && length > u32::MAX as usize {
            return Err(self.range_error(p, "invalid array length".into()));
        }

        let mut values = Vec::new();
        let mut undefined_count = 0;
        for index in 0..length {
            let key = Value::number(index as f64);
            if !mutate || self.has_property(p, object, key)? {
                let value = self.get_index(p, object, key)?;
                if value.is_undefined() {
                    undefined_count += 1;
                } else {
                    values.push(value);
                }
            }
        }

        self.sort_values(p, &mut values, comparator)?;
        if mutate {
            let sorted_count = values.len();
            for (index, value) in values.into_iter().enumerate() {
                self.set_index_mode(p, object, Value::number(index as f64), value, true)?;
            }
            for index in sorted_count..sorted_count + undefined_count {
                self.set_index_mode(
                    p,
                    object,
                    Value::number(index as f64),
                    Value::UNDEFINED,
                    true,
                )?;
            }
            for index in sorted_count + undefined_count..length {
                let deleted =
                    self.object_delete_property(p, &[object, Value::number(index as f64)])?;
                if !self.truthy(deleted) {
                    return Err(self.type_error(p, "cannot delete array-like element".into()));
                }
            }
            Ok(object)
        } else {
            values.resize(length, Value::UNDEFINED);
            Ok(self.new_array(values))
        }
    }

    fn sort_values(
        &mut self,
        p: &ResidualProgram,
        values: &mut [Value],
        comparator: Option<Value>,
    ) -> Result<(), JsError> {
        for index in 1..values.len() {
            let value = values[index];
            let mut position = index;
            while position > 0
                && self.sort_compare(p, comparator, values[position - 1], value)? > 0.0
            {
                values[position] = values[position - 1];
                position -= 1;
            }
            values[position] = value;
        }
        Ok(())
    }

    fn sort_compare(
        &mut self,
        p: &ResidualProgram,
        comparator: Option<Value>,
        left: Value,
        right: Value,
    ) -> Result<f64, JsError> {
        if left.is_undefined() || right.is_undefined() {
            return Ok(match (left.is_undefined(), right.is_undefined()) {
                (true, true) => 0.0,
                (true, false) => 1.0,
                (false, true) => -1.0,
                _ => unreachable!(),
            });
        }
        if let Some(comparator) = comparator {
            let result = self.call_value(p, comparator, Value::UNDEFINED, &[left, right])?;
            let number = self.to_number(p, result)?;
            return Ok(if number.is_nan() { 0.0 } else { number });
        }
        let left = self.to_string(p, left)?;
        let right = self.to_string(p, right)?;
        Ok(match left.cmp(&right) {
            std::cmp::Ordering::Less => -1.0,
            std::cmp::Ordering::Equal => 0.0,
            std::cmp::Ordering::Greater => 1.0,
        })
    }
}
