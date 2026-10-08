use super::*;

impl<H: Host> Vm<H> {
    pub(super) fn install_symbol_for_realm(
        &mut self,
        program: &ResidualProgram,
        global: Value,
        object_prototype: Value,
    ) -> Result<(), JsError> {
        let constructor = self.native_with_realm(Native::Symbol, global, global);
        self.set_builtin_function_name(constructor, "Symbol")?;
        let prototype = self
            .heap
            .alloc(Cell::Object(Self::empty_object(object_prototype)));
        self.set_named_constant(program, constructor, "prototype", prototype)?;
        self.set_builtin_value_named(prototype, "constructor", constructor)?;
        self.set_builtin_value_named(global, "Symbol", constructor)?;
        for (name, native) in [("for", Native::SymbolFor), ("keyFor", Native::SymbolKeyFor)] {
            let method = self.native_with_realm(native, global, global);
            self.set_builtin_function_name(method, name)?;
            self.set_builtin_value_named(constructor, name, method)?;
        }
        for (name, symbol) in self.well_known_symbols.clone() {
            self.set_named_constant(program, constructor, &name, symbol)?;
        }
        for (name, native) in [
            ("toString", Native::SymbolToString),
            ("valueOf", Native::SymbolValueOf),
        ] {
            let method = self.native_with_realm(native, global, global);
            self.set_builtin_function_name(method, name)?;
            self.set_builtin_value_named(prototype, name, method)?;
        }
        if let Some(to_primitive) = self.well_known_symbols.get("toPrimitive").copied() {
            let method = self.native_with_realm(Native::SymbolToPrimitive, global, global);
            self.set_builtin_function_name(method, "[Symbol.toPrimitive]")?;
            self.set_symbol_property(prototype, to_primitive, method)?;
            self.set_property_attributes(
                prototype,
                PropertyKey::symbol(to_primitive),
                PropertyAttributes {
                    writable: false,
                    enumerable: false,
                    configurable: true,
                    accessor: false,
                    getter: None,
                    setter: None,
                },
            );
        }
        if let Some(to_string_tag) = self.well_known_symbols.get("toStringTag").copied() {
            let tag = self.heap.alloc(Cell::String(JsString::from_str("Symbol")));
            self.set_symbol_property(prototype, to_string_tag, tag)?;
            self.set_property_attributes(
                prototype,
                PropertyKey::symbol(to_string_tag),
                PropertyAttributes {
                    writable: false,
                    enumerable: false,
                    configurable: true,
                    accessor: false,
                    getter: None,
                    setter: None,
                },
            );
        }
        Ok(())
    }

    pub(super) fn call_symbol_constructor(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let description = match args.first().copied() {
            None | Some(Value::UNDEFINED) => None,
            Some(value) => Some(self.to_string(p, value)?),
        };
        Ok(self.heap.alloc(Cell::Symbol(description)))
    }

    pub(super) fn call_symbol_value_native(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        this: Value,
    ) -> Result<Value, JsError> {
        match native {
            Native::SymbolToString => {
                let value = if matches!(self.heap.get(this), Some(Cell::Symbol(_))) {
                    this
                } else {
                    let value_atom = self.intern_atom("\0quench:symbol-value");
                    self.own_property(this, value_atom)
                        .filter(|value| matches!(self.heap.get(*value), Some(Cell::Symbol(_))))
                        .ok_or_else(|| {
                            self.type_error(p, "Symbol method receiver is not a symbol".into())
                        })?
                };
                let Some(Cell::Symbol(description)) = self.heap.get(value) else {
                    unreachable!("symbol value was checked above")
                };
                let text = format!("Symbol({})", description.as_deref().unwrap_or(""));
                Ok(self.heap.alloc(Cell::String(text.into())))
            }
            Native::SymbolValueOf | Native::SymbolToPrimitive => {
                if matches!(self.heap.get(this), Some(Cell::Symbol(_))) {
                    Ok(this)
                } else {
                    let value_atom = self.intern_atom("\0quench:symbol-value");
                    self.own_property(this, value_atom).ok_or_else(|| {
                        self.type_error(
                            p,
                            "Symbol.prototype.valueOf called on incompatible receiver".into(),
                        )
                    })
                }
            }
            Native::SymbolDescriptionGetter => {
                let symbol = if matches!(self.heap.get(this), Some(Cell::Symbol(_))) {
                    this
                } else {
                    let value_atom = self.intern_atom("\0quench:symbol-value");
                    self.own_property(this, value_atom)
                        .filter(|value| matches!(self.heap.get(*value), Some(Cell::Symbol(_))))
                        .ok_or_else(|| {
                            self.type_error(
                                p,
                                "Symbol.prototype.description called on incompatible receiver"
                                    .into(),
                            )
                        })?
                };
                match self.heap.get(symbol) {
                    Some(Cell::Symbol(Some(description))) => Ok(self
                        .heap
                        .alloc(Cell::String(JsString::from_str(description)))),
                    Some(Cell::Symbol(None)) => Ok(Value::UNDEFINED),
                    _ => unreachable!("symbol receiver was validated"),
                }
            }
            _ => Err(JsError("invalid Symbol method".into())),
        }
    }

    pub(super) fn call_symbol_native(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        args: &[Value],
    ) -> Result<Value, JsError> {
        match native {
            Native::SymbolFor => {
                let key = self.to_string(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
                if let Some(value) = self.symbol_registry.get(&key).copied() {
                    return Ok(value);
                }
                let value = self.heap.alloc(Cell::Symbol(Some(key.clone())));
                self.symbol_registry.insert(key, value);
                Ok(value)
            }
            Native::SymbolKeyFor => {
                let value = args.first().copied().unwrap_or(Value::UNDEFINED);
                if !matches!(self.heap.get(value), Some(Cell::Symbol(_))) {
                    return Err(self.type_error(p, "symbol keyFor argument is not a symbol".into()));
                }
                Ok(self
                    .symbol_registry
                    .iter()
                    .find_map(|(key, candidate)| {
                        (*candidate == value)
                            .then(|| self.heap.alloc(Cell::String(key.clone().into())))
                    })
                    .unwrap_or(Value::UNDEFINED))
            }
            _ => Err(JsError("invalid symbol native".into())),
        }
    }
}
