use super::*;

impl<H: Host> Vm<H> {
    pub(super) fn equal(
        &mut self,
        p: &ResidualProgram,
        a: Value,
        b: Value,
    ) -> Result<bool, JsError> {
        if a.as_number().is_some() && b.as_number().is_some() {
            return Ok(self.strict_equal(a, b));
        }
        if a == b {
            return Ok(true);
        }
        if self.is_html_dda(a) && (b.is_null() || b.is_undefined())
            || self.is_html_dda(b) && (a.is_null() || a.is_undefined())
        {
            return Ok(true);
        }
        if a.is_null() && b.is_undefined() || a.is_undefined() && b.is_null() {
            return Ok(true);
        }
        if a.is_null() || a.is_undefined() || b.is_null() || b.is_undefined() {
            return Ok(false);
        }
        let (a_string, b_string) = (self.heap.is_string(a), self.heap.is_string(b));
        if a_string || b_string {
            if a_string && b_string {
                return Ok(self.heap.string(a) == self.heap.string(b));
            }
            let (string, other) = if a_string { (a, b) } else { (b, a) };
            if let Some(Cell::BigInt(bigint)) = self.heap.get(other) {
                let text = self.heap.string(string).expect("a string operand");
                return Ok(crate::bigint::parse_string(text.host_string())
                    .is_some_and(|value| value == **bigint));
            }
            if let Some(number) = other.as_number() {
                return Ok(self.to_number(p, string)? == number);
            }
        }
        match (self.heap.get(a), self.heap.get(b)) {
            (Some(Cell::BigInt(a)), Some(Cell::BigInt(b))) => return Ok(a == b),
            (Some(Cell::BigInt(bigint)), _) if b.as_number().is_some() => {
                return Ok(crate::bigint::number_as_bigint(b.as_number().unwrap())
                    .is_some_and(|number| number == **bigint));
            }
            (_, Some(Cell::BigInt(bigint))) if a.as_number().is_some() => {
                return Ok(crate::bigint::number_as_bigint(a.as_number().unwrap())
                    .is_some_and(|number| number == **bigint));
            }
            _ => {}
        }
        if let Some(boolean) = a.as_bool() {
            return self.equal(p, Value::number(f64::from(boolean)), b);
        }
        if let Some(boolean) = b.as_bool() {
            return self.equal(p, a, Value::number(f64::from(boolean)));
        }
        match (self.is_object_like(a), self.is_object_like(b)) {
            (true, false) | (false, true) => self.with_call_roots([a, b], |vm| {
                let (a, b) = if vm.is_object_like(a) {
                    (vm.to_primitive(p, a, "default")?, b)
                } else {
                    (a, vm.to_primitive(p, b, "default")?)
                };
                vm.equal(p, a, b)
            }),
            _ => Ok(false),
        }
    }
}
