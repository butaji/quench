use super::promise::PromiseState;
use super::property_key::PropertyKey;

const ENTRIES: &str = "\0rqj:disposable-stack:entries";
const DISPOSED: &str = "\0rqj:disposable-stack:disposed";
const ASYNC_STACK: &str = "\0rqj:disposable-stack:async";
const ASYNC_DISPOSAL_ENTRIES_SLOT: usize = 0;
const ASYNC_DISPOSAL_CURSOR_SLOT: usize = 1;
const ASYNC_DISPOSAL_COMPLETION_SLOT: usize = 2;
const ASYNC_DISPOSAL_RESULT_SLOT: usize = 3;
const DISPOSAL_ENTRY_CALLBACK_SLOT: usize = 0;
const DISPOSAL_ENTRY_RECEIVER_SLOT: usize = 1;
const DISPOSAL_ENTRY_MODE_SLOT: usize = 2;
const DISPOSAL_ENTRY_AWAIT_RESULT_SLOT: usize = 3;
const DISPOSAL_USE_MODE: i32 = 0;
const DISPOSAL_ADOPT_MODE: i32 = 1;
const DISPOSAL_DEFER_MODE: i32 = 2;
const DISPOSAL_AWAIT_MODE: i32 = 3;
const DISPOSAL_INVALID_MODE: i32 = -1;

impl<H: Host> Vm<H> {
    pub(super) fn is_disposal_native(native: Native) -> bool {
        matches!(
            native,
            Native::DisposableStack
                | Native::AsyncDisposableStack
                | Native::AsyncDisposableStackUse
                | Native::AsyncDisposableStackAdopt
                | Native::AsyncDisposableStackDefer
                | Native::AsyncDisposableStackMove
                | Native::AsyncDisposableStackDisposeAsync
                | Native::AsyncDisposableStackDisposed
                | Native::DisposableStackUse
                | Native::DisposableStackAdopt
                | Native::DisposableStackDefer
                | Native::DisposableStackDispose
                | Native::DisposableStackUseAsync
                | Native::DisposableStackDisposeAsync
                | Native::DisposableStackDisposeWithCompletion
                | Native::DisposableStackDisposeAsyncWithCompletion
                | Native::DisposableStackAsyncDisposalFulfilled
                | Native::DisposableStackAsyncDisposalRejected
                | Native::AsyncIteratorDispose
                | Native::AsyncIteratorDisposeFulfilled
        )
    }

    pub(super) fn install_disposal(&mut self, p: &ResidualProgram) -> Result<(), JsError> {
        let constructor = self.native_value(Native::DisposableStack);
        let prototype = self.object();
        for (name, native) in [
            ("use", Native::DisposableStackUse),
            ("adopt", Native::DisposableStackAdopt),
            ("defer", Native::DisposableStackDefer),
            ("dispose", Native::DisposableStackDispose),
            ("useAsync", Native::DisposableStackUseAsync),
            ("disposeAsync", Native::DisposableStackDisposeAsync),
        ] {
            self.set_builtin_named(p, prototype, name, native)?;
        }
        if let Some(symbol) = self.well_known_symbols.get("dispose").copied() {
            self.set_index(
                p,
                prototype,
                symbol,
                self.native_value(Native::DisposableStackDispose),
            )?;
        }
        if let Some(symbol) = self.well_known_symbols.get("asyncDispose").copied() {
            self.set_index(
                p,
                prototype,
                symbol,
                self.native_value(Native::DisposableStackDisposeAsync),
            )?;
        }
        self.set_builtin_value_named(
            prototype,
            "\0rqj:disposeWithCompletion",
            self.native_value(Native::DisposableStackDisposeWithCompletion),
        )?;
        self.set_builtin_value_named(
            prototype,
            "\0rqj:disposeAsyncWithCompletion",
            self.native_value(Native::DisposableStackDisposeAsyncWithCompletion),
        )?;
        self.set_named(p, constructor, "prototype", prototype)?;
        self.set_builtin_function_name(constructor, "DisposableStack")?;
        self.global(p, "DisposableStack", constructor)?;

        let constructor = self.native_value(Native::AsyncDisposableStack);
        let prototype = self.object();
        for (name, native) in [
            ("use", Native::AsyncDisposableStackUse),
            ("adopt", Native::AsyncDisposableStackAdopt),
            ("defer", Native::AsyncDisposableStackDefer),
            ("move", Native::AsyncDisposableStackMove),
            ("disposeAsync", Native::AsyncDisposableStackDisposeAsync),
        ] {
            self.set_builtin_named(p, prototype, name, native)?;
        }
        self.set_builtin_value_named(
            prototype,
            "constructor",
            constructor,
        )?;
        self.install_builtin_to_string_tag(prototype, "AsyncDisposableStack")?;
        if let Some(symbol) = self.well_known_symbols.get("asyncDispose").copied() {
            let method = self.native_value(Native::AsyncDisposableStackDisposeAsync);
            self.set_index(
                p,
                prototype,
                symbol,
                method,
            )?;
            self.set_property_attributes(
                prototype,
                PropertyKey::symbol(symbol),
                PropertyAttributes {
                    writable: true,
                    enumerable: false,
                    configurable: true,
                    accessor: false,
                    getter: None,
                    setter: None,
                },
            );
        }
        let disposed = self.native_value(Native::AsyncDisposableStackDisposed);
        let disposed_atom = self.intern_atom("disposed");
        self.set_named(p, prototype, "disposed", disposed)?;
        self.set_property_attributes(
            prototype,
            PropertyKey::string(disposed_atom),
            PropertyAttributes {
                writable: false,
                enumerable: false,
                configurable: true,
                accessor: true,
                getter: Some(disposed),
                setter: None,
            },
        );
        self.set_builtin_function_name(disposed, "get disposed")?;
        self.set_builtin_value_named(constructor, "prototype", prototype)?;
        let prototype_atom = self.intern_atom("prototype");
        self.set_property_attributes(
            constructor,
            PropertyKey::string(prototype_atom),
            PropertyAttributes {
                writable: false,
                enumerable: false,
                configurable: false,
                accessor: false,
                getter: None,
                setter: None,
            },
        );
        self.set_builtin_function_name(constructor, "AsyncDisposableStack")?;
        self.global(p, "AsyncDisposableStack", constructor)
    }

    pub(super) fn construct_disposable_stack_native(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        new_target: Value,
    ) -> Result<Value, JsError> {
        let async_stack = native == Native::AsyncDisposableStack;
        let constructor = self.native_value(native);
        let prototype_atom = self.intern_atom("prototype");
        let candidate = self.get_property(p, new_target, prototype_atom)?;
        let prototype = if self.object_data(candidate).is_some() {
            candidate
        } else {
            let realm = match self.heap.get(new_target) {
                Some(Cell::Function { realm, .. }) => *realm,
                _ => self.realm.globals,
            };
            let constructor_atom = self.intern_atom(if async_stack {
                "AsyncDisposableStack"
            } else {
                "DisposableStack"
            });
            let realm_constructor = self.get_property(p, realm, constructor_atom)?;
            let realm_prototype = self.get_property(p, realm_constructor, prototype_atom)?;
            if self.object_data(realm_prototype).is_some() {
                realm_prototype
            } else {
                self.own_property(constructor, prototype_atom)
                    .unwrap_or(self.object_proto)
            }
        };
        let stack = self.heap.alloc(Cell::Object(Self::empty_object(prototype)));
        let root = self.heap.root(stack);
        let entries = self.heap.alloc(Cell::Array {
            object: Self::empty_object(self.array_proto),
            elements: Rc::new(vec![]),
        });
        let entries_atom = self.intern_atom(ENTRIES);
        let disposed_atom = self.intern_atom(DISPOSED);
        self.set_property(stack, entries_atom, entries)?;
        self.set_property(stack, disposed_atom, Value::FALSE)?;
        let async_atom = self.intern_atom(ASYNC_STACK);
        self.set_property(
            stack,
            async_atom,
            if async_stack { Value::TRUE } else { Value::FALSE },
        )?;
        self.heap.release_root(root);
        Ok(stack)
    }

    pub(super) fn call_disposal_native(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        if native == Native::DisposableStack {
            return Err(JsError("DisposableStack constructor requires new".into()));
        }
        if native == Native::AsyncDisposableStack {
            return Err(JsError("AsyncDisposableStack constructor requires new".into()));
        }
        if native == Native::AsyncDisposableStackDisposed {
            self.require_async_stack(p, this)?;
            return self.stack_disposed(this);
        }
        if native == Native::AsyncIteratorDispose {
            return self.async_iterator_dispose(p, this);
        }
        if native == Native::AsyncIteratorDisposeFulfilled {
            return Ok(Value::UNDEFINED);
        }
        if matches!(
            native,
            Native::AsyncDisposableStackUse
                | Native::AsyncDisposableStackAdopt
                | Native::AsyncDisposableStackDefer
                | Native::AsyncDisposableStackMove
        ) {
            self.require_async_stack(p, this)?;
        }
        if native == Native::AsyncDisposableStackDisposeAsync {
            return self.stack_dispose_async_native(p, this);
        }
        if native == Native::AsyncDisposableStackMove {
            return self.stack_move(p, this, true);
        }
        if native == Native::AsyncDisposableStackUse {
            self.require_open_stack(p, this)?;
            return self.stack_use_async(p, this, args);
        }
        if native == Native::AsyncDisposableStackAdopt {
            self.require_open_stack(p, this)?;
            return self.stack_adopt(p, this, args, true);
        }
        if native == Native::AsyncDisposableStackDefer {
            self.require_open_stack(p, this)?;
            return self.stack_defer(p, this, args, true);
        }
        if native == Native::DisposableStackDispose {
            self.require_stack(p, this)?;
            return self.stack_dispose(p, this);
        }
        if native == Native::DisposableStackDisposeAsync {
            self.require_stack(p, this)?;
            return self.stack_dispose_async(p, this);
        }
        if native == Native::DisposableStackDisposeWithCompletion {
            self.require_stack(p, this)?;
            let completion = args.first().copied().unwrap_or(Value::UNDEFINED);
            return self.stack_dispose_with_completion(p, this, completion);
        }
        if native == Native::DisposableStackDisposeAsyncWithCompletion {
            self.require_stack(p, this)?;
            let completion = args.first().copied().unwrap_or(Value::UNDEFINED);
            return self.stack_dispose_async_with_completion(p, this, completion);
        }
        if matches!(
            native,
            Native::DisposableStackAsyncDisposalFulfilled
                | Native::DisposableStackAsyncDisposalRejected
        ) {
            let state = self.active_native_env().unwrap_or(Value::UNDEFINED);
            if native == Native::DisposableStackAsyncDisposalRejected {
                let reason = args.first().copied().unwrap_or(Value::UNDEFINED);
                self.add_async_disposal_error(p, state, reason)?;
            }
            return self.continue_async_disposal(p, state);
        }
        self.require_open_stack(p, this)?;
        match native {
            Native::DisposableStackUse => self.stack_use(p, this, args),
            Native::DisposableStackUseAsync => self.stack_use_async(p, this, args),
            Native::DisposableStackAdopt => self.stack_adopt(p, this, args, false),
            Native::DisposableStackDefer => self.stack_defer(p, this, args, false),
            _ => Err(JsError("invalid disposal native".into())),
        }
    }

    fn async_iterator_dispose(
        &mut self,
        p: &ResidualProgram,
        receiver: Value,
    ) -> Result<Value, JsError> {
        let return_atom = self.intern_atom("return");
        let method = match self.get_property(p, receiver, return_atom) {
            Ok(method) => method,
            Err(error) => {
                let reason = self.thrown_value_for(p, error);
                let promise = self.promise_object();
                self.promise_settle(p, promise, PromiseState::Rejected, reason)?;
                return Ok(promise);
            }
        };
        if method.is_undefined() || method.is_null() {
            let promise = self.promise_object();
            self.promise_settle(p, promise, PromiseState::Fulfilled, Value::UNDEFINED)?;
            return Ok(promise);
        }
        if !self.is_function(method) {
            let error = self.type_error(p, "AsyncIterator return is not callable".into());
            let reason = self.thrown_value_for(p, error);
            let promise = self.promise_object();
            self.promise_settle(p, promise, PromiseState::Rejected, reason)?;
            return Ok(promise);
        }
        let result = match self.call_value(p, method, receiver, &[Value::UNDEFINED]) {
            Ok(result) => result,
            Err(error) => {
                let reason = self.thrown_value_for(p, error);
                let promise = self.promise_object();
                self.promise_settle(p, promise, PromiseState::Rejected, reason)?;
                return Ok(promise);
            }
        };
        let awaited = match self.promise_for_value(p, result) {
            Ok(promise) => promise,
            Err(error) => {
                let reason = self.thrown_value_for(p, error);
                let promise = self.promise_object();
                self.promise_settle(p, promise, PromiseState::Rejected, reason)?;
                return Ok(promise);
            }
        };
        self.promise_then(
            p,
            awaited,
            self.native_value(Native::AsyncIteratorDisposeFulfilled),
            Value::UNDEFINED,
        )
    }

    fn require_stack(&mut self, p: &ResidualProgram, stack: Value) -> Result<(), JsError> {
        let Some(Cell::Object(_)) = self.heap.get(stack) else {
            return Err(self.type_error(p, "DisposableStack receiver is invalid".into()));
        };
        let entries = self
            .lookup_atom(ENTRIES)
            .and_then(|atom| self.own_property(stack, atom))
            .filter(|value| matches!(self.heap.get(*value), Some(Cell::Array { .. })));
        let async_marker = self
            .lookup_atom(ASYNC_STACK)
            .and_then(|atom| self.own_property(stack, atom));
        if entries.is_none() || async_marker.is_none() {
            return Err(self.type_error(p, "DisposableStack receiver is invalid".into()));
        }
        Ok(())
    }

    fn require_async_stack(&mut self, p: &ResidualProgram, stack: Value) -> Result<(), JsError> {
        self.require_stack(p, stack)?;
        let async_stack = self
            .lookup_atom(ASYNC_STACK)
            .and_then(|atom| self.own_property(stack, atom))
            .is_some_and(|value| self.truthy(value));
        if async_stack {
            Ok(())
        } else {
            Err(self.type_error(p, "AsyncDisposableStack receiver is invalid".into()))
        }
    }

    fn require_open_stack(&mut self, p: &ResidualProgram, stack: Value) -> Result<(), JsError> {
        self.require_stack(p, stack)?;
        let disposed = self
            .lookup_atom(DISPOSED)
            .and_then(|atom| self.own_property(stack, atom))
            .is_some_and(|value| self.truthy(value));
        if disposed {
            return Err(self.reference_error(p, "DisposableStack is already disposed".into()));
        }
        Ok(())
    }

    fn stack_disposed(&mut self, stack: Value) -> Result<Value, JsError> {
        let atom = self.intern_atom(DISPOSED);
        Ok(self
            .own_property(stack, atom)
            .filter(|value| self.truthy(*value))
            .map_or(Value::FALSE, |_| Value::TRUE))
    }

    fn stack_move(
        &mut self,
        p: &ResidualProgram,
        stack: Value,
        async_stack: bool,
    ) -> Result<Value, JsError> {
        self.require_open_stack(p, stack)?;
        let entries = self.stack_entries(stack)?;
        let moved = match self.heap.get_mut(entries) {
            Some(Cell::Array { elements, .. }) => std::mem::replace(elements, Rc::new(vec![])),
            _ => return Err(JsError("DisposableStack entries are invalid".into())),
        };
        let disposed_atom = self.intern_atom(DISPOSED);
        self.set_property(stack, disposed_atom, Value::TRUE)?;
        let constructor = self.native_value(if async_stack {
            Native::AsyncDisposableStack
        } else {
            Native::DisposableStack
        });
        let prototype_atom = self.intern_atom("prototype");
        let prototype = self
            .own_property(constructor, prototype_atom)
            .unwrap_or(self.object_proto);
        let moved_stack = self.heap.alloc(Cell::Object(Self::empty_object(prototype)));
        let root = self.heap.root(moved_stack);
        let entries = self.heap.alloc(Cell::Array {
            object: Self::empty_object(self.array_proto),
            elements: moved,
        });
        let entries_atom = self.intern_atom(ENTRIES);
        self.set_property(moved_stack, entries_atom, entries)?;
        self.set_property(moved_stack, disposed_atom, Value::FALSE)?;
        let async_atom = self.intern_atom(ASYNC_STACK);
        self.set_property(
            moved_stack,
            async_atom,
            if async_stack { Value::TRUE } else { Value::FALSE },
        )?;
        self.heap.release_root(root);
        Ok(moved_stack)
    }

    fn stack_use(
        &mut self,
        p: &ResidualProgram,
        stack: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let value = args.first().copied().unwrap_or(Value::UNDEFINED);
        if is_nullish(value) {
            return Ok(value);
        }
        if !self.is_object_like(value) {
            return Err(self.type_error(p, "using value is not an object".into()));
        }
        let symbol = self
            .well_known_symbols
            .get("dispose")
            .copied()
            .ok_or_else(|| JsError("Symbol.dispose is unavailable".into()))?;
        let callback = self.get_index(p, value, symbol)?;
        if !self.is_function(callback) {
            return Err(self.type_error(p, "dispose method is not callable".into()));
        }
        self.push_stack_entry(stack, callback, value, DISPOSAL_USE_MODE, false)?;
        Ok(value)
    }

    fn stack_use_async(
        &mut self,
        p: &ResidualProgram,
        stack: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let value = args.first().copied().unwrap_or(Value::UNDEFINED);
        if is_nullish(value) {
            self.push_stack_entry(
                stack,
                Value::UNDEFINED,
                Value::UNDEFINED,
                DISPOSAL_AWAIT_MODE,
                true,
            )?;
            return Ok(value);
        }
        if !self.is_object_like(value) {
            return Err(self.type_error(p, "await using value is not an object".into()));
        }
        let async_symbol = self
            .well_known_symbols
            .get("asyncDispose")
            .copied()
            .ok_or_else(|| JsError("Symbol.asyncDispose is unavailable".into()))?;
        let dispose_symbol = self
            .well_known_symbols
            .get("dispose")
            .copied()
            .ok_or_else(|| JsError("Symbol.dispose is unavailable".into()))?;
        let callback = self.get_index(p, value, async_symbol)?;
        let callback = if callback.is_undefined() || callback.is_null() {
            self.get_index(p, value, dispose_symbol)?
        } else {
            callback
        };
        if !self.is_function(callback) {
            return Err(self.type_error(p, "async dispose method is not callable".into()));
        }
        self.push_stack_entry(stack, callback, value, DISPOSAL_USE_MODE, true)?;
        Ok(value)
    }

    fn stack_adopt(
        &mut self,
        p: &ResidualProgram,
        stack: Value,
        args: &[Value],
        await_result: bool,
    ) -> Result<Value, JsError> {
        let value = args.first().copied().unwrap_or(Value::UNDEFINED);
        let callback = args.get(1).copied().unwrap_or(Value::UNDEFINED);
        if !self.is_function(callback) {
            return Err(self.type_error(p, "DisposableStack.adopt callback is not callable".into()));
        }
        self.push_stack_entry(stack, callback, value, DISPOSAL_ADOPT_MODE, await_result)?;
        Ok(value)
    }

    fn stack_defer(
        &mut self,
        p: &ResidualProgram,
        stack: Value,
        args: &[Value],
        await_result: bool,
    ) -> Result<Value, JsError> {
        let callback = args.first().copied().unwrap_or(Value::UNDEFINED);
        if !self.is_function(callback) {
            return Err(self.type_error(p, "DisposableStack.defer callback is not callable".into()));
        }
        self.push_stack_entry(
            stack,
            callback,
            Value::UNDEFINED,
            DISPOSAL_DEFER_MODE,
            await_result,
        )?;
        Ok(Value::UNDEFINED)
    }

    fn push_stack_entry(
        &mut self,
        stack: Value,
        callback: Value,
        value: Value,
        mode: i32,
        await_result: bool,
    ) -> Result<(), JsError> {
        let entries = self.stack_entries(stack)?;
        let entry = self.heap.alloc(Cell::Array {
            object: Self::empty_object(self.array_proto),
            elements: Rc::new(vec![
                callback,
                value,
                Value::integer(mode),
                if await_result { Value::TRUE } else { Value::FALSE },
            ]),
        });
        let Some(Cell::Array { elements, .. }) = self.heap.get_mut(entries) else {
            return Err(JsError("DisposableStack entries are invalid".into()));
        };
        let mut values = (**elements).clone();
        values.push(entry);
        *elements = Rc::new(values);
        Ok(())
    }

    fn stack_dispose(&mut self, p: &ResidualProgram, stack: Value) -> Result<Value, JsError> {
        self.stack_dispose_resources(p, stack, None)
    }

    fn stack_dispose_with_completion(
        &mut self,
        p: &ResidualProgram,
        stack: Value,
        completion: Value,
    ) -> Result<Value, JsError> {
        self.stack_dispose_resources(p, stack, Some(completion))
    }

    fn stack_dispose_resources(
        &mut self,
        p: &ResidualProgram,
        stack: Value,
        mut completion: Option<Value>,
    ) -> Result<Value, JsError> {
        let disposed_atom = self.intern_atom(DISPOSED);
        if self.truthy(
            self.own_property(stack, disposed_atom)
                .unwrap_or(Value::FALSE),
        ) {
            return completion.map_or(Ok(Value::UNDEFINED), |value| {
                Err(JsError::thrown(value, "Error".into()))
            });
        }
        self.set_property(stack, disposed_atom, Value::TRUE)?;
        let entries = self.stack_entries(stack)?;
        let values = match self.heap.get_mut(entries) {
            Some(Cell::Array { elements, .. }) => std::mem::replace(elements, Rc::new(vec![])),
            _ => return Err(JsError("DisposableStack entries are invalid".into())),
        };
        for entry in values.iter().rev().copied() {
            let Some(Cell::Array { elements, .. }) = self.heap.get(entry) else {
                continue;
            };
            let callback = elements
                .get(DISPOSAL_ENTRY_CALLBACK_SLOT)
                .copied()
                .unwrap_or(Value::UNDEFINED);
            let value = elements
                .get(DISPOSAL_ENTRY_RECEIVER_SLOT)
                .copied()
                .unwrap_or(Value::UNDEFINED);
            let mode = elements
                .get(DISPOSAL_ENTRY_MODE_SLOT)
                .and_then(|value| value.as_int())
                .unwrap_or(DISPOSAL_INVALID_MODE);
            let result = match mode {
                DISPOSAL_USE_MODE => self.call_value(p, callback, value, &[]),
                DISPOSAL_ADOPT_MODE => {
                    self.call_value(p, callback, Value::UNDEFINED, &[value])
                }
                DISPOSAL_DEFER_MODE => {
                    self.call_value(p, callback, Value::UNDEFINED, &[])
                }
                _ => return Err(JsError("DisposableStack entry mode is invalid".into())),
            };
            if let Err(error) = result {
                let error = self.thrown_value_for(p, error);
                completion = Some(match completion {
                    Some(suppressed) => self.construct_error_native(
                        p,
                        Native::SuppressedError,
                        &[error, suppressed],
                    )?,
                    None => error,
                });
            }
        }
        completion.map_or(Ok(Value::UNDEFINED), |value| {
            Err(JsError::thrown(value, "Error".into()))
        })
    }

    fn stack_dispose_async(&mut self, p: &ResidualProgram, stack: Value) -> Result<Value, JsError> {
        self.begin_async_disposal(p, stack, Value::DELETED)
    }

    fn stack_dispose_async_native(
        &mut self,
        p: &ResidualProgram,
        stack: Value,
    ) -> Result<Value, JsError> {
        if let Err(error) = self.require_async_stack(p, stack) {
            let reason = self.thrown_value_for(p, error);
            let promise = self.promise_object();
            self.promise_settle(p, promise, PromiseState::Rejected, reason)?;
            return Ok(promise);
        }
        self.stack_dispose_async(p, stack)
    }

    fn stack_dispose_async_with_completion(
        &mut self,
        p: &ResidualProgram,
        stack: Value,
        completion: Value,
    ) -> Result<Value, JsError> {
        self.begin_async_disposal(p, stack, completion)
    }

    fn begin_async_disposal(
        &mut self,
        p: &ResidualProgram,
        stack: Value,
        completion: Value,
    ) -> Result<Value, JsError> {
        let disposed_atom = self.intern_atom(DISPOSED);
        if self.truthy(
            self.own_property(stack, disposed_atom)
                .unwrap_or(Value::FALSE),
        ) {
            return self.disposal_result_promise(p, completion);
        }
        self.set_property(stack, disposed_atom, Value::TRUE)?;
        let entries = self.stack_entries(stack)?;
        let values = match self.heap.get_mut(entries) {
            Some(Cell::Array { elements, .. }) => std::mem::replace(elements, Rc::new(vec![])),
            _ => return Err(JsError("DisposableStack entries are invalid".into())),
        };
        let resource_count = values.len();
        let resources = self.heap.alloc(Cell::Array {
            object: Self::empty_object(self.array_proto),
            elements: values,
        });
        let result = self.promise_object();
        let state = self.heap.alloc(Cell::Array {
            object: Self::empty_object(self.array_proto),
            elements: Rc::new(vec![
                resources,
                Value::number(resource_count as f64),
                completion,
                result,
            ]),
        });
        self.continue_async_disposal(p, state)?;
        Ok(result)
    }

    fn async_disposal_state_value(&self, state: Value, slot: usize) -> Value {
        match self.heap.get(state) {
            Some(Cell::Array { elements, .. }) => elements.get(slot).copied(),
            _ => None,
        }
        .unwrap_or(Value::UNDEFINED)
    }

    fn set_async_disposal_state_value(&mut self, state: Value, slot: usize, value: Value) {
        if let Some(Cell::Array { elements, .. }) = self.heap.get_mut(state) {
            let mut updated = (**elements).clone();
            updated[slot] = value;
            *elements = Rc::new(updated);
        }
    }

    fn add_async_disposal_error(
        &mut self,
        p: &ResidualProgram,
        state: Value,
        error: Value,
    ) -> Result<(), JsError> {
        let completion = self.async_disposal_state_value(state, ASYNC_DISPOSAL_COMPLETION_SLOT);
        let completion = if completion.is_deleted() {
            error
        } else {
            self.construct_error_native(p, Native::SuppressedError, &[error, completion])?
        };
        self.set_async_disposal_state_value(
            state,
            ASYNC_DISPOSAL_COMPLETION_SLOT,
            completion,
        );
        Ok(())
    }

    fn continue_async_disposal(
        &mut self,
        p: &ResidualProgram,
        state: Value,
    ) -> Result<Value, JsError> {
        loop {
            let cursor = self
                .async_disposal_state_value(state, ASYNC_DISPOSAL_CURSOR_SLOT)
                .as_number()
                .expect("async disposal cursor is numeric") as usize;
            if cursor == 0 {
                let completion =
                    self.async_disposal_state_value(state, ASYNC_DISPOSAL_COMPLETION_SLOT);
                let result = self.async_disposal_state_value(state, ASYNC_DISPOSAL_RESULT_SLOT);
                if completion.is_deleted() {
                    self.promise_settle(p, result, PromiseState::Fulfilled, Value::UNDEFINED)?;
                } else {
                    self.promise_settle(p, result, PromiseState::Rejected, completion)?;
                }
                return Ok(Value::UNDEFINED);
            }

            let cursor = cursor - 1;
            self.set_async_disposal_state_value(
                state,
                ASYNC_DISPOSAL_CURSOR_SLOT,
                Value::number(cursor as f64),
            );
            let resources = self.async_disposal_state_value(state, ASYNC_DISPOSAL_ENTRIES_SLOT);
            let entry = match self.heap.get(resources) {
                Some(Cell::Array { elements, .. }) => elements[cursor],
                _ => return Err(JsError("DisposableStack entries are invalid".into())),
            };
            let (callback, value, mode, await_result) = match self.heap.get(entry) {
                Some(Cell::Array { elements, .. }) => (
                    elements
                        .get(DISPOSAL_ENTRY_CALLBACK_SLOT)
                        .copied()
                        .unwrap_or(Value::UNDEFINED),
                    elements
                        .get(DISPOSAL_ENTRY_RECEIVER_SLOT)
                        .copied()
                        .unwrap_or(Value::UNDEFINED),
                    elements
                        .get(DISPOSAL_ENTRY_MODE_SLOT)
                        .and_then(|value| value.as_int())
                        .unwrap_or(DISPOSAL_INVALID_MODE),
                    elements
                        .get(DISPOSAL_ENTRY_AWAIT_RESULT_SLOT)
                        .is_some_and(|value| self.truthy(*value)),
                ),
                _ => continue,
            };
            let result = match mode {
                DISPOSAL_USE_MODE => self.call_value(p, callback, value, &[]),
                DISPOSAL_ADOPT_MODE => {
                    self.call_value(p, callback, Value::UNDEFINED, &[value])
                }
                DISPOSAL_DEFER_MODE => {
                    self.call_value(p, callback, Value::UNDEFINED, &[])
                }
                DISPOSAL_AWAIT_MODE => Ok(Value::UNDEFINED),
                _ => return Err(JsError("DisposableStack entry mode is invalid".into())),
            };
            match result {
                Err(error) => {
                    let error = self.thrown_value_for(p, error);
                    self.add_async_disposal_error(p, state, error)?;
                }
                Ok(value) if await_result => {
                    let promise = self.promise_for_value(p, value)?;
                    let fulfilled = self.native_with_env(
                        Native::DisposableStackAsyncDisposalFulfilled,
                        state,
                    );
                    let rejected = self.native_with_env(
                        Native::DisposableStackAsyncDisposalRejected,
                        state,
                    );
                    let _ = self.call_promise_native(
                        p,
                        Native::PromiseThen,
                        promise,
                        &[fulfilled, rejected],
                    )?;
                    return Ok(Value::UNDEFINED);
                }
                Ok(_) => {}
            }
        }
    }

    fn disposal_result_promise(
        &mut self,
        p: &ResidualProgram,
        completion: Value,
    ) -> Result<Value, JsError> {
        let promise = self.promise_object();
        let (state, value) = if completion.is_deleted() {
            (PromiseState::Fulfilled, Value::UNDEFINED)
        } else {
            (PromiseState::Rejected, completion)
        };
        self.promise_settle(p, promise, state, value)?;
        Ok(promise)
    }

    fn stack_entries(&mut self, stack: Value) -> Result<Value, JsError> {
        let atom = self.intern_atom(ENTRIES);
        self.own_property(stack, atom)
            .filter(|value| matches!(self.heap.get(*value), Some(Cell::Array { .. })))
            .ok_or_else(|| JsError("DisposableStack entries are invalid".into()))
    }
}

fn is_nullish(value: Value) -> bool {
    value.is_null() || value.is_undefined()
}
