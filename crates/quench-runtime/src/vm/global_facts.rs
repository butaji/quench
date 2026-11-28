//! The global scope's values as Stable facts (`heap::global_value_fact`): the cell a by-name
//! read of the global scope resolves to now, which a lane body folds under a dependency, and
//! the global lexical assignments and declarations that retire the code holding one.
use super::{Atom, Value, Vm};
use crate::code_dependencies::SlotDependency;
use crate::heap::{GlobalCell, GlobalFact, GlobalLayoutFact};
use crate::staged::field_cache_layout::spilled_slot;
use crate::typed_ir::Type;

/// A global object's own data property that a lane body stores to, or loads for a `typeof`,
/// at a slot it holds under the global object's layout (`GlobalLayoutFact`) and the slot's
/// observed type: neither can change without retiring the code, which then checks nothing
/// per use but the realm. For a store, no code holds the slot's value (the cell is `Mutable`,
/// so its writes notify nothing) and no inheritor reads the global object.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GlobalSlot {
    /// The field-cache slot word of the property: its field index, or a spilled slot word (`spilled_slot`) where the
    /// global class spills there. Readers address it through `staged::entry::class_slot_address` (inline and spilled alike).
    pub slot: u32,
    /// The slot count of the global object's shape.
    pub storage_len: usize,
    /// The slot's observed type: a store of a value of it needs no admission.
    pub ty: Type,
    /// What the code holds of `ty` (`None` where any value fits or none was observed). Its slot is the field index.
    pub dependency: Option<SlotDependency>,
    pub layout: GlobalLayoutFact,
    /// The global object, and the realm field that names it while its realm is active.
    pub global: Value,
    /// The granule of the global object's class cell (`Value::direct_object_index`), read from the heap the code compiles against.
    pub granule: u32,
    pub realm_offset: i32,
}

/// A global lexical binding's word in the active realm's declarative record (`Realm::global_lexical_bindings`), which code reads (and,
/// where nothing holds the binding's value, writes) live at its address. The record's words stay where they are until a new
/// declaration moves them, which retires every holder of the global layout (`Vm::note_global_shadowing`), so the address is
/// held under that layout, with the realm that owns the record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LexicalSlot {
    pub address: usize,
    pub layout: GlobalLayoutFact,
    pub global: Value,
    pub realm_offset: i32,
}

/// The Stable cell a folded read of a global's member holds (`GlobalRead::NativeMember`'s `cell`): the cell's fact, and the global
/// object and realm offset it is of (`GlobalRead::Stable`). A row that folds the read carries it, so its body subscribes to the
/// fact (a reassignment retires the body) and proves the realm once.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StableCell {
    pub fact: GlobalFact,
    pub global: Value,
    pub realm_offset: i32,
    /// The value the cell held when the code was compiled: code that holds no cell compares a live read with it.
    pub value: Value,
}

/// The receivers a method read by name (`GlobalRead::NativeMember`) folds on: those of no named
/// property linked to a prototype that holds the member, so the member is that prototype's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReceiverLink {
    /// No receiver is proved: the read runs.
    None,
    /// A direct array linked to `Array.prototype`, whose link this is (`elements::plain_array_of`).
    Array(u32),
    /// A Map or Set of the exact shape and prototype link the read's cache observed, whose prototype
    /// holds the member (`elements::collection_of`).
    Collection { shape: u32, link: u32 },
}

/// What a lane body's by-name global read takes when it is compiled (`Vm::lane_global_reads`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GlobalRead {
    /// An immutable global's value (`builtins::immutable_globals`), which no binding changes.
    Constant(Value),
    /// A global cell's value, of type `ty`, held under `fact` by code that runs while the realm
    /// of global object `global` is active: the one the VM's field at `realm_offset` names.
    Stable { value: Value, ty: Type, fact: GlobalFact, global: Value, realm_offset: i32 },
    /// A whole-program AOT fact: the closed world's value of the binding at every read, which the
    /// dominating root store fixes (`AotFacts::dominated_root_store`); the read needs no check.
    Frozen { value: Value, ty: Type },
    /// A `StoreName` or `LoadNameTypeof` of a slot of the global object held directly.
    Slot(GlobalSlot),
    /// A global lexical binding, read (or stored) live at its address in the declarative record (`Vm::global_lexical_slot`).
    Lexical(LexicalSlot),
    /// A store of a global lexical binding that is not `Mutable` yet: the VM's own store, which the body holds
    /// the cell for (`Vm::held_lexical`), so that the write that makes it `Mutable` retires the body for a
    /// compile that stores the word live.
    Held(GlobalFact),
    /// The lane's own read at run time (`LaneHelper::GlobalGet`).
    Dynamic,
    /// At a field read of a Stable global (`Math.imul`): the member's value when the body was
    /// compiled, the native function `native`, held by the watched prototype of exact shape
    /// `holder`, a code dependency (`StableUses::holders`) that keeps the function's identity
    /// from being recycled. A call of the register it defines runs the native's operation
    /// inline while the callee is still that value (`Lowering::intrinsic_call`).
    /// `folds` where the receiver is that global, so the read itself may fold to the value; a
    /// method read by name off an unknown receiver (`x.push`) only names the call's candidate,
    /// and the read still runs: the receiver may hold its own member of that name.
    /// `link` names the receivers of no own member of that name, where the read folds (`ReceiverLink`).
    NativeMember { value: Value, native: super::Native, holder: u32, folds: bool, cell: Option<StableCell>, link: ReceiverLink },
    /// A `Slot` read (the b-jit's) of a global name whose function is the native `native` (`parseInt`) when the body was
    /// compiled, held by the global object's watched exact shape `holder` (`NativeMember`'s code dependency). The read is the
    /// slot's; a call of the register it defines guards the function's identity (`base_region::function::native_call`).
    NativeSlot { slot: GlobalSlot, value: Value, native: super::Native, holder: u32 },
}

impl GlobalRead {
    /// The value a folded read stands for: a constant, or a Stable or frozen cell's value under its
    /// dependency. The one list of the kinds that fold to a compile-time value.
    pub fn folded_value(&self) -> Option<Value> {
        match self {
            GlobalRead::Constant(value) | GlobalRead::Stable { value, .. } | GlobalRead::Frozen { value, .. } => Some(*value),
            _ => None,
        }
    }
}

/// The natives whose call a loop or body runs in place on scalar arguments and whose result allocates nothing: they
/// run no guest code, change no array and observe no frame (`dispatch_fast::in_place_natives::primitive_leaf`, with a
/// Number or Boolean result), and the Math functions of Numbers (`number::math_call`), which a loop calling one
/// (`sin`, `cos`, ...) otherwise declines to run natively. A global cell holding one of them is read as a `GlobalRead::NativeSlot`.
pub fn scalar_native(native: super::Native) -> bool {
    matches!(native, super::Native::ParseInt | super::Native::NumberParseFloat) || super::number::math_call(native).is_some()
}

/// Which tier a table of by-name global accesses is for (`Vm::global_table`): the lane specializer
/// folds a cell's value as a Stable fact; the b-jit's rows read a global object's own data
/// property from its slot live, under the object's layout.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GlobalTable {
    Lane,
    Base,
}

/// What each by-name global access of a function or unit takes, by PC: a flat table, empty at
/// a PC that is no such access (`Vm::global_reads_in_lane`).
#[derive(Clone, Default, PartialEq)]
pub struct GlobalReads(Box<[Option<GlobalRead>]>);

impl GlobalReads {
    /// The read at `pc`, if the instruction there is a by-name global read.
    pub fn get(&self, pc: &usize) -> Option<&GlobalRead> {
        self.0.get(*pc).and_then(Option::as_ref)
    }

    pub fn contains_key(&self, pc: &usize) -> bool {
        self.get(pc).is_some()
    }

    /// The reads at the PCs of `range`, in order.
    pub fn range(&self, range: std::ops::RangeInclusive<usize>) -> impl Iterator<Item = (usize, &GlobalRead)> {
        let end = (*range.end()).saturating_add(1).min(self.0.len());
        let start = (*range.start()).min(end);
        self.0[start..end].iter().enumerate().filter_map(move |(offset, read)| read.as_ref().map(|read| (start + offset, read)))
    }
}

impl std::ops::Index<&usize> for GlobalReads {
    type Output = GlobalRead;
    fn index(&self, pc: &usize) -> &GlobalRead {
        self.0[*pc].as_ref().expect("a global read at this PC")
    }
}

impl FromIterator<(usize, GlobalRead)> for GlobalReads {
    fn from_iter<I: IntoIterator<Item = (usize, GlobalRead)>>(reads: I) -> Self {
        let mut table: Vec<Option<GlobalRead>> = Vec::new();
        for (pc, read) in reads {
            if pc >= table.len() { table.resize(pc + 1, None); }
            table[pc] = Some(read);
        }
        Self(table.into_boxed_slice())
    }
}

impl Vm {
    /// The read of `atom` through cache `site` from the active realm's global scope as a Stable
    /// fact, where the cell the lane's own read resolves it to (`Vm::global_name_read`) is not
    /// `Mutable`: an initialized global lexical binding. A binding that is `Mutable` is read live
    /// (`GlobalRead::Lexical`).
    pub fn stable_global_read(&mut self, code: &super::ResidualProgram, atom: Atom, site: u16) -> GlobalRead {
        let globals = self.realm.globals;
        let super::field_cache::GlobalNameRead::Value(value) = self.global_name_read(code, atom, site) else {
            return GlobalRead::Dynamic;
        };
        // A lexical binding's value, or a cached field (which names no cell of the scope).
        let index = self.realm.global_lexical_declarations.contains(&atom)
            .then(|| self.realm.global_lexical_bindings.get_index_of(&atom)).flatten();
        let Some(index) = index.and_then(|index| u32::try_from(index).ok()) else { return GlobalRead::Dynamic };
        let cell = GlobalCell::Binding(index);
        self.heap.ensure_cell_extras(globals);
        let Some(table) = self.heap.object_mut(globals).map(|mut o| o.global_values_table()) else {
            return GlobalRead::Dynamic;
        };
        // A function's identity lets a call through the read skip its callee check.
        let ty = Type::function(value).filter(|_| self.heap.function(value).is_some()).unwrap_or_else(|| Type::immediate(value));
        let realm_offset = Self::realm_globals_offset();
        match crate::heap::global_value_fact(table, cell) {
            Some(fact) => GlobalRead::Stable { value, ty, fact, global: globals, realm_offset },
            // A binding that is written (`Mutable`) holds no value for code to fold: its word is read live.
            None => self.global_lexical_slot(atom, false).map_or(GlobalRead::Dynamic, GlobalRead::Lexical),
        }
    }

    /// The byte offset in the VM of the active realm's global object, which code that folds a
    /// realm's global cells reads to prove that realm active (`Lowering::prove_realm`, `Heap::Realm`).
    pub fn realm_globals_offset() -> i32 {
        i32::try_from(std::mem::offset_of!(Vm, realm) + std::mem::offset_of!(super::Realm, globals)).expect("the realm's global lies within the VM")
    }

    /// The global object's own data property `atom` under its exact shape, as its shape and field index: plain data
    /// (writable where `store`), of a shape that is no dictionary or exotic one, and not a private name.
    fn global_own_data_field(&self, globals: Value, atom: Atom, store: bool) -> Option<(u32, u32)> {
        let object = self.heap.object(globals)?;
        let shape = object.exact_shape();
        if shape != object.shape() || self.owned_shape_edits_in_place(shape) || self.is_private_name(atom) {
            return None;
        }
        let slot = self.shape_slot(shape, atom)?;
        let attributes = self.slot_attributes(object, slot)?;
        if attributes.accessor || (store && !attributes.writable) {
            return None;
        }
        Some((shape, u32::try_from(slot).ok()?))
    }

    /// Makes the active realm's global object's table of value cells, where the realm is set up (made, entered or read
    /// back): `global_slot`'s load-time choice then depends on the name's fact alone, never on whether a compiled body
    /// asked first (`stable_global_slot`). Idempotent.
    pub(crate) fn make_global_values_table(&mut self) {
        let globals = self.realm.globals;
        self.heap.ensure_cell_extras(globals);
        if let Some(mut object) = self.heap.object_mut(globals) { object.global_values_table(); }
    }

    /// The global object's own data property `atom` as a slot a lane body holds, for its `store` or its `typeof`; `None`
    /// where a lexical binding may shadow the name, the property is no (writable) data property, or a store would be
    /// seen by code that holds the cell's value or by an inheritor of the global object (`GlobalSlot`).
    pub fn stable_global_slot(&mut self, atom: Atom, store: bool) -> Option<GlobalSlot> {
        let globals = self.realm.globals;
        if self.global_name_hintable(atom)
            && let Some((_, field)) = self.global_own_data_field(globals, atom, store)
        {
            self.heap.ensure_cell_extras(globals);
            if let Some(table) = self.heap.object_mut(globals).map(|mut o| o.global_values_table())
                && store
            {
                // Code that stores the slot makes it `Mutable` now (its first store would): its folded readers retire.
                let cell = GlobalCell::Slot(field);
                while crate::heap::global_value_fact(table, cell).is_some() { crate::heap::note_global_write(table, cell); }
            }
        }
        self.global_slot(atom, store)
    }

    /// `stable_global_slot` without its effects: the same slot, where the global object already has its table of cells
    /// (`stable_global_slot` makes it). A `store` whose slot a body holds (or an inheritor reads) is not admissible here,
    /// so `None`; `stable_global_slot` retires those holders before it asks. The lane's by-name rows read at load through
    /// this form (`named_name_row`).
    pub fn global_slot(&self, atom: Atom, store: bool) -> Option<GlobalSlot> {
        let globals = self.realm.globals;
        if !self.global_name_hintable(atom) { return None; }
        let granule = globals.direct_object_index()?.granule();
        let (shape, field) = self.global_own_data_field(globals, atom, store)?;
        let table = self.heap.object(globals)?.global_values()?;
        let cell = GlobalCell::Slot(field);
        let held = crate::heap::global_value_fact(table, cell).is_some();
        let inherited = self.heap.object(globals)?.was_used_as_prototype();
        if store && (held || inherited || self.method_caches_may_name(atom)) { return None; }
        let index = usize::try_from(field).ok()?;
        let ty = self.heap.slot_type(shape, index);
        let dependency = (ty != Type::ANY && ty != Type::NEVER)
            .then_some(SlotDependency { shape, slot: u16::try_from(field).ok()?, ty });
        // A field at or past the class's spill slot is a spilled slot word, the same as an IC entry's (`spill_encoded_entry`).
        let slot = match self.heap.live_spill_from(granule).filter(|&from| field >= from) {
            Some(from) => spilled_slot(from, field - from),
            None => field,
        };
        Some(GlobalSlot { slot, storage_len: self.shapes.get(shape as usize)?.storage_len(), ty, dependency,
            layout: crate::heap::global_layout_fact(table)?, global: globals, granule,
            realm_offset: Self::realm_globals_offset() })
    }

    /// The value of the active realm's global object's own data property at slot word `slot` (a `GlobalSlot`'s), read live
    /// from the object's class cell (`Heap::class_entry_word`); `None` where the slot holds the deleted marker.
    pub fn global_slot_value(&self, slot: u32) -> Option<Value> {
        let granule = self.realm.globals.direct_object_index()?.granule();
        let value = self.heap.class_entry_word(granule, slot);
        (!value.is_deleted()).then_some(value)
    }

    /// The global lexical binding `atom` of the active realm as a slot code reads live (`LexicalSlot`), where the record holds it
    /// retained (a script that declared it finished). A `store` also needs a binding assignable with no code holding its
    /// value: no constant, and a cell that is `Mutable`, which no code folds and whose writes notify nothing.
    pub fn global_lexical_slot(&mut self, atom: Atom, store: bool) -> Option<LexicalSlot> {
        let globals = self.realm.globals;
        let index = self.realm.global_lexical_declarations.contains(&atom).then(|| self.realm.global_lexical_bindings.get_index_of(&atom)).flatten()?;
        self.heap.ensure_cell_extras(globals);
        let table = self.heap.object_mut(globals).map(|mut o| o.global_values_table())?;
        let held = crate::heap::global_value_fact(table, GlobalCell::Binding(u32::try_from(index).ok()?)).is_some();
        if store && (held || self.realm.immutable_global_lexical_bindings.contains(&atom)) {
            return None;
        }
        let (_, word) = self.realm.global_lexical_bindings.get_index(index)?;
        Some(LexicalSlot { address: std::ptr::from_ref(word).addr(), layout: crate::heap::global_layout_fact(table)?, global: globals, realm_offset: Self::realm_globals_offset() })
    }

    /// The fact of the global lexical binding `atom` of the active realm while it is not `Mutable`: the store
    /// the VM makes holds the cell, so the write that makes it `Mutable` retires the code that stores through the VM.
    pub fn held_lexical(&mut self, atom: Atom) -> Option<GlobalFact> {
        let index = self.realm.global_lexical_declarations.contains(&atom).then(|| self.realm.global_lexical_bindings.get_index_of(&atom)).flatten()?;
        self.heap.ensure_cell_extras(self.realm.globals);
        let table = self.heap.object_mut(self.realm.globals).map(|mut o| o.global_values_table())?;
        crate::heap::global_value_fact(table, GlobalCell::Binding(u32::try_from(index).ok()?))
    }

    /// Binds global lexical `atom` to `value` in the active realm, noting a name it declared
    /// for the first time (`note_global_lexical_names`), a record that grew (a declared name's
    /// binding retained: the words code reads live may have moved, `LexicalSlot`) and an assignment
    /// to an initialized binding, which retires the code holding its value.
    pub(super) fn bind_global_lexical(&mut self, atom: Atom, value: Value) {
        let assigned = self.realm.global_lexical_bindings.get_full(&atom)
            .filter(|(_, _, held)| !held.is_deleted()).map(|(index, _, _)| index);
        let retained = self.realm.global_lexical_bindings.len();
        if self.realm.bind_global_lexical(atom, value) {
            self.note_global_lexical_names();
        } else if self.realm.global_lexical_bindings.len() != retained {
            self.note_global_shadowing();
        }
        if let Some(index) = assigned {
            self.note_global_lexical_write(index);
        }
    }

    /// Initializes the active realm's global lexical binding of `atom`, which a script's own declaration
    /// of it makes: the binding is in its temporal dead zone from the script's instantiation
    /// (`instantiate_global_declarations`). The write retires the code that holds the binding's value.
    pub(super) fn initialize_global_lexical(&mut self, atom: Atom, value: Value) {
        match self.realm.global_lexical_bindings.get_index_of(&atom) {
            Some(index) => self.assign_global_lexical_at(index, value),
            None => self.bind_global_lexical(atom, value),
        }
    }

    /// Assigns `value` to the active realm's initialized global lexical binding at `index`, which
    /// `global_lexical_store_at` has proved is mutable and holds an initialized value of the name.
    pub(super) fn assign_global_lexical_at(&mut self, index: usize, value: Value) {
        if let Some((_, word)) = self.realm.global_lexical_bindings.get_index_mut(index) {
            *word = value;
        }
        self.note_global_lexical_write(index);
    }

    /// The code holding the value of the active realm's global lexical binding at `index` retires:
    /// an assignment to an initialized binding (`bind_global_lexical`, `assign_global_lexical_at`).
    fn note_global_lexical_write(&self, index: usize) {
        if let Some(table) = self.active_global_values()
            && let Ok(index) = u32::try_from(index)
        {
            crate::heap::note_global_write(table, GlobalCell::Binding(index));
        }
    }

    /// Maps each of the root's declared global lexicals to the active realm's binding index of its
    /// name (`GlobalDeclarations::slots`), at instantiation; a name not bound yet stays unmapped.
    pub(super) fn map_global_lexical_slots(&self, program: &super::ResidualProgram) {
        for (atom, slot) in program.function(0).global_lexical_slots() {
            self.refresh_global_lexical_slot(slot, atom);
        }
    }

    /// A global lexical declaration may shadow a property of the active realm's global object
    /// that code holds: that code retires (`heap::note_global_shadowed`).
    pub(super) fn note_global_shadowing(&self) {
        if let Some(table) = self.active_global_values() {
            crate::heap::note_global_shadowed(table);
        }
    }

    /// The table of the values code read from the active realm's global object, if any.
    fn active_global_values(&self) -> Option<u32> {
        self.heap.object(self.realm.globals).and_then(|o| o.global_values())
    }

    /// A compiled Script's global declaration instantiation: its lexical names may not
    /// redeclare the realm's, which later scripts then see.
    pub(super) fn declare_compiled_script_globals(&mut self, context: &crate::ResidualProgram, id: super::ProgramId) -> Result<(), crate::JsError> {
        let residual = self.programs.get(id).ok_or_else(|| self.type_error(context, "dynamic program is unavailable".into()))?;
        let eval_script_context = std::mem::replace(&mut self.eval_script_context, true);
        let result = self.instantiate_global_declarations(&residual);
        self.eval_script_context = eval_script_context;
        result
    }
}
