//! The call protocol's frame hooks for a row's places (`staged::invoke::CallFrame`). The direct
//! call row (`call_rows.rs`) runs its callee identity, receiver binding and admission through
//! `staged::protocol`, and reads the caller's register file through this impl, so a row's frame
//! answers the file's base where the protocol's windows and registers live. The rest of the
//! hooks (room, entry, settle) stay the row's: the native frame push and the entry call are
//! the row's own code, since the body call is a `call reg` only a `call_*` row may make.

use crate::places::Places;
use crate::staged::{CallFrame, Concrete};

impl CallFrame<Concrete> for Places {
    /// The caller's register file: the frame word the row was entered with.
    #[inline(always)]
    fn call_registers(&mut self, _stage: &mut Concrete) -> Option<u64> {
        Some(self.s.frame as u64)
    }
}
