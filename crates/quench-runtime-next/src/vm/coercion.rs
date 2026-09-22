use super::*;

impl<H: Host> Vm<H> {
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
        match self.heap.get(value) {
            Some(Cell::Date(value)) => return Ok(*value),
            Some(Cell::String(value)) => return Ok(value.parse().unwrap_or(f64::NAN)),
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
            return Ok(if value == 0.0 {
                "0".into()
            } else if value.fract() == 0.0 {
                format!("{value:.0}")
            } else {
                value.to_string()
            });
        }
        match self.heap.get(value) {
            Some(Cell::String(value)) | Some(Cell::Error(value)) => return Ok(value.clone()),
            Some(Cell::BigInt(value)) => return Ok(value.clone()),
            Some(Cell::Symbol(description)) => {
                return Ok(format!("Symbol({})", description.as_deref().unwrap_or("")));
            }
            Some(Cell::Date(value)) => return Ok(value.to_string()),
            _ => {}
        }
        if let Some(atom) = self.lookup_atom("toString") {
            let function = self.get_property(program, value, atom)?;
            if !function.is_undefined() {
                let result = self.call_value(program, function, value, &[])?;
                return self.to_string(program, result);
            }
        }
        Ok("[object Object]".into())
    }
}
