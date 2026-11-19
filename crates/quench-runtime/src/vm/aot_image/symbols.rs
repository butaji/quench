//! The image's symbol space: every run-time address its code names. It extends
//! `lane_region::relocs::RelocKind` (classes A and B: entry cells, descriptors, capture words,
//! the invalidation word, helpers, the constant pool, atoms) with the base body's PC entries,
//! the semantic entries of the generic row, and classes C and D: heap values and shapes, which
//! the loader builds from recipes in the VM that runs the image. A symbol is
//! `(RelocKind as u32, key)`, the pair `RelocatableCode` relocations carry.
//!
//! Owner: relocation classes C/D (heap values, shapes) and the table; base and join add their
//! kinds through `SymbolTable::intern`.

use crate::bytecode::ResidualProgram;

/// What a run-time address in image code addresses. Class A/B variants mirror `RelocKind`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum Symbol {
    EntryCell(u32),
    Descriptor(u32),
    CaptureWords(u32),
    Invalidation,
    Helper(u32),
    ConstantPool(u32),
    Atom(u32),
    /// The monomorphized semantic entry of opcode number `op` (`Op::native_semantic_entry`).
    SemanticEntry(u16),
    /// The base body of `function` entered at record `pc` (a lane leave's continuation).
    BasePc { function: u32, pc: u32 },
    /// Class C: heap value number `key` of `AotImage::heap_values`.
    HeapValue(u32),
    /// Class D: shape number `key` of `AotImage::shapes`.
    Shape(u32),
}

/// How the loader finds a heap value the code folded, in the VM that runs the image.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum HeapValueRecipe {
    /// The realm intrinsic numbered `id` (`%Array.prototype%` and the like).
    Intrinsic(u32),
    /// The value of global binding `atom` once the realm is initialized.
    Global(u32),
    /// The program constant in pool slot `slot` (`FrozenAssumptions::constants`).
    Constant(u32),
    /// The `prototype` object of function `function`'s closure in the root activation.
    PrototypeOf(u32),
}

/// How the loader builds a shape the code folded: the prototype it has and its own fields in
/// insertion order, each by atom key and attribute bits, interned in the VM's shape tree.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct ShapeRecipe {
    pub(crate) prototype: Option<u32>,
    pub(crate) fields: Box<[(u32, u8)]>,
}

/// Interns symbols, recipes and atoms while an image lowers; one per image.
pub(crate) struct SymbolTable {
    shapes: Vec<ShapeRecipe>,
    heap_values: Vec<HeapValueRecipe>,
    atoms: Vec<Box<str>>,
}

impl SymbolTable {
    pub(crate) fn new(_program: &ResidualProgram) -> Self {
        Self { shapes: Vec::new(), heap_values: Vec::new(), atoms: Vec::new() }
    }

    /// The `(namespace, key)` pair code emits for `symbol`; recipes are interned once.
    pub(crate) fn intern(&mut self, symbol: Symbol) -> (u32, u32) {
        todo!("relocation classes C/D: {symbol:?}")
    }

    pub(crate) fn heap_value(&mut self, recipe: HeapValueRecipe) -> Symbol {
        todo!("relocation classes C/D: {recipe:?}")
    }

    pub(crate) fn shape(&mut self, recipe: ShapeRecipe) -> Symbol {
        todo!("relocation classes C/D: {recipe:?}")
    }

    /// The symbol `(namespace, key)` names, the inverse of `intern`.
    pub(crate) fn decode(namespace: u32, key: u32) -> Option<Symbol> {
        todo!("relocation classes C/D: ({namespace}, {key})")
    }

    pub(crate) fn finish(self) -> (Vec<ShapeRecipe>, Vec<HeapValueRecipe>, Vec<Box<str>>) {
        (self.shapes, self.heap_values, self.atoms)
    }
}
