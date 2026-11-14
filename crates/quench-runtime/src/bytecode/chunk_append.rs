//! The tables a first-call chunk adds to its program (`ChunkDelta`), which the program takes
//! after its own entries (`ResidualProgram::append_chunk`) in the room its load reserved, so
//! a call between the chunk's functions and the program's stays a call within one program.
use super::*;
use std::cell::Cell;

/// A program's count of inline-cache sites, and how many it may grow to when chunks are
/// appended: the field caches the VM lays out for the program cover the whole range, so a
/// chunk's sites need no table that moves.
#[derive(Clone, Debug, Default)]
pub(crate) struct SiteCount {
    used: Cell<u16>,
    room: u16,
}

impl SiteCount {
    pub(crate) fn get(&self) -> u16 {
        self.used.get()
    }

    pub(crate) fn set(&self, used: u16) {
        self.used.set(used);
    }

    /// `used` sites, with room for chunks to bring the count to `capacity`.
    pub(crate) fn with_capacity(used: u16, capacity: u16) -> Self {
        Self { used: Cell::new(used), room: capacity.saturating_sub(used) }
    }

    /// The count the program's caches are laid out for: the sites used and the room left, which
    /// stops at the whole site space.
    pub(crate) fn capacity(&self) -> u16 {
        self.used.get().saturating_add(self.room)
    }
}

impl From<u16> for SiteCount {
    fn from(used: u16) -> Self {
        Self::with_capacity(used, used)
    }
}

/// The lengths of a program's tables, where a chunk's entries begin.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TableLengths {
    pub(crate) functions: usize,
    pub(crate) constants: usize,
    pub(crate) method_sites: usize,
    pub(crate) method_arguments: usize,
    pub(crate) field_sites: usize,
    pub(crate) object_sites: usize,
    pub(crate) regexp_literal_sites: usize,
    pub(crate) literal_templates: usize,
    pub(crate) superinstructions: usize,
    pub(crate) sites: u16,
}

/// A chunk's functions and the table entries they added, at the indices that follow
/// `base`; and the atoms it compiled against, which name its own after `first_atom`.
pub(crate) struct ChunkDelta {
    pub(crate) base: TableLengths,
    pub(crate) atoms: AtomTable,
    pub(crate) first_atom: usize,
    pub(crate) functions: Vec<FunctionSlot>,
    pub(crate) constants: Vec<Constant>,
    pub(crate) method_sites: Vec<MethodSite>,
    pub(crate) method_arguments: Vec<Register>,
    pub(crate) field_sites: Vec<FieldSite>,
    pub(crate) object_sites: Vec<ObjectSite>,
    pub(crate) regexp_literal_sites: Vec<RegExpLiteralSite>,
    pub(crate) literal_templates: Vec<Box<[LiteralStep]>>,
    /// The stubs of the chunk's functions, with the scopes their own chunks compile against.
    pub(crate) stubs: Vec<(u32, std::rc::Rc<crate::compile::first_call::FirstCallScopes>)>,
    pub(crate) sites: u16,
}

impl ChunkDelta {
    /// The id of the chunk's function, the first it made.
    pub(crate) fn target(&self) -> u32 {
        self.base.functions as u32
    }
}

impl ResidualProgram {
    pub(crate) fn table_lengths(&self) -> TableLengths {
        TableLengths {
            functions: self.functions.len(),
            constants: self.constants.len(),
            method_sites: self.method_sites.len(),
            method_arguments: self.method_arguments.len(),
            field_sites: self.field_sites.len(),
            object_sites: self.object_sites.len(),
            regexp_literal_sites: self.regexp_literal_sites.len(),
            literal_templates: self.literal_templates.len(),
            superinstructions: self.superinstructions.len(),
            sites: self.cache_sites.get(),
        }
    }

    /// Whether `delta` can be appended: the program follows the chunk's rewrites as they come
    /// (its records' rewrites wait for their first reads), shares its atoms with a VM, and
    /// has room in every table the chunk adds to.
    pub(crate) fn has_room_for(&self, delta: &ChunkDelta) -> bool {
        self.rewrite_ranges.kept_pending
            && self.atoms.is_shared()
            && self.table_lengths() == delta.base
            && delta.sites <= self.cache_sites.capacity()
            && self.tables_fit(delta)
    }

    /// Whether every table the chunk adds to has room for its entries.
    fn tables_fit(&self, delta: &ChunkDelta) -> bool {
        [
            (self.functions.spare(), delta.functions.len()),
            (self.constants.spare(), delta.constants.len()),
            (self.method_sites.spare(), delta.method_sites.len()),
            (self.method_arguments.spare(), delta.method_arguments.len()),
            (self.field_sites.spare(), delta.field_sites.len()),
            (self.object_sites.spare(), delta.object_sites.len()),
            (self.regexp_literal_sites.spare(), delta.regexp_literal_sites.len()),
            (self.literal_templates.spare(), delta.literal_templates.len()),
            (self.first_call_scopes.spare(), delta.stubs.len()),
        ]
        .iter()
        .all(|(spare, needed)| spare >= needed)
    }

    /// Appends `delta`, which `has_room_for`, whose atoms the VM has taken, and returns the id
    /// of the chunk's function. The tables never move, so what runs holds the same slices.
    pub(crate) fn append_chunk(&self, delta: ChunkDelta) -> u32 {
        debug_assert!(self.has_room_for(&delta));
        let target = delta.target();
        self.atoms.extend_shared(delta.atoms.len());
        self.constants.append(delta.constants.into_iter());
        self.method_sites.append(delta.method_sites.into_iter());
        self.method_arguments.append(delta.method_arguments.into_iter());
        self.field_sites.append(delta.field_sites.into_iter());
        self.object_sites.append(delta.object_sites.into_iter());
        self.regexp_literal_sites.append(delta.regexp_literal_sites.into_iter());
        self.literal_templates.append(delta.literal_templates.into_iter());
        self.first_call_scopes.append(delta.stubs.into_iter());
        self.cache_sites.set(delta.sites);
        self.functions.append(delta.functions.into_iter());
        target
    }
}
