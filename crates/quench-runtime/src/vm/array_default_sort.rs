//! `Array.prototype.sort` without a comparator: SortCompare orders two values by the code
//! units of their ToString.
use super::*;

impl<H: Host> Vm<H> {
    /// Sorts `values` (none `undefined`) as SortCompare without a comparator orders them.
    /// ToString of a value that is no object runs no guest code, so when none is one each
    /// value's string is made once and the values follow their strings' order; an object's
    /// ToString is called at each comparison, as the comparison calls it.
    pub(super) fn sort_by_string_order(&mut self, p: &ResidualProgram, values: &mut Vec<Value>) -> Result<(), JsError> {
        // A lone value is never compared, so it is not converted.
        if values.len() <= 1 || values.iter().any(|value| self.is_object_like(*value)) {
            return crate::vm::sort::try_stable_sort_by(values, |left, right| {
                let (left, right) = (self.coerce_js_string(p, *left)?, self.coerce_js_string(p, *right)?);
                Ok(left.units().cmp(&right.units()))
            });
        }
        let keys = values.iter().map(|value| self.coerce_js_string(p, *value)).collect::<Result<Vec<_>, _>>()?;
        let mut order: Vec<usize> = (0..values.len()).collect();
        crate::vm::sort::try_stable_sort_by(&mut order, |left, right| Ok::<_, JsError>(keys[*left].units().cmp(&keys[*right].units())))?;
        *values = order.iter().map(|index| values[*index]).collect();
        Ok(())
    }
}
