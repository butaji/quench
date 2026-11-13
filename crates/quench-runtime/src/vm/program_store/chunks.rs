//! What a stored program keeps for the first-call chunks appended to it
//! (`Vm::append_first_chunk`). Every table the VM keeps per function, constant or site of a
//! program is laid out for the capacity its residual reserved, at its install, so appending a
//! chunk moves none of them (`ProgramStore::insert_shared`); the values of the constants it
//! adds are all it needs.
use super::*;

impl ProgramStore {
    /// Whether chunks may be appended to program `id`, whose residual is `p`: `p` is what the
    /// store holds, and nothing was derived from its function set that a new function would
    /// escape (compiled or linked code names every function of the program).
    pub(in crate::vm) fn accepts_chunks(&self, id: ProgramId, p: &ResidualProgram) -> bool {
        let Some(entry) = self.entry(id) else { return false };
        #[cfg(feature = "native-execution")]
        if entry.native_js.is_some() {
            return false;
        }
        std::ptr::eq(&**entry.residual, p) && entry.compiled.is_none() && entry.wasm_signatures.is_none()
    }

    /// Adds `values`, the materialized constants a chunk appended to program `id`, to its own.
    pub(in crate::vm) fn push_constants(&mut self, id: ProgramId, values: impl ExactSizeIterator<Item = Value>) {
        if let Some(entry) = self.entry_mut(id) {
            debug_assert!(entry.constants.capacity() - entry.constants.len() >= values.len(), "a program's constants never move");
            entry.constants.extend(values);
        }
    }
}
