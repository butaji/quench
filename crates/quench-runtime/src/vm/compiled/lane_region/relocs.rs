//! Run-time addresses in emitted code, each named by what it addresses. A live lowering embeds
//! the address as it is now; a linking lowering (`LaneHelpers::Linked`, `Bound`) emits a symbol
//! the loader patches, as the stencils' holes are (`native_stencil::hole_value`).
use super::*;

/// What a run-time address in emitted code addresses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub(super) enum RelocKind {
    /// The entry cell of function `key`.
    EntryCell,
    /// The `DirectCallee` descriptor of the call to function `key`.
    Descriptor,
    /// The dense capture words of function `key`.
    CaptureWords,
    /// The code's invalidation word.
    Invalidation,
    /// Slot `key` of the program's constant pool.
    ConstantPool,
}

impl Lowering<'_, '_> {
    /// The address of `kind` for `key`, which is `live` now. A live lowering embeds `live`; a
    /// linking one emits the symbol the loader resolves to the same address.
    pub(super) fn reloc(&mut self, kind: RelocKind, key: u32, live: u64) -> ir::Value {
        if matches!(self.helpers, LaneHelpers::Numeric | LaneHelpers::Process(_)) {
            return self.word_const(live as i64);
        }
        crate::vm::jit_backend::symbol_address(&mut self.builder, types::I64, kind as u32, key)
    }
}
