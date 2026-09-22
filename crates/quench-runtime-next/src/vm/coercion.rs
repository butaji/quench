use super::wtf16::JsString;
use super::*;

impl<H: Host> Vm<H> {
    pub(super) fn to_property_key(
        &mut self,
        program: &ResidualProgram,
        value: Value,
    ) -> Result<Value, JsError> {
        if matches!(self.heap.get(value), Some(Cell::Symbol(_))) {
            return Ok(value);
        }
        let text = self.coerce_js_string(program, value)?;
        Ok(self.heap.alloc(Cell::String(text)))
    }

    pub(super) fn coerce_js_string(
        &mut self,
        program: &ResidualProgram,
        value: Value,
    ) -> Result<JsString, JsError> {
        if let Some(Cell::String(text)) = self.heap.get(value) {
            return Ok(text.clone());
        }
        Ok(self.to_string(program, value)?.into())
    }

    pub(super) fn unary(
        &mut self,
        p: &ResidualProgram,
        op: u32,
        value: Value,
    ) -> Result<Value, JsError> {
        Ok(match op {
            0 => Value::number(self.to_number(p, value)?),
            1 => Value::number(-self.to_number(p, value)?),
            2 => {
                if self.truthy(value) {
                    Value::FALSE
                } else {
                    Value::TRUE
                }
            }
            3 => Value::number((!(number_to_u32(self.to_number(p, value)?) as i32)) as f64),
            4 => {
                let text = if value.is_undefined() || value.is_deleted() {
                    "undefined"
                } else if value.as_bool().is_some() {
                    "boolean"
                } else if value.as_number().is_some() {
                    "number"
                } else if matches!(self.heap.get(value), Some(Cell::String(_))) {
                    "string"
                } else if matches!(self.heap.get(value), Some(Cell::BigInt(_))) {
                    "bigint"
                } else if matches!(self.heap.get(value), Some(Cell::Symbol(_))) {
                    "symbol"
                } else if matches!(self.heap.get(value), Some(Cell::Function { .. })) {
                    "function"
                } else {
                    "object"
                };
                self.heap.alloc(Cell::String(text.into()))
            }
            5 => Value::UNDEFINED,
            _ => return Err(JsError("unsupported unary operator".into())),
        })
    }

    #[expect(clippy::wrong_self_convention)]
    pub(super) fn to_number(
        &mut self,
        program: &ResidualProgram,
        value: Value,
    ) -> Result<f64, JsError> {
        if let Some(value) = value.as_number() {
            return Ok(value);
        }
        if let Some(value) = value.as_bool() {
            return Ok(if value { 1.0 } else { 0.0 });
        }
        if value.is_null() {
            return Ok(0.0);
        }
        if value.is_undefined() || value.is_deleted() {
            return Ok(f64::NAN);
        }
        if self.object_data(value).is_some() {
            let primitive = self.to_primitive(program, value, "number")?;
            return self.to_number(program, primitive);
        }
        match self.heap.get(value) {
            Some(Cell::Date(value)) => return Ok(*value),
            Some(Cell::Symbol(_)) => {
                return Err(self.type_error(program, "cannot convert a Symbol value to a number".into()));
            }
            Some(Cell::String(value)) => {
                let text = value.host_string().trim();
                let radix = if text.starts_with("0x") || text.starts_with("0X") {
                    Some(16)
                } else if text.starts_with("0o") || text.starts_with("0O") {
                    Some(8)
                } else if text.starts_with("0b") || text.starts_with("0B") {
                    Some(2)
                } else {
                    None
                };
                return Ok(radix
                    .and_then(|radix| u64::from_str_radix(&text[2..], radix).ok())
                    .map_or_else(|| text.parse().unwrap_or(f64::NAN), |value| value as f64));
            }
            Some(Cell::BigInt(value)) => return Ok(value.parse().unwrap_or(f64::NAN)),
            _ => {}
        }
        if let Some(atom) = self.lookup_atom("valueOf") {
            let function = self.get_property(program, value, atom)?;
            if !function.is_undefined() {
                let result = self.call_value(program, function, value, &[])?;
                return self.to_number(program, result);
            }
        }
        Ok(f64::NAN)
    }

    #[expect(clippy::wrong_self_convention)]
    pub(super) fn to_string(
        &mut self,
        program: &ResidualProgram,
        value: Value,
    ) -> Result<String, JsError> {
        if value.is_undefined() || value.is_deleted() {
            return Ok("undefined".into());
        }
        if value.is_null() {
            return Ok("null".into());
        }
        if let Some(value) = value.as_bool() {
            return Ok(value.to_string());
        }
        if let Some(value) = value.as_number() {
            return Ok(number_string(value));
        }
        if self.object_data(value).is_some() {
            let primitive = self.to_primitive(program, value, "string")?;
            return self.to_string(program, primitive);
        }
        match self.heap.get(value) {
            Some(Cell::Function { .. }) => return Ok("function () { [native code] }".into()),
            Some(Cell::String(value)) => return Ok(value.to_string()),
            Some(Cell::Error(value)) => return Ok(value.clone()),
            Some(Cell::BigInt(value)) => return Ok(value.clone()),
            Some(Cell::Symbol(description)) => {
                return Ok(format!("Symbol({})", description.as_deref().unwrap_or("")));
            }
            Some(Cell::Date(value)) => return Ok(value.to_string()),
            _ => {}
        }
        if let Some(atom) = self.lookup_atom("toString") {
            let function = self.get_property(program, value, atom)?;
            if self.is_function(function) {
                let result = self.call_value(program, function, value, &[])?;
                return self.to_string(program, result);
            }
        }
        if let Some(atom) = self.lookup_atom("valueOf") {
            let function = self.get_property(program, value, atom)?;
            if self.is_function(function) {
                let result = self.call_value(program, function, value, &[])?;
                if !result.is_heap() || matches!(self.heap.get(result), Some(Cell::String(_))) {
                    return self.to_string(program, result);
                }
            }
        }
        Ok("[object Object]".into())
    }

    fn to_primitive(
        &mut self,
        program: &ResidualProgram,
        value: Value,
        hint: &str,
    ) -> Result<Value, JsError> {
        if !self.object_data(value).is_some() {
            return Ok(value);
        }
        if let Some(symbol) = self.well_known_symbols.get("toPrimitive").copied() {
            let method = self.get_index(program, value, symbol)?;
            if !method.is_undefined() {
                let hint = self.heap.alloc(Cell::String(hint.into()));
                let result = self.call_value(program, method, value, &[hint])?;
                if self.object_data(result).is_none() {
                    return Ok(result);
                }
                return Err(self.type_error(
                    program,
                    "Cannot convert object to primitive value".into(),
                ));
            }
        }
        let names = if hint == "string" {
            ["toString", "valueOf"]
        } else {
            ["valueOf", "toString"]
        };
        let mut attempted = false;
        for name in names {
            let atom = self.intern_atom(name);
            let method = self.get_property(program, value, atom)?;
            if self.is_function(method) {
                attempted = true;
                let result = self.call_value(program, method, value, &[])?;
                if self.object_data(result).is_none() {
                    return Ok(result);
                }
            }
        }
        if !attempted {
            return Ok(self.heap.alloc(Cell::String("[object Object]".into())));
        }
        Err(self.type_error(
            program,
            "Cannot convert object to primitive value".into(),
        ))
    }

    pub(super) fn strict_equal(&self, a: Value, b: Value) -> bool {
        if a == b {
            return true;
        }
        matches!(
            (self.heap.get(a), self.heap.get(b)),
            (Some(Cell::String(a)), Some(Cell::String(b))) if a == b
        )
    }

    #[inline(always)]
    pub(super) fn truthy(&self, v: Value) -> bool {
        !(v.is_null()
            || v.is_undefined()
            || v.is_deleted()
            || v == Value::FALSE
            || v.as_number().is_some_and(|n| n == 0.0 || n.is_nan()))
    }
}

fn number_string(value: f64) -> String {
    if value.is_nan() {
        return "NaN".into();
    }
    if value.is_infinite() {
        return if value.is_sign_negative() {
            "-Infinity"
        } else {
            "Infinity"
        }
        .into();
    }
    if value == 0.0 {
        return "0".into();
    }
    let magnitude = value.abs();
    if magnitude >= 1e21 || magnitude < 1e-6 {
        let scientific = format!("{value:e}");
        let (mantissa, exponent) = scientific.split_once('e').unwrap();
        let mantissa = mantissa.trim_end_matches('0').trim_end_matches('.');
        let exponent = exponent.parse::<i32>().unwrap();
        return format!("{mantissa}e{:+}", exponent);
    }
    if value.fract() == 0.0 {
        format!("{value:.0}")
    } else {
        value.to_string()
    }
}
