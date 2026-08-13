// `JSON.stringify`: spec-ordered serialization over runtime values.

pub(crate) struct Serializer {
    replacer_function: Option<Value>,
    property_list: Option<Vec<String>>,
    gap: String,
    stack: Vec<Value>,
}

pub(crate) fn stringify(arguments: &[Value]) -> Result<Value, VmError> {
    let value = arguments.first().unwrap_or(&Value::Undefined);
    let replacer = arguments.get(1).unwrap_or(&Value::Undefined);
    let space = arguments.get(2).unwrap_or(&Value::Undefined);
    let (replacer_function, property_list) = replacer_setup(replacer)?;
    let gap = gap(space)?;
    let mut serializer = Serializer {
        replacer_function,
        property_list,
        gap,
        stack: Vec::new(),
    };
    let holder = Value::Object(Rc::new(crate::value::ObjectData::new(vec![(
        String::new(),
        value.clone(),
    )])));
    match serializer.property("", &holder)? {
        Some(text) => Ok(Value::String(text)),
        None => Ok(Value::Undefined),
    }
}

impl Serializer {
    fn property(&mut self, key: &str, holder: &Value) -> Result<Option<String>, VmError> {
        let holder = crate::locals::resolved_replacement(holder.clone());
        let mut value = crate::execute::get_property_result(&holder, key)?;
        value = crate::locals::resolved_replacement(value);
        if crate::value::is_object(&value) || matches!(value, Value::BigInt(_)) {
            let to_json = crate::execute::get_property_result(&value, "toJSON")?;
            if crate::conversion::is_callable(&to_json) {
                let key = Value::String(key.to_string());
                value = crate::functions::execute_target(&to_json, &value, &[key])?;
            }
        }
        if let Some(replacer) = &self.replacer_function {
            let arguments = [Value::String(key.to_string()), value];
            value = crate::functions::execute_target(replacer, &holder, &arguments)?;
        }
        self.serialize(value)
    }

    fn serialize(&mut self, value: Value) -> Result<Option<String>, VmError> {
        let value = unbox(resolve_alias(value))?;
        if let Some(raw) = raw_json_text(&value) {
            return Ok(Some(raw));
        }
        match &value {
            Value::Null => Ok(Some("null".to_string())),
            Value::Boolean(value) => Ok(Some(value.to_string())),
            Value::Number(value) => Ok(Some(serialize_number(*value))),
            Value::String(_) if crate::conversion::is_symbol(&value) => Ok(None),
            Value::String(value) => Ok(Some(quote(value))),
            Value::BigInt(_) => Err(big_int_error()),
            _ if crate::value::is_object(&value) && !crate::conversion::is_callable(&value) => {
                self.nested(value)
            }
            _ => Ok(None),
        }
    }

    fn nested(&mut self, value: Value) -> Result<Option<String>, VmError> {
        if self
            .stack
            .iter()
            .any(|entry| crate::builtins::same_value(Some(entry), Some(&value)))
        {
            return Err(crate::value::error::throw_type_error(
                "Converting circular structure to JSON",
            ));
        }
        self.stack.push(value.clone());
        let result = if is_json_array(&value)? {
            self.array(&value).map(Some)
        } else {
            self.object(&value).map(Some)
        };
        self.stack.pop();
        result
    }

    fn array(&mut self, value: &Value) -> Result<String, VmError> {
        let length = crate::execute::get_property_result(value, "length")?;
        let length = to_length(&length)?;
        let mut elements = Vec::with_capacity(length);
        for index in 0..length {
            let key = index.to_string();
            let element = self.property(&key, value)?.unwrap_or("null".to_string());
            elements.push(element);
        }
        Ok(self.wrap("[", "]", elements))
    }

    fn object(&mut self, value: &Value) -> Result<String, VmError> {
        let keys = match &self.property_list {
            Some(list) => list.clone(),
            None => enumerable_keys(value)?,
        };
        let mut members = Vec::new();
        for key in keys {
            if let Some(serialized) = self.property(&key, value)? {
                let colon = if self.gap.is_empty() { ":" } else { ": " };
                members.push(format!("{}{colon}{}", quote(&key), serialized));
            }
        }
        Ok(self.wrap("{", "}", members))
    }

    fn wrap(&self, open: &str, close: &str, parts: Vec<String>) -> String {
        if parts.is_empty() {
            return format!("{open}{close}");
        }
        if self.gap.is_empty() {
            return format!("{open}{}{close}", parts.join(","));
        }
        let indent = self.gap.repeat(self.stack.len());
        let outer = self.gap.repeat(self.stack.len() - 1);
        let separator = format!(",\n{indent}");
        format!("{open}\n{indent}{}\n{outer}{close}", parts.join(&separator))
    }
}

fn replacer_setup(replacer: &Value) -> Result<(Option<Value>, Option<Vec<String>>), VmError> {
    if !crate::value::is_object(replacer) {
        return Ok((None, None));
    }
    if crate::conversion::is_callable(replacer) {
        return Ok((Some(replacer.clone()), None));
    }
    if is_json_array(replacer)? {
        return Ok((None, Some(property_list(replacer)?)));
    }
    Ok((None, None))
}

fn property_list(replacer: &Value) -> Result<Vec<String>, VmError> {
    let length = crate::execute::get_property_result(replacer, "length")?;
    let length = to_length(&length)?;
    let mut list = Vec::new();
    for index in 0..length {
        let item = crate::execute::get_property_result(replacer, &index.to_string())?;
        if !is_string_or_number(&item) {
            continue;
        }
        let name = crate::conversion::to_string(&item)?;
        if !list.contains(&name) {
            list.push(name);
        }
    }
    Ok(list)
}

fn is_string_or_number(value: &Value) -> bool {
    match value {
        Value::Number(_) => true,
        Value::String(_) => !crate::conversion::is_symbol(value),
        Value::Object(properties) => properties.iter().rev().any(|(name, value)| {
            name == "_value" && matches!(value, Value::String(_) | Value::Number(_))
        }),
        _ => false,
    }
}

fn gap(space: &Value) -> Result<String, VmError> {
    if let Value::Object(properties) = space {
        let boxed = properties
            .iter()
            .rev()
            .find(|(name, _)| name == "_value")
            .map(|(_, value)| value.clone());
        return match boxed {
            Some(Value::Number(_)) => number_gap(crate::conversion::to_number(space)?),
            Some(Value::String(_)) => string_gap(&crate::conversion::to_string(space)?),
            _ => Ok(String::new()),
        };
    }
    match space {
        Value::Number(_) => number_gap(crate::conversion::to_number(space)?),
        Value::String(value) if !crate::conversion::is_symbol(space) => string_gap(value),
        _ => Ok(String::new()),
    }
}

fn number_gap(value: f64) -> Result<String, VmError> {
    if value.is_nan() || value <= 0.0 {
        return Ok(String::new());
    }
    let count = value.floor().min(10.0) as usize;
    Ok(" ".repeat(count))
}

fn string_gap(value: &str) -> Result<String, VmError> {
    Ok(value.chars().take(10).collect())
}

fn unbox(value: Value) -> Result<Value, VmError> {
    let Value::Object(properties) = &value else {
        return Ok(value);
    };
    let Some((_, boxed)) = properties.iter().rev().find(|(name, _)| name == "_value") else {
        return Ok(value);
    };
    match boxed {
        Value::Boolean(_) | Value::BigInt(_) => Ok(boxed.clone()),
        Value::Number(_) => Ok(Value::Number(crate::conversion::to_number(&value)?)),
        Value::String(_) => Ok(Value::String(crate::conversion::to_string(&value)?)),
        _ => Ok(value),
    }
}

pub(crate) fn raw_json_text(value: &Value) -> Option<String> {
    let Value::Object(properties) = value else {
        return None;
    };
    properties
        .iter()
        .rev()
        .find(|(name, _)| name == RAW_JSON_KEY)?;
    match properties.iter().rev().find(|(name, _)| name == "rawJSON") {
        Some((_, Value::String(text))) => Some(text.clone()),
        _ => None,
    }
}

fn enumerable_keys(value: &Value) -> Result<Vec<String>, VmError> {
    if let Value::Proxy(proxy) = value {
        if crate::proxy::is_revoked(proxy) {
            return Err(revoked_error());
        }
        if crate::proxy::get_handler_trap(proxy, "ownKeys").is_some() {
            return proxy_trapped_keys(value);
        }
        return enumerable_keys(&proxy.target.clone());
    }
    Ok(crate::own_keys::enumerable_key_strings(Some(value)))
}

fn proxy_trapped_keys(value: &Value) -> Result<Vec<String>, VmError> {
    let keys = crate::proxy::proxy_own_keys(value)?;
    let Value::Array(keys) = keys else {
        return Ok(Vec::new());
    };
    let mut result = Vec::new();
    for key in keys.iter() {
        let Value::String(key) = key else { continue };
        if crate::conversion::is_symbol_string(key) {
            continue;
        }
        let descriptor = crate::proxy::proxy_get_own_property_descriptor(value, key)?;
        if descriptor_enumerable(&descriptor) {
            result.push(key.clone());
        }
    }
    Ok(result)
}

fn revoked_error() -> VmError {
    crate::value::error::throw_type_error("Cannot perform operation on a revoked proxy")
}

fn resolve_alias(value: Value) -> Value {
    let Value::ObjectAlias(alias) = &value else {
        return value;
    };
    let upgraded = alias.0.borrow().upgrade();
    match upgraded {
        Some(properties) => Value::Object(properties),
        None => value,
    }
}

fn descriptor_enumerable(descriptor: &Value) -> bool {
    let Value::Object(properties) = descriptor else {
        return false;
    };
    properties
        .iter()
        .rev()
        .any(|(name, value)| name == "enumerable" && matches!(value, Value::Boolean(true)))
}

fn is_json_array(value: &Value) -> Result<bool, VmError> {
    match value {
        Value::Array(_) => Ok(true),
        Value::Proxy(proxy) => {
            if crate::proxy::is_revoked(proxy) {
                return Err(crate::value::error::throw_type_error(
                    "Cannot perform operation on a revoked proxy",
                ));
            }
            is_json_array(&proxy.target.clone())
        }
        _ => Ok(false),
    }
}

fn serialize_number(value: f64) -> String {
    if !value.is_finite() {
        return "null".to_string();
    }
    crate::conversion::number_to_string(value)
}

fn quote(value: &str) -> String {
    let mut output = String::with_capacity(value.len() + 2);
    output.push('"');
    let mut rest = value;
    while let Some(character) = rest.chars().next() {
        rest = &rest[character.len_utf8()..];
        match character {
            '"' => output.push_str("\\\""),
            '\\' => {
                let (escaped, consumed) = escape_sequence(rest);
                output.push_str(&escaped);
                rest = &rest[consumed..];
            }
            '\u{8}' => output.push_str("\\b"),
            '\t' => output.push_str("\\t"),
            '\n' => output.push_str("\\n"),
            '\u{C}' => output.push_str("\\f"),
            '\r' => output.push_str("\\r"),
            '\u{0}'..='\u{1F}' => output.push_str(&format!("\\u{:04x}", character as u32)),
            _ => output.push(character),
        }
    }
    output.push('"');
    output
}

// Engine strings store lone surrogates as literal `\uXXXX` text; re-emit the
// escape, combining an adjacent high+low pair back into the real character.
fn escape_sequence(rest: &str) -> (String, usize) {
    let Some((len, unit)) = escape_unit(rest) else {
        return ("\\\\".to_string(), 0);
    };
    if !(0xD800..0xDC00).contains(&unit) {
        return (format!("\\u{unit:04x}"), len);
    }
    let after = &rest[len..];
    let low = after
        .strip_prefix('\\')
        .and_then(escape_unit)
        .filter(|(_, low)| (0xDC00..0xE000).contains(low));
    match low {
        Some((low_len, low)) => {
            let code = 0x1_0000 + (((unit - 0xD800) as u32) << 10) + (low - 0xDC00) as u32;
            match char::from_u32(code) {
                Some(character) => (character.to_string(), len + 1 + low_len),
                None => (format!("\\u{unit:04x}"), len),
            }
        }
        None => (format!("\\u{unit:04x}"), len),
    }
}

fn big_int_error() -> VmError {
    crate::value::error::throw_type_error("Do not know how to serialize a BigInt")
}

// Parse an engine-internal lone-surrogate escape (`uXXXX` following a
// backslash), returning the consumed length and code unit.
fn escape_unit(text: &str) -> Option<(usize, u16)> {
    let digits = text.strip_prefix('u')?.get(..4)?;
    if !digits.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let unit = u16::from_str_radix(digits, 16).ok()?;
    (0xD800..0xE000).contains(&unit).then_some((5, unit))
}

fn to_length(value: &Value) -> Result<usize, VmError> {
    let number = crate::conversion::to_number(value)?;
    if number.is_nan() || number <= 0.0 {
        return Ok(0);
    }
    const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;
    Ok(number.floor().min(MAX_SAFE_INTEGER) as usize)
}
