//! Array element accesses in native code past a present dense element, answered
//! by the interpreter's own element steps without the exact step: a read the array and its
//! prototypes do not hold (`Vm::absent_array_element`) and a plain store
//! (`Vm::store_plain_element`). Anything else takes the step at the same PC.
use super::{Host, Vm};
use crate::Value;

/// `Vm::absent_array_element` of `base` and `key`, else `Value::DELETED`.
///
/// # Safety
/// `vm` is the running VM, live and unborrowed for the call.
pub(super) unsafe extern "C" fn general_element_get<H: Host>(vm: u64, _residual: u64, base: u64, key: u64) -> u64 {
    // SAFETY: as the caller promises; the read allocates nothing.
    let vm = unsafe { &*(vm as *const Vm<H>) };
    vm.absent_array_element(Value::from_bits(base), Value::from_bits(key)).unwrap_or(Value::DELETED).to_bits()
}
