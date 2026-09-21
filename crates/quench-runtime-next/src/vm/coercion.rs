use super::*;

impl<H: Host> Vm<H> {
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
        if value.is_undefined() {
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

    pub(super) fn to_string(
        &mut self,
        program: &ResidualProgram,
        value: Value,
    ) -> Result<String, JsError> {
        if value.is_undefined() {
            return Ok("undefined".into());
        }
        if value.is_null() {
            return Ok("null".into());
        }
        if let Some(value) = value.as_bool() {
            return Ok(value.to_string());
        }
        if let Some(value) = value.as_number() {
            return Ok(if value.fract() == 0.0 {
                format!("{value:.0}")
            } else {
                value.to_string()
            });
        }
        match self.heap.get(value) {
            Some(Cell::String(value)) | Some(Cell::Error(value)) => return Ok(value.clone()),
            Some(Cell::BigInt(value)) => return Ok(value.clone()),
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
