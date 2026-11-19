//! The builtins a b-jit call runs on their stencil rows (`quench_bjit::call::BuiltinRow`): the natives
//! the u-jit runs inline by identity (`lane_region::native_calls::Inline::of`, `builtin_calls::Builtin::of`),
//! as a call site names them. The array builtins (`push`, `pop`) run on their staged rows too, which
//! miss to the helper row where the staged body leaves.

use crate::heap::Native;
use crate::staged::call_native::MathCall;
use crate::vm::number::math_call;
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
    match math_call(native)? {
        MathCall::Imul => Some(BuiltinRow::Imul),
        _ => None,
    }
}
