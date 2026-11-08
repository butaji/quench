//! The values a realm's global scope offers by name, held as constants by the code that read
//! them: a Stable fact (`typed_ir::BindingTime::Stable`) per global cell, retired when that
//! cell is written.
//!
//! A global object that code read a constant from keeps a table (`Object::global_values_table`)
//! whose cells are its own slots and the global lexical bindings of its realm, by their index.
//! Each cell's constancy only rises (`Constancy`): code folds a cell's value while it is not
//! `Mutable`, so a binding written in a loop stops being folded after its second write rather
//! than retiring code at every write. Every change of a slot's value is a slot write
//! (`Object::note_value_write`): an assignment, a redefinition, a delete (which writes the
//! hole). A layout change keeps each remaining property in its slot (an add appends, a delete
//! leaves a hole or drops the last slot, a dictionary keeps the indices), so it retires
//! nothing; a lexical declaration that may shadow a property retires every slot's holders
//! (`note_global_shadowed`), and a lexical binding's assignment its own (`Vm::bind_global_lexical`).
//! The code holding a cell therefore checks nothing per read. Guest heaps and their native
//! publications are thread-confined, as `prototype_values` is.

use crate::code_dependencies::{CodeReaders, DependentCode};
use std::rc::Rc;

/// How often a global cell was written since it was created: written at most once, its value
/// is folded under a dependency; written again, it is read at run time from then on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Constancy {
    #[default]
    Unwritten,
    WrittenOnce,
    Mutable,
}

impl Constancy {
    fn rise(self) -> Self {
        match self { Self::Unwritten => Self::WrittenOnce, _ => Self::Mutable }
    }
}

/// A cell of a global table: an own slot of the global object, or the index of a global
/// lexical binding of its realm (never removed while the realm is active).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum GlobalCell {
    Slot(u32),
    Binding(u32),
}

/// The Stable fact code took by folding a global cell's value: the cell of `table`, as it
/// stood when the code read it (its constancy and the table's shadowing epoch), which its
/// publication revalidates before it subscribes (`subscribe_global_value`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct GlobalFact {
    pub(crate) table: u32,
    pub(crate) cell: GlobalCell,
    constancy: Constancy,
    epoch: u32,
}

/// One kind of cell of a table: who holds each, and how often each was written.
#[derive(Default)]
struct Cells {
    readers: CodeReaders,
    constancy: Vec<Constancy>,
}

impl Cells {
    fn constancy(&self, index: u32) -> Constancy {
        self.constancy.get(index as usize).copied().unwrap_or_default()
    }

    fn note_write(&mut self, index: u32) {
        let index_usize = index as usize;
        if self.constancy.len() <= index_usize {
            self.constancy.resize(index_usize + 1, Constancy::Unwritten);
        }
        self.constancy[index_usize] = self.constancy[index_usize].rise();
        self.readers.retire(index);
    }
}

#[derive(Default)]
struct Table {
    slots: Cells,
    bindings: Cells,
    /// Advanced by each lexical declaration that may shadow a slot's property.
    epoch: u32,
}

impl Table {
    fn cells(&mut self, cell: GlobalCell) -> (&mut Cells, u32) {
        match cell {
            GlobalCell::Slot(slot) => (&mut self.slots, slot),
            GlobalCell::Binding(index) => (&mut self.bindings, index),
        }
    }
}

thread_local! {
    /// Every global table of this thread, by its id less one (`Object::global_values_table`).
    static TABLES: std::cell::RefCell<Vec<Table>> = const { std::cell::RefCell::new(Vec::new()) };
}

fn with_table<T>(table: u32, work: impl FnOnce(&mut Table) -> T) -> Option<T> {
    // Retiring drops code, which never writes a global cell, so the tables are not reentered.
    TABLES.with(|tables| tables.borrow_mut().get_mut(table.checked_sub(1)? as usize).map(work))
}

/// A new table's id, never zero.
pub(super) fn new_table() -> u32 {
    TABLES.with(|tables| {
        let mut tables = tables.borrow_mut();
        tables.push(Table::default());
        u32::try_from(tables.len()).expect("global tables fit u32")
    })
}

/// The fact code takes by folding `cell` of `table` now; `None` once the cell is `Mutable`.
pub(crate) fn global_value_fact(table: u32, cell: GlobalCell) -> Option<GlobalFact> {
    with_table(table, |held| {
        let epoch = held.epoch;
        let (cells, index) = held.cells(cell);
        let constancy = cells.constancy(index);
        (constancy != Constancy::Mutable).then_some(GlobalFact { table, cell, constancy, epoch })
    }).flatten()
}

/// Makes `code` retire when `fact`'s cell is written or a declaration shadows its slot; `false`
/// where the cell changed since the fact was taken or the reader table is full.
pub(crate) fn subscribe_global_value(fact: GlobalFact, code: &Rc<dyn DependentCode>) -> bool {
    with_table(fact.table, |held| {
        let epoch = held.epoch;
        let (cells, index) = held.cells(fact.cell);
        epoch == fact.epoch && cells.constancy(index) == fact.constancy && cells.readers.subscribe(index, code)
    }).unwrap_or(false)
}

/// `cell` of `table` was written: its constancy rises and the code holding it retires.
pub(crate) fn note_global_write(table: u32, cell: GlobalCell) {
    with_table(table, |held| {
        let (cells, index) = held.cells(cell);
        cells.note_write(index);
    });
}

/// A global lexical declaration may shadow a property of the global object of `table`: every
/// slot's holders retire.
pub(crate) fn note_global_shadowed(table: u32) {
    with_table(table, |held| {
        held.epoch = held.epoch.wrapping_add(1);
        held.slots.readers.reset();
    });
}

/// What a global object's extras keep of its table: its id, and the slots known `Mutable`,
/// which hold no fact, so their writes notify nothing (a cell's constancy never falls).
#[derive(Clone, Debug, Default)]
pub(super) struct GlobalValues {
    table: u32,
    mutable: Vec<u64>,
}

impl GlobalValues {
    /// Notes a write to slot `slot`, skipped for a slot known `Mutable`.
    #[inline(always)]
    fn note_slot_write(&mut self, slot: usize) {
        let (word, bit) = (slot / u64::BITS as usize, 1 << (slot % u64::BITS as usize));
        if self.mutable.get(word).is_some_and(|bits| bits & bit != 0) {
            return;
        }
        self.note_new_slot_write(word, bit, slot);
    }

    #[inline(never)]
    fn note_new_slot_write(&mut self, word: usize, bit: u64, slot: usize) {
        let cell = GlobalCell::Slot(slot as u32);
        note_global_write(self.table, cell);
        if global_value_fact(self.table, cell).is_none() {
            self.mutable.resize(self.mutable.len().max(word + 1), 0);
            self.mutable[word] |= bit;
        }
    }
}

impl super::Object {
    /// This global object's table (`global_values`), made on first use.
    pub(crate) fn global_values_table(&mut self) -> u32 {
        self.extras_mut().global_values.get_or_insert_with(|| Box::new(GlobalValues { table: new_table(), mutable: Vec::new() })).table
    }

    /// The table code that read this global object's values made, if any.
    pub(crate) fn global_values(&self) -> Option<u32> {
        self.extras()?.global_values.as_ref().map(|values| values.table)
    }

    /// Whether code holds what this object's slots hold, so a raw store that skips
    /// `note_value_write` must not write them: a prototype's or a read global object's.
    pub(crate) fn observes_value_writes(&self) -> bool {
        self.was_used_as_prototype() || self.global_values().is_some()
    }

    /// A write to slot `slot` of this object: what it offers its inheritors, or as a global
    /// object by name, may change.
    #[inline(always)]
    pub(crate) fn note_value_write(&mut self, slot: usize) {
        if self.was_used_as_prototype() {
            super::prototype_values::retire(self.exact_shape());
        }
        if let Some(values) = self.extras_mut_existing().and_then(|extras| extras.global_values.as_mut()) {
            values.note_slot_write(slot);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::code_dependencies::PublishedCode;

    fn published() -> (Rc<PublishedCode<()>>, Rc<dyn DependentCode>) {
        let code = Rc::new(PublishedCode::new(()));
        let reader: Rc<dyn DependentCode> = code.clone();
        (code, reader)
    }

    #[test]
    fn a_cell_retires_its_holders_on_write_and_folds_until_its_second_write() {
        let table = new_table();
        let slot = GlobalCell::Slot(3);
        let fact = global_value_fact(table, slot).unwrap();
        let (code, reader) = published();
        assert!(subscribe_global_value(fact, &reader));
        let (other, other_reader) = published();
        assert!(subscribe_global_value(global_value_fact(table, GlobalCell::Binding(3)).unwrap(), &other_reader));
        note_global_write(table, slot);
        assert!(!code.is_published(), "a write retires the cell's holders");
        assert!(other.is_published(), "a binding of the same index is another cell");
        assert!(!subscribe_global_value(fact, &published().1), "a fact taken before the write is stale");
        let once = global_value_fact(table, slot).expect("written once, still folded");
        note_global_write(table, slot);
        assert!(global_value_fact(table, slot).is_none(), "written twice, read at run time");
        assert!(!subscribe_global_value(once, &published().1));
    }

    #[test]
    fn shadowing_retires_every_slot_holder_but_no_binding_holder() {
        let table = new_table();
        let (slot_code, slot_reader) = published();
        let (binding_code, binding_reader) = published();
        let slot_fact = global_value_fact(table, GlobalCell::Slot(0)).unwrap();
        assert!(subscribe_global_value(slot_fact, &slot_reader));
        assert!(subscribe_global_value(global_value_fact(table, GlobalCell::Binding(0)).unwrap(), &binding_reader));
        note_global_shadowed(table);
        assert!(!slot_code.is_published());
        assert!(binding_code.is_published());
        assert!(!subscribe_global_value(slot_fact, &published().1), "the declaration shadows what the slot holds");
        assert!(global_value_fact(table, GlobalCell::Slot(0)).is_some(), "a declaration writes no slot value");
    }
}
