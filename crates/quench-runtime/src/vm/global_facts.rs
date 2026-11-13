//! The global scope's values as Stable facts (`heap::global_value_fact`): the cell a by-name
//! read of the global scope resolves to now, which a lane body folds under a dependency, and
//! the global lexical assignments and declarations that retire the code holding one.
use super::{Atom, Host, Value, Vm};
use crate::code_dependencies::SlotDependency;
use crate::heap::{GlobalCell, GlobalFact, GlobalLayoutFact};
use crate::typed_ir::Type;
use crate::ValueType;

/// The kinds whose `typeof` name one string spells for every value of the kind: an object's
/// does not (an `[[IsHTMLDDA]]` object reads as `undefined`), and `undefined` may be a hole.
const TYPEOF_NAMED_KINDS: [ValueType; 5] =
    [ValueType::Number, ValueType::Boolean, ValueType::String, ValueType::BigInt, ValueType::Symbol];

/// A global object's own data property that a lane body stores to, or asks the `typeof` of,
/// at a slot it holds under the global object's layout (`GlobalLayoutFact`) and the slot's
/// observed type: neither can change without retiring the code, which then checks nothing
/// per use but the realm. For a store, no code holds the slot's value (the cell is `Mutable`,
/// so its writes notify nothing) and no inheritor reads the global object.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct GlobalSlot {
    pub(crate) slot: u16,
    /// The slot count of the global object's shape.
    pub(crate) storage_len: usize,
    /// The slot's observed type: a store of a value of it needs no admission.
    pub(crate) ty: Type,
    /// What the code holds of `ty` (`None` where any value fits or none was observed).
    pub(crate) dependency: Option<SlotDependency>,
    /// The one string `typeof` yields for every value of `ty`, where one does.
    pub(crate) typeof_name: Option<Value>,
    pub(crate) layout: GlobalLayoutFact,
    /// The global object, and the realm field that names it while its realm is active.
    pub(crate) global: Value,
    pub(crate) realm_offset: i32,
}

/// What a lane body's by-name global read takes when it is compiled (`Vm::lane_global_reads`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GlobalRead {
    /// An immutable global's value (`builtins::immutable_globals`), which no binding changes.
    Constant(Value),
    /// A global cell's value, of type `ty`, held under `fact` by code that runs while the realm
    /// of global object `global` is active: the one the VM's field at `realm_offset` names.
    Stable { value: Value, ty: Type, fact: GlobalFact, global: Value, realm_offset: i32 },
    /// A whole-program AOT fact: the cell's value at every read, checked once per read
    /// site and folded while it holds (`Division::Frozen`).
    #[cfg_attr(not(feature = "aot"), allow(dead_code))]
    Frozen { value: Value, ty: Type },
    /// A `StoreName` or `LoadNameTypeof` of a slot of the global object held directly.
    Slot(GlobalSlot),
    /// The lane's own read at run time (`LaneHelper::GlobalGet`).
    Dynamic,
    /// At a field read of a Stable global (`Math.imul`): the member's value when the body was
    /// compiled, the native function `native`, held by the watched prototype of exact shape
    /// `holder`, a code dependency (`StableUses::holders`) that keeps the function's identity
    /// from being recycled. A call of the register it defines runs the native's operation
    /// inline while the callee is still that value (`Lowering::intrinsic_call`).
    NativeMember { value: Value, native: super::Native, holder: u32 },
}

/// What each by-name global access of a function or unit takes, by PC: a flat table, empty at
/// a PC that is no such access (`Vm::lane_global_reads`).
#[derive(Default)]
pub(crate) struct GlobalReads(Box<[Option<GlobalRead>]>);

impl GlobalReads {
    /// The read at `pc`, if the instruction there is a by-name global read.
    pub(crate) fn get(&self, pc: &usize) -> Option<&GlobalRead> {
        self.0.get(*pc).and_then(Option::as_ref)
    }

    pub(crate) fn contains_key(&self, pc: &usize) -> bool {
        self.get(pc).is_some()
    }

    /// The reads at the PCs of `range`, in order.
    pub(crate) fn range(&self, range: std::ops::RangeInclusive<usize>) -> impl Iterator<Item = (usize, &GlobalRead)> {
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

impl<H: Host> Vm<H> {
    /// The read of `atom` through cache `site` from the active realm's global scope as a Stable
    /// fact, where the cell the lane's own read resolves it to (`Vm::global_name_read`) is not
    /// `Mutable`: an initialized global lexical binding, or the global object's own data field.
    pub(super) fn stable_global_read(&mut self, code: &super::ResidualProgram, atom: Atom, site: u16) -> GlobalRead {
        let globals = self.realm.globals;
        let (value, cell) = match self.global_name_read(code, atom, site) {
            super::field_cache::GlobalNameRead::Value(value, Some(hint)) => (value, GlobalCell::Slot(u32::from(hint.slot))),
            super::field_cache::GlobalNameRead::Value(value, None) => {
                // A lexical binding's value, or a cached field (which names no cell of the scope).
                let index = self.realm.global_lexical_declarations.contains(&atom)
                    .then(|| self.realm.global_lexical_bindings.get_index_of(&atom)).flatten();
                let Some(index) = index.and_then(|index| u32::try_from(index).ok()) else { return GlobalRead::Dynamic };
                (value, GlobalCell::Binding(index))
            }
            _ => return GlobalRead::Dynamic,
        };
        let Some(table) = self.heap.object_mut(globals).map(crate::heap::Object::global_values_table) else {
            return GlobalRead::Dynamic;
        };
        // A function's identity lets a call through the read skip its callee check.
        let ty = Type::function(value).filter(|_| self.heap.function(value).is_some()).unwrap_or_else(|| Type::immediate(value));
        let realm_offset = Self::realm_global_offset();
        crate::heap::global_value_fact(table, cell)
            .map_or(GlobalRead::Dynamic, |fact| GlobalRead::Stable { value, ty, fact, global: globals, realm_offset })
    }

    /// The offset of the VM's field that names the active realm's global object.
    fn realm_global_offset() -> i32 {
        i32::try_from(std::mem::offset_of!(Vm<H>, realm) + std::mem::offset_of!(super::Realm, globals))
            .expect("the realm's global lies within the VM")
    }

    /// The global object's own data property `atom` as a slot a lane body holds, for its
    /// `store` or its `typeof`; `None` where a lexical binding may shadow the name, the property
    /// is no (writable) data property, or a store would be seen by code that holds the cell's
    /// value or by an inheritor of the global object (`GlobalSlot`).
    pub(super) fn stable_global_slot(&mut self, atom: Atom, store: bool) -> Option<GlobalSlot> {
        let globals = self.realm.globals;
        if !self.global_name_hintable(atom) { return None; }
        let hint = if store { self.own_data_field_hint::<true>(globals, atom) } else { self.own_data_field_hint::<false>(globals, atom) }?;
        let table = self.heap.object_mut(globals).map(crate::heap::Object::global_values_table)?;
        let held = crate::heap::global_value_fact(table, GlobalCell::Slot(u32::from(hint.slot))).is_some();
        let inherited = self.heap.object(globals)?.was_used_as_prototype();
        if store && (held || inherited || self.method_caches_may_name(atom)) { return None; }
        let ty = self.heap.slot_type(hint.receiver, usize::from(hint.slot));
        let dependency = (ty != Type::ANY && ty != Type::NEVER)
            .then_some(SlotDependency { shape: hint.receiver, slot: hint.slot, ty });
        let typeof_name = TYPEOF_NAMED_KINDS.into_iter().find(|kind| ty != Type::NEVER && ty.is_subtype(Type::language(*kind)))
            .and_then(|kind| self.typeof_constant(super::coercion::typeof_text(kind)));
        Some(GlobalSlot { slot: hint.slot, storage_len: self.shapes.get(hint.receiver as usize)?.storage_len(), ty, dependency,
            typeof_name, layout: crate::heap::global_layout_fact(table)?, global: globals,
            realm_offset: Self::realm_global_offset() })
    }

    /// Binds global lexical `atom` to `value` in the active realm, noting a name it declared
    /// for the first time (`note_global_lexical_names`) and an assignment to an initialized
    /// binding, which retires the code holding its value.
    pub(super) fn bind_global_lexical(&mut self, atom: Atom, value: Value) {
        let assigned = self.realm.global_lexical_bindings.get_full(&atom)
            .filter(|(_, _, held)| !held.is_deleted()).map(|(index, _, _)| index);
        if self.realm.bind_global_lexical(atom, value) {
            self.note_global_lexical_names();
        }
        if let Some(index) = assigned.and_then(|index| u32::try_from(index).ok())
            && let Some(table) = self.active_global_values()
        {
            crate::heap::note_global_write(table, GlobalCell::Binding(index));
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
        self.heap.object(self.realm.globals).and_then(crate::heap::Object::global_values)
    }
}
