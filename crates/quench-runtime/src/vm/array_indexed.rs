use super::*;

impl<H: Host> Vm<H> {
    pub(super) fn array_indexed_native(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        match native {
            Native::ArrayAt => self.array_at_native(p, this, args),
            Native::ArrayLastIndexOf => self.array_last_index_of_native(p, this, args),
            Native::ArrayIndexOf => self.array_index_of_native(p, this, args),
            Native::ArrayCopyWithin => self.array_copy_within_native(p, this, args),
            Native::ArrayWith => self.array_copy_native(p, native, this, args),
            native if native.is_array_callback() => self.array_callback_native(p, native, this, args),
            Native::ArrayGroup | Native::ArrayGroupToMap => {
                self.array_group_native(p, native, this, args)
            }
            Native::ArrayFlatMap => self.array_flatten_native(p, native, this, args),
            Native::ArrayReduce | Native::ArrayReduceRight => {
                self.array_reduce_native(p, native, this, args)
            }
            _ => unreachable!("non-indexed native routed to array index dispatch"),
        }
    }

    pub(super) fn array_at_native(
        &mut self,
        p: &ResidualProgram,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let object = self.box_object_or_type_error(p, this)?;
        let length = self.array_like_length(p, object)?;
        let Some(index) =
            self.indexed_at_index(p, args.first().copied().unwrap_or(Value::UNDEFINED), length)?
        else {
            return Ok(Value::UNDEFINED);
        };
        self.get_index(p, object, Value::number(index as f64))
    }

    pub(super) fn array_last_index_of_native(
        &mut self,
        p: &ResidualProgram,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let object = self.box_object_or_type_error(p, this)?;
        let length = self.array_like_length(p, object)?;
        self.array_last_index_of_with_length(p, object, args, length)
    }

    pub(super) fn typed_array_last_index_of_native(
        &mut self,
        p: &ResidualProgram,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let Some(Cell::TypedArray { buffer, .. }) = self.heap.get(this) else {
            return Err(self.type_error(p, "typed array lastIndexOf receiver is invalid".into()));
        };
        if self.typed_array_out_of_bounds(this) || self.array_buffer_detached(*buffer) {
            return Err(self.type_error(p, "typed array lastIndexOf receiver is invalid".into()));
        }
        let length = self.typed_array_length(this).unwrap_or(0);
        self.array_last_index_of_with_length(p, this, args, length)
    }

    fn array_last_index_of_with_length(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        args: &[Value],
        length: usize,
    ) -> Result<Value, JsError> {
        if length == 0 {
            return Ok(Value::number(-1.0));
        }
        let from = match args.get(1) {
            None => length as isize - 1,
            Some(value) => {
                let number = self.to_number(p, *value)?;
                if number.is_nan() || number == 0.0 {
                    0
                } else if number == f64::NEG_INFINITY {
                    return Ok(Value::number(-1.0));
                } else if number == f64::INFINITY {
                    length as isize - 1
                } else if number < 0.0 {
                    (length as f64 + number.trunc()).floor() as isize
                } else {
                    (number.trunc() as usize).min(length - 1) as isize
                }
            }
        };
        if from < 0 {
            return Ok(Value::number(-1.0));
        }
        let search = args.first().copied().unwrap_or(Value::UNDEFINED);
        for index in (0..=from as usize).rev() {
            let key = Value::number(index as f64);
            if self.has_property(p, object, key)? {
                let value = self.get_index(p, object, key)?;
                if self.strict_equal(value, search) {
                    return Ok(Value::number(index as f64));
                }
            }
        }
        Ok(Value::number(-1.0))
    }

    pub(super) fn array_index_of_native(
        &mut self,
        p: &ResidualProgram,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        if let Some(found) = self.dense_index_of(this, args) {
            return Ok(found);
        }
        let object = self.box_object_or_type_error(p, this)?;
        let length = self.array_like_length(p, object)?;
        if length == 0 {
            return Ok(Value::number(-1.0));
        }
        let start = match args.get(1) {
            None => 0,
            Some(value) => {
                let number = self.to_number(p, *value)?;
                if number.is_nan() || number == 0.0 || number == f64::NEG_INFINITY {
                    0
                } else if number.is_infinite() {
                    return Ok(Value::number(-1.0));
                } else if number < 0.0 {
                    length.saturating_sub(number.abs().trunc() as usize)
                } else {
                    (number.trunc() as usize).min(length)
                }
            }
        };
        let search = args.first().copied().unwrap_or(Value::UNDEFINED);
        for index in start..length {
            let key = Value::number(index as f64);
            if self.has_property(p, object, key)? {
                let value = self.get_index(p, object, key)?;
                if self.strict_equal(value, search) {
                    return Ok(Value::number(index as f64));
                }
            }
        }
        Ok(Value::number(-1.0))
    }

    /// `Array.prototype.indexOf` of a dense array (`dense_array_length`) from an integer or
    /// absent `fromIndex`: each element is its own data property, compared by strict
    /// equality, and a hole no prototype holds (`inherited_index_absent`) is skipped, so no
    /// guest code runs. `None` for any other receiver or start, or a hole a prototype holds.
    pub(super) fn dense_index_of(&self, array: Value, args: &[Value]) -> Option<Value> {
        let length = self.dense_array_length(array)?;
        let start = match args.get(1) {
            None => 0,
            Some(from) => {
                let from = from.as_int()?;
                if from < 0 {
                    length.saturating_sub(from.unsigned_abs() as usize)
                } else {
                    (from as usize).min(length)
                }
            }
        };
        let search = args.first().copied().unwrap_or(Value::UNDEFINED);
        let elements = self.heap.array(array)?.elements();
        for (index, value) in elements.iter().enumerate().take(length).skip(start) {
            if value.is_deleted() {
                if !self.inherited_index_absent(array, index) {
                    return None;
                }
            } else if self.strict_equal(*value, search) {
                return Some(Value::number(index as f64));
            }
        }
        Some(Value::number(-1.0))
    }

    pub(super) fn array_copy_within_native(
        &mut self,
        p: &ResidualProgram,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let object = self.box_object_or_type_error(p, this)?;
        let length = self.array_like_length(p, object)?;
        let target = args
            .first()
            .map(|value| self.array_relative_index(p, *value, length))
            .transpose()?
            .unwrap_or(0);
        let source = args
            .get(1)
            .map(|value| self.array_relative_index(p, *value, length))
            .transpose()?
            .unwrap_or(0);
        let end = args
            .get(2)
            .copied()
            .filter(|value| !value.is_undefined())
            .map(|value| self.array_relative_index(p, value, length))
            .transpose()?
            .unwrap_or(length);
        let count = end
            .saturating_sub(source)
            .min(length.saturating_sub(target));
        for offset in 0..count {
            let from = if target <= source {
                source + offset
            } else {
                source + count - offset - 1
            };
            let to = if target <= source {
                target + offset
            } else {
                target + count - offset - 1
            };
            let from_key = Value::number(from as f64);
            let to_key = Value::number(to as f64);
            if self.has_property(p, object, from_key)? {
                let value = self.get_index(p, object, from_key)?;
                self.set_index_mode(p, object, to_key, value, true)?;
            } else {
                self.delete_array_like_property(p, object, to_key)?;
            }
        }
        Ok(object)
    }

    pub(super) fn array_callback_native(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        if this.is_null() || this.is_undefined() {
            return Err(self.type_error(p, "array callback receiver is nullish".into()));
        }
        let this = self.box_object_or_type_error(p, this)?;
        let length = self.array_like_length(p, this)?;
        self.array_callback_with_length(p, native, this, args, length, false)
    }

    pub(super) fn typed_array_callback_native(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let Some(Cell::TypedArray { buffer, .. }) = self.heap.get(this) else {
            return Err(self.type_error(p, "typed array callback receiver is invalid".into()));
        };
        if self.typed_array_out_of_bounds(this) || self.array_buffer_detached(*buffer) {
            return Err(self.type_error(p, "typed array callback receiver is invalid".into()));
        }
        let length = self.typed_array_length(this).unwrap_or(0);
        if matches!(native, Native::ArrayReduce | Native::ArrayReduceRight) {
            return self.array_reduce_with_length(p, native, this, args, length, true);
        }
        self.array_callback_with_length(p, native, this, args, length, true)
    }

    fn array_callback_with_length(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        this: Value,
        args: &[Value],
        length: usize,
        typed_array: bool,
    ) -> Result<Value, JsError> {
        let callback = args.first().copied().unwrap_or(Value::UNDEFINED);
        if !self.is_function(callback) {
            return Err(self.type_error(p, "array callback is not callable".into()));
        }
        let this_arg = args.get(1).copied().unwrap_or(Value::UNDEFINED);
        let fresh = if typed_array { None } else { self.fresh_array_iteration(native, this, callback, this_arg, length) };
        let mut iteration = match fresh {
            Some(iteration) => iteration,
            None => {
                let result = match native {
                    Native::ArrayMap if typed_array => self.typed_array_species_create_for_writing(p, this, length)?,
                    Native::ArrayMap => self.array_species_create(p, this, length)?,
                    Native::ArrayFilter if !typed_array => self.array_species_create(p, this, 0)?,
                    _ => Value::UNDEFINED,
                };
                ArrayIteration::new(native, this, callback, this_arg, result, false, typed_array, length)
            }
        };
        self.array_iteration_run(p, &mut iteration)
    }

    /// The iteration of an array callback method whose result, if it makes one, ArrayCreate
    /// makes, unreachable from guest code until it is returned, so its elements are stored as
    /// its own dense storage (`store_fresh_array_element`): a map's over a dense receiver,
    /// which bounds its length, or a filter's. `None` where the species makes the result.
    #[inline(always)]
    pub(super) fn fresh_array_iteration(
        &mut self,
        native: Native,
        this: Value,
        callback: Value,
        this_arg: Value,
        length: usize,
    ) -> Option<ArrayIteration> {
        let result = match native {
            Native::ArrayMap | Native::ArrayFilter if !self.array_species_is_array_create(this) => return None,
            Native::ArrayMap if self.heap.array_elements(this).is_some_and(|elements| elements.len() == length) => {
                self.hole_array(length)
            }
            Native::ArrayMap => return None,
            Native::ArrayFilter => self.hole_array(0),
            _ => Value::UNDEFINED,
        };
        Some(ArrayIteration::new(native, this, callback, this_arg, result, true, false, length))
    }

    /// The loop of an array callback method from `iteration`'s next element to its end,
    /// with a callback prepared for its calls (`Vm::with_prepared_callback`): the native's
    /// own loop, and the continuation of one a lane began (`ArrayIteration`).
    pub(super) fn array_iteration_run(
        &mut self,
        p: &ResidualProgram,
        iteration: &mut ArrayIteration,
    ) -> Result<Value, JsError> {
        let result_root = (!iteration.result.is_undefined()).then(|| self.heap.root(iteration.result));
        let mut filtered_value_roots = Vec::new();
        let (this, typed_array) = (iteration.receiver, iteration.typed_array);
        let typed_filter = typed_array && iteration.native == Native::ArrayFilter;
        let finds_holes = matches!(
            iteration.native,
            Native::ArrayFind | Native::ArrayFindIndex | Native::ArrayFindLast | Native::ArrayFindLastIndex
        );
        let outcome = self.with_prepared_callback(iteration.callback, iteration.this_arg, |vm, prepared| {
            while let Some(index) = iteration.advance() {
                let key = Value::length(index);
                // A present dense element is an own data property: HasProperty holds and Get
                // reads it, observing nothing.
                let value = match vm.dense_array_element(this, key) {
                    Some(value) if !typed_array => value,
                    _ => {
                        if !typed_array && !finds_holes && !vm.has_property(p, this, key)? {
                            continue;
                        }
                        vm.get_index(p, this, key)?
                    }
                };
                let mapped = vm.call_prepared(p, prepared, &[value, key, this])?;
                if typed_filter {
                    if vm.truthy(mapped) {
                        filtered_value_roots.push(vm.heap.root(value));
                    }
                    continue;
                }
                if let Some(done) = vm.array_iteration_apply(p, iteration, index, value, mapped)? {
                    return Ok(done);
                }
            }
            if !typed_filter {
                return Ok(iteration.completion());
            }
            let target = vm.typed_array_species_create_for_writing(p, this, filtered_value_roots.len())?;
            let target_root = vm.heap.root(target);
            let result = filtered_value_roots.iter().enumerate().try_for_each(|(index, value_root)| {
                let value = vm.heap.root_value(*value_root).unwrap();
                vm.typed_array_set(p, target, index, value).map(drop)
            });
            vm.heap.release_root(target_root);
            result.map(|()| target)
        });
        if let Some(root) = result_root {
            self.heap.release_root(root);
        }
        for root in filtered_value_roots {
            self.heap.release_root(root);
        }
        outcome
    }

    /// The step of an array callback method after its callback returned `mapped` for the
    /// element `value` at `index`: the result's element stored, or the method's value where
    /// the step ends it (a `some`, `every` or `find*` answer).
    pub(super) fn array_iteration_apply(
        &mut self,
        p: &ResidualProgram,
        iteration: &mut ArrayIteration,
        index: usize,
        value: Value,
        mapped: Value,
    ) -> Result<Option<Value>, JsError> {
        match iteration.native {
            Native::ArrayMap if iteration.typed_array => {
                self.typed_array_set(p, iteration.result, index, mapped)?;
            }
            Native::ArrayMap if !iteration.fresh => {
                self.create_data_property_or_throw(p, iteration.result, index, mapped)?;
            }
            Native::ArrayFilter if !iteration.fresh => {
                if self.truthy(mapped) {
                    self.create_data_property_or_throw(p, iteration.result, iteration.kept, value)?;
                    iteration.kept += 1;
                }
            }
            _ => return Ok(self.fresh_array_step(iteration, index, value, mapped)),
        };
        Ok(None)
    }

    /// `array_iteration_apply` of an iteration whose result, if it makes one, ArrayCreate
    /// made (`fresh_array_iteration`): its store and every answer run no guest code and raise
    /// nothing.
    #[inline]
    pub(super) fn fresh_array_step(
        &mut self,
        iteration: &mut ArrayIteration,
        index: usize,
        value: Value,
        mapped: Value,
    ) -> Option<Value> {
        match iteration.native {
            Native::ArrayForEach => None,
            Native::ArrayMap => {
                self.store_fresh_array_element(iteration.result, index, mapped);
                None
            }
            Native::ArrayFilter => {
                if self.truthy(mapped) {
                    self.store_fresh_array_element(iteration.result, iteration.kept, value);
                    iteration.kept += 1;
                }
                None
            }
            Native::ArraySome => self.truthy(mapped).then_some(Value::TRUE),
            Native::ArrayEvery => (!self.truthy(mapped)).then_some(Value::FALSE),
            Native::ArrayFind | Native::ArrayFindLast => self.truthy(mapped).then_some(value),
            _ => self.truthy(mapped).then(|| Value::number(index as f64)),
        }
    }

    pub(super) fn array_like_length(
        &mut self,
        p: &ResidualProgram,
        object: Value,
    ) -> Result<usize, JsError> {
        if let Some(length) = self.own_array_length(object) {
            return Ok(length);
        }
        let length_atom = self.length_atom;
        let value = self.get_property(p, object, length_atom)?;
        let number = self.to_number(p, value)?;
        if number.is_nan() || number <= 0.0 {
            return Ok(0);
        }
        Ok(number.floor().min(MAX_SAFE_INTEGER).min(usize::MAX as f64) as usize)
    }

    pub(super) fn array_reduce_native(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let object = self.box_object_or_type_error(p, this)?;
        let length = self.array_like_length(p, object)?;
        self.array_reduce_with_length(p, native, object, args, length, false)
    }

    fn array_reduce_with_length(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        object: Value,
        args: &[Value],
        length: usize,
        typed_array: bool,
    ) -> Result<Value, JsError> {
        let callback = args.first().copied().unwrap_or(Value::UNDEFINED);
        if !self.is_function(callback) {
            return Err(self.type_error(p, "reduce callback is not callable".into()));
        }
        let object_root = self.heap.root(object);
        let callback_root = self.heap.root(callback);
        let mut accumulator_root = args.get(1).map(|value| self.heap.root(*value));
        let reverse = matches!(native, Native::ArrayReduceRight);
        let mut indices = 0..length;
        let outcome = self.with_prepared_callback(callback, Value::UNDEFINED, |vm, prepared| {
            while let Some(index) = if reverse {
                indices.next_back()
            } else {
                indices.next()
            } {
                let key = Value::length(index);
                let object = vm.heap.root_value(object_root).unwrap();
                if !typed_array && !vm.has_property(p, object, key)? {
                    continue;
                }
                let object = vm.heap.root_value(object_root).unwrap();
                let value = vm.get_index(p, object, key)?;
                match accumulator_root {
                    None => accumulator_root = Some(vm.heap.root(value)),
                    Some(root) => {
                        let accumulator = vm.heap.root_value(root).unwrap();
                        let object = vm.heap.root_value(object_root).unwrap();
                        let result = vm.call_prepared(p, prepared, &[accumulator, value, key, object])?;
                        vm.heap.update_root(root, result);
                    }
                }
            }
            match accumulator_root {
                Some(root) => Ok(vm.heap.root_value(root).unwrap()),
                None => {
                    Err(vm.type_error(p, "reduce of empty array with no initial value".into()))
                }
            }
        });
        if let Some(root) = accumulator_root {
            self.heap.release_root(root);
        }
        self.heap.release_root(callback_root);
        self.heap.release_root(object_root);
        outcome
    }
}

/// What an array callback method's loop carries from one element to the next past its
/// setup (the receiver made an object, its length read once, the callback tested and the
/// result made): the state V8's `ArrayForEachLoopContinuation` resumes from. The native's own
/// loop runs it (`Vm::array_iteration_run`); a lane steps it one callback activation at a
/// time (`lane_call_array_callback`), and a frame of such a callback, once materialized,
/// finishes it on the general path (`CallCompletion::ArrayStep`).
#[derive(Clone, Copy)]
pub(crate) struct ArrayIteration {
    pub(super) native: Native,
    pub(super) receiver: Value,
    pub(super) callback: Value,
    pub(super) this_arg: Value,
    /// A map's or filter's result, `undefined` for the methods that make none and for a
    /// typed filter, which makes its result at the end.
    pub(super) result: Value,
    /// Whether ArrayCreate made the result (`Vm::fresh_array_iteration`).
    pub(super) fresh: bool,
    pub(super) typed_array: bool,
    /// The elements visited, from the end for `findLast*`.
    pub(super) visited: usize,
    pub(super) length: usize,
    /// The elements a filter kept.
    pub(super) kept: usize,
}

impl ArrayIteration {
    pub(super) fn new(
        native: Native,
        receiver: Value,
        callback: Value,
        this_arg: Value,
        result: Value,
        fresh: bool,
        typed_array: bool,
        length: usize,
    ) -> Self {
        Self { native, receiver, callback, this_arg, result, fresh, typed_array, visited: 0, length, kept: 0 }
    }

    /// The index of the next element, which it counts as visited; `None` past the last.
    pub(super) fn advance(&mut self) -> Option<usize> {
        if self.visited == self.length {
            return None;
        }
        self.visited += 1;
        Some(match self.native {
            Native::ArrayFindLast | Native::ArrayFindLastIndex => self.length - self.visited,
            _ => self.visited - 1,
        })
    }

    /// The method's value once every element was visited without a step ending it.
    #[inline(always)]
    pub(super) fn completion(&self) -> Value {
        match self.native {
            Native::ArrayMap | Native::ArrayFilter => self.result,
            Native::ArraySome => Value::FALSE,
            Native::ArrayEvery => Value::TRUE,
            Native::ArrayFindIndex | Native::ArrayFindLastIndex => Value::number(-1.0),
            _ => Value::UNDEFINED,
        }
    }
}

/// The values an `ArrayStepCall` roots: the receiver, callback, thisArg, result and element.
const ARRAY_STEP_VALUES: usize = 5;

/// A lane's array iteration whose callback's activation became a frame
/// (`CallCompletion::ArrayStep`): the loop's state, the element and index that callback was
/// passed, and roots for the values the iteration reads once its frame returns, which only
/// the iteration holds.
pub(crate) struct ArrayStepCall {
    iteration: ArrayIteration,
    element: Value,
    index: usize,
    roots: [crate::heap::RootId; ARRAY_STEP_VALUES],
}

impl<H: Host> Vm<H> {
    /// The `ArrayStepCall` of a lane's iteration whose callback, passed `element` at `index`,
    /// becomes a frame.
    pub(super) fn array_step_call(&mut self, iteration: ArrayIteration, element: Value, index: usize) -> ArrayStepCall {
        let values = [iteration.receiver, iteration.callback, iteration.this_arg, iteration.result, element];
        ArrayStepCall { iteration, element, index, roots: values.map(|value| self.heap.root(value)) }
    }

    /// The value of the array callback method whose callback's frame (`ArrayStepCall`)
    /// returned `mapped`: its step, then the rest of its loop (`Vm::array_iteration_run`).
    pub(super) fn finish_array_step(
        &mut self,
        p: &ResidualProgram,
        call: ArrayStepCall,
        mapped: Value,
    ) -> Result<Value, JsError> {
        let ArrayStepCall { mut iteration, element, index, roots } = call;
        let result = match self.fresh_array_step(&mut iteration, index, element, mapped) {
            Some(value) => Ok(value),
            None => self.array_iteration_run(p, &mut iteration),
        };
        self.release_array_step(roots);
        result
    }

    /// Release what an `ArrayStepCall` rooted, as its frame unwinds.
    pub(super) fn abandon_array_step(&mut self, call: ArrayStepCall) {
        self.release_array_step(call.roots);
    }

    fn release_array_step(&mut self, roots: [crate::heap::RootId; ARRAY_STEP_VALUES]) {
        for root in roots {
            self.heap.release_root(root);
        }
    }
}
