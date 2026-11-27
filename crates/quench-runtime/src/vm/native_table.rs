//! The natives' function table. `Vm::call_native_dispatch` runs a native by indexing this table
//! with its discriminant, so a call makes no family match. Each entry is the native's entry
//! (`operations::native_entry`); the table is built once per process, from `Native::ALL`.

use super::*;
use std::sync::OnceLock;

/// The entry that runs a native's call with its receiver and arguments.
pub type NativeFn = fn(&mut Vm, &ResidualProgram, Native, Value, &[Value]) -> Result<Value, JsError>;

/// Each native's entry, by discriminant.
pub(super) struct NativeTable {
    entries: [Option<NativeFn>; Native::COUNT as usize],
}

impl NativeTable {
    /// The table of every native: the one every runtime dispatches through.
    pub(super) fn all() -> &'static NativeTable {
        static ALL: OnceLock<NativeTable> = OnceLock::new();
        ALL.get_or_init(|| NativeTable {
            entries: Native::ALL.map(|native| Some(operations::native_entry(native))),
        })
    }

    /// The table of a closed world: the entries of `natives` alone, every other native `None`. The
    /// image runs one table for its process, so the table is leaked once per image runtime.
    pub(crate) fn closed(natives: &[Native]) -> &'static NativeTable {
        let mut entries = [None; Native::COUNT as usize];
        for &native in natives {
            entries[native as usize] = Some(operations::native_entry(native));
        }
        Box::leak(Box::new(NativeTable { entries }))
    }

    /// The entry that runs `native`, or `None` when the table holds none for it.
    #[inline(always)]
    pub(super) fn entry(&self, native: Native) -> Option<NativeFn> {
        self.entries[native as usize]
    }
}
