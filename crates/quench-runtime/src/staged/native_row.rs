//! The natives the interpreter runs in place by identity (`native_row_table`), and the one map from a
//! `Native` to its row (`native_row_of`). Each row's steps are the shared staged ones
//! (`strings::step_string_native_of`, `call_native::step_number_native_of`, `builtins::step_array_native_of`);
//! the b-jit's planner reads the map to decline a region holding such a call (`call_plan::decline_builtin`).

pub use super::native_row_table::NativeRow;

/// The row `native` runs on, if it is one: the one map from natives to rows.
pub const fn native_row_of(native: crate::heap::Native) -> Option<NativeRow> {
    use crate::heap::Native;
    Some(match native {
        Native::StringCharCodeAt => NativeRow::CharCodeAt,
        Native::StringCharAt => NativeRow::CharAt,
        Native::NumberIsInteger => NativeRow::IsInteger,
        Native::MathImul => NativeRow::Imul,
        Native::ArrayPush => NativeRow::Push,
        Native::ArrayPop => NativeRow::Pop,
        _ => return None,
    })
}
