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

/// The namespace of `Symbol::BasePc`, whose key is its label in the image's label list: the first number after the `RelocKind` ones.
const BASE_PC_NAMESPACE: u32 = RelocKind::ClassMethods as u32 + 1;
/// The namespace of `Symbol::CallRecord`, whose key is its site in the image's call table.
pub(crate) const CALL_RECORD_NAMESPACE: u32 = BASE_PC_NAMESPACE + 1;
/// The namespace of `Symbol::NarrowUpperWords`, one table keyed 0.
pub(crate) const NARROW_WORDS_NAMESPACE: u32 = CALL_RECORD_NAMESPACE + 1;
/// The namespace of `Symbol::Attention`, one byte keyed 0.
const ATTENTION_NAMESPACE: u32 = NARROW_WORDS_NAMESPACE + 1;

/// The id a guarded shape resolves to where the VM builds none: no record's shape word holds it
/// (a shape id is below `ic::SYMBOLIC_SHAPE`, and dictionary storage only adds the top bit).
pub(crate) const NO_SHAPE: u32 = u32::MAX;

/// The attribute bits of a `ShapeRecipe` field, as `DataAttributes` reads them.
pub(crate) const ATTRIBUTE_WRITABLE: u8 = 1 << 0;
pub(crate) const ATTRIBUTE_ENUMERABLE: u8 = 1 << 1;
pub(crate) const ATTRIBUTE_CONFIGURABLE: u8 = 1 << 2;
/// The attribute bits of a field an initializer's store adds: a plain data property.
pub(crate) const ADDED_FIELD: u8 = ATTRIBUTE_WRITABLE | ATTRIBUTE_ENUMERABLE | ATTRIBUTE_CONFIGURABLE;

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
    /// The base body of `function` entered at record `pc` (a lane leave's continuation).
    BasePc { function: u32, pc: u32 },
    /// The record (`CallRecord`) the direct call of record `pc` of `function` to `callee` reads, a call the closed
    /// world proves (`AotFacts::call_targets`): data the linker places and binds (`AotImage::calls`). A call of a
    /// closed set has one per function of the set.
    CallRecord { function: u32, pc: u32, callee: u32 },
    /// The narrow decode's upper-word table (`NarrowLayout::upper_words`): a process address, which a
    /// base body's field and element reads take as a hole. The object carries its own copy.
    NarrowUpperWords,
    /// The process-wide attention byte a poll row reads first (`execution_interrupt::attention_address`).
    Attention,
    /// Class C: heap value number `key` of `AotImage::heap_values`.
    HeapValue(u32),
    /// Class D: shape number `key` of `AotImage::shapes`.
    Shape(u32),
    /// The shape of a field read's guard, keyed by its recipe and slot (`guard_key`): `NO_SHAPE`
    /// where the VM builds none, or where the slot's words are not tagged (`guarded_shape`).
    GuardShape(u32),
    /// The prototype link of the holder a prototype method read guards by holder guard key `key`
    /// (`GuardAccess::Holder`), where that holder holds the name as a plain data field at the
    /// key's slot; a link no receiver has otherwise (`holder_constant`).
    HolderLink(u32),
    /// The value that holder's slot holds when the code links (`holder_constant`).
    HolderValue(u32),
    /// The class id of the cell a field guard key's construction makes (`guarded_layout`):
    /// `NO_LAYOUT` where the guard does not resolve.
    GuardLayout(u32),
    /// The layout id of the instances shape recipe `key` builds (`class_layout_id`): `NO_LAYOUT` where its
    /// prototype does not resolve. Keyed by the recipe, as `Shape`.
    ClassLayout(u32),
    /// The base of the class method table (`layouts::class_methods_base`), which a closed-set indirect call reads each
    /// class's vector from by its layout id. One table for the process: keyed 0.
    ClassMethods,
}

pub(crate) use quench_runtime::vm::engine_types::HeapValueRecipe;

pub(crate) use quench_runtime::vm::engine_types::ShapeRecipe;
#[allow(unused_imports)] use quench_runtime::staged::glue_prelude::*;

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
    calls: Interned<(u32, u32, u32)>,
    /// The program's method selectors (`Selectors::of`, the same numbering the interpreter's store keeps): a method's
    /// selector indexes every class's method vector (`layouts::class_methods`).
    selectors: std::sync::Arc<quench_runtime::compile::call_plan::Selectors>,
}

/// The tables `SymbolTable::finish` leaves: shapes, heap values, atom texts, base PC labels,
/// and call sites, each by key.
pub(crate) struct Finished {
    pub(crate) shapes: Vec<ShapeRecipe>,
    pub(crate) heap_values: Vec<HeapValueRecipe>,
    pub(crate) atoms: Vec<Box<str>>,
    pub(crate) labels: Vec<(u32, u32)>,
    pub(crate) calls: Vec<(u32, u32, u32)>,
}

impl SymbolTable {
    /// The table of `program`'s image: its atom texts by program atom number, the key `Symbol::Atom`
    /// and a recipe's field name carry (atom numbers are per VM, texts are not).
    pub(crate) fn new(program: &ResidualProgram, selectors: std::sync::Arc<quench_runtime::compile::call_plan::Selectors>) -> Self {
        let atoms = program.atoms.iter().map(Box::<str>::from).collect();
        Self { shapes: Interned::default(), heap_values: Interned::default(), atoms, labels: Interned::default(),
            calls: Interned::default(), selectors }
    }

    /// The selector of method atom `atom` (`Selectors::get`), where a call plan names it.
    pub(crate) fn selector(&self, atom: u32) -> Option<u32> {
        self.selectors.get(atom)
    }

    /// The `(namespace, key)` pair code emits for `symbol`; a base PC's key is its interned label,
    /// a call record's its site in the call table.
    pub(crate) fn intern(&mut self, symbol: Symbol) -> (u32, u32) {
        match symbol {
            Symbol::BasePc { function, pc } => (BASE_PC_NAMESPACE, self.labels.key((function, pc))),
            Symbol::CallRecord { function, pc, callee } => (CALL_RECORD_NAMESPACE, self.calls.key((function, pc, callee))),
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

    /// The key of the guard of `access` of `slot` of an object of `recipe` (`Symbol::GuardShape`):
    /// the recipe's number in the same table as `shape`, the access and the slot, in one key
    /// (`guard_key`).
    /// The layout id of the instances `recipe` builds (`Symbol::ClassLayout`): the recipe's number in the shape table.
    pub(crate) fn class_layout(&mut self, recipe: ShapeRecipe) -> Symbol {
        Symbol::ClassLayout(self.shapes.key(recipe))
    }


    pub(crate) fn guard_shape(&mut self, recipe: ShapeRecipe, slot: u16, access: GuardAccess) -> Option<u32> {
        guard_key(self.shapes.key(recipe), slot, access)
    }

    /// The symbol `(namespace, key)` names in `image`, the inverse of `intern`.
    pub(crate) fn decode(image: &AotImage, namespace: u32, key: u32) -> Option<Symbol> {
        match namespace {
            BASE_PC_NAMESPACE => image.labels.get(key as usize).map(|&(function, pc)| Symbol::BasePc { function, pc }),
            CALL_RECORD_NAMESPACE => image.calls.get(key as usize).map(|call| Symbol::CallRecord { function: call.function, pc: call.pc, callee: call.callee }),
            NARROW_WORDS_NAMESPACE => (key == 0).then_some(Symbol::NarrowUpperWords),
            ATTENTION_NAMESPACE => (key == 0).then_some(Symbol::Attention),
            _ => KEYED.iter().find(|(kind, _)| *kind == namespace).map(|(_, make)| make(key)),
        }
    }

    /// The recipe tables, the label list and the call sites the image carries.
    pub(crate) fn finish(self) -> Finished {
        Finished { shapes: self.shapes.values, heap_values: self.heap_values.values, atoms: self.atoms, labels: self.labels.values,
            calls: self.calls.values }
    }
}

/// The constructor of each symbol kind keyed by its own number, by namespace.
const KEYED: [(u32, fn(u32) -> Symbol); 15] = [
    (RelocKind::EntryCell as u32, Symbol::EntryCell),
    (RelocKind::Descriptor as u32, Symbol::Descriptor),
    (RelocKind::CaptureWords as u32, Symbol::CaptureWords),
    (RelocKind::Invalidation as u32, invalidation),
    (RelocKind::Helper as u32, Symbol::Helper),
    (RelocKind::ConstantPool as u32, Symbol::ConstantPool),
    (RelocKind::Atom as u32, Symbol::Atom),
    (RelocKind::HeapValue as u32, Symbol::HeapValue),
    (RelocKind::Shape as u32, Symbol::Shape),
    (RelocKind::GuardShape as u32, Symbol::GuardShape),
    (RelocKind::HolderLink as u32, Symbol::HolderLink),
    (RelocKind::HolderValue as u32, Symbol::HolderValue),
    (RelocKind::GuardLayout as u32, Symbol::GuardLayout),
    (RelocKind::ClassLayout as u32, Symbol::ClassLayout),
    (RelocKind::ClassMethods as u32, class_methods),
];

/// The invalidation word's symbol: one word, keyed 0.
fn invalidation(_key: u32) -> Symbol {
    Symbol::Invalidation
}

/// The method table's base symbol: one table, keyed 0.
fn class_methods(_key: u32) -> Symbol {
    Symbol::ClassMethods
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
            Self::GuardShape(_) => RelocKind::GuardShape as u32,
            Self::HolderLink(_) => RelocKind::HolderLink as u32,
            Self::HolderValue(_) => RelocKind::HolderValue as u32,
            Self::GuardLayout(_) => RelocKind::GuardLayout as u32,
            Self::ClassLayout(_) => RelocKind::ClassLayout as u32,
            Self::ClassMethods => RelocKind::ClassMethods as u32,
            Self::BasePc { .. } => BASE_PC_NAMESPACE,
            Self::CallRecord { .. } => CALL_RECORD_NAMESPACE,
            Self::NarrowUpperWords => NARROW_WORDS_NAMESPACE,
            Self::Attention => ATTENTION_NAMESPACE,
        }
    }

    /// The key of a symbol keyed by its own number (a base PC is keyed by its label, `intern`).
    fn key(self) -> u32 {
        match self {
            Self::EntryCell(key) | Self::Descriptor(key) | Self::CaptureWords(key) | Self::Helper(key)
                | Self::ConstantPool(key) | Self::Atom(key) | Self::HeapValue(key) | Self::Shape(key) | Self::GuardShape(key)
                | Self::HolderLink(key) | Self::HolderValue(key) | Self::GuardLayout(key) | Self::ClassLayout(key) => key,
            Self::Invalidation | Self::NarrowUpperWords | Self::Attention | Self::ClassMethods => 0,
            Self::BasePc { .. } | Self::CallRecord { .. } =>
                unreachable!("a base PC or a call record is keyed by its site (`intern`)"),
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
        Symbol::GuardShape(key) => Some(u64::from(guarded_shape(image, vm, key).map_or(NO_SHAPE, |(shape, _, _)| shape))),
        Symbol::HolderLink(key) => Some(holder_constant(image, vm, key).map_or(NO_LINK, |(_, link, _)| u64::from(link))),
        Symbol::HolderValue(key) => Some(holder_constant(image, vm, key).map_or(Value::UNDEFINED, |(_, _, value)| value).to_bits()),
        Symbol::GuardLayout(key) => Some(u64::from(guarded_layout(image, vm, key).unwrap_or(NO_LAYOUT))),
        Symbol::ClassLayout(key) => Some(u64::from(class_layout_id(image, vm, key).unwrap_or(NO_LAYOUT))),
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

/// The shape of shape recipe `key`: the empty object of its prototype, as its constructor's
/// construction names it, then each field added in order, along the VM's transitions.
fn shape(image: &AotImage, vm: &mut Vm, key: u32) -> Option<u32> {
    let recipe = image.shapes.get(key as usize)?;
    let prototype = match recipe.prototype {
        Some(prototype) => heap_value(image, vm, prototype)?,
        None => Value::NULL,
    };
    let mut shape = Vm::empty_object(prototype).shape();
    if let Some(constructor) = recipe.constructor {
        let constructor = heap_value(image, vm, constructor)?;
        shape = vm.constructed_root_shape(shape, constructor);
    }
    for &(atom, attributes) in recipe.fields.iter() {
        let atom = vm.intern_atom(image.atoms.get(atom as usize)?);
        shape = vm.transition_data_property_shape(shape, PropertyKey::string(atom), data_attributes(attributes));
    }
    Some(shape)
}

/// The link a holder that does not resolve names (`Symbol::HolderLink`): bits outside the link
/// field, so no receiver's masked link equals it and the read asks the VM's caches.
const NO_LINK: u64 = !quench_runtime::tier_surface::NATIVE_OBJECT_LAYOUT.proto_mask as u64 & 0xffff_ffff;
const _: () = assert!(NO_LINK != 0);

/// The prototype method fact of holder guard key `key` (`GuardAccess::Holder`), proven in `vm`
/// as the image links: the holder's class word (its shape), its prototype link, and the value its slot
/// holds, where the holder holds the name as a plain data field at the key's slot. The holder is a
/// prototype, a class cell: its value is read through the cell's own data property (`Vm::own_property`), so a
/// write of its values retires the code that took the value (`ahead_images::holder_dependencies`
/// subscribes it under the shape).
pub(crate) fn holder_constant(image: &AotImage, vm: &mut Vm, key: u32) -> Option<(u32, u32, Value)> {
    let slot = key & ((1 << GUARD_ACCESS_SHIFT) - 1);
    let (holder, atom, class) = holder_cell(image, vm, key >> GUARD_SLOT_BITS, slot)?;
    let value = vm.own_property(holder, atom)?;
    Some((class, quench_runtime::tier_surface::ProtoLink::of(holder).bits(), value))
}

/// The bits of a guard key below its recipe (`guard_key`): the access, then the slot.
const GUARD_SLOT_BITS: u32 = 12;
/// The bits of a guard key below its access (`GuardAccess`): the slot.
const GUARD_ACCESS_SHIFT: u32 = GUARD_SLOT_BITS - GuardAccess::BITS;

/// What the access a shape guard admits needs of its slot (`guarded_shape`): a read reads any
/// tagged word; a store writes in place, with no join of the slot's type, only the values of one
/// class (`StoreArm::of` of the slot typed `GuardAccess::stored`), so its guard resolves only where
/// the slot's type holds every value of that class. A prototype read guards two shapes: its
/// receiver's (`Inherited`), which owns no field of the name, and its holder's (`Holder`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum GuardAccess {
    Read,
    /// A store of an int32.
    StoreInt32,
    /// A store of a direct object or `null`.
    StoreReference,
    /// A store of any tagged word: the slot is admitted to every value (`ahead_images::fields`).
    StoreTagged,
    /// The receiver of a read its prototype answers: the recipe's shape, whatever its slots hold.
    Inherited,
    /// The holder of a prototype read: the class cell the recipe's `prototype` names, where the
    /// slot holds the recipe's one field, the name read, as a plain data field (`holder_cell`). The
    /// recipe names no shape of its own: it carries the holder and the name.
    Holder,
    /// A read of a slot every store of which writes an int32 (`AotFacts::field_type`): the slot is
    /// admitted to int32s, as its stores' guards admit it, and the read takes its words as int32s
    /// under the slot's dependency (`ahead_images::guard_dependencies`), so a store of anything else
    /// retires the code: the type is proven where the slot is written, never at the read.
    ReadInt32,
    /// A read of a slot every store of which writes a direct object or `null`, as `ReadInt32`.
    ReadReference,
}

impl GuardAccess {
    /// The bits of the access in a guard key.
    const BITS: u32 = 3;
    const ALL: [Self; 8] = [Self::Read, Self::StoreInt32, Self::StoreReference, Self::StoreTagged, Self::Inherited, Self::Holder,
        Self::ReadInt32, Self::ReadReference];

    /// The type the slot's type must hold for the access: `None` for a read.
    pub(crate) fn stored(self) -> Option<quench_runtime::typed_ir::Type> {
        use quench_runtime::typed_ir::Type;
        match self {
            Self::Read | Self::Inherited | Self::Holder | Self::ReadInt32 | Self::ReadReference => None,
            Self::StoreInt32 => Some(Type::INT32),
            Self::StoreReference => Some(Type::DIRECT_OBJECT_FAMILY.join(Type::language(quench_runtime::ValueType::Null))),
            Self::StoreTagged => Some(Type::ANY),
        }
    }

    /// The type a typed read takes its slot's words as, which the slot's type must be within: `None`
    /// for any other access.
    pub(crate) fn read(self) -> Option<quench_runtime::typed_ir::Type> {
        use quench_runtime::typed_ir::Type;
        match self {
            Self::ReadInt32 => Some(Type::INT32),
            Self::ReadReference => Some(Type::NULLABLE_DIRECT_OBJECT),
            _ => None,
        }
    }

    /// The accesses of a field whose stores all write values of type `ty` (`AotFacts::field_type`):
    /// its reads' and its stores', typed by the one class `ty` is within, or any tagged word.
    pub(crate) fn of_field(ty: quench_runtime::typed_ir::Type) -> (Self, Self) {
        use quench_runtime::typed_ir::Type;
        if ty == Type::NEVER { (Self::Read, Self::StoreTagged) }
        else if ty.is_subtype(Type::INT32) { (Self::ReadInt32, Self::StoreInt32) }
        else if ty.is_subtype(Type::NULLABLE_DIRECT_OBJECT) { (Self::ReadReference, Self::StoreReference) }
        else { (Self::Read, Self::StoreTagged) }
    }
}

/// The type the guard `key` admits its slot's words as for a typed read (`GuardAccess::read`).
pub(crate) fn guard_read_type(key: u32) -> Option<quench_runtime::typed_ir::Type> {
    GuardAccess::ALL.get(((key >> GUARD_ACCESS_SHIFT) & ((1 << GuardAccess::BITS) - 1)) as usize)?.read()
}

/// The shape recipe guard key `key` guards a slot of (`guard_key`).
pub(crate) fn guard_recipe(key: u32) -> u32 {
    key >> GUARD_SLOT_BITS
}

/// The key of the guard of `access` of `slot` of shape recipe `recipe`: the recipe above the
/// access and the slot. `None` where either does not fit.
fn guard_key(recipe: u32, slot: u16, access: GuardAccess) -> Option<u32> {
    (u32::from(slot) < 1 << GUARD_ACCESS_SHIFT && recipe < 1 << (u32::BITS - GUARD_SLOT_BITS))
        .then_some(recipe << GUARD_SLOT_BITS | (access as u32) << GUARD_ACCESS_SHIFT | u32::from(slot))
}

/// The shape guard key `key` names in `vm`, its slot and the type an access of the slot depends on
/// (`Vm::tagged_slot_type`), a store's class joined in: `None` where `vm` builds no such shape,
/// where the slot's words are not tagged values, which the access compiled for
/// (`ahead_images::fields`) does not decode, or for a store's guard where the slot's type does not
/// hold every value the store writes with no join of its type (`GuardAccess::stored`).
pub(crate) fn guarded_shape(image: &AotImage, vm: &mut Vm, key: u32) -> Option<(u32, u16, quench_runtime::typed_ir::Type)> {
    let slot = (key & ((1 << GUARD_ACCESS_SHIFT) - 1)) as u16;
    let access = *GuardAccess::ALL.get(((key >> GUARD_ACCESS_SHIFT) & ((1 << GuardAccess::BITS) - 1)) as usize)?;
    let recipe = key >> GUARD_SLOT_BITS;
    match access {
        GuardAccess::Holder => return holder_shape(image, vm, recipe, slot),
        // The receiver's shape owns no field of the name it reads (`ahead_images::fields`).
        GuardAccess::Inherited => return shape(image, vm, recipe).map(|shape| (shape, slot, quench_runtime::typed_ir::Type::ANY)),
        _ => {}
    }
    let shape = shape(image, vm, recipe);
    if quench_runtime::trace::Channel::AotLanes.on() {
        eprintln!("aot-lanes: guard {key:#x} ({access:?}) resolves to shape {shape:?}, slot {slot} type {:?}", shape.map(|shape| vm.tagged_slot_type(shape, slot)));
    }
    let shape = shape?;
    // A store's class joins the slot's type before the store is exposed (`Vm::admit_slot_writes`),
    // as a JIT body's writer's domain does: a field the construction made `null` holds objects
    // once the run stores one, which the image's store does in place from its first run. A
    // widening retires the slot's readers; one that would make the words raw refuses below.
    // A typed read admits the class its field's stores write, as their guards do: the slot then holds
    // that class from its first store, and a read resolves only where it holds nothing wider.
    if let Some(stored) = access.stored().or(access.read()) { vm.admit_slot_writes(shape, usize::from(slot), stored); }
    let ty = vm.tagged_slot_type(shape, slot)?;
    (access.stored().is_none_or(|stored| stored.is_subtype(ty)) && access.read().is_none_or(|read| ty.is_subtype(read))).then_some((shape, slot, ty))
}

/// The layout id no record has (`Symbol::GuardLayout`): past the layout table's capacity.
const NO_LAYOUT: u32 = u32::MAX;
const _: () = assert!(NO_LAYOUT as usize >= quench_runtime::tier_surface::layouts::LAYOUT_CAPACITY);

/// The class id of the cell field guard key `key`'s construction (`guarded_shape`) makes: the class a
/// cell carries, its exact shape and its fields in a class cell's region (`ProtoLink::class`, which
/// names the prototype's link and the region). A construction of more fields than inline storage holds
/// is a class cell, so a compare of the live cell's class with this id proves the shape and the link
/// (`IcOp::GuardImageClass`). `None` where the guard does not resolve, or the recipe is of a construction
/// whose fields may stay inline.
pub(crate) fn guarded_layout(image: &AotImage, vm: &mut Vm, key: u32) -> Option<u32> {
    let (shape, _, _) = guarded_shape(image, vm, key)?;
    let recipe = image.shapes.get((key >> GUARD_SLOT_BITS) as usize)?;
    if recipe.fields.len() <= quench_runtime::tier_surface::NATIVE_OBJECT_LAYOUT.inline_property_count { return None; }
    let prototype = heap_value(image, vm, recipe.prototype?)?;
    let region = quench_runtime::tier_surface::cell_region_for(recipe.fields.len())?;
    Some(quench_runtime::tier_surface::ProtoLink::of(prototype).class(shape, region))
}

/// The holder guard of recipe `recipe` at `slot` (`GuardAccess::Holder`): the class word of the class cell its
/// `prototype` names, where that cell holds its one field's name at `slot` as a plain data field of tagged words;
/// `None` otherwise, so the guard resolves to `NO_SHAPE`.
fn holder_shape(image: &AotImage, vm: &mut Vm, recipe: u32, slot: u16) -> Option<(u32, u16, quench_runtime::typed_ir::Type)> {
    let (holder, atom, class) = holder_cell(image, vm, recipe, u32::from(slot))?;
    let shape = vm.heap_reads().object(holder)?.shape();
    let ty = vm.tagged_slot_type(shape, slot)?;
    vm.own_property(holder, atom)?;
    Some((class, slot, ty))
}

/// The holder cell of a prototype read (`GuardAccess::Holder`) of recipe `recipe`, whose name is its one field, at
/// `slot`: the object its `prototype` names, the name's atom, and the class word of that cell. `None` where the
/// holder is no live class cell, or the name is not the holder's field at `slot`.
fn holder_cell(image: &AotImage, vm: &mut Vm, recipe: u32, slot: u32) -> Option<(Value, quench_runtime::bytecode::Atom, u32)> {
    let recipe = image.shapes.get(recipe as usize)?;
    let holder = heap_value(image, vm, recipe.prototype?)?;
    let (name, _) = *recipe.fields.first()?;
    let atom = vm.intern_atom(image.atoms.get(name as usize)?);
    let class = vm.heap_reads().live_class_key(holder)? & !quench_runtime::staged::field_cache_layout::CLASS_KEY;
    let shape = vm.heap_reads().object(holder)?.shape();
    (vm.shape_slot(shape, atom) == Some(slot as usize)).then_some((holder, atom, class))
}

/// The layout id of the instances shape recipe `key` builds (`Symbol::ClassLayout`): the header a construction of its
/// fields allocates with (`InlineObjectHeader::for_fields`, the construct plan's own rule) over the shape `shape`
/// gives it. `None` where the recipe's prototype does not resolve.
fn class_layout_id(image: &AotImage, vm: &mut Vm, key: u32) -> Option<u32> {
    let recipe = image.shapes.get(key as usize)?;
    let prototype = heap_value(image, vm, recipe.prototype?)?;
    let shape = shape(image, vm, key)?;
    Some(quench_runtime::tier_surface::InlineObjectHeader::for_fields(prototype, shape, recipe.fields.len()).layout_id())
}

/// The data attributes of a field's attribute bits.
fn data_attributes(bits: u8) -> DataAttributes {
    DataAttributes {
        writable: bits & ATTRIBUTE_WRITABLE != 0,
        enumerable: bits & ATTRIBUTE_ENUMERABLE != 0,
        configurable: bits & ATTRIBUTE_CONFIGURABLE != 0,
    }
}
