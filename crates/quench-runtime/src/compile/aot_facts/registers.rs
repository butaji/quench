//! The cells and sources a function's registers hold at every point of the settled solve: the answers a tier that lowers
//! the function reads per register and PC, so none re-derives them. The solve (`flow`) fills them from the same words it
//! joins, so a cell and its source always describe the same value.

use super::value::{Key, Origin};
use crate::typed_ir::Cell;

/// Where a register's value was read from at a point, as the solve follows it (`Origin`). A value no binding or property
/// it was read from names is `Computed`: its cell describes it, and nothing else is known of its origin.
///
/// `Parameter`, `Receiver` and `Element` are not produced yet: the solve's words carry no origin for an argument, `this`
/// or an element read, so such a value is `Computed` until the origins for them are added. They are named here so a
/// lowering matches the whole set once.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// The argument of parameter `0`, held by the register the function binds it to.
    Parameter(u16),
    /// The `this` the function is called with.
    Receiver,
    /// An element of an array, read by index.
    Element,
    /// The named property with atom `0`, read off an object.
    Field(u32),
    /// The global binding with name atom `0`, read by name.
    Global(u32),
    /// A slot of an environment or frame the function binds, read by its binding.
    Local,
    /// A property-key constant of the program: the atom a key constant interns to.
    Constant,
    /// The value of an operation: no binding or property it was read from.
    Computed,
}

impl Source {
    /// The source of a word read from `origin` (`Word::origin`): a global or slot binding, a named property (read off any
    /// object or one class's), a property-key constant. A word with no origin, or another kind of origin, is `Computed`.
    pub(super) fn of(origin: Option<Origin>) -> Self {
        match origin {
            None => Self::Computed,
            Some(Origin::Binding(Key::Global(atom))) => Self::Global(atom),
            Some(Origin::Binding(Key::Slot { .. })) => Self::Local,
            Some(Origin::Member(atom) | Origin::MemberOn { atom, .. }) => Self::Field(atom),
            Some(Origin::Atom(_)) => Self::Constant,
            Some(_) => Self::Computed,
        }
    }
}

/// The cells and sources a function's registers hold at every point, kept compactly: each reachable block's at entry, and
/// each instruction's register writes with what they hold after it. The value of a register before the instruction at `pc`
/// is its last write earlier in `pc`'s block, else its value at the block's entry.
#[derive(Default)]
pub struct RegisterCells {
    /// By block, its first PC, and its registers' cells and sources at entry, in PC order; a block no path reaches is absent.
    entries: Vec<(u32, Box<[Cell]>, Box<[Source]>)>,
    /// By PC, in order, the registers the instruction there writes, with their cells and sources after it.
    writes: Vec<(u32, Box<[(u16, Cell, Source)]>)>,
}

impl RegisterCells {
    /// Records the block that starts at `start`, with its registers' cells and sources at entry, in PC order.
    pub(super) fn push_entry(&mut self, start: u32, cells: Box<[Cell]>, sources: Box<[Source]>) {
        self.entries.push((start, cells, sources));
    }

    /// Records the registers the instruction at `pc` writes, with their cells and sources after it.
    pub(super) fn push_writes(&mut self, pc: u32, written: Box<[(u16, Cell, Source)]>) {
        self.writes.push((pc, written));
    }

    /// Orders the records by PC, so the lookups binary-search them.
    pub(super) fn sort(&mut self) {
        self.entries.sort_by_key(|(start, ..)| *start);
        self.writes.sort_by_key(|(pc, _)| *pc);
    }

    /// The cell and source `register` holds just before the instruction at `pc`: none where no path reaches `pc`.
    fn lookup(&self, pc: u32, register: u16) -> Option<(Cell, Source)> {
        let block = self.entries.partition_point(|(start, ..)| *start <= pc).checked_sub(1)?;
        let (start, cells, sources) = &self.entries[block];
        let from = self.writes.partition_point(|(at, _)| *at < *start);
        let to = self.writes.partition_point(|(at, _)| *at < pc);
        let written = self.writes[from..to].iter().rev()
            .find_map(|(_, written)| written.iter().find(|(written, ..)| *written == register).map(|&(_, cell, source)| (cell, source)));
        let index = usize::from(register);
        written.or_else(|| Some((*cells.get(index)?, *sources.get(index)?)))
    }

    /// The cell `register` holds just before the instruction at `pc`: none where no path reaches `pc`.
    pub fn at(&self, pc: u32, register: u16) -> Option<Cell> {
        self.lookup(pc, register).map(|(cell, _)| cell)
    }

    /// The cell `register` holds just after the instruction at `pc`: the cell it writes there, else what it held before:
    /// none where no path reaches `pc`.
    pub fn after(&self, pc: u32, register: u16) -> Option<Cell> {
        let written = self.writes.binary_search_by_key(&pc, |(at, _)| *at).ok()
            .and_then(|at| self.writes[at].1.iter().find(|(written, ..)| *written == register).map(|&(_, cell, _)| cell));
        written.or_else(|| self.at(pc, register))
    }

    /// The source of the value `register` holds just before the instruction at `pc`: none where no path reaches `pc`.
    pub fn source_at(&self, pc: u32, register: u16) -> Option<Source> {
        self.lookup(pc, register).map(|(_, source)| source)
    }

    /// The cells of every register at the entry of the block that starts at `pc`, where one does.
    pub fn entry(&self, pc: u32) -> Option<&[Cell]> {
        self.entries.binary_search_by_key(&pc, |(start, ..)| *start).ok().map(|index| &*self.entries[index].1)
    }
}
