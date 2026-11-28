//! What a constant or immediate denotes, the one reading every tier uses.

#[allow(unused_imports)]
use crate::ext_traits::*;
use quench_runtime::bytecode::{Constant, Operand, OperandKind};
use quench_runtime::typed_ir::Type;
use quench_runtime::value::Value;
#[allow(unused_imports)] use quench_runtime::staged::glue_prelude::*;

/// The value a constant denotes where it is an immediate (no heap allocation): numbers,
/// booleans, `null` and `undefined`.
pub(crate) fn immediate(constant: &Constant) -> Option<Value> {
    match constant {
        Constant::Number(number) => Some(Value::number(*number)),
        Constant::Boolean(value) => Some(Value::boolean(*value)),
        Constant::Null => Some(Value::NULL),
        Constant::Undefined => Some(Value::UNDEFINED),
        _ => None,
    }
}

/// A numeric constant is a static semantic value, regardless of machine view.
pub(crate) fn constant_number(constants: &[Constant], operand: Operand) -> Option<f64> {
    (operand.kind() == Some(OperandKind::Constant)).then_some(())?;
    constants.get(usize::from(operand.payload())).and_then(immediate).and_then(Value::as_number)
}

/// Whether an immediate is an int32 Number in its tagged encoding.
pub(crate) fn is_int32(value: Value) -> bool {
    Type::immediate(value).is_subtype(Type::INT32)
}
