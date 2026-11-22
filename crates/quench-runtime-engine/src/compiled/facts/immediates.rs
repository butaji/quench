//! What a constant or immediate denotes, the one reading every tier uses.

#[allow(unused_imports)]
use crate::ext_traits::*;
use quench_runtime::bytecode::{Constant, Operand, OperandKind};
use quench_runtime::typed_ir::{IntRange, Type};
use quench_runtime::value::Value;

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

/// A constant's type: an immediate's (an int32 number, its own range), a string's or a BigInt's.
pub(crate) fn constant_type(constant: &Constant) -> Type {
    match constant {
        Constant::String(_) | Constant::StringUnits(_) => Type::language(quench_runtime::ValueType::String),
        Constant::BigInt(_) => Type::language(quench_runtime::ValueType::BigInt),
        constant => immediate(constant).map_or(Type::ANY, |value|
            value.as_int().map_or_else(|| Type::immediate(value), |int| Type::int(IntRange::singleton(int)))),
    }
}
