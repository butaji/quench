//! The builtins a b-jit call runs on their stencil rows (`quench_bjit::call::BuiltinRow`): the natives
//! the u-jit runs inline by identity (`lane_region::native_calls::Inline::of`, `builtin_calls::Builtin::of`),
//! as a call site names them. The array builtins (`push`, `pop`) run on their staged rows too, which
//! miss to the helper row where the staged body leaves.

use crate::staged::math::FloatUnary;
use crate::heap::Native;
use crate::vm::number::{MathFunction, math_function};
use quench_bjit::call::BuiltinRow;

/// A call whose callee the code guards as the builtin `row`: its identity is the word the callee
/// register must hold.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in super::super) struct BuiltinCall {
    /// The callee's identity word (the function value's bits).
    pub(in super::super) identity: u64,
    /// The row the call runs.
    pub(in super::super) row: BuiltinRow,
}

/// The row `native` runs on, if the b-jit runs it on one.
pub(in super::super) fn of(native: Native) -> Option<BuiltinRow> {
    match native {
        Native::StringCharCodeAt => return Some(BuiltinRow::CharCodeAt),
        Native::StringCharAt => return Some(BuiltinRow::CharAt),
        Native::NumberIsInteger => return Some(BuiltinRow::IsInteger),
        Native::ArrayPush => return Some(BuiltinRow::Push),
        Native::ArrayPop => return Some(BuiltinRow::Pop),
        _ => {}
    }
    match math_function(native)? {
        MathFunction::Row(operation) => unary_row(operation),
        MathFunction::Extreme(maximum) => Some(if maximum { BuiltinRow::Max } else { BuiltinRow::Min }),
        MathFunction::Imul => Some(BuiltinRow::Imul),
        _ => None,
    }
}

/// The row of a machine Math function of one Number (`staged::math::float_unary`).
fn unary_row(operation: FloatUnary) -> Option<BuiltinRow> {
    Some(match operation {
        FloatUnary::Floor => BuiltinRow::Floor,
        FloatUnary::Ceil => BuiltinRow::Ceil,
        FloatUnary::Trunc => BuiltinRow::Trunc,
        FloatUnary::Abs => BuiltinRow::Abs,
        FloatUnary::Sqrt => BuiltinRow::Sqrt,
        FloatUnary::Fround => BuiltinRow::Fround,
        FloatUnary::Round => BuiltinRow::Round,
        _ => return None,
    })
}
