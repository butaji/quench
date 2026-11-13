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

/// The functions a compilation makes, by id. At load the compiler owns every entry. A replay
/// makes one record, the replayed function's; the functions nested in it keep the ids and
/// records the load gave them, and the replay reads their headers from the program.
pub(super) enum FunctionTable<'a> {
    Owned(Vec<FunctionEntry>),
    Replay {
        program: &'a [crate::bytecode::FunctionSlot],
        target: u32,
        next: u32,
        record: Option<Box<crate::bytecode::Function>>,
    },
}

impl Default for FunctionTable<'_> {
    fn default() -> Self {
        Self::Owned(Vec::new())
    }
}

impl<'a> FunctionTable<'a> {
    /// The id the next function takes.
    pub(super) fn len(&self) -> usize {
        match self {
            Self::Owned(entries) => entries.len(),
            Self::Replay { next, .. } => *next as usize,
        }
    }

    pub(super) fn reserve_exact(&mut self, additional: usize) {
        if let Self::Owned(entries) = self {
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
            Self::Owned(entries) => entries.push(FunctionEntry::Reserved),
            Self::Replay { next, .. } => *next += 1,
        }
        id
    }

    pub(super) fn set(&mut self, id: u32, function: crate::bytecode::Function) {
        match self {
            Self::Owned(entries) => entries[id as usize] = FunctionEntry::Compiled(Box::new(function)),
            Self::Replay { target, record, .. } => {
                assert_eq!(id, *target, "a replay makes only its target's record");
                *record = Some(Box::new(function));
            }
        }
    }

    /// A record this compilation made and still holds.
    pub(super) fn get(&self, id: u32) -> Option<&crate::bytecode::Function> {
        match self {
            Self::Owned(entries) => match entries.get(id as usize)? {
                FunctionEntry::Compiled(function) => Some(function),
                _ => None,
            },
            Self::Replay { target, record, .. } => (id == *target).then_some(record.as_deref()?),
        }
    }

    pub(super) fn get_mut(&mut self, id: u32) -> Option<&mut crate::bytecode::Function> {
        match self {
            Self::Owned(entries) => match entries.get_mut(id as usize)? {
                FunctionEntry::Compiled(function) => Some(function),
                _ => None,
            },
            Self::Replay { target, record, .. } => {
                if id == *target { record.as_deref_mut() } else { None }
            }
        }
    }

    /// The header of a function whose record this compilation does not hold: one a replay
    /// skipped, which the load made, or one the load released.
    pub(super) fn header_without_record(&self, id: u32) -> Option<&crate::bytecode::FunctionHeader> {
        match self {
            Self::Owned(entries) => match entries.get(id as usize)? {
                FunctionEntry::Released(released) => Some(&released.header),
                _ => None,
            },
            Self::Replay { program, .. } => Some(program.get(id as usize)?.header()),
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
        if let Self::Owned(entries) = self
            && let Some(FunctionEntry::Released(released)) = entries.get_mut(id as usize)
        {
            released.header.inferred_name = Some(name);
            #[cfg(feature = "verify-replay")]
            {
                released.record.inferred_name = Some(name);
            }
        }
    }

    /// Takes the record of a compiled function to finish it.
    pub(super) fn take(&mut self, id: u32) -> Option<crate::bytecode::Function> {
        let Self::Owned(entries) = self else {
            return None;
        };
        let entry = entries.get_mut(id as usize)?;
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
        if let Self::Owned(entries) = self {
            entries[id as usize] = FunctionEntry::Released(ReleasedFunction::of(function));
        }
    }

    pub(super) fn first_mut(&mut self) -> Option<&mut crate::bytecode::Function> {
        match self {
            Self::Owned(entries) => match entries.first_mut()? {
                FunctionEntry::Compiled(function) => Some(function),
                _ => None,
            },
            Self::Replay { .. } => None,
        }
    }

    /// Every entry, in id order, once all are made.
    pub(super) fn into_entries(self) -> Vec<FunctionEntry> {
        match self {
            Self::Owned(entries) => entries,
            Self::Replay { record, .. } => record.into_iter().map(FunctionEntry::Compiled).collect(),
        }
    }

    /// A replay's record.
    pub(super) fn into_replayed(self) -> Option<crate::bytecode::Function> {
        match self {
            Self::Owned(_) => None,
            Self::Replay { record, .. } => record.map(|record| *record),
        }
    }
}
