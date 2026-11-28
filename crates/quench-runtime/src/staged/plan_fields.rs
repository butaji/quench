//! A construct plan's field value as one staged row: a constant field's constant, a passed
//! field's argument, a defaulted field's argument or its fallback where that is falsy. The
//! interpreter reads each field's class from the plan at run time (`Concrete`); a specializer
//! holds the plan constant, so the class folds and a field costs its own arm alone. A computed
//! field is a Number operator over the argument and its constant (`numeric::binary64_value`); an
//! argument that is no Number leaves at `miss`.

// Read by the lane construct plan (`vm/dispatch_fast/construct_plan.rs`) and the u-jit and b-jit construct lowering.

use super::{Float64, NumericStage};
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
            return super::numeric::binary64_value(stage, operator as u32, passed, constant, miss);
            #[cfg(stencil_build)]
            { let _ = (operator, passed, constant); None }
        }
    }
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
