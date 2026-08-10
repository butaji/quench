//! VM helpers for executing residual operations.
pub use crate::vm::{
    copy_register, execute as run_vm, execute_builtin_with_receiver, execute_in_place,
    execute_with_registers, get_property, get_property_result, is_truthy, read_register,
    write_value, VmError,
};
