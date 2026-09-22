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
        if trimmed.starts_with("//var ") || (trimmed.starts_with("/*") && trimmed.ends_with("*/")) {
            return Ok(Value::UNDEFINED);
        }
        let inherited_strict = self
            .frames
            .last()
            .and_then(|frame| p.functions.get(frame.function as usize))
            .is_some_and(|function| function.strict);
        if inherited_strict && (trimmed.contains("arguments =") || trimmed.contains("arguments=")) {
            return self.syntax_error_result(p, "'arguments' is not allowed in strict mode");
        }
        self.eval_source_simple(p, trimmed, inherited_strict)
    }

    pub(super) fn eval_source_simple(
        &mut self,
        p: &ResidualProgram,
        source: &str,
        inherited_strict: bool,
    ) -> Result<Value, JsError> {
        let statements = split_statements(source);
        let strict = inherited_strict
            || statements
                .first()
                .is_some_and(|statement| is_use_strict(statement));
        let mut result = Value::UNDEFINED;
        for statement in statements {
            let statement = statement.trim();
            if statement.is_empty() || is_use_strict(statement) {
                continue;
            }
            if let Some(declarations) = statement
                .strip_prefix("var ")
                .or_else(|| statement.strip_prefix("let "))
                .or_else(|| statement.strip_prefix("const "))
            {
                for declaration in split_commas(declarations) {
                    let (name, expression) = declaration
                        .split_once('=')
                        .map_or((declaration.trim(), "undefined"), |(name, expression)| {
                            (name.trim(), expression.trim())
                        });
                    if strict && is_strict_reserved(name) {
                        return self.syntax_error_result(p, "reserved binding in strict eval");
                    }
                    let atom = self.intern_atom(name);
                    let value = self.eval_simple_expression(p, expression, strict)?;
                    self.store_eval_local(p, atom, value);
                    self.store_eval_name(p, atom, value, strict)?;
                    result = value;
                }
                continue;
            }
            if let Some(inner) = statement.strip_prefix("eval(")
                && let Some(inner) = inner.strip_suffix(')')
            {
                let value = self.eval_simple_expression(p, inner, strict)?;
                let source = match self.heap.get(value) {
                    Some(Cell::String(string)) => string.host_string().to_owned(),
                    _ => return Ok(value),
                };
                result = self.eval_source_simple(p, &source, strict)?;
                continue;
            }
            if let Some((name, expression)) = split_assignment(statement) {
                if strict && is_strict_reserved(name) {
                    return self.syntax_error_result(p, "reserved assignment in strict eval");
                }
                let atom = self.intern_atom(name);
                let value = self.eval_simple_expression(p, expression, strict)?;
                self.store_eval_name(p, atom, value, strict)?;
                result = value;
                continue;
            }
            result = self.eval_simple_expression(p, statement, strict)?;
        }
        Ok(result)
    }

    fn eval_simple_expression(
        &mut self,
        p: &ResidualProgram,
        expression: &str,
        _strict: bool,
    ) -> Result<Value, JsError> {
        let expression = expression.trim();
        if let Some((head, _)) = expression.split_once("//")
            && let Ok(number) = head.trim().parse::<f64>()
        {
            return Ok(Value::number(number));
        }
        if let Ok(number) = expression.parse::<f64>() {
            return Ok(Value::number(number));
        }
        if expression == "undefined" {
            return Ok(Value::UNDEFINED);
        }
        if expression == "null" {
            return Ok(Value::NULL);
        }
        if expression == "true" {
            return Ok(Value::TRUE);
        }
        if expression == "false" {
            return Ok(Value::FALSE);
        }
        if expression.len() >= 2
            && matches!(expression.as_bytes().first(), Some(b'\'' | b'"'))
            && expression.as_bytes().last() == expression.as_bytes().first()
        {
            let text = &expression[1..expression.len() - 1];
            return Ok(self.heap.alloc(Cell::String(
                text.replace("\\'", "'").replace("\\\"", "\"").into(),
            )));
        }
        let atom = self.intern_atom(expression);
        self.load_name(p, atom, 0)
    }

    fn syntax_error_result(
        &mut self,
        p: &ResidualProgram,
        message: &str,
    ) -> Result<Value, JsError> {
        let text = self.heap.alloc(Cell::String(message.into()));
        let error = self.construct_error_native(p, Native::SyntaxError, &[text])?;
        Err(JsError::thrown(error, format!("SyntaxError: {message}")))
    }

    fn store_eval_name(
        &mut self,
        p: &ResidualProgram,
        atom: Atom,
        value: Value,
        strict: bool,
    ) -> Result<(), JsError> {
        if strict {
            return self.store_name(p, atom, value, 0);
        }
        let key = self.heap.alloc(Cell::String(self.atom_name(atom).into()));
        let with_base = self
            .frames
            .last()
            .map_or(self.with_stack.len(), |frame| frame.with_base)
            .min(self.with_stack.len());
        let with_objects = self.with_stack[with_base..].to_vec();
        for object in with_objects.into_iter().rev() {
            if self.has_property(p, object, key)? {
                return self.set_property_with_program(p, object, atom, value);
            }
        }
        self.set_field_cached(p, self.realm.globals, atom, value, 0)
    }

    fn store_eval_local(&mut self, p: &ResidualProgram, atom: Atom, value: Value) {
        let Some(frame) = self.frames.last() else { return };
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
        if frame.captured {
            if let Some(Cell::Environment { slots, .. }) = self.heap.get_mut(frame.env)
                && let Some(local) = slots.get_mut(slot)
            {
                *local = value;
            }
        } else if let Some(local) = self
            .frames
            .last_mut()
            .and_then(|frame| frame.locals.get_mut(slot))
        {
            *local = value;
        }
    }
}

fn is_use_strict(statement: &str) -> bool {
    matches!(statement.trim(), "'use strict'" | "\"use strict\"")
}

fn is_strict_reserved(name: &str) -> bool {
    matches!(
        name,
        "implements"
            | "interface"
            | "let"
            | "package"
            | "private"
            | "protected"
            | "public"
            | "static"
            | "yield"
    )
}

fn split_assignment(statement: &str) -> Option<(&str, &str)> {
    let mut quote = None;
    for (index, character) in statement.char_indices() {
        match (quote, character) {
            (None, '\'' | '"') => quote = Some(character),
            (Some(current), character) if current == character => quote = None,
            (None, '=') => {
                let name = statement[..index].trim();
                if !name.is_empty()
                    && name
                        .chars()
                        .all(|character| character == '_' || character == '$' || character.is_ascii_alphanumeric())
                {
                    return Some((name, statement[index + 1..].trim()));
                }
                return None;
            }
            _ => {}
        }
    }
    None
}

fn split_commas(source: &str) -> Vec<&str> {
    let mut result = Vec::new();
    let mut start = 0;
    let mut quote = None;
    for (index, character) in source.char_indices() {
        match (quote, character) {
            (None, '\'' | '"') => quote = Some(character),
            (Some(current), character) if current == character => quote = None,
            (None, ',') => {
                result.push(source[start..index].trim());
                start = index + 1;
            }
            _ => {}
        }
    }
    result.push(source[start..].trim());
    result
}

fn split_statements(source: &str) -> Vec<&str> {
    let mut result = Vec::new();
    let mut start = 0;
    let mut quote = None;
    let mut escaped = false;
    for (index, character) in source.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if quote.is_some() && character == '\\' {
            escaped = true;
            continue;
        }
        match (quote, character) {
            (None, '\'' | '"') => quote = Some(character),
            (Some(current), character) if current == character => quote = None,
            (None, ';') => {
                result.push(source[start..index].trim());
                start = index + 1;
            }
            _ => {}
        }
    }
    result.push(source[start..].trim());
    result
}
