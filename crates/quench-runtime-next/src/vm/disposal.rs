use super::promise::PromiseState;

const ENTRIES: &str = "\0rqj:disposable-stack:entries";
const DISPOSED: &str = "\0rqj:disposable-stack:disposed";

impl<H: Host> Vm<H> {
    pub(super) fn is_disposal_native(native: Native) -> bool {
        matches!(
            native,
            Native::DisposableStack
                | Native::DisposableStackUse
                | Native::DisposableStackAdopt
                | Native::DisposableStackDefer
                | Native::DisposableStackDispose
                | Native::DisposableStackUseAsync
                | Native::DisposableStackDisposeAsync
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
            self.set_named(
                p,
                prototype,
                name,
                self.native_value(native),
            )?;
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
        self.set_named(p, constructor, "prototype", prototype)?;
        self.global(p, "DisposableStack", constructor)
    }

    pub(super) fn construct_disposable_stack_native(
        &mut self,
        _p: &ResidualProgram,
    ) -> Result<Value, JsError> {
        let constructor = self.native_value(Native::DisposableStack);
        let prototype_atom = self.intern_atom("prototype");
        let prototype = self
            .own_property(constructor, prototype_atom)
            .unwrap_or(self.object_proto);
        let stack = self
            .heap
            .alloc(Cell::Object(Self::empty_object(prototype)));
        let root = self.heap.root(stack);
        let entries = self.heap.alloc(Cell::Array {
            object: Self::empty_object(self.array_proto),
            elements: Rc::new(vec![]),
        });
        let entries_atom = self.intern_atom(ENTRIES);
        let disposed_atom = self.intern_atom(DISPOSED);
        self.set_property(stack, entries_atom, entries)?;
        self.set_property(stack, disposed_atom, Value::FALSE)?;
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
        if native == Native::DisposableStackDispose {
            self.require_stack(this)?;
            return self.stack_dispose(p, this);
        }
        if native == Native::DisposableStackDisposeAsync {
            self.require_stack(this)?;
            return self.stack_dispose_async(p, this);
        }
        self.require_open_stack(this)?;
        match native {
            Native::DisposableStackUse => self.stack_use(p, this, args),
            Native::DisposableStackUseAsync => self.stack_use_async(p, this, args),
            Native::DisposableStackAdopt => self.stack_adopt(this, args),
            Native::DisposableStackDefer => self.stack_defer(this, args),
            _ => Err(JsError("invalid disposal native".into())),
        }
    }

    fn require_stack(&self, stack: Value) -> Result<(), JsError> {
        let Some(Cell::Object(_)) = self.heap.get(stack) else {
            return Err(JsError("DisposableStack receiver is invalid".into()));
        };
        let entries = self
            .lookup_atom(ENTRIES)
            .and_then(|atom| self.own_property(stack, atom))
            .filter(|value| matches!(self.heap.get(*value), Some(Cell::Array { .. })));
        if entries.is_none() {
            return Err(JsError("DisposableStack receiver is invalid".into()));
        }
        Ok(())
    }

    fn require_open_stack(&self, stack: Value) -> Result<(), JsError> {
        self.require_stack(stack)?;
        let disposed = self
            .lookup_atom(DISPOSED)
            .and_then(|atom| self.own_property(stack, atom))
            .is_some_and(|value| self.truthy(value));
        if disposed {
            return Err(JsError("DisposableStack is already disposed".into()));
        }
        Ok(())
    }

    fn stack_use(
        &mut self,
        p: &ResidualProgram,
        stack: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let value = args.first().copied().unwrap_or(Value::UNDEFINED);
        let symbol = self
            .well_known_symbols
            .get("dispose")
            .copied()
            .ok_or_else(|| JsError("Symbol.dispose is unavailable".into()))?;
        let callback = self.get_index(p, value, symbol)?;
        if !self.is_function(callback) {
            return Err(JsError(
                "disposable value has no callable dispose method".into(),
            ));
        }
        self.push_stack_entry(stack, callback, value, 0)?;
        Ok(value)
    }

    fn stack_use_async(
        &mut self,
        p: &ResidualProgram,
        stack: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let value = args.first().copied().unwrap_or(Value::UNDEFINED);
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
            return Err(JsError(
                "disposable value has no callable async dispose method".into(),
            ));
        }
        self.push_stack_entry(stack, callback, value, 0)?;
        Ok(value)
    }

    fn stack_adopt(&mut self, stack: Value, args: &[Value]) -> Result<Value, JsError> {
        let value = args.first().copied().unwrap_or(Value::UNDEFINED);
        let callback = args.get(1).copied().unwrap_or(Value::UNDEFINED);
        if !self.is_function(callback) {
            return Err(JsError(
                "DisposableStack.adopt callback is not callable".into(),
            ));
        }
        self.push_stack_entry(stack, callback, value, 1)?;
        Ok(value)
    }

    fn stack_defer(&mut self, stack: Value, args: &[Value]) -> Result<Value, JsError> {
        let callback = args.first().copied().unwrap_or(Value::UNDEFINED);
        if !self.is_function(callback) {
            return Err(JsError(
                "DisposableStack.defer callback is not callable".into(),
            ));
        }
        self.push_stack_entry(stack, callback, Value::UNDEFINED, 2)?;
        Ok(Value::UNDEFINED)
    }

    fn push_stack_entry(
        &mut self,
        stack: Value,
        callback: Value,
        value: Value,
        mode: i32,
    ) -> Result<(), JsError> {
        let entries = self.stack_entries(stack)?;
        let entry = self.heap.alloc(Cell::Array {
            object: Self::empty_object(self.array_proto),
            elements: Rc::new(vec![callback, value, Value::integer(mode)]),
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
        let disposed_atom = self.intern_atom(DISPOSED);
        if self.truthy(
            self.own_property(stack, disposed_atom)
                .unwrap_or(Value::FALSE),
        ) {
            return Ok(Value::UNDEFINED);
        }
        self.set_property(stack, disposed_atom, Value::TRUE)?;
        let entries = self.stack_entries(stack)?;
        let values = match self.heap.get_mut(entries) {
            Some(Cell::Array { elements, .. }) => std::mem::replace(elements, Rc::new(vec![])),
            _ => return Err(JsError("DisposableStack entries are invalid".into())),
        };
        let mut first_error = None;
        for entry in values.iter().rev().copied() {
            let Some(Cell::Array { elements, .. }) = self.heap.get(entry) else {
                continue;
            };
            let callback = elements.first().copied().unwrap_or(Value::UNDEFINED);
            let value = elements.get(1).copied().unwrap_or(Value::UNDEFINED);
            let mode = elements
                .get(2)
                .and_then(|value| value.as_int())
                .unwrap_or(0);
            let result = if mode == 0 {
                self.call_value(p, callback, value, &[])
            } else if mode == 1 {
                self.call_value(p, callback, Value::UNDEFINED, &[value])
            } else {
                self.call_value(p, callback, Value::UNDEFINED, &[])
            };
            if first_error.is_none() {
                first_error = result.err();
            }
        }
        first_error.map_or(Ok(Value::UNDEFINED), Err)
    }

    fn stack_dispose_async(&mut self, p: &ResidualProgram, stack: Value) -> Result<Value, JsError> {
        let disposed_atom = self.intern_atom(DISPOSED);
        if self.truthy(
            self.own_property(stack, disposed_atom)
                .unwrap_or(Value::FALSE),
        ) {
            return self.promise_for_value(p, Value::UNDEFINED);
        }
        self.set_property(stack, disposed_atom, Value::TRUE)?;
        let entries = self.stack_entries(stack)?;
        let values = match self.heap.get_mut(entries) {
            Some(Cell::Array { elements, .. }) => std::mem::replace(elements, Rc::new(vec![])),
            _ => return Err(JsError("DisposableStack entries are invalid".into())),
        };
        let mut results = Vec::with_capacity(values.len());
        let mut first_error = None;
        for entry in values.iter().rev().copied() {
            let Some(Cell::Array { elements, .. }) = self.heap.get(entry) else {
                continue;
            };
            let callback = elements.first().copied().unwrap_or(Value::UNDEFINED);
            let value = elements.get(1).copied().unwrap_or(Value::UNDEFINED);
            match self.call_value(p, callback, value, &[]) {
                Ok(result) => results.push(result),
                Err(error) => {
                    if first_error.is_none() {
                        first_error = Some(
                            error
                                .thrown_value()
                                .unwrap_or_else(|| self.heap.alloc(Cell::Error(error.into_message()))),
                        );
                    }
                }
            }
        }
        let array = self.heap.alloc(Cell::Array {
            object: Self::empty_object(self.array_proto),
            elements: Rc::new(results),
        });
        if let Some(reason) = first_error {
            let promise = self.promise_object();
            self.promise_settle(p, promise, PromiseState::Rejected, reason)?;
            return Ok(promise);
        }
        self.call_promise_native(p, Native::PromiseAll, Value::UNDEFINED, &[array])
    }

    fn stack_entries(&mut self, stack: Value) -> Result<Value, JsError> {
        let atom = self.intern_atom(ENTRIES);
        self.own_property(stack, atom)
            .filter(|value| matches!(self.heap.get(*value), Some(Cell::Array { .. })))
            .ok_or_else(|| JsError("DisposableStack entries are invalid".into()))
    }
}
