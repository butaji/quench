//! The functions a compilation makes, by id: owned at load, or one replayed record.
use crate::bytecode::Atom;

/// One function of a compilation at load: reserved while it compiles, then compiled, or
/// finished and released when a replay can make its record again.
/// An entry is boxed when it holds anything: a large program's compilation keeps an entry
/// per function while most of them are released, and an inline record (408 bytes) left
/// each released entry that large (typescript.js: 21.7K entries, 8.9 MB at its compile's
/// peak).
pub(super) enum FunctionEntry {
    Reserved,
    Compiled(Box<crate::bytecode::Function>),
    Released(Box<ReleasedFunction>),
}

/// A function finished during its program's load and released: its header, which the
/// function making it may still name, and in verifying builds the record a replay must
/// equal.
pub(super) struct ReleasedFunction {
    pub(super) header: crate::bytecode::FunctionHeader,
    #[cfg(feature = "verify-replay")]
    pub(super) record: crate::bytecode::Function,
}

impl ReleasedFunction {
    pub(super) fn of(function: crate::bytecode::Function) -> Box<Self> {
        Box::new(Self {
            header: crate::bytecode::FunctionHeader::of(&function),
            #[cfg(feature = "verify-replay")]
            record: function,
        })
    }
}

/// The functions a compilation makes, by id. A load makes every function, from id 0. A replay
/// makes one record, the replayed function's; the functions nested in it keep the ids and
/// records the load gave them, and the replay reads their headers from the program. A
/// first-call chunk appended to its program makes the functions after the program's, whose
/// headers it reads for the ids before them.
pub(super) enum FunctionTable<'a> {
    Entries {
        /// The functions before the first this compilation makes: none at load.
        program: &'a [crate::bytecode::FunctionSlot],
        entries: Vec<FunctionEntry>,
    },
    Replay {
        program: &'a [crate::bytecode::FunctionSlot],
        target: u32,
        next: u32,
        record: Option<Box<crate::bytecode::Function>>,
    },
}

impl Default for FunctionTable<'_> {
    fn default() -> Self {
        Self::chunk(&[])
    }
}

impl<'a> FunctionTable<'a> {
    /// A table whose first id follows the `program`'s functions: a chunk's.
    pub(super) fn chunk(program: &'a [crate::bytecode::FunctionSlot]) -> Self {
        Self::Entries { program, entries: Vec::new() }
    }

    fn entry(&self, id: u32) -> Option<&FunctionEntry> {
        if let Self::Entries { program, entries } = self {
            return entries.get(id.checked_sub(program.len() as u32)? as usize);
        }
        None
    }

    fn entry_mut(&mut self, id: u32) -> Option<&mut FunctionEntry> {
        match self {
            Self::Entries { program, entries } => entries.get_mut(id.checked_sub(program.len() as u32)? as usize),
            Self::Replay { .. } => None,
        }
    }

    /// The id the next function takes.
    pub(super) fn len(&self) -> usize {
        match self {
            Self::Entries { program, entries } => program.len() + entries.len(),
            Self::Replay { next, .. } => *next as usize,
        }
    }

    pub(super) fn reserve_exact(&mut self, additional: usize) {
        if let Self::Entries { entries, .. } = self {
            entries.reserve_exact(additional);
        }
    }

    /// Moves a replay past the ids of functions the load compiled.
    pub(super) fn seek(&mut self, id: u32) {
        if let Self::Replay { next, .. } = self {
            *next = id;
        }
    }

    /// Takes the next id for a function whose record follows.
    pub(super) fn reserve(&mut self) -> u32 {
        let id = self.len() as u32;
        match self {
            Self::Entries { entries, .. } => entries.push(FunctionEntry::Reserved),
            Self::Replay { next, .. } => *next += 1,
        }
        id
    }

    pub(super) fn set(&mut self, id: u32, function: crate::bytecode::Function) {
        if let Some(entry) = self.entry_mut(id) {
            *entry = FunctionEntry::Compiled(Box::new(function));
        } else if let Self::Replay { target, record, .. } = self {
            assert_eq!(id, *target, "a replay makes only its target's record");
            *record = Some(Box::new(function));
        }
    }

    /// A record this compilation made and still holds.
    pub(super) fn get(&self, id: u32) -> Option<&crate::bytecode::Function> {
        match self {
            Self::Replay { target, record, .. } => (id == *target).then_some(record.as_deref()?),
            Self::Entries { .. } => match self.entry(id)? {
                FunctionEntry::Compiled(function) => Some(function),
                _ => None,
            },
        }
    }

    pub(super) fn get_mut(&mut self, id: u32) -> Option<&mut crate::bytecode::Function> {
        match self {
            Self::Replay { target, record, .. } => {
                if id == *target { record.as_deref_mut() } else { None }
            }
            Self::Entries { .. } => match self.entry_mut(id)? {
                FunctionEntry::Compiled(function) => Some(function),
                _ => None,
            },
        }
    }

    /// The header of a function whose record this compilation does not hold: one a replay
    /// skipped, which the load made, or one the load released; in a chunk, one of its
    /// program's.
    pub(super) fn header_without_record(&self, id: u32) -> Option<&crate::bytecode::FunctionHeader> {
        match self {
            Self::Replay { program, .. } => Some(program.get(id as usize)?.header()),
            Self::Entries { program, .. } if (id as usize) < program.len() => Some(program[id as usize].header()),
            Self::Entries { .. } => match self.entry(id)? {
                FunctionEntry::Released(released) => Some(&released.header),
                _ => None,
            },
        }
    }

    /// Whether function `id` is an arrow, from its record or its released header.
    pub(super) fn is_arrow(&self, id: u32) -> Option<bool> {
        self.get(id)
            .map(|function| function.is_arrow)
            .or_else(|| self.header_without_record(id).map(|header| header.is_arrow))
    }

    /// Names a released function made from an anonymous definition.
    pub(super) fn name_released(&mut self, id: u32, name: Atom) {
        if let Some(FunctionEntry::Released(released)) = self.entry_mut(id) {
            released.header.inferred_name = Some(name);
            #[cfg(feature = "verify-replay")]
            {
                released.record.inferred_name = Some(name);
            }
        }
    }

    /// Takes the record of a compiled function to finish it.
    pub(super) fn take(&mut self, id: u32) -> Option<crate::bytecode::Function> {
        let entry = self.entry_mut(id)?;
        match std::mem::replace(entry, FunctionEntry::Reserved) {
            FunctionEntry::Compiled(function) => Some(*function),
            other => {
                *entry = other;
                None
            }
        }
    }

    /// Releases a finished function, keeping its header.
    pub(super) fn release(&mut self, id: u32, function: crate::bytecode::Function) {
        if let Some(entry) = self.entry_mut(id) {
            *entry = FunctionEntry::Released(ReleasedFunction::of(function));
        }
    }

    pub(super) fn first_mut(&mut self) -> Option<&mut crate::bytecode::Function> {
        match self {
            Self::Entries { program, entries } if program.is_empty() => match entries.first_mut()? {
                FunctionEntry::Compiled(function) => Some(function),
                _ => None,
            },
            _ => None,
        }
    }

    /// Every entry, in id order, once all are made.
    pub(super) fn into_entries(self) -> Vec<FunctionEntry> {
        match self {
            Self::Entries { entries, .. } => entries,
            Self::Replay { record, .. } => record.into_iter().map(FunctionEntry::Compiled).collect(),
        }
    }

    /// A replay's record.
    pub(super) fn into_replayed(self) -> Option<crate::bytecode::Function> {
        match self {
            Self::Entries { .. } => None,
            Self::Replay { record, .. } => record.map(|record| *record),
        }
    }
}

/// What a compilation notes of each function it makes, by id from the first it makes: at
/// load every function's, in a first-call chunk those after its program's.
#[derive(Default)]
pub(super) struct FunctionRecords<T> {
    base: u32,
    entries: Vec<T>,
}

impl<T: Clone + Default> FunctionRecords<T> {
    /// Records for the functions from id `base` on.
    pub(super) fn from_base(base: u32) -> Self {
        Self { base, entries: Vec::new() }
    }

    fn index(&self, id: u32) -> usize {
        id.checked_sub(self.base).expect("a record lies at or after the first function made") as usize
    }

    pub(super) fn set(&mut self, id: u32, value: T) {
        let at = self.index(id);
        if self.entries.len() <= at {
            self.entries.resize(at + 1, T::default());
        }
        self.entries[at] = value;
    }

    pub(super) fn at(&self, id: u32) -> &T {
        &self.entries[self.index(id)]
    }

    pub(super) fn at_mut(&mut self, id: u32) -> &mut T {
        let at = self.index(id);
        &mut self.entries[at]
    }

    /// The records of ids `from` to `to`.
    pub(super) fn range(&self, from: u32, to: u32) -> &[T] {
        &self.entries[self.index(from)..self.index(to)]
    }

    /// The record of `id`, when it lies among them.
    pub(super) fn get(&self, id: u32) -> Option<&T> {
        self.entries.get(id.checked_sub(self.base)? as usize)
    }

    /// The records from id `from` on.
    pub(super) fn after(&self, from: u32) -> &[T] {
        self.entries.get(from.saturating_sub(self.base) as usize..).unwrap_or_default()
    }

    /// The records in id order.
    pub(super) fn as_slice(&self) -> &[T] {
        &self.entries
    }

    pub(super) fn into_vec(self) -> Vec<T> {
        self.entries
    }

    #[cfg(feature = "profile-memory")]
    pub(super) fn capacity(&self) -> usize {
        self.entries.capacity()
    }
}
