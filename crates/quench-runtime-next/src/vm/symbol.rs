use super::*;

impl<H: Host> Vm<H> {
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
                if !matches!(self.heap.get(this), Some(Cell::Symbol(_))) {
                    return Err(JsError("Symbol method receiver is not a symbol".into()));
                }
                let text = self.to_string(p, this)?;
                Ok(self.heap.alloc(Cell::String(text.into())))
            }
            Native::SymbolValueOf => {
                if matches!(self.heap.get(this), Some(Cell::Symbol(_))) {
                    Ok(this)
                } else {
                    let value_atom = self.intern_atom("\0rqj:symbol-value");
                    self.own_property(this, value_atom).ok_or_else(|| {
                        JsError("Symbol.prototype.valueOf called on incompatible receiver".into())
                    })
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
                    return Err(JsError("symbol keyFor argument is not a symbol".into()));
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
