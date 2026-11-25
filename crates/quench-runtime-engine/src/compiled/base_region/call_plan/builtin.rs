//! The builtins a call site's callee may be: the natives the interpreter runs in place by identity
//! (`staged::native_row`). A b-jit region declines a call of one (`call_plan::decline_builtin`).

#[allow(unused_imports)]
use crate::ext_traits::*;
use quench_runtime::tier_surface::Native;
use quench_runtime::staged::native_row::NativeRow;

/// A call whose callee is the builtin `row`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct BuiltinCall {
    /// The builtin.
    pub(crate) row: NativeRow,
}

/// The builtin `native` is, if it is one: the runtime's one map (`staged::native_row`).
pub(crate) fn of(native: Native) -> Option<NativeRow> {
    quench_runtime::staged::native_row::native_row_of(native)
}
