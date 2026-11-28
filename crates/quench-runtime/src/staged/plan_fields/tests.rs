use super::*;
use crate::staged::{Concrete, Stage};
use crate::Value;

fn run(class: FieldClass, passed: Value) -> Option<Value> {
    let mut stage = Concrete::pure();
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
    let mut stage = Concrete::pure();
    let mut computed = |operator: u32, passed: Value, constant: f64| field_value(&mut stage, FieldClass::Computed,
        Value::number(constant).to_bits(), u64::from(operator), |stage| stage.constant(passed.to_bits()), ()).map(Value::from_bits);
    assert_eq!(computed(Addition, Value::integer(3), 2.5), Some(Value::number(5.5)));
    assert_eq!(computed(ShiftRightZeroFill, Value::integer(-1), 0.0), Some(Value::number(4294967295.0)));
    assert_eq!(computed(Addition, Value::UNDEFINED, 1.0), None);
}

#[test]
fn a_constant_field_never_fetches_its_argument() {
    let mut stage = Concrete::pure();
    let fallback = Value::number(7.0);
    let value = field_value(&mut stage, FieldClass::Constant, fallback.to_bits(), 0, |_| panic!("fetched"), ());
    assert_eq!(value.map(Value::from_bits), Some(fallback));
}
