//! Comparisons of two primitives whose conversions run no guest code and raise nothing: the
//! abstract equality and relational tables of `undefined`, `null`, Booleans, Numbers, strings
//! and Symbols (ECMA-262 7.2.14, 7.2.15). A BigInt beside another type, an object, and a
//! Symbol's conversion error stay with the general operation.
use super::*;

impl<H: Host> Vm<H> {
    /// `==`, `!=` or a relational operator of two primitives, which each decide as the
    /// specification's tables do; `None` for strict equality (`identity_equality` decides it),
    /// any other operator, an object, and a pair that converts through a BigInt or raises.
    pub(in crate::vm) fn primitive_comparison(&self, op: u32, left: Value, right: Value) -> Option<bool> {
        if left.is_deleted() || right.is_deleted() {
            return None;
        }
        match equality_operator(op) {
            Some((strict, negated)) => (!strict).then(|| self.primitive_loose_equality(left, right)).flatten().map(|equal| equal != negated),
            None => self.primitive_relation(op, left, right),
        }
    }

    /// A relational operator of two primitives that are not both strings (which compare as
    /// text) through ToNumber of each, the comparison of the Numbers they make; `NaN` makes it
    /// false. `None` where a side is a BigInt, a Symbol or an object.
    fn primitive_relation(&self, op: u32, left: Value, right: Value) -> Option<bool> {
        let relation = RelationalOperator::from_immediate(op)?;
        if self.heap.string(left).is_some() && self.heap.string(right).is_some() {
            return None;
        }
        Some(relation.compare_numbers(self.primitive_to_number(left)?, self.primitive_to_number(right)?))
    }

    /// IsLooselyEqual of two primitives: `null` and `undefined` equal only each other, a Symbol
    /// only itself, two strings by their text, and any other pair by ToNumber of each (a Boolean
    /// is its Number, a string its parse). `None` for an object or a BigInt.
    fn primitive_loose_equality(&self, left: Value, right: Value) -> Option<bool> {
        if self.is_object_like(left) || self.is_object_like(right) {
            return None;
        }
        let nullish = |value: Value| value.is_null() || value.is_undefined();
        if nullish(left) || nullish(right) {
            return Some(nullish(left) && nullish(right));
        }
        let symbol = |value: Value| matches!(self.heap.get(value), Some(Cell::Symbol(_)));
        if symbol(left) || symbol(right) {
            return Some(left == right);
        }
        if let (Some(left), Some(right)) = (self.heap.string(left), self.heap.string(right)) {
            return Some(left == right);
        }
        Some(self.primitive_to_number(left)? == self.primitive_to_number(right)?)
    }
}
