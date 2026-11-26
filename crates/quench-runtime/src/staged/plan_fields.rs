//! A construct plan's field value as one staged row: a constant field's constant, a passed
//! field's argument, a defaulted field's argument or its fallback where that is falsy. The
//! interpreter reads each field's class from the plan at run time (`Concrete`); a specializer
//! holds the plan constant, so the class folds and a field costs its own arm alone. A computed
//! field is a Number operator over the argument and its constant (`number_binary_word`); an
//! argument that is no Number leaves at `miss`.

// Read by the lane construct plan (`vm/dispatch_fast/construct_plan.rs`) and the u-jit and b-jit construct lowering.

use super::{Float64, NumericStage};
#[cfg(not(stencil_build))]
use super::{Int32, NumberOf};
#[cfg(not(stencil_build))]
use crate::vm::operations::{NumberArithmetic, NumberBitwise};
use super::value::tagged_truthiness;

/// How a plan stores one field.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FieldClass {
    Passed,
    Constant,
    Defaulted,
    Computed,
}

/// The value of one field. `class`, `fallback` and `operator` are per-site operands; `argument` fetches the
/// parameter's value only on the arms that read it.
#[inline(always)]
pub fn field_value<S: NumericStage>(stage: &mut S, class: FieldClass, fallback: u64, operator: u64, argument: impl FnOnce(&mut S) -> S::Word,
    miss: S::Miss) -> Option<S::Word> {
    let fallback_word = |stage: &mut S| {
        let zero = stage.constant(0);
        let fallback = stage.site(fallback);
        stage.add_imm(zero, fallback)
    };
    match class {
        FieldClass::Constant => Some(fallback_word(stage)),
        FieldClass::Passed => Some(argument(stage)),
        FieldClass::Defaulted => {
            let passed = argument(stage);
            let (undecided, truthy) = tagged_truthiness(stage, passed);
            let decided = stage.not(undecided);
            stage.guard(decided, miss)?;
            let fallback = fallback_word(stage);
            Some(stage.select(truthy, passed, fallback))
        }
        FieldClass::Computed => {
            let passed = argument(stage);
            let passed = Float64::guard(stage, passed, miss)?;
            let constant = fallback_word(stage);
            let constant = Float64::guard(stage, constant, miss)?;
            #[cfg(not(stencil_build))]
            return number_binary_word(stage, operator as u32, passed, constant, miss);
            #[cfg(stencil_build)]
            { let _ = (operator, passed, constant); None }
        }
    }
}

/// `operator` (a `Binary`'s Number operator, by `binary_operation`'s tables: an arithmetic, the remainder or a
/// bitwise) of two proved binary64s, boxed as `Value::number` boxes it: the Number a computed initializer field
/// stores (`field_value`). An operator with no arm here (`**`) leaves at `miss`.
#[cfg(not(stencil_build))]
#[inline(always)]
fn number_binary_word<S: NumericStage>(stage: &mut S, operator: u32, left: NumberOf<S>, right: NumberOf<S>,
    miss: S::Miss) -> Option<S::Word> {
    let number = if let Some(operation) = NumberArithmetic::from_operator(operator) {
        stage.float_arithmetic(operation, left.word(), right.word())
    } else if operator == crate::vm::operations::binary_operator::Remainder {
        stage.float_remainder(left.word(), right.word())
    } else if let Some(operation) = NumberBitwise::from_operator(operator) {
        let (left, right) = (Int32::truncating(stage, left, miss)?, Int32::truncating(stage, right, miss)?);
        let bits = stage.int_bitwise(operation, left.word(), right.word());
        // A zero-fill shift's result is a uint32; every other bitwise result an int32.
        if operation == NumberBitwise::ShiftRightUnsigned { stage.u32_to_f64(bits) } else { stage.i32_to_f64(bits) }
    } else {
        let zero = stage.constant(0);
        let never = stage.ne_imm(zero, 0u64);
        stage.guard(never, miss)?;
        return None;
    };
    Some(stage.box_number(Float64::of_row(number)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::staged::Concrete;
    use crate::Value;

    fn run(class: FieldClass, passed: Value) -> Option<Value> {
        let mut stage = Concrete::new(std::ptr::null());
        field_value(&mut stage, class, Value::number(2.5).to_bits(), 0, #[inline(always)] |stage| stage.constant(passed.to_bits()), ()).map(Value::from_bits)
    }

    #[test]
    fn each_class_reads_what_it_names() {
        let fallback = Value::number(2.5);
        assert_eq!(run(FieldClass::Passed, Value::integer(3)), Some(Value::integer(3)));
        assert_eq!(run(FieldClass::Constant, Value::integer(3)), Some(fallback));
        assert_eq!(run(FieldClass::Defaulted, Value::integer(3)), Some(Value::integer(3)));
        assert_eq!(run(FieldClass::Defaulted, Value::integer(0)), Some(fallback));
        assert_eq!(run(FieldClass::Defaulted, Value::UNDEFINED), Some(fallback));
        assert_eq!(run(FieldClass::Computed, Value::UNDEFINED), None);
    }

    #[test]
    fn a_computed_field_is_its_number_operator() {
        use crate::vm::operations::binary_operator::{Addition, ShiftRightZeroFill};
        let mut stage = Concrete::new(std::ptr::null());
        let mut computed = |operator: u32, passed: Value, constant: f64| field_value(&mut stage, FieldClass::Computed,
            Value::number(constant).to_bits(), u64::from(operator), |stage| stage.constant(passed.to_bits()), ()).map(Value::from_bits);
        assert_eq!(computed(Addition, Value::integer(3), 2.5), Some(Value::number(5.5)));
        assert_eq!(computed(ShiftRightZeroFill, Value::integer(-1), 0.0), Some(Value::number(4294967295.0)));
        assert_eq!(computed(Addition, Value::UNDEFINED, 1.0), None);
    }

    #[test]
    fn a_constant_field_never_fetches_its_argument() {
        let mut stage = Concrete::new(std::ptr::null());
        let fallback = Value::number(7.0);
        let value = field_value(&mut stage, FieldClass::Constant, fallback.to_bits(), 0, |_| panic!("fetched"), ());
        assert_eq!(value.map(Value::from_bits), Some(fallback));
    }
}
