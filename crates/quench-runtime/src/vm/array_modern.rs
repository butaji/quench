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
        let Some(elements) = self.heap.array_elements(this) else {
            return Err(JsError("modern array method receiver is not array".into()));
        };
        let length = self.heap.sparse_length(this).unwrap_or(elements.len());
        Ok((0..length)
            .map(|index| self.array_value_at(this, index))
            .collect())
    }

    pub(super) fn new_array(&mut self, values: Vec<Value>) -> Value {
        self.new_array_with_prototype(values, self.array_prototype_for_realm(self.realm.globals))
    }

    /// Append an own data element to an unexposed dense array. No guest code runs.
    pub(super) fn append_fresh_array_element(&mut self, array: Value, value: Value) -> usize {
        let Some(elements) = self.heap.array_elements_mut(array) else {
            unreachable!("fresh array owner remains an array");
        };
        let elements = super::index::mutable_array_elements(elements);
        let capacity = elements.capacity();
        elements.push(value);
        let length = elements.len();
        let grown = elements.capacity() - capacity;
        self.heap.charge_payload_growth(grown * size_of::<Value>());
        length
    }

    /// Store element `index` of a dense array no guest code reaches, at most one past its
    /// end: its own data property, every element a default one. The store passes the write
    /// barrier, since the array may have been promoted while guest code ran.
    pub(super) fn store_fresh_array_element(&mut self, array: Value, index: usize, value: Value) {
        let Some(object) = self.heap.array_store_mut(array, value) else {
            unreachable!("fresh array owner remains an array");
        };
        let elements = super::index::mutable_array_elements(object.elements_mut());
        if index < elements.len() {
            elements[index] = value;
            return;
        }
        debug_assert_eq!(index, elements.len(), "a fresh array grows by one element at a time");
        let capacity = elements.capacity();
        elements.push(value);
        let grown = elements.capacity() - capacity;
        self.heap.charge_payload_growth(grown * size_of::<Value>());
    }

    pub(super) fn new_array_with_prototype(
        &mut self,
        values: Vec<Value>,
        prototype: Value,
    ) -> Value {
        self.heap.alloc(Cell::Array {
            object: crate::heap::ArrayObject::new(Self::empty_object(prototype), values),
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
        Ok(self.heap.alloc_string(result.into()))
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

    pub(super) fn array_create(
        &mut self,
        p: &ResidualProgram,
        length: usize,
    ) -> Result<Value, JsError> {
        if length > MAX_ARRAY_LENGTH {
            return Err(self.range_error(p, "invalid array length".into()));
        }
        let prototype = self.array_prototype_for_realm(self.realm.globals);
        let array = self.heap.alloc(Cell::Array {
            object: crate::heap::ArrayObject::new(
                Self::empty_object(prototype),
                Vec::new(),
            ),
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
                self.validate_typed_array_result(p, result, length.unwrap_or_default(), true)?;
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
        let iterator_symbol = self.well_known_symbols.get(WellKnownSymbol::Iterator);
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
            // Constructing the intrinsic Array (or no constructor) runs no guest code, so the
            // source still iterates as `iterator_method` found it; filling an empty dense array
            // with intact iteration values is then the protocol without its objects.
            let intrinsic_target = !self.is_constructable(p, constructor)
                || self.is_current_realm_native(constructor, Native::Array);
            let target = self.array_from_target(p, constructor, None, target_kind)?;
            if mapfn.is_none()
                && intrinsic_target
                && self.dense_array_length(target) == Some(0)
                && let Some(values) = self.intact_iteration_values(source)
                && self.append_literal_elements(target, &values)
            {
                return Ok(target);
            }
            let root = self.heap.root(target);
            let outcome = (|| {
                let iterator = self.call_value(p, iterator_method, source, &[])?;
                if !self.is_object_like(iterator) {
                    return Err(
                        self.type_error(p, "iterator method did not return an object".into())
                    );
                }
                let iterator_root = self.heap.root(iterator);
                let mut next_root = None;
                let outcome = (|| {
                    // GetIteratorFromMethod reads `next` once; every step calls that method.
                    let next = self.get_property(p, iterator, self.runtime_atoms.next)?;
                    let next = *next_root.insert(self.heap.root(next));
                    let mut index = 0;
                    loop {
                        let iterator = self.heap.root_value(iterator_root).unwrap_or(iterator);
                        let Some(mut value) = self.rooted_iterator_step_value(p, iterator_root, next)?
                        else {
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
                for root in [Some(iterator_root), next_root].into_iter().flatten() {
                    self.heap.release_root(root);
                }
                outcome
            })();
            self.heap.release_root(root);
            return outcome;
        }

        let length = self.array_like_length(p, source)?;
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
            let target = self.heap.root_value(root).unwrap();
            if target_kind == ArrayFromTarget::Array {
                self.set_array_like_length(p, target, length)?;
            }
            Ok(target)
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
        let constructor = self.heap.root(constructor);
        let mapfn = mapfn.map(|value| self.heap.root(value));
        let map_this = mapfn.map(|_| self.heap.root(map_this));
        let mut values = Vec::new();
        let mut target = None;
        let outcome = (|| {
            values = self.rooted_iterator_list(p, source, iterator_method)?;
            let constructor = self.heap.root_value(constructor).unwrap();
            let result = self.array_from_target(
                p,
                constructor,
                Some(values.len()),
                ArrayFromTarget::TypedArray,
            )?;
            let target_root = self.heap.root(result);
            target = Some(target_root);
            for (index, value) in values.iter().enumerate() {
                let value = self.heap.root_value(*value).unwrap();
                let value = if let Some(mapfn) = mapfn {
                    let mapfn = self.heap.root_value(mapfn).unwrap();
                    let map_this = self.heap.root_value(map_this.unwrap()).unwrap();
                    self.call_value(p, mapfn, map_this, &[value, Value::number(index as f64)])?
                } else {
                    value
                };
                let target = self.heap.root_value(target_root).unwrap();
                self.typed_array_set(p, target, index, value)?;
            }
            Ok(self.heap.root_value(target_root).unwrap())
        })();
        for root in [Some(constructor), mapfn, map_this, target]
            .into_iter()
            .flatten()
        {
            self.heap.release_root(root);
        }
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
        let promise = self.promise_object()?;
        self.with_call_roots([promise], |vm| {
            if let Err(error) = vm.array_from_async_start(p, promise, this, args) {
                let reason = vm.thrown_value_for(p, error);
                vm.promise_settle(p, promise, super::promise::PromiseState::Rejected, reason)?;
            }
            Ok(promise)
        })
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
            let length_atom = self.length_atom;
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
        let state_value = self.heap.alloc(Cell::ArrayFromAsyncState(Box::new(state)));
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
            let promise = self.with_call_roots([value], |vm| vm.promise_object())?;
            self.with_call_roots([promise], |vm| {
                vm.promise_resolve_value(p, promise, value)?;
                let state_value = vm
                    .heap
                    .root_value(state_root)
                    .expect("rooted Array.fromAsync continuation remains live");
                let fulfilled = vm.native_with_env(Native::ArrayFromAsyncFulfilled, state_value);
                let rejected = vm.native_with_env(Native::ArrayFromAsyncRejected, state_value);
                vm.promise_then(p, promise, fulfilled, rejected)?;
                Ok(())
            })
        })();
        self.heap.release_root(state_root);
        result
    }

    fn array_from_async_state(&self, state: Value) -> Result<ArrayFromAsyncState, JsError> {
        match self.heap.get(state) {
            Some(Cell::ArrayFromAsyncState(state)) => Ok(**state),
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
                let done_atom = self.runtime_atoms.done;
                let done = self.get_property(p, value, done_atom)?;
                if self.truthy(done) {
                    self.set_array_from_async_length(p, state.result, state.index)?;
                    return self.promise_resolve_value(p, state.output, state.result);
                }
                let value_atom = self.runtime_atoms.value;
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
            **current = state;
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
        if self.heap.array(target).is_some() {
            return Ok(());
        }
        let length_atom = self.length_atom;
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
        if self.create_plain_array_element(target, index, value) {
            return Ok(());
        }
        let key = self.to_property_key(p, Value::number(index as f64))?;
        let record = super::object_descriptors::PropertyDescriptorRecord::data(value);
        if self.define_own_property_record(p, target, key, record)? {
            Ok(())
        } else {
            Err(self.type_error(p, "cannot create data property".into()))
        }
    }

    /// CreateDataProperty on an extensible ordinary array whose elements and `length` carry
    /// only default attributes: every element is a writable, enumerable, configurable data
    /// property, so the definition always succeeds and is the element store itself. Other
    /// receivers take the full DefineOwnProperty path.
    fn create_plain_array_element(&mut self, target: Value, index: usize, value: Value) -> bool {
        let Some(object) = self.heap.array(target) else {
            return false;
        };
        // An array's `length` keeps non-default attributes only as a recorded descriptor.
        let length_descriptor = object.has_length_descriptor();
        let plain = index < MAX_ARRAY_LENGTH
            && object.is_extensible()
            && !object.has_indexed_descriptors()
            && !object.is_arguments_object()
            && (!length_descriptor
                || self
                    .descriptors
                    .get(&(target, PropertyKey::string(self.length_atom)))
                    .is_none_or(|length| length.writable));
        plain && self.set_array_element(target, index, value)
    }

    pub(super) fn iterator_abrupt(
        &mut self,
        p: &ResidualProgram,
        iterator: Value,
        mut error: JsError,
    ) -> JsError {
        let thrown = error.thrown_value().map(|value| self.heap.root(value));
        let _ = self.iterator_close(p, iterator);
        if let Some(root) = thrown {
            error.replace_thrown_value(self.heap.root_value(root).unwrap());
            self.heap.release_root(root);
        }
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
        let object = self.box_object_or_type_error(p, this)?;
        // The receiver, the comparator and every value read stay reachable while guest code
        // (getters, the comparator) runs: they join the call roots for the whole sort.
        self.with_call_roots([object].into_iter().chain(comparator), |vm| {
            vm.array_sort_rooted(p, object, comparator, mutate)
        })
    }

    /// `array_sort_native` once its receiver and comparator are rooted.
    fn array_sort_rooted(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        comparator: Option<Value>,
        mutate: bool,
    ) -> Result<Value, JsError> {
        let length = self.array_like_length(p, object)?;
        if !mutate && length > MAX_ARRAY_LENGTH {
            return Err(self.range_error(p, "invalid array length".into()));
        }
        let roots = self.active_call_roots.len();
        let mut undefined_count = 0;
        if let Some(elements) = self.present_dense_elements(object, 0..length).map(<[Value]>::to_vec) {
            // SortIndexedProperties reads each present own data element, observing nothing.
            let present = elements.into_iter().filter(|value| !value.is_undefined());
            self.active_call_roots.extend(present);
            undefined_count = length - (self.active_call_roots.len() - roots);
        } else {
            for index in 0..length {
                let key = Value::number(index as f64);
                if !mutate || self.has_property(p, object, key)? {
                    match self.get_index(p, object, key)? {
                        Value::UNDEFINED => undefined_count += 1,
                        value => self.active_call_roots.push(value),
                    }
                }
            }
        }
        let mut values = self.active_call_roots[roots..].to_vec();
        let outcome = super::sort::try_stable_sort_by(&mut values, |left, right| {
            self.sort_compare(p, comparator, *left, *right)
        })
        .and_then(|()| self.store_sorted(p, object, &values, undefined_count, length, mutate));
        self.active_call_roots.truncate(roots);
        outcome
    }

    /// The sorted `values`, then `undefined_count` undefineds and holes up to `length`:
    /// written back into `object` when sorting in place, else a new array.
    fn store_sorted(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        values: &[Value],
        undefined_count: usize,
        length: usize,
        mutate: bool,
    ) -> Result<Value, JsError> {
        if !mutate {
            let mut result = values.to_vec();
            result.resize(length, Value::UNDEFINED);
            return Ok(self.new_array(result));
        }
        let sorted_count = values.len();
        // Each Set of a dense array's present element writes an own writable data element.
        if sorted_count == length
            && self.dense_array_length(object) == Some(length)
            && self.present_dense_elements(object, 0..length).is_some()
        {
            let Some(array) = self.heap.array_mut(object) else {
                unreachable!("matched an array above");
            };
            super::index::mutable_array_elements(array.elements_mut()).copy_from_slice(values);
            return Ok(object);
        }
        for (index, value) in values.iter().enumerate() {
            self.set_index_mode(p, object, Value::number(index as f64), *value, true)?;
        }
        for index in sorted_count..sorted_count + undefined_count {
            self.set_index_mode(p, object, Value::number(index as f64), Value::UNDEFINED, true)?;
        }
        for index in sorted_count + undefined_count..length {
            let deleted = self.object_delete_property(p, &[object, Value::number(index as f64)])?;
            if !self.truthy(deleted) {
                return Err(self.type_error(p, "cannot delete array-like element".into()));
            }
        }
        Ok(object)
    }

    fn sort_compare(
        &mut self,
        p: &ResidualProgram,
        comparator: Option<Value>,
        left: Value,
        right: Value,
    ) -> Result<std::cmp::Ordering, JsError> {
        if let Some(comparator) = comparator {
            return self.compare_sort_callback(p, comparator, left, right);
        }
        let left = self.to_string(p, left)?;
        let right = self.to_string(p, right)?;
        Ok(left.cmp(&right))
    }
}
