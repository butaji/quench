use super::*;
use num_bigint::BigInt;

impl<H: Host> Vm<H> {
    pub(super) fn equal(
        &mut self,
        p: &ResidualProgram,
        a: Value,
        b: Value,
    ) -> Result<bool, JsError> {
        if a.as_number().is_some_and(f64::is_nan) || b.as_number().is_some_and(f64::is_nan) {
            return Ok(false);
        }
        if a == b {
            return Ok(true);
        }
        if a.is_null() && b.is_undefined() || a.is_undefined() && b.is_null() {
            return Ok(true);
        }
        if a.is_null() || a.is_undefined() || b.is_null() || b.is_undefined() {
            return Ok(false);
        }
        match (self.heap.get(a), self.heap.get(b)) {
            (Some(Cell::String(a)), Some(Cell::String(b))) => return Ok(a == b),
            (Some(Cell::BigInt(a)), Some(Cell::BigInt(b))) => {
                return Ok(matches!(
                    (a.parse::<BigInt>().ok(), b.parse::<BigInt>().ok()),
                    (Some(left), Some(right)) if left == right
                ));
            }
            (Some(Cell::BigInt(bigint)), Some(Cell::String(string)))
            | (Some(Cell::String(string)), Some(Cell::BigInt(bigint))) => {
                return Ok(crate::bigint::parse_string(string.host_string())
                    .is_some_and(|value| value.to_string() == *bigint));
            }
            (Some(Cell::BigInt(bigint)), _) if b.as_number().is_some() => {
                return Ok(crate::bigint::number_as_bigint(b.as_number().unwrap())
                    .is_some_and(|number| number.to_string() == *bigint));
            }
            (_, Some(Cell::BigInt(bigint))) if a.as_number().is_some() => {
                return Ok(crate::bigint::number_as_bigint(a.as_number().unwrap())
                    .is_some_and(|number| number.to_string() == *bigint));
            }
            (Some(Cell::String(_)), _) if b.as_number().is_some() => {
                return Ok(self.to_number(p, a)? == b.as_number().unwrap());
            }
            (_, Some(Cell::String(_))) if a.as_number().is_some() => {
                return Ok(a.as_number().unwrap() == self.to_number(p, b)?);
            }
            _ => {}
        }
        if let Some(boolean) = a.as_bool() {
            return self.equal(p, Value::number(f64::from(boolean)), b);
        }
        if let Some(boolean) = b.as_bool() {
            return self.equal(p, a, Value::number(f64::from(boolean)));
        }
        if self.is_object_like(a) && !b.is_null() && !b.is_undefined() && !self.is_object_like(b) {
            let primitive = self.to_primitive(p, a, "default")?;
            return self.equal(p, primitive, b);
        }
        if self.is_object_like(b) && !a.is_null() && !a.is_undefined() && !self.is_object_like(a) {
            let primitive = self.to_primitive(p, b, "default")?;
            return self.equal(p, a, primitive);
        }
        Ok(false)
    }
}
