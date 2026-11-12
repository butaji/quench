//! A construct plan's field value as one staged row: a constant field's constant, a passed
//! field's argument, a defaulted field's argument or its fallback where that is falsy. The
//! interpreter reads each field's class from the plan at run time (`Concrete`); a specializer
//! holds the plan constant, so the class folds and a field costs its own arm alone. A computed
//! field (a Number operator over the argument) leaves at `miss` for its caller until its arithmetic row lands.

// Read by the u-jit and b-jit construct lowering (thread 13); the interpreter keeps its field loop,
// which measures cheaper per call than this row run on Concrete (224 against 289 Ir).
#![allow(dead_code)]

use super::Stage;
use super::value::tagged_truthiness;

/// How a plan stores one field.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum FieldClass {
    Passed,
    Constant,
    Defaulted,
    Computed,
}

/// The value of one field. `class`, `fallback` and `operator` are per-site operands; `argument` fetches the
/// parameter's value only on the arms that read it.
#[inline(always)]
pub(crate) fn field_value<S: Stage>(stage: &mut S, class: FieldClass, fallback: u64, operator: u64, argument: impl FnOnce(&mut S) -> S::Word,
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
            let constant = fallback_word(stage);
            let operator = stage.site(operator);
            let (value, computed) = stage.number_operator(operator, passed, constant);
            stage.guard(computed, miss)?;
            Some(value)
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
        field_value(&mut stage, class, Value::number(2.5).to_bits(), 0, |stage| stage.constant(passed.to_bits()), ()).map(Value::from_bits)
    }

    #[test]
    fn each_class_reads_what_it_names() {
        let fallback = Value::number(2.5);
        assert_eq!(run(FieldClass::Passed, Value::integer(3)), Some(Value::integer(3)));
        assert_eq!(run(FieldClass::Constant, Value::integer(3)), Some(fallback));
        assert_eq!(run(FieldClass::Defaulted, Value::integer(3)), Some(Value::integer(3)));
        assert_eq!(run(FieldClass::Defaulted, Value::integer(0)), Some(fallback));
        assert_eq!(run(FieldClass::Defaulted, Value::UNDEFINED), Some(fallback));
        assert_eq!(run(FieldClass::Computed, Value::integer(3)), None);
    }

    #[test]
    fn a_constant_field_never_fetches_its_argument() {
        let mut trace = crate::staged::trace::Trace::default();
        field_value(&mut trace, FieldClass::Constant, 7, 0, |_| panic!("fetched"), ());
        assert_eq!(trace.holes, [7]);
    }
}
