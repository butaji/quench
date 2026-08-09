//! VM helpers for executing residual operations.
pub use crate::vm::{
    copy_register, execute, execute_builtin_with_receiver, execute_in_place,
    execute_with_registers, get_property, is_truthy, read_register, write_value, VmError,
};
