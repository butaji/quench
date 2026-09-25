use super::wtf16::JsString;
use super::*;

impl<H: Host> Vm<H> {
    pub(super) fn typeof_value(&mut self, value: Value) -> Value {
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
        } else if value == self.function_proto || self.is_function(self.proxy_target(value)) {
            "function"
        } else {
            "object"
        };
        self.heap.alloc(Cell::String(text.into()))
    }

    pub(super) fn to_numeric(
        &mut self,
        p: &ResidualProgram,
        value: Value,
    ) -> Result<Value, JsError> {
        let primitive = if self.is_object_like(value) {
            self.to_primitive(p, value, "number")?
        } else {
            value
        };
        if matches!(self.heap.get(primitive), Some(Cell::BigInt(_))) {
            Ok(primitive)
        } else {
            Ok(Value::number(self.to_number(p, primitive)?))
        }
    }

    pub(super) fn to_bigint(
        &mut self,
        p: &ResidualProgram,
        value: Value,
    ) -> Result<num_bigint::BigInt, JsError> {
        let primitive = if self.is_object_like(value) {
            self.to_primitive(p, value, "number")?
        } else {
            value
        };
        match self.heap.get(primitive) {
            Some(Cell::BigInt(value)) => value
                .parse::<num_bigint::BigInt>()
                .map_err(|_| self.type_error(p, "invalid BigInt value".into())),
            Some(Cell::String(value)) => {
                let Some(value) = crate::bigint::parse_string(&value.host_string()) else {
                    return self
                        .syntax_error_result(p, "invalid BigInt value")
                        .map(|_| num_bigint::BigInt::default());
                };
                Ok(value)
            }
            _ => primitive
                .as_bool()
                .map(|value| num_bigint::BigInt::from(i32::from(value)))
                .ok_or_else(|| self.type_error(p, "cannot convert value to BigInt".into())),
        }
    }

    pub(super) fn to_property_key(
        &mut self,
        program: &ResidualProgram,
        value: Value,
    ) -> Result<Value, JsError> {
        let primitive = if self.is_object_like(value) {
            self.to_primitive(program, value, "string")?
        } else {
            value
        };
        if matches!(self.heap.get(primitive), Some(Cell::Symbol(_))) {
            return Ok(primitive);
        }
        let text = self.coerce_js_string(program, primitive)?;
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
        let value = if matches!(op, 0 | 1 | 3) && self.is_object_like(value) {
            self.to_primitive(p, value, "number")?
        } else {
            value
        };
        if let Some(Cell::BigInt(value)) = self.heap.get(value).cloned() {
            if op == 0 {
                return Err(self.type_error(p, "Cannot convert BigInt value to number".into()));
            }
            let value = value
                .parse::<num_bigint::BigInt>()
                .map_err(|_| self.type_error(p, "Invalid BigInt value".into()))?;
            if op == 1 {
                return Ok(self.heap.alloc(Cell::BigInt((-value).to_str_radix(10))));
            }
            if op == 3 {
                return Ok(self.heap.alloc(Cell::BigInt((!value).to_str_radix(10))));
            }
        }
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
            4 => return Ok(self.typeof_value(value)),
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
            Some(Cell::Date { milliseconds, .. }) => return Ok(*milliseconds),
            Some(Cell::Symbol(_)) => {
                return Err(
                    self.type_error(program, "cannot convert a Symbol value to a number".into())
                );
            }
            Some(Cell::String(value)) => {
                return Ok(super::number::parse_number_string(&value.host_string()));
            }
            Some(Cell::BigInt(_)) => {
                return Err(
                    self.type_error(program, "cannot convert a BigInt value to a number".into())
                );
            }
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
            return Ok(crate::number_to_string::format(value));
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
            Some(Cell::Symbol(_)) => {
                return Err(
                    self.type_error(program, "Cannot convert a Symbol value to a string".into())
                );
            }
            Some(Cell::Date { milliseconds, .. }) => return Ok(milliseconds.to_string()),
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

    pub(super) fn to_primitive(
        &mut self,
        program: &ResidualProgram,
        value: Value,
        hint: &str,
    ) -> Result<Value, JsError> {
        if !self.is_object_like(value) {
            return Ok(value);
        }
        let hint = if matches!(self.heap.get(value), Some(Cell::Date { .. })) && hint == "default" {
            "string"
        } else {
            hint
        };
        if let Some(symbol) = self.well_known_symbols.get("toPrimitive").copied() {
            let method = self.get_index(program, value, symbol)?;
            if !method.is_undefined() && !method.is_null() {
                if !self.is_function(method) {
                    return Err(
                        self.type_error(program, "Symbol.toPrimitive is not callable".into())
                    );
                }
                let hint = self.heap.alloc(Cell::String(hint.into()));
                let result = self.call_value(program, method, value, &[hint])?;
                if !self.is_object_like(result) {
                    return Ok(result);
                }
                return Err(
                    self.type_error(program, "Cannot convert object to primitive value".into())
                );
            }
        }
        let names = if hint == "string" {
            ["toString", "valueOf"]
        } else {
            ["valueOf", "toString"]
        };
        for name in names {
            let atom = self.intern_atom(name);
            let method = self.get_property(program, value, atom)?;
            if self.is_function(method) {
                let result = self.call_value(program, method, value, &[])?;
                if !self.is_object_like(result) {
                    return Ok(result);
                }
            }
        }
        Err(self.type_error(program, "Cannot convert object to primitive value".into()))
    }

    pub(super) fn strict_equal(&self, a: Value, b: Value) -> bool {
        if let (Some(left), Some(right)) = (a.as_number(), b.as_number()) {
            return left == right;
        }
        if a == b {
            return true;
        }
        match (self.heap.get(a), self.heap.get(b)) {
            (Some(Cell::String(a)), Some(Cell::String(b))) => a == b,
            (Some(Cell::BigInt(a)), Some(Cell::BigInt(b))) => a == b,
            _ => false,
        }
    }

    #[inline(always)]
    pub(super) fn truthy(&self, v: Value) -> bool {
        !(v.is_null()
            || v.is_undefined()
            || v.is_deleted()
            || v == Value::FALSE
            || v.as_number().is_some_and(|n| n == 0.0 || n.is_nan())
            || matches!(self.heap.get(v), Some(Cell::String(text)) if text.units().is_empty())
            || matches!(self.heap.get(v), Some(Cell::BigInt(value)) if value == "0"))
    }
}
