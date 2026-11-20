//! The image's symbol space: every run-time address its code names. It extends
//! `lane_region::relocs::RelocKind` (classes A and B: entry cells, descriptors, capture words,
//! the invalidation word, helpers, the constant pool, atoms) with the base body's PC entries,
//! the semantic entries of the generic row, and classes C and D: heap values and shapes, which
//! the loader builds from recipes in the VM that runs the image. A symbol is
//! `(namespace, key)`, the pair `RelocatableCode` relocations carry: the namespace is the
//! `RelocKind` discriminant for classes A to D, and the key is the symbol's number.
//!
//! Owner: relocation classes C/D (heap values, shapes) and the table; base and join add their
//! kinds through `SymbolTable::intern`.

use super::AotImage;
use crate::bytecode::ResidualProgram;
use crate::vm::compiled::RelocKind;
use crate::vm::{DataAttributes, PropertyKey, Vm};
use crate::{Host, Value};
use rustc_hash::FxHashMap;
use std::hash::Hash;

/// The namespaces of the symbols no `RelocKind` names, above every kind: image code's own
/// relocations use the kinds' numbers, so these cannot be derived from the enum's last variant
/// (adding one would silently take over the next symbol kind) and must be disjoint from it.
const SEMANTIC_ENTRY_NAMESPACE: u32 = u32::MAX - 1;
/// The namespace of `Symbol::BasePc`, whose key is its label in the image's label list.
const BASE_PC_NAMESPACE: u32 = u32::MAX;

/// The attribute bits of a `ShapeRecipe` field, as `DataAttributes` reads them.
pub(crate) const ATTRIBUTE_WRITABLE: u8 = 1 << 0;
pub(crate) const ATTRIBUTE_ENUMERABLE: u8 = 1 << 1;
pub(crate) const ATTRIBUTE_CONFIGURABLE: u8 = 1 << 2;

/// What a run-time address in image code addresses. Class A/B variants mirror `RelocKind`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum Symbol {
    EntryCell(u32),
    Descriptor(u32),
    CaptureWords(u32),
    Invalidation,
    Helper(u32),
    /// The helper table itself (`RelocKind::HelperTable`): the address an image loads a helper from.
    HelperTable,
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
    /// The realm intrinsic numbered `id` (`%Array.prototype%` and the like): `Native::ALL[id]`.
    Intrinsic(u32),
    /// The value of the global binding named by image atom `atom` once the realm is initialized.
    Global(u32),
    /// The program constant in pool slot `slot` (`FrozenAssumptions::constants`).
    Constant(u32),
    /// The `prototype` object of function `function`'s closure in the root activation.
    PrototypeOf(u32),
}

/// How the loader builds a shape the code folded: the prototype it has and its own fields in
/// insertion order, each by image atom key and attribute bits, interned in the VM's shape tree.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct ShapeRecipe {
    /// The `HeapValueRecipe` key of the prototype, or `None` for a null prototype.
    pub(crate) prototype: Option<u32>,
    pub(crate) fields: Box<[(u32, u8)]>,
}

/// Recipes or labels interned to dense keys: one key per distinct value, in first-seen order.
struct Interned<T> {
    values: Vec<T>,
    keys: FxHashMap<T, u32>,
}

impl<T> Default for Interned<T> {
    fn default() -> Self {
        Self { values: Vec::new(), keys: FxHashMap::default() }
    }
}

impl<T: Clone + Eq + Hash> Interned<T> {
    fn key(&mut self, value: T) -> u32 {
        if let Some(&key) = self.keys.get(&value) {
            return key;
        }
        let key = u32::try_from(self.values.len()).expect("symbol keys fit u32");
        self.keys.insert(value.clone(), key);
        self.values.push(value);
        key
    }
}

/// Interns symbols, recipes and atoms while an image lowers; one per image.
pub(crate) struct SymbolTable {
    shapes: Interned<ShapeRecipe>,
    heap_values: Interned<HeapValueRecipe>,
    atoms: Vec<Box<str>>,
    labels: Interned<(u32, u32)>,
}

impl SymbolTable {
    pub(crate) fn new(_program: &ResidualProgram) -> Self {
        Self { shapes: Interned::default(), heap_values: Interned::default(), atoms: Vec::new(), labels: Interned::default() }
    }

    /// The `(namespace, key)` pair code emits for `symbol`; a base PC's key is its interned label.
    pub(crate) fn intern(&mut self, symbol: Symbol) -> (u32, u32) {
        match symbol {
            Symbol::BasePc { function, pc } => (BASE_PC_NAMESPACE, self.labels.key((function, pc))),
            other => (other.namespace(), other.key()),
        }
    }

    pub(crate) fn heap_value(&mut self, recipe: HeapValueRecipe) -> Symbol {
        Symbol::HeapValue(self.heap_values.key(recipe))
    }

    pub(crate) fn shape(&mut self, recipe: ShapeRecipe) -> Symbol {
        Symbol::Shape(self.shapes.key(recipe))
    }

    /// The symbol `(namespace, key)` names, the inverse of `intern`; `labels` is the image's label list.
    pub(crate) fn decode(labels: &[(u32, u32)], namespace: u32, key: u32) -> Option<Symbol> {
        match namespace {
            BASE_PC_NAMESPACE => labels.get(key as usize).map(|&(function, pc)| Symbol::BasePc { function, pc }),
            SEMANTIC_ENTRY_NAMESPACE => u16::try_from(key).ok().map(Symbol::SemanticEntry),
            _ => KEYED.iter().find(|(kind, _)| *kind == namespace).map(|(_, make)| make(key)),
        }
    }

    /// The recipe tables and the label list the image carries: shapes, heap values, atom texts, labels.
    pub(crate) fn finish(self) -> (Vec<ShapeRecipe>, Vec<HeapValueRecipe>, Vec<Box<str>>, Vec<(u32, u32)>) {
        (self.shapes.values, self.heap_values.values, self.atoms, self.labels.values)
    }
}

/// The helper table names no key: one symbol for the whole table.
fn helper_table(_key: u32) -> Symbol { Symbol::HelperTable }

/// The constructor of each symbol kind keyed by its own number, by namespace.
const KEYED: [(u32, fn(u32) -> Symbol); 10] = [
    (RelocKind::EntryCell as u32, Symbol::EntryCell),
    (RelocKind::Descriptor as u32, Symbol::Descriptor),
    (RelocKind::CaptureWords as u32, Symbol::CaptureWords),
    (RelocKind::Invalidation as u32, invalidation),
    (RelocKind::Helper as u32, Symbol::Helper),
    (RelocKind::HelperTable as u32, helper_table),
    (RelocKind::ConstantPool as u32, Symbol::ConstantPool),
    (RelocKind::Atom as u32, Symbol::Atom),
    (RelocKind::HeapValue as u32, Symbol::HeapValue),
    (RelocKind::Shape as u32, Symbol::Shape),
];

/// The invalidation word's symbol: one word, keyed 0.
fn invalidation(_key: u32) -> Symbol {
    Symbol::Invalidation
}

impl Symbol {
    fn namespace(self) -> u32 {
        match self {
            Self::EntryCell(_) => RelocKind::EntryCell as u32,
            Self::Descriptor(_) => RelocKind::Descriptor as u32,
            Self::CaptureWords(_) => RelocKind::CaptureWords as u32,
            Self::Invalidation => RelocKind::Invalidation as u32,
            Self::Helper(_) => RelocKind::Helper as u32,
            Self::HelperTable => RelocKind::HelperTable as u32,
            Self::ConstantPool(_) => RelocKind::ConstantPool as u32,
            Self::Atom(_) => RelocKind::Atom as u32,
            Self::HeapValue(_) => RelocKind::HeapValue as u32,
            Self::Shape(_) => RelocKind::Shape as u32,
            Self::SemanticEntry(_) => SEMANTIC_ENTRY_NAMESPACE,
            Self::BasePc { .. } => BASE_PC_NAMESPACE,
        }
    }

    /// The key of a symbol keyed by its own number (a base PC is keyed by its label, `intern`).
    fn key(self) -> u32 {
        match self {
            Self::HelperTable => 0,
            Self::EntryCell(key) | Self::Descriptor(key) | Self::CaptureWords(key) | Self::Helper(key)
                | Self::ConstantPool(key) | Self::Atom(key) | Self::HeapValue(key) | Self::Shape(key) => key,
            Self::Invalidation => 0,
            Self::SemanticEntry(op) => u32::from(op),
            Self::BasePc { .. } => unreachable!("a base PC is keyed by its label"),
        }
    }
}

/// The word of the heap value or shape `symbol` names, built in `vm` from `image`'s recipes: the
/// word a live lowering would embed (a heap value's bits, a shape's id). `None` for any other
/// symbol, and for a recipe `vm` cannot build from the image alone (`Constant`, `PrototypeOf`).
pub(crate) fn resolve_recipe<H: Host>(symbol: Symbol, image: &AotImage, vm: &mut Vm<H>) -> Option<u64> {
    match symbol {
        Symbol::HeapValue(key) => Some(heap_value(image, vm, key)?.to_bits()),
        Symbol::Shape(key) => Some(u64::from(shape(image, vm, key)?)),
        _ => None,
    }
}

/// The value of heap value recipe `key`, in `vm`.
fn heap_value<H: Host>(image: &AotImage, vm: &mut Vm<H>, key: u32) -> Option<Value> {
    match *image.heap_values.get(key as usize)? {
        HeapValueRecipe::Intrinsic(id) => Some(vm.native_value(*crate::heap::Native::ALL.get(id as usize)?)),
        HeapValueRecipe::Global(atom) => {
            let atom = vm.intern_atom(image.atoms.get(atom as usize)?);
            let globals = vm.realm.globals;
            vm.own_property(globals, atom)
        }
        HeapValueRecipe::Constant(_) | HeapValueRecipe::PrototypeOf(_) => None,
    }
}

/// The shape of shape recipe `key`: the empty object of its prototype, then each field added
/// in order, along the VM's transitions.
fn shape<H: Host>(image: &AotImage, vm: &mut Vm<H>, key: u32) -> Option<u32> {
    let recipe = image.shapes.get(key as usize)?;
    let prototype = match recipe.prototype {
        Some(prototype) => heap_value(image, vm, prototype)?,
        None => Value::NULL,
    };
    let mut shape = Vm::<H>::empty_object(prototype).shape();
    for &(atom, attributes) in recipe.fields.iter() {
        let atom = vm.intern_atom(image.atoms.get(atom as usize)?);
        shape = vm.transition_data_property_shape(shape, PropertyKey::string(atom), data_attributes(attributes));
    }
    Some(shape)
}

/// The data attributes of a field's attribute bits.
fn data_attributes(bits: u8) -> DataAttributes {
    DataAttributes {
        writable: bits & ATTRIBUTE_WRITABLE != 0,
        enumerable: bits & ATTRIBUTE_ENUMERABLE != 0,
        configurable: bits & ATTRIBUTE_CONFIGURABLE != 0,
    }
}
