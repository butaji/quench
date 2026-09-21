use super::*;

impl<H: Host> Vm<H> {
    pub(super) fn string_split_regexp_native(
        &mut self,
        p: &ResidualProgram,
        this: Value,
        separator: Value,
        limit: usize,
    ) -> Result<Value, JsError> {
        let Some(Cell::String(receiver)) = self.heap.get(this).cloned() else {
            return Err(JsError("string method receiver is not a string".into()));
        };
        let source_atom = self.intern_atom("source");
        let flags_atom = self.intern_atom("flags");
        let source = self.to_string(p, self.get_property(p, separator, source_atom)?)?;
        let flags = self.to_string(p, self.get_property(p, separator, flags_atom)?)?;
        let regex = Self::compile_regexp(&source, &flags)?;
        let mut values = Vec::new();
        let mut cursor = 0;
        for captures in regex.captures_iter(&receiver) {
            let Some(whole) = captures.get(0) else {
                continue;
            };
            values.push(
                self.heap
                    .alloc(Cell::String(receiver[cursor..whole.start()].to_owned())),
            );
            for capture in captures.iter().skip(1) {
                values.push(capture.map_or(Value::UNDEFINED, |value| {
                    self.heap.alloc(Cell::String(value.as_str().into()))
                }));
            }
            cursor = whole.end();
            if values.len() >= limit {
                break;
            }
        }
        if values.len() < limit {
            values.push(self.heap.alloc(Cell::String(receiver[cursor..].to_owned())));
        }
        values.truncate(limit);
        Ok(self.heap.alloc(Cell::Array {
            object: Self::empty_object(self.array_proto),
            elements: Rc::new(values),
        }))
    }

    pub(super) fn string_replace_native(
        &mut self,
        p: &ResidualProgram,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let Some(Cell::String(receiver)) = self.heap.get(this).cloned() else {
            return Err(JsError("string method receiver is not a string".into()));
        };
        let replacement = self.to_string(p, args.get(1).copied().unwrap_or(Value::UNDEFINED))?;
        let search_value = args.first().copied().unwrap_or(Value::UNDEFINED);
        if self.is_regexp(search_value) {
            let source_atom = self.intern_atom("source");
            let flags_atom = self.intern_atom("flags");
            let source = self.to_string(p, self.get_property(p, search_value, source_atom)?)?;
            let flags = self.to_string(p, self.get_property(p, search_value, flags_atom)?)?;
            let regex = Self::compile_regexp(&source, &flags)?;
            let global = flags.contains('g');
            let mut result = String::with_capacity(receiver.len());
            let mut cursor = 0;
            let mut replaced = false;
            for captures in regex.captures_iter(&receiver) {
                if replaced && !global {
                    break;
                }
                let Some(whole) = captures.get(0) else {
                    continue;
                };
                result.push_str(&receiver[cursor..whole.start()]);
                let mut replacement_text = replacement.clone();
                replacement_text = replacement_text.replace("$&", whole.as_str());
                for (index, capture) in captures.iter().enumerate().skip(1) {
                    let token = format!("${index}");
                    replacement_text = replacement_text
                        .replace(&token, capture.map_or("", |value| value.as_str()));
                }
                result.push_str(&replacement_text);
                cursor = whole.end();
                replaced = true;
            }
            if !replaced {
                return Ok(self.heap.alloc(Cell::String(receiver)));
            }
            result.push_str(&receiver[cursor..]);
            return Ok(self.heap.alloc(Cell::String(result)));
        }
        let search = self.to_string(p, search_value)?;
        let Some(index) = receiver.find(&search) else {
            return Ok(self.heap.alloc(Cell::String(receiver)));
        };
        let replacement = replacement.replace("$&", &search);
        let mut result =
            String::with_capacity(receiver.len() + replacement.len().saturating_sub(search.len()));
        result.push_str(&receiver[..index]);
        result.push_str(&replacement);
        result.push_str(&receiver[index + search.len()..]);
        Ok(self.heap.alloc(Cell::String(result)))
    }
}
