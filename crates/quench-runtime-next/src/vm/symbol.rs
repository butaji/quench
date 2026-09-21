use super::*;

impl<H: Host> Vm<H> {
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
                        (*candidate == value).then(|| self.heap.alloc(Cell::String(key.clone())))
                    })
                    .unwrap_or(Value::UNDEFINED))
            }
            _ => Err(JsError("invalid symbol native".into())),
        }
    }
}
