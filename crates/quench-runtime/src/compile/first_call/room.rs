//! The room a program reserves in its tables for the chunks of its first-call stubs
//! (`Engine::append_first_call_chunk`), when it has any stub: the chunks append to them in
//! place, so the tables never move. A stub's chunk compiles what its source says, which is
//! not known before it runs, so the room follows the stubs' source sizes, each table by a
//! share of the bytes a table entry takes in compiled code. A chunk that does not fit
//! compiles as a program of its own, so the room bounds what a program spends, never what runs.

/// Source bytes of one function a program's compilation makes, from the programs measured.
const BYTES_PER_FUNCTION: usize = 64;
const BYTES_PER_CONSTANT: usize = 24;
const BYTES_PER_FIELD_SITE: usize = 24;
const BYTES_PER_METHOD_SITE: usize = 48;
const BYTES_PER_OBJECT_SITE: usize = 160;
const BYTES_PER_REGEXP_SITE: usize = 800;
const BYTES_PER_LITERAL_TEMPLATE: usize = 320;
const BYTES_PER_CACHE_SITE: usize = 12;
/// Source bytes of one instruction a function compiles to, which bounds the sites the rewrites
/// that wait for its first read add.
const BYTES_PER_INSTRUCTION: usize = 6;
/// The argument registers of a method site, at most.
const ARGUMENTS_PER_METHOD_SITE: usize = 3;

/// The stubs a program compiled, and the source bytes of their functions.
#[derive(Default)]
pub(in crate::compile) struct ChunkRoom {
    stubs: usize,
    bytes: usize,
}

impl ChunkRoom {
    /// Notes a stub whose function's source is `bytes` long.
    pub(in crate::compile) fn add_stub(&mut self, bytes: u32) {
        self.stubs += 1;
        self.bytes += bytes as usize;
    }

    /// Whether the program has a stub whose chunk may append to it.
    pub(in crate::compile) fn reserves(&self) -> bool {
        self.stubs != 0
    }

    /// A table's room: an entry for each `bytes_per` of the stubs' source.
    fn entries(&self, bytes_per: usize) -> usize {
        self.bytes / bytes_per
    }

    /// Each stub's chunk makes its own function, and the functions nested in it.
    pub(in crate::compile) fn functions(&self) -> usize {
        self.stubs + self.entries(BYTES_PER_FUNCTION)
    }

    pub(in crate::compile) fn constants(&self) -> usize {
        self.entries(BYTES_PER_CONSTANT)
    }

    pub(in crate::compile) fn field_sites(&self) -> usize {
        self.entries(BYTES_PER_FIELD_SITE)
    }

    pub(in crate::compile) fn method_sites(&self) -> usize {
        self.entries(BYTES_PER_METHOD_SITE)
    }

    pub(in crate::compile) fn method_arguments(&self) -> usize {
        self.method_sites() * ARGUMENTS_PER_METHOD_SITE
    }

    pub(in crate::compile) fn object_sites(&self) -> usize {
        self.entries(BYTES_PER_OBJECT_SITE)
    }

    pub(in crate::compile) fn regexp_literal_sites(&self) -> usize {
        self.entries(BYTES_PER_REGEXP_SITE)
    }

    pub(in crate::compile) fn literal_templates(&self) -> usize {
        self.entries(BYTES_PER_LITERAL_TEMPLATE)
    }

    pub(in crate::compile) fn cache_sites(&self) -> usize {
        self.entries(BYTES_PER_CACHE_SITE)
    }

    /// The field cache sites a program that has used `used` may have once its chunks append,
    /// within the sites it numbers one per instruction.
    pub(in crate::compile) fn cache_capacity(&self, used: u16) -> u16 {
        let room = u16::try_from(self.cache_sites()).unwrap_or(u16::MAX);
        used.max(used.saturating_add(room).min(crate::bytecode::UNIQUE_CACHE_SITES))
    }

    /// The instructions the stubs' functions may compile to, whose rewrites add the sites the
    /// field-site and superinstruction tables reserve for them (`FIELD_SITE_WINDOW`).
    pub(in crate::compile) fn code(&self) -> usize {
        self.entries(BYTES_PER_INSTRUCTION)
    }
}
