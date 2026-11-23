//! Run-time addresses in emitted code, each named by what it addresses. A live lowering embeds
//! the address as it is now; a linking lowering (`LaneHelpers::Linked`, `Bound`) emits a symbol
//! the loader patches (`aot_image::live` and `object` bind them).
#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;
use super::super::object_slots::emit;
use quench_runtime::staged::machine::{Lane, MachineScalar};

/// What a run-time address in emitted code addresses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub(crate) enum RelocKind {
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
    /// The atom of the program's table numbered `key`, in the VM that loads the code.
    Atom,
    /// The heap value numbered `key` of the image's recipes, built in the VM that loads the code.
    HeapValue,
    /// The shape numbered `key` of the image's recipes, interned in the VM that loads the code.
    Shape,
}

impl RelocKind {
    /// Whether `kind` names a *word* the code reads (`Atom`'s number, a heap value's bits, a shape's
    /// id) rather than an address. AArch64's page-relative reference cannot carry a value, so a
    /// linking lowering loads one from the table the loader fills beside the code.
    pub(crate) const fn is_value(self) -> bool {
        matches!(self, Self::Atom | Self::HeapValue | Self::Shape)
    }
}

/// The address in this process of the symbol `(namespace, key)` a numeric image names, which is
/// all an image of a frameless Number region can name.
pub(crate) fn numeric_symbol(namespace: u32, key: u32) -> Option<u64> {
    (namespace == RelocKind::Helper as u32 && key == LaneHelper::InterruptOwner as u32)
        .then_some(super::super::interrupt_owner_in_lane as *const () as usize as u64)
}

/// The helper number a symbol `(namespace, key)` names, if it names one.
pub(crate) fn helper_key(namespace: u32, key: u32) -> Option<usize> {
    (namespace == RelocKind::Helper as u32).then_some(key as usize)
}

/// The atom number a symbol `(namespace, key)` names, if it names one.
pub(crate) fn atom_key(namespace: u32, key: u32) -> Option<u32> {
    (namespace == RelocKind::Atom as u32).then_some(key)
}

/// The symbol of the atom numbered `atom`, which an ahead image's loader resolves to that atom in its VM.
pub(crate) fn atom_symbol(builder: &mut FunctionBuilder<'_>, atom: u32) -> ir::Value {
    crate::symbol_words::symbol_address(builder, RelocKind::Atom as u32, atom)
}

/// The address of `helper` as the symbol a frameless Number region's loader resolves.
pub(crate) fn helper_symbol(builder: &mut FunctionBuilder<'_>, helper: LaneHelper) -> ir::Value {
    crate::symbol_words::symbol_address(builder, RelocKind::Helper as u32, helper as u32)
}

impl Lowering<'_, '_> {
    /// The word of `atom`: its id now for a live lowering, its symbol for a linking one.
    pub(crate) fn atom_word(&mut self, atom: u32) -> ir::Value {
        self.helpers.atom_word(&mut self.builder, atom)
    }

    /// Whether no method cache names `atom`, from the payload's atom bits.
    pub(crate) fn atom_unnamed(&mut self, atom: u32) -> ir::Value {
        if !self.helpers.links() { return super::super::general_store::emit_atom_unnamed(&mut self.builder, self.payload, atom); }
        let words = emit_js_payload_word(&mut self.builder, self.payload, std::mem::offset_of!(JsStepDispatch, head.method_cache_atoms));
        let length = emit_js_payload_word(&mut self.builder, self.payload, std::mem::offset_of!(JsStepDispatch, head.method_cache_atom_words));
        let atom = self.atom_word(atom);
        crate::symbol_words::atom_unnamed(&mut self.builder, (words, length), atom)
    }


    /// The address of `kind` for `key`, which is `live` now (`address`).
    pub(crate) fn reloc(&mut self, kind: RelocKind, key: u32, live: u64) -> ir::Value {
        address(self.helpers, &mut self.builder, kind, key, live)
    }
}

/// The address of `kind` for `key`, which is `live` now. A live lowering embeds `live`; a
/// linking one emits the symbol the loader resolves to the same address.
pub(crate) fn address(helpers: LaneHelpers, builder: &mut FunctionBuilder<'_>, kind: RelocKind, key: u32, live: u64) -> ir::Value {
    if !helpers.links() {
        return emit(builder).raw_const(Lane::I64, live as i64);
    }
    let symbol = crate::symbol_words::symbol_address(builder, kind as u32, key);
    // A value's symbol is the address of its word in the loader's table (`live::place_values`).
    if kind.is_value() {
        let mut stage = emit(builder);
        return stage.raw_load(Lane::I64, super::super::invariant(), symbol, 0);
    }
    symbol
}
