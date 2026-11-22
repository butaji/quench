//! Wasm atomic operations as data: the fence, which orders memory, and the access, a
//! read-modify-write, compare-exchange, wait or notify on the memory an instruction names. The
//! interpreter runs them over the instance's memory (`dispatch_wasm.rs`, `wasm_atomic_access`);
//! the tiers take no atomic step, so a function that has one stays interpreted.

use crate::bytecode::Op;

/// The atomic operations a Wasm function's instruction can be.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Atomic {
    /// `atomic.fence`: orders the memory accesses around it.
    Fence,
    /// An atomic access: its operator is the instruction's immediate, and its window of
    /// registers holds the memory, the address and the operator's values.
    Access,
}

impl Atomic {
    /// The atomic operation `op` is, if it is one.
    pub(crate) const fn of(op: Op) -> Option<Self> {
        match op {
            Op::WasmAtomicFence => Some(Self::Fence),
            Op::WasmAtomicAccess => Some(Self::Access),
            _ => None,
        }
    }
}
