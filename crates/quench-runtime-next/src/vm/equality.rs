use super::*;

impl<H: Host> Vm<H> {
    pub(super) fn equal(
        &mut self,
        p: &ResidualProgram,
        a: Value,
        b: Value,
    ) -> Result<bool, JsError> {
        if a == b {
            return Ok(true);
        }
        if let (Some(Cell::String(a)), Some(Cell::String(b))) = (self.heap.get(a), self.heap.get(b))
        {
            return Ok(a == b);
        }
        if a.is_null() && b.is_undefined() || a.is_undefined() && b.is_null() {
            return Ok(true);
        }
        if a.is_null() || a.is_undefined() || b.is_null() || b.is_undefined() {
            return Ok(false);
        }
        let a_number = a.as_number().is_some();
        let b_number = b.as_number().is_some();
        if a_number || b_number {
            return Ok(self.to_number(p, a)? == self.to_number(p, b)?);
        }
        if a.as_bool().is_some() || b.as_bool().is_some() {
            return Ok(self.to_number(p, a)? == self.to_number(p, b)?);
        }
        Ok(false)
    }
}
