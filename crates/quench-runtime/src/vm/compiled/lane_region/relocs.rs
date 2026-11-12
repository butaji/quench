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
    /// The address of `LaneHelper` number `key`.
    Helper,
    /// Slot `key` of the program's constant pool.
    ConstantPool,
}

/// The address in this process of the symbol `(namespace, key)` a numeric image names, which is
/// all an image of a frameless Number region can name.
pub(in crate::vm::compiled) fn numeric_symbol(namespace: u32, key: u32) -> Option<u64> {
    (namespace == RelocKind::Helper as u32 && key == LaneHelper::InterruptOwner as u32)
        .then_some(super::super::lane_interrupt_owner as *const () as usize as u64)
}

/// The helper number a symbol `(namespace, key)` names, if it names one.
pub(in crate::vm::compiled) fn helper_key(namespace: u32, key: u32) -> Option<usize> {
    (namespace == RelocKind::Helper as u32).then_some(key as usize)
}

/// The address of `helper` as the symbol a frameless Number region's loader resolves.
pub(in crate::vm::compiled) fn helper_symbol(builder: &mut FunctionBuilder<'_>, helper: LaneHelper) -> ir::Value {
    crate::vm::jit_backend::symbol_address(builder, types::I64, RelocKind::Helper as u32, helper as u32)
}

impl Lowering<'_, '_> {
    /// The address of `kind` for `key`, which is `live` now. A live lowering embeds `live`; a
    /// linking one emits the symbol the loader resolves to the same address.
    pub(super) fn reloc(&mut self, kind: RelocKind, key: u32, live: u64) -> ir::Value {
        if !self.helpers.links() {
            return self.word_const(live as i64);
        }
        crate::vm::jit_backend::symbol_address(&mut self.builder, types::I64, kind as u32, key)
    }
}
