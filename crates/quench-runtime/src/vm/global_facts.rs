//! The global scope's values as Stable facts (`heap::global_value_fact`): the cell a by-name
//! read of the global scope resolves to now, which a lane body folds under a dependency, and
//! the global lexical assignments and declarations that retire the code holding one.
use super::{Atom, Host, Value, Vm};
use crate::heap::{GlobalCell, GlobalFact};
use crate::typed_ir::Type;

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
    /// The lane's own read at run time (`LaneHelper::GlobalGet`).
    Dynamic,
}

/// What each by-name global read of a function or unit takes, by PC: a flat table, empty at
/// a PC that is no such read (`Vm::lane_global_reads`).
#[derive(Default)]
pub(crate) struct GlobalReads(Box<[Option<GlobalRead>]>);

impl GlobalReads {
    pub(crate) fn contains_key(&self, pc: &usize) -> bool {
        self.0.get(*pc).is_some_and(Option::is_some)
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
        let realm_offset = i32::try_from(std::mem::offset_of!(Vm<H>, realm) + std::mem::offset_of!(super::Realm, globals))
            .expect("the realm's global lies within the VM");
        crate::heap::global_value_fact(table, cell)
            .map_or(GlobalRead::Dynamic, |fact| GlobalRead::Stable { value, ty, fact, global: globals, realm_offset })
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
