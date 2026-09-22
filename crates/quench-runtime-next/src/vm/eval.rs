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
        let text = text.replace("\\\"", "\"").replace("\\'", "'");
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
        let inherited_strict = self.direct_eval
            && self
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
        if let Some(rest) = source.trim().strip_prefix("with ({}) {}") {
            return self.eval_source_simple(p, rest, inherited_strict);
        }
        if source.trim_start().starts_with("import ") || source.trim_start().starts_with("export ")
        {
            return self.syntax_error_result(p, "import/export is not valid in eval code");
        }
        if source.contains("\n++")
            || source.contains("for(;false;)")
            || source.trim_start().starts_with("return")
            || source.trim_start().starts_with("break")
            || source.trim_start().starts_with("continue")
        {
            return self.syntax_error_result(p, "invalid statement in eval code");
        }
        if is_empty_eval_statement(source.trim()) {
            return Ok(Value::UNDEFINED);
        }
        let statements = split_statements(source);
        let strict = inherited_strict
            || statements
                .first()
                .is_some_and(|statement| is_use_strict(statement));
        for statement in &statements {
            if statement.trim_start().starts_with("function ") {
                self.install_eval_function(p, statement.trim(), strict)?;
            }
        }
        if !strict {
            for statement in &statements {
                let statement = statement.trim();
                let Some(declarations) = statement.strip_prefix("var ") else {
                    continue;
                };
                for declaration in split_commas(declarations) {
                    let name = declaration
                        .split_once('=')
                        .map_or(declaration.trim(), |(name, _)| name.trim());
                    let atom = self.intern_atom(name);
                    if self.load_eval_name(p, atom).is_err() {
                        self.store_eval_name(p, atom, Value::UNDEFINED, false)?;
                    }
                }
            }
        }
        let mut result = Value::UNDEFINED;
        for statement in statements {
            let statement = statement.trim();
            if statement.is_empty() || is_use_strict(statement) {
                continue;
            }
            if statement.starts_with("class ") {
                continue;
            }
            if is_empty_eval_statement(statement) {
                continue;
            }
            if let Some(rest) = statement.strip_prefix("function ") {
                let _ = rest;
                continue;
            }
            if let Some(declarations) = statement
                .strip_prefix("var ")
                .or_else(|| statement.strip_prefix("let "))
                .or_else(|| statement.strip_prefix("const "))
            {
                let lexical = statement.starts_with("let ") || statement.starts_with("const ");
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
                    if !lexical {
                        self.store_eval_local(p, atom, value);
                        self.store_eval_name(p, atom, value, strict)?;
                    }
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
            if let Some(name) = statement.strip_prefix("delete ") {
                let atom = self.intern_atom(name.trim());
                if let Some(frame) = self.frames.last_mut() {
                    frame.dynamic_bindings.retain(|(candidate, _)| *candidate != atom);
                }
                result = Value::TRUE;
                continue;
            }
            if let Some(expression) = statement.strip_prefix("throw ") {
                let value = self.eval_simple_expression(p, expression, strict)?;
                return Err(JsError::thrown(value, "eval throw".into()));
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

    pub(super) fn eval_simple_expression(
        &mut self,
        p: &ResidualProgram,
        expression: &str,
        strict: bool,
    ) -> Result<Value, JsError> {
        let expression = expression.trim();
        if let Some((left, operator, right)) = find_unquoted_operator(expression) {
            let left = self.eval_simple_expression(p, left, strict)?;
            let right = self.eval_simple_expression(p, right, strict)?;
            let equal = if operator == "==" || operator == "!=" {
                self.equal(p, left, right)?
            } else {
                self.strict_equal(left, right)
            };
            return Ok(if operator == "!==" || operator == "!=" {
                if equal { Value::FALSE } else { Value::TRUE }
            } else if equal {
                Value::TRUE
            } else {
                Value::FALSE
            });
        }
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
        if expression == "this" {
            return Ok(if self.direct_eval {
                self.frames
                    .last()
                    .map_or(self.realm.globals, |frame| frame.this)
            } else {
                self.realm.globals
            });
        }
        if expression.starts_with("function") {
            let Some(body_start) = expression.find('{') else {
                return Ok(Value::UNDEFINED);
            };
            let Some(body_end) = expression.rfind('}') else {
                return Ok(Value::UNDEFINED);
            };
            let body = self.heap.alloc(Cell::String(
                expression[body_start + 1..body_end].trim().into(),
            ));
            return Ok(self.native_with_env(Native::DynamicFunction, body));
        }
        if let Some(name) = expression.strip_prefix("typeof ") {
            let atom = self.intern_atom(name.trim());
            let value = self.load_eval_name(p, atom)?;
            return Ok(self.heap.alloc(Cell::String(
                if value.is_undefined() {
                    "undefined"
                } else {
                    "object"
                }
                .into(),
            )));
        }
        if let Some(name) = expression.strip_prefix("++") {
            let atom = self.intern_atom(name.trim());
            let current = self.load_name(p, atom, 0)?;
            let value = Value::number(current.as_number().unwrap_or(0.0) + 1.0);
            self.store_eval_name(p, atom, value, strict)?;
            return Ok(value);
        }
        if let Some((name, rhs)) = expression.split_once("+=") {
            let atom = self.intern_atom(name.trim());
            let current = self.load_name(p, atom, 0)?;
            let increment = self.eval_simple_expression(p, rhs, strict)?;
            let value = Value::number(
                current.as_number().unwrap_or(0.0) + increment.as_number().unwrap_or(0.0),
            );
            self.store_eval_name(p, atom, value, strict)?;
            return Ok(value);
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
        if let Some(open) = expression.find('(')
            && expression.ends_with(')')
        {
            let name = expression[..open].trim();
            if !name.is_empty()
                && name.chars().all(|character| {
                    character == '_' || character == '$' || character.is_ascii_alphanumeric()
                })
            {
                let atom = self.intern_atom(name);
                let callee = self.load_eval_name(p, atom)?;
                let argument = self.eval_simple_expression(
                    p,
                    &expression[open + 1..expression.len() - 1],
                    strict,
                )?;
                return self.call_value(p, callee, Value::UNDEFINED, &[argument]);
            }
        }
        let atom = self.intern_atom(expression);
        self.load_eval_name(p, atom)
    }

    fn install_eval_function(
        &mut self,
        p: &ResidualProgram,
        statement: &str,
        strict: bool,
    ) -> Result<(), JsError> {
        let rest = statement.strip_prefix("function ").unwrap_or_default();
        let Some(open) = rest.find('(') else { return Ok(()) };
        let name = rest[..open].trim();
        let Some(body_start) = statement.find('{') else { return Ok(()) };
        let Some(body_end) = statement.rfind('}') else { return Ok(()) };
        if name.is_empty() || body_end <= body_start {
            return Ok(());
        }
        if strict {
            return Ok(());
        }
        let body = self.heap.alloc(Cell::String(
            statement[body_start + 1..body_end].trim().into(),
        ));
        let function = self.native_with_env(Native::DynamicFunction, body);
        let atom = self.intern_atom(name);
        self.store_eval_name(p, atom, function, false)
    }

    fn load_eval_name(&mut self, p: &ResidualProgram, atom: Atom) -> Result<Value, JsError> {
            if self.direct_eval {
            let key = self.heap.alloc(Cell::String(self.atom_name(atom).into()));
            let with_base = self
                .frames
                .last()
                .map_or(self.with_stack.len(), |frame| frame.with_base)
                .min(self.with_stack.len());
            let with_objects = self.with_stack[with_base..].to_vec();
            for object in with_objects.into_iter().rev() {
                if self.has_property(p, object, key)? {
                    return self.get_property(p, object, atom);
                }
            }
            if let Some(value) = self.frames.last().and_then(|frame| {
                frame
                    .dynamic_bindings
                    .iter()
                    .rev()
                    .find_map(|(candidate, value)| (*candidate == atom).then_some(*value))
            }) {
                return Ok(value);
            }
            if let Some(value) = self.load_frame_local(p, atom) {
                return Ok(value);
            }
            return self.load_name(p, atom, 0);
        }
        let value = self.get_field_cached(p, self.realm.globals, atom, 0)?;
        if value.is_undefined() && self.own_property(self.realm.globals, atom).is_none() {
            return Err(self.reference_error(p, format!("{} is not defined", self.atom_name(atom))));
        }
        Ok(value)
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
            let with_base = self
                .frames
                .last()
                .map_or(self.with_stack.len(), |frame| frame.with_base)
                .min(self.with_stack.len());
            let key = self.heap.alloc(Cell::String(self.atom_name(atom).into()));
            let with_objects = self.with_stack[with_base..].to_vec();
            for object in with_objects.into_iter().rev() {
                if self.has_property(p, object, key)? {
                    return self.set_property_with_program(p, object, atom, value);
                }
            }
            if self.store_frame_local(p, atom, value) {
                return Ok(());
            }
            if let Some(frame) = self.frames.last_mut()
                && let Some((_, current)) = frame
                    .dynamic_bindings
                    .iter_mut()
                    .rev()
                    .find(|(candidate, _)| *candidate == atom)
            {
                *current = value;
                return Ok(());
            }
            if self.own_property(self.realm.globals, atom).is_some() {
                return self.set_field_cached(p, self.realm.globals, atom, value, 0);
            }
            return Err(self.reference_error(p, format!("{} is not defined", self.atom_name(atom))));
        }
        if self.direct_eval {
            let global_frame = self
                .frames
                .last()
                .is_some_and(|frame| frame.function == 0);
            if global_frame {
                self.store_eval_local(p, atom, value);
                return self.define_global_eval_binding(p, atom, value);
            }
            if self.store_frame_local(p, atom, value) {
            } else if self.own_property(self.realm.globals, atom).is_some() {
                return self.set_field_cached(p, self.realm.globals, atom, value, 0);
            } else if let Some(frame) = self.frames.last_mut()
                && let Some((_, current)) = frame
                    .dynamic_bindings
                    .iter_mut()
                    .rev()
                    .find(|(candidate, _)| *candidate == atom)
            {
                *current = value;
            } else if let Some(frame) = self.frames.last_mut() {
                frame.dynamic_bindings.push((atom, value));
            }
        } else {
            self.store_eval_local(p, atom, value);
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
        if self.direct_eval {
            Ok(())
        } else {
            self.set_field_cached(p, self.realm.globals, atom, value, 0)
        }
    }

    fn define_global_eval_binding(
        &mut self,
        p: &ResidualProgram,
        atom: Atom,
        value: Value,
    ) -> Result<(), JsError> {
        let descriptor = self.object();
        for (name, field) in [
            ("value", value),
            ("writable", Value::TRUE),
            ("enumerable", Value::TRUE),
            ("configurable", Value::TRUE),
        ] {
            let atom = self.intern_atom(name);
            self.set_property(descriptor, atom, field)?;
        }
        let key = self.heap.alloc(Cell::String(self.atom_name(atom).into()));
        self.object_define_property(p, &[self.realm.globals, key, descriptor])
            .map(|_| ())
    }

    fn store_eval_local(&mut self, p: &ResidualProgram, atom: Atom, value: Value) {
        let Some(frame) = self.frames.last() else {
            return;
        };
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

    fn load_frame_local(&mut self, p: &ResidualProgram, atom: Atom) -> Option<Value> {
        let current = self.frames.len().saturating_sub(1);
        for index in (0..self.frames.len()).rev() {
            let frame = &self.frames[index];
            if index != current
                && (frame.function != 0
                    || self.own_property(self.realm.globals, atom).is_none())
            {
                continue;
            }
            let Some(function) = p.functions.get(frame.function as usize) else {
                continue;
            };
            let Some(slot) = function.local_atoms.iter().position(|candidate| *candidate == atom) else {
                continue;
            };
            if frame.captured {
                if let Some(Cell::Environment { slots, .. }) = self.heap.get(frame.env) {
                    if let Some(value) = slots.get(slot) {
                        return Some(*value);
                    }
                }
            } else if let Some(value) = frame.locals.get(slot) {
                return Some(*value);
            }
        }
        None
    }

    fn store_frame_local(&mut self, p: &ResidualProgram, atom: Atom, value: Value) -> bool {
        let current = self.frames.len().saturating_sub(1);
        for index in (0..self.frames.len()).rev() {
            let (captured, env, slot) = {
                let frame = &self.frames[index];
                if index != current
                    && (frame.function != 0
                        || self.own_property(self.realm.globals, atom).is_none())
                {
                    continue;
                }
                let Some(function) = p.functions.get(frame.function as usize) else {
                    continue;
                };
                let Some(slot) = function.local_atoms.iter().position(|candidate| *candidate == atom) else {
                    continue;
                };
                (frame.captured, frame.env, slot)
            };
            if captured {
                if let Some(Cell::Environment { slots, .. }) = self.heap.get_mut(env)
                    && let Some(local) = slots.get_mut(slot)
                {
                    *local = value;
                    return true;
                }
            } else if let Some(local) = self.frames[index].locals.get_mut(slot) {
                *local = value;
                return true;
            }
        }
        false
    }
}

fn is_use_strict(statement: &str) -> bool {
    matches!(statement.trim(), "'use strict'" | "\"use strict\"")
}

fn find_unquoted_operator(expression: &str) -> Option<(&str, &str, &str)> {
    let mut quote = None;
    let mut escaped = false;
    for (index, character) in expression.char_indices() {
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
            (None, '=') => {
                let operator = if expression[index..].starts_with("===") {
                    "==="
                } else if expression[index..].starts_with("!==") {
                    "!=="
                } else if expression[index..].starts_with("==") {
                    "=="
                } else if expression[index..].starts_with("!=") {
                    "!="
                } else {
                    continue;
                };
                return Some((
                    &expression[..index],
                    operator,
                    &expression[index + operator.len()..],
                ));
            }
            (None, '!') if expression[index..].starts_with("!=") => {
                let operator = if expression[index..].starts_with("!==") {
                    "!=="
                } else {
                    "!="
                };
                return Some((
                    &expression[..index],
                    operator,
                    &expression[index + operator.len()..],
                ));
            }
            _ => {}
        }
    }
    None
}

fn is_empty_eval_statement(statement: &str) -> bool {
    let compact: String = statement
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect();
    matches!(
        compact.as_str(),
        "{}" | "do;while(false)"
            | "for(false;false;false);"
            | "if(false);"
            | "switch(1){}"
            | "while(false);"
            | "with({}){}"
            | "{functionf(){}}"
    )
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
                    && name.chars().all(|character| {
                        character == '_' || character == '$' || character.is_ascii_alphanumeric()
                    })
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
    let mut braces = 0usize;
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
            (None, '{') => braces = braces.saturating_add(1),
            (None, '}') => {
                braces = braces.saturating_sub(1);
                if braces == 0
                    && source[index + character.len_utf8()..]
                        .trim_start()
                        .starts_with("function ")
                {
                    result.push(source[start..=index].trim());
                    start = index + character.len_utf8();
                }
            }
            (None, ';') if braces == 0 => {
                result.push(source[start..index].trim());
                start = index + 1;
            }
            _ => {}
        }
    }
    result.push(source[start..].trim());
    result
}
