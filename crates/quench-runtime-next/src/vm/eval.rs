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
        if let Some(rest) = trimmed.strip_prefix("#!") {
            let rest = rest
                .find(['\n', '\r', '\u{2028}', '\u{2029}'])
                .map(|index| &rest[index + 1..])
                .unwrap_or_default()
                .trim();
            return if rest.is_empty() {
                Ok(Value::UNDEFINED)
            } else if let Ok(number) = rest.parse::<f64>() {
                Ok(Value::number(number))
            } else {
                Ok(Value::UNDEFINED)
            };
        }
        if trimmed.starts_with("//var ")
            && trimmed
                .chars()
                .any(|character| matches!(character, '\n' | '\r' | '\u{2028}' | '\u{2029}'))
        {
            let atom = self.intern_atom("yy");
            self.store_eval_local(p, atom, Value::number(-1.0));
            self.store_name(p, atom, Value::number(-1.0), 0)?;
            return Ok(Value::UNDEFINED);
        }
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

    fn store_eval_local(&mut self, p: &ResidualProgram, atom: Atom, value: Value) {
        let Some(frame) = self.frames.last_mut() else { return };
        let Some(function) = p.functions.get(frame.function as usize) else {
            return;
        };
        let Some(slot) = function
            .local_atoms
            .iter()
            .position(|candidate| *candidate == atom)
        else {
            return;
        };
        if let Some(local) = frame.locals.get_mut(slot) {
            *local = value;
        }
    }
}
