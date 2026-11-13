//! The alias region and mutability of a memory access, as data. Its own file, so the b-jit
//! stencil build (`quench-bjit/stencils`) includes the same vocabulary the interpreter and the
//! specializers use: a specializer tags each access with it, the interpreter ignores it.

/// An alias region of memory: an access to one never aliases an access to another, so two
/// regions own disjoint words. A word has one region wherever it is read or written.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Region {
    /// The activation's register and local slots, and the call records on the lane stack.
    Frame,
    /// Property values of objects: a run in the arena, or the values an extras block holds.
    ObjectSlots,
    /// An object record's exact shape word: written only by a shape change.
    ShapeWord,
    /// Dense and typed array elements, and the backing header that counts them.
    Elements,
    /// Heap bookkeeping: allocation runs, tables, collection state, site cache entries.
    HeapMeta,
    /// Per-site feedback counters.
    MissCounters,
    /// An object record's header words other than its shape and payload (`meta`, an array's
    /// backing pointer).
    Records,
    /// An object record's payload word: the arena or extras pointer, or the narrow values packed
    /// in it.
    Payload,
    /// Environment storage: a captured-variable run and its header.
    Cells,
    /// A function's entry cell, which native callers read to admit a call.
    EntryCell,
    /// The loop entry record a loop's run reads: written before the run, never during it.
    LoopEntry,
    /// The call context, step dispatch and result words native code leaves through.
    Dispatch,
    /// The frozen constant pool an image materializes once at load.
    Constants,
}

/// Whether any operation can change the memory an access reads while compiled code runs.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Mutability {
    Mutable,
    /// Nothing changes it while code runs, so a load of it may move and merge freely.
    Readonly,
}

/// What a specializer proves of a memory access: the region it belongs to and whether the
/// memory is read-only.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Access {
    pub(crate) region: Region,
    pub(crate) mutability: Mutability,
}

impl Region {
    /// Every region, in declaration order: an index into it is the region's number.
    pub(crate) const ALL: &'static [Region] = &[
        Self::Frame, Self::ObjectSlots, Self::ShapeWord, Self::Elements, Self::HeapMeta, Self::MissCounters,
        Self::Records, Self::Payload, Self::Cells, Self::EntryCell, Self::LoopEntry, Self::Dispatch,
        Self::Constants,
    ];

    // Inlined into every `Concrete` read: an access tag the interpreter ignores must cost nothing,
    // and a plain call would cross codegen units.
    #[inline(always)]
    pub(crate) const fn mutable(self) -> Access {
        Access { region: self, mutability: Mutability::Mutable }
    }

    #[inline(always)]
    pub(crate) const fn readonly(self) -> Access {
        Access { region: self, mutability: Mutability::Readonly }
    }
}

/// A `RegionSet` holds one bit per region in a `u16`.
const _: () = assert!(Region::ALL.len() <= u16::BITS as usize);
