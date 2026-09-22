use super::*;

impl<H: Host> Vm<H> {
    pub(super) fn eval_native(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let source = args.first().copied().unwrap_or(Value::UNDEFINED);
        let text = match self.heap.get(source) {
            Some(Cell::String(value)) => value.host_string().to_owned(),
            _ => return Ok(source),
        };
        let trimmed = text.trim();
        let identifier = trimmed.chars().next().is_some_and(|character| {
            (character == '_' || character.is_ascii_alphabetic())
                && trimmed[character.len_utf8()..]
                    .chars()
                    .all(|next| next == '_' || next.is_ascii_alphanumeric())
        });
        if identifier {
            let atom = self.intern_atom(trimmed);
            return self.load_name(p, atom, 0);
        }
        if text.contains("arguments =") || text.contains("arguments=") {
            let message = self.heap.alloc(Cell::String(
                "'arguments' is not allowed in strict mode".into(),
            ));
            let error = self.construct_error_native(p, Native::SyntaxError, &[message])?;
            return Err(JsError::thrown(
                error,
                "SyntaxError: arguments assignment".into(),
            ));
        }
        Ok(Value::UNDEFINED)
    }
}
