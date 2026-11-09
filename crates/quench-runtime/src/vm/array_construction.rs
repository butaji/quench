//! `new Array(...)`: the general construction (ECMA-262 23.1.1.1) and the intrinsic one
//! native code makes in place without running guest code.
use super::*;

impl<H: Host> Vm<H> {
    fn array_prototype_from_new_target(
        &mut self,
        p: &ResidualProgram,
        new_target: Value,
    ) -> Result<Value, JsError> {
        let prototype = self.constructor_prototype_value(p, new_target)?;
        if self.object_data(prototype).is_some() {
            return Ok(prototype);
        }
        let realm = self.function_realm(p, new_target)?;
        Ok(self.array_prototype_for_realm(realm))
    }

    /// `new Array(length)` of a Number `length` with `prototype` (ECMA-262 23.1.1.1 step 5):
    /// an array of that length whose elements are all absent; `None` for a length that is no
    /// array length, which throws a RangeError. Out of line: the lane makes it too
    /// (`lane_native_construct`), whose frame then holds none of its temporaries.
    #[inline(never)]
    pub(in crate::vm) fn array_of_length(&mut self, prototype: Value, length: f64) -> Option<Value> {
        if !length.is_finite() || length < 0.0 || length.fract() != 0.0 || length > f64::from(u32::MAX) {
            return None;
        }
        let array = self.heap.alloc(Cell::Array {
            object: crate::heap::ArrayObject::new(Self::empty_object(prototype), Vec::new()),
        });
        self.heap.sparse_set_length(array, length as usize);
        Some(array)
    }

    pub(super) fn construct_array_native(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
        new_target: Value,
    ) -> Result<Value, JsError> {
        let prototype = self.array_prototype_from_new_target(p, new_target)?;
        if let [length] = args
            && let Some(length) = length.as_number()
        {
            return match self.array_of_length(prototype, length) {
                Some(array) => Ok(array),
                None => Err(self.range_error(p, "Invalid array length".into())),
            };
        }

        Ok(self.array_of_values(prototype, args))
    }

    /// `new Array(...args)` with the intrinsic `Array` as its own new target, which runs no
    /// guest code: `construct_array_native`'s array with the callee's own `prototype` data.
    /// `None` for any other callee, a prototype held otherwise, or an invalid length, whose
    /// RangeError the general path throws.
    pub(in crate::vm) fn intrinsic_array_construction(&mut self, callee: Value, args: &[Value]) -> Option<Value> {
        if !matches!(self.heap.function(callee), Some(FunctionRecord { kind: FunctionKind::Native(Native::Array), .. })) {
            return None;
        }
        let prototype_atom = self.prototype_atom();
        let prototype = self.own_function_prototype_data(callee, prototype_atom)
            .filter(|prototype| self.object_data(*prototype).is_some())?;
        match args {
            [length] if let Some(length) = length.as_number() => self.array_of_length(prototype, length),
            _ => Some(self.array_of_values(prototype, args)),
        }
    }

    /// The array of `values` with `prototype`. Out of line: the cell it builds lives in its
    /// own frame, not its caller's.
    #[inline(never)]
    fn array_of_values(&mut self, prototype: Value, values: &[Value]) -> Value {
        self.heap.alloc(Cell::Array {
            object: crate::heap::ArrayObject::new(Self::empty_object(prototype), values.to_vec()),
        })
    }
}
