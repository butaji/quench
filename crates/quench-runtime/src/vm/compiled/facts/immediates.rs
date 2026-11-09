//! What a constant or immediate denotes, the one reading every tier uses.

use crate::bytecode::{Constant, Operand, OperandKind};
use crate::typed_ir::Type;
use crate::value::Value;

/// The value a constant denotes where it is an immediate (no heap allocation): numbers,
/// booleans, `null` and `undefined`.
pub(in crate::vm) fn immediate(constant: &Constant) -> Option<Value> {
    match constant {
        Constant::Number(number) => Some(Value::number(*number)),
        Constant::Boolean(value) => Some(Value::boolean(*value)),
        Constant::Null => Some(Value::NULL),
        Constant::Undefined => Some(Value::UNDEFINED),
        _ => None,
    }
}

/// A numeric constant is a static semantic value, regardless of machine view.
pub(in crate::vm) fn constant_number(constants: &[Constant], operand: Operand) -> Option<f64> {
    (operand.kind() == Some(OperandKind::Constant)).then_some(())?;
    constants.get(usize::from(operand.payload())).and_then(immediate).and_then(Value::as_number)
}

/// Whether an immediate is an int32 Number in its tagged encoding.
pub(in crate::vm) fn is_int32(value: Value) -> bool {
    Type::immediate(value).is_subtype(Type::INT32)
}

/// A constant's type: an immediate's, a string's or a BigInt's.
pub(in crate::vm) fn constant_type(constant: &Constant) -> Type {
    match constant {
        Constant::String(_) | Constant::StringUnits(_) => Type::language(crate::ValueType::String),
        Constant::BigInt(_) => Type::language(crate::ValueType::BigInt),
        constant => immediate(constant).map_or(Type::ANY, Type::immediate),
    }
}
