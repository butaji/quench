//! The image's symbol space: every run-time address its code names. It extends
//! `lane_region::relocs::RelocKind` (classes A and B: entry cells, descriptors, capture words,
//! the invalidation word, helpers, the constant pool, atoms) with the base body's PC entries,
//! the semantic entries and operand records of the generic row, the call records of the calls the
//! closed world proves (`Symbol::CallRecord`), and classes C and D: heap values and shapes, which
//! the loader builds from recipes in the VM that runs the image. A symbol is
//! `(namespace, key)`, the pair `RelocatableCode` relocations carry: the namespace is the
//! `RelocKind` discriminant for classes A to D, and the key is the symbol's number.
//!
//! Owner: relocation classes C/D (heap values, shapes) and the table; base and join add their
//! kinds through `SymbolTable::intern`.

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::AotImage;
use quench_runtime::bytecode::ResidualProgram;
use crate::compiled::RelocKind;
use quench_runtime::vm::{DataAttributes, PropertyKey, Vm};
use quench_runtime::Value;
use rustc_hash::FxHashMap;
use std::hash::Hash;

/// The namespace of `Symbol::SemanticEntry`: the first number after the `RelocKind` ones.
const SEMANTIC_ENTRY_NAMESPACE: u32 = RelocKind::Shape as u32 + 1;
/// The namespace of `Symbol::BasePc`, whose key is its label in the image's label list.
const BASE_PC_NAMESPACE: u32 = SEMANTIC_ENTRY_NAMESPACE + 1;
/// The namespace of `Symbol::Operands`, whose key is its record in the image's operand table.
pub(crate) const OPERANDS_NAMESPACE: u32 = BASE_PC_NAMESPACE + 1;
/// The namespace of `Symbol::CallRecord`, whose key is its site in the image's call table.
pub(crate) const CALL_RECORD_NAMESPACE: u32 = OPERANDS_NAMESPACE + 1;
/// The namespace of `Symbol::NarrowUpperWords`, one table keyed 0.
const NARROW_WORDS_NAMESPACE: u32 = CALL_RECORD_NAMESPACE + 1;
/// The namespace of `Symbol::Attention`, one byte keyed 0.
const ATTENTION_NAMESPACE: u32 = NARROW_WORDS_NAMESPACE + 1;

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
    ConstantPool(u32),
    Atom(u32),
    /// The monomorphized semantic entry of opcode number `op` (`Op::native_semantic_entry`).
    SemanticEntry(u16),
    /// The base body of `function` entered at record `pc` (a lane leave's continuation).
    BasePc { function: u32, pc: u32 },
    /// The operand record (`NativeJsOperands`) of record `pc` of `function`, which its generic row
    /// passes its semantic entry: data the linker places (`AotImage::operands`).
    Operands { function: u32, pc: u32 },
    /// The record (`CallRecord`) the direct call of record `pc` of `function` reads, a call the closed
    /// world proves (`AotFacts::call_target`): data the linker places and binds (`AotImage::calls`).
    CallRecord { function: u32, pc: u32 },
    /// The narrow decode's upper-word table (`NarrowLayout::upper_words`): a process address, which a
    /// base body's field and element reads take as a hole. The object carries its own copy.
    NarrowUpperWords,
    /// The process-wide attention byte a poll row reads first (`execution_interrupt::attention_address`).
    Attention,
    /// Class C: heap value number `key` of `AotImage::heap_values`.
    HeapValue(u32),
    /// Class D: shape number `key` of `AotImage::shapes`.
    Shape(u32),
}

pub(crate) use quench_runtime::vm::engine_types::HeapValueRecipe;

pub(crate) use quench_runtime::vm::engine_types::ShapeRecipe;

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
    operands: Interned<(u32, u32)>,
    calls: Interned<(u32, u32)>,
}

/// The tables `SymbolTable::finish` leaves: shapes, heap values, atom texts, base PC labels,
/// operand sites and call sites, each by key.
pub(crate) struct Finished {
    pub(crate) shapes: Vec<ShapeRecipe>,
    pub(crate) heap_values: Vec<HeapValueRecipe>,
    pub(crate) atoms: Vec<Box<str>>,
    pub(crate) labels: Vec<(u32, u32)>,
    pub(crate) operands: Vec<(u32, u32)>,
    pub(crate) calls: Vec<(u32, u32)>,
}

impl SymbolTable {
    /// The table of `program`'s image: its atom texts by program atom number, the key `Symbol::Atom`
    /// and a recipe's field name carry (atom numbers are per VM, texts are not).
    pub(crate) fn new(program: &ResidualProgram) -> Self {
        let atoms = program.atoms.iter().map(Box::<str>::from).collect();
        Self { shapes: Interned::default(), heap_values: Interned::default(), atoms, labels: Interned::default(), operands: Interned::default(),
            calls: Interned::default() }
    }

    /// The `(namespace, key)` pair code emits for `symbol`; a base PC's key is its interned label,
    /// an operand record's its site in the operand table, a call record's its site in the call table.
    pub(crate) fn intern(&mut self, symbol: Symbol) -> (u32, u32) {
        match symbol {
            Symbol::BasePc { function, pc } => (BASE_PC_NAMESPACE, self.labels.key((function, pc))),
            Symbol::Operands { function, pc } => (OPERANDS_NAMESPACE, self.operands.key((function, pc))),
            Symbol::CallRecord { function, pc } => (CALL_RECORD_NAMESPACE, self.calls.key((function, pc))),
            other => (other.namespace(), other.key()),
        }
    }

    /// The image atom of `text`: a program atom with that text, or a text added past them.
    pub(crate) fn text_atom(&mut self, text: &str) -> u32 {
        let at = self.atoms.iter().position(|atom| &**atom == text).unwrap_or_else(|| {
            self.atoms.push(text.into());
            self.atoms.len() - 1
        });
        at as u32
    }

    pub(crate) fn heap_value(&mut self, recipe: HeapValueRecipe) -> Symbol {
        Symbol::HeapValue(self.heap_values.key(recipe))
    }

    pub(crate) fn shape(&mut self, recipe: ShapeRecipe) -> Symbol {
        Symbol::Shape(self.shapes.key(recipe))
    }

    /// The symbol `(namespace, key)` names in `image`, the inverse of `intern`.
    pub(crate) fn decode(image: &AotImage, namespace: u32, key: u32) -> Option<Symbol> {
        match namespace {
            BASE_PC_NAMESPACE => image.labels.get(key as usize).map(|&(function, pc)| Symbol::BasePc { function, pc }),
            OPERANDS_NAMESPACE => image.operands.get(key as usize).map(|record| Symbol::Operands { function: record.function, pc: record.pc }),
            CALL_RECORD_NAMESPACE => image.calls.get(key as usize).map(|call| Symbol::CallRecord { function: call.function, pc: call.pc }),
            SEMANTIC_ENTRY_NAMESPACE => u16::try_from(key).ok().map(Symbol::SemanticEntry),
            NARROW_WORDS_NAMESPACE => (key == 0).then_some(Symbol::NarrowUpperWords),
            ATTENTION_NAMESPACE => (key == 0).then_some(Symbol::Attention),
            _ => KEYED.iter().find(|(kind, _)| *kind == namespace).map(|(_, make)| make(key)),
        }
    }

    /// The recipe tables, the label list, the operand sites and the call sites the image carries.
    pub(crate) fn finish(self) -> Finished {
        Finished { shapes: self.shapes.values, heap_values: self.heap_values.values, atoms: self.atoms, labels: self.labels.values,
            operands: self.operands.values, calls: self.calls.values }
    }
}

/// The constructor of each symbol kind keyed by its own number, by namespace.
const KEYED: [(u32, fn(u32) -> Symbol); 9] = [
    (RelocKind::EntryCell as u32, Symbol::EntryCell),
    (RelocKind::Descriptor as u32, Symbol::Descriptor),
    (RelocKind::CaptureWords as u32, Symbol::CaptureWords),
    (RelocKind::Invalidation as u32, invalidation),
    (RelocKind::Helper as u32, Symbol::Helper),
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
            Self::ConstantPool(_) => RelocKind::ConstantPool as u32,
            Self::Atom(_) => RelocKind::Atom as u32,
            Self::HeapValue(_) => RelocKind::HeapValue as u32,
            Self::Shape(_) => RelocKind::Shape as u32,
            Self::SemanticEntry(_) => SEMANTIC_ENTRY_NAMESPACE,
            Self::BasePc { .. } => BASE_PC_NAMESPACE,
            Self::Operands { .. } => OPERANDS_NAMESPACE,
            Self::CallRecord { .. } => CALL_RECORD_NAMESPACE,
            Self::NarrowUpperWords => NARROW_WORDS_NAMESPACE,
            Self::Attention => ATTENTION_NAMESPACE,
        }
    }

    /// The key of a symbol keyed by its own number (a base PC is keyed by its label, `intern`).
    fn key(self) -> u32 {
        match self {
            Self::EntryCell(key) | Self::Descriptor(key) | Self::CaptureWords(key) | Self::Helper(key)
                | Self::ConstantPool(key) | Self::Atom(key) | Self::HeapValue(key) | Self::Shape(key) => key,
            Self::Invalidation | Self::NarrowUpperWords | Self::Attention => 0,
            Self::SemanticEntry(op) => u32::from(op),
            Self::BasePc { .. } | Self::Operands { .. } | Self::CallRecord { .. } =>
                unreachable!("a base PC, an operand record or a call record is keyed by its site (`intern`)"),
        }
    }
}

/// The word of the heap value or shape `symbol` names, built in `vm` from `image`'s recipes: the
/// word a live lowering would embed (a heap value's bits, a shape's id). `None` for any other
/// symbol, for a recipe `vm` cannot build from the image alone (`Constant`), and for one the
/// running program has not made yet (`PrototypeOf` before the root activation creates the
/// closure): a lane body that names one links when it is published (`live::link_lane`).
pub(crate) fn resolve_recipe(symbol: Symbol, image: &AotImage, vm: &mut Vm) -> Option<u64> {
    match symbol {
        Symbol::HeapValue(key) => Some(heap_value(image, vm, key)?.to_bits()),
        Symbol::Shape(key) => Some(u64::from(shape(image, vm, key)?)),
        _ => None,
    }
}

/// The value of heap value recipe `key`, in `vm`.
fn heap_value(image: &AotImage, vm: &mut Vm, key: u32) -> Option<Value> {
    match *image.heap_values.get(key as usize)? {
        HeapValueRecipe::Intrinsic(id) => Some(vm.native_value(*quench_runtime::tier_surface::Native::ALL.get(id as usize)?)),
        HeapValueRecipe::Global(atom) => {
            let atom = vm.intern_atom(image.atoms.get(atom as usize)?);
            let globals = vm.realm.globals;
            vm.own_property(globals, atom)
        }
        HeapValueRecipe::PrototypeOf(function) => prototype_of(vm, function),
        HeapValueRecipe::ClosureOf(function) => closure_of(vm, function),
        HeapValueRecipe::String(atom) => {
            let text = image.atoms.get(atom as usize)?;
            Some(vm.allocate_string(quench_runtime::vm::wtf16::JsString::from_str(text)))
        }
    }
}

/// Function `function`'s closure in the running program, where the program made exactly one (a
/// top-level function, recorded as `Vm::cached_functions` finds it): with several, no one closure
/// is the function's, so the recipe resolves to none.
fn closure_of(vm: &Vm, function: u32) -> Option<Value> {
    let mut closures = vm.cached_functions(vm.active_program, function);
    let closure = closures.next()?;
    closures.next().is_none().then_some(closure)
}

/// The own `prototype` of function `function`'s one closure (`closure_of`), an object.
fn prototype_of(vm: &mut Vm, function: u32) -> Option<Value> {
    let closure = closure_of(vm, function)?;
    let atom = vm.prototype_atom();
    vm.own_property(closure, atom).filter(|prototype| prototype.is_heap())
}

/// The shape of shape recipe `key`: the empty object of its prototype, then each field added
/// in order, along the VM's transitions.
fn shape(image: &AotImage, vm: &mut Vm, key: u32) -> Option<u32> {
    let recipe = image.shapes.get(key as usize)?;
    let prototype = match recipe.prototype {
        Some(prototype) => heap_value(image, vm, prototype)?,
        None => Value::NULL,
    };
    let mut shape = Vm::empty_object(prototype).shape();
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
