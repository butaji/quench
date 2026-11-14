//! The atoms method caches may name, and the compiled code that holds an atom unnamed.

use crate::bytecode::Atom;

/// A bit per atom.
pub(in crate::vm) type AtomBits = crate::bitset::BitSet;

/// The atoms method caches may name: a bit per atom, set when an entry for the atom is
/// recorded and cleared only when every method cache is, so a clear bit proves that no method
/// cache names the atom. Exact per atom, so a program with many cached method names does not
/// make every property add on every object scan the caches.
///
/// Compiled code that stores an atom no cache named folded that as a Stable fact and reads
/// it (`readers`); the cache that first names the atom retires that code.
#[derive(Default)]
pub(in crate::vm) struct MethodCacheAtoms {
    bits: AtomBits,
    /// Per atom, the sites whose entries may name it: every entry for the atom lies at a
    /// listed site, so invalidating the atom visits those sites alone.
    sites: Vec<AtomSites>,
    readers: crate::code_dependencies::CodeReaders,
}

/// The call sites whose method cache entries name one atom: a superset (an entry a collection
/// dropped leaves its site listed until the atom's next invalidation).
#[derive(Default)]
pub(in crate::vm) struct AtomSites {
    pub(in crate::vm) sites: Vec<u32>,
    /// Whether a megamorphic set may hold an entry for the atom.
    pub(in crate::vm) megamorphic: bool,
}

impl MethodCacheAtoms {
    #[inline(always)]
    pub(in crate::vm) fn contains(&self, atom: Atom) -> bool {
        self.bits.contains(atom as usize)
    }

    /// Names `atom`, retiring the code that held it unnamed.
    pub(in crate::vm) fn name(&mut self, atom: Atom) {
        if self.bits.insert_growing(atom as usize) {
            self.readers.retire(atom);
        }
    }

    /// Unnames every atom: no code held one named, so none retires.
    pub(in crate::vm) fn clear(&mut self) {
        self.bits.clear();
        self.sites.clear();
    }

    /// Lists `site` for `atom`, where an entry for it was recorded; `megamorphic` marks that
    /// the entry lies in the site's megamorphic set.
    pub(in crate::vm) fn list_site(&mut self, atom: Atom, site: usize, megamorphic: bool) {
        if self.sites.len() <= atom as usize {
            self.sites.resize_with(atom as usize + 1, AtomSites::default);
        }
        let listed = &mut self.sites[atom as usize];
        listed.sites.push(site as u32);
        listed.megamorphic |= megamorphic;
    }

    /// The sites that may hold entries for `atom`.
    pub(in crate::vm) fn sites_of(&self, atom: Atom) -> Option<&AtomSites> {
        self.sites.get(atom as usize)
    }

    /// The sites for `atom`, handed over for its entries to be dropped.
    pub(in crate::vm) fn take_sites(&mut self, atom: Atom) -> AtomSites {
        self.sites.get_mut(atom as usize).map(std::mem::take).unwrap_or_default()
    }

    /// The names now, for a lowering that folds them (`compiled::OnlineFacts`).
    pub(in crate::vm) fn bits(&self) -> &AtomBits {
        &self.bits
    }

    /// The bit words, for generated code that tests a name (`emit_js_own_field_set`).
    pub(in crate::vm) fn words(&self) -> &[u64] {
        self.bits.words()
    }

    /// The code that holds atoms unnamed, which a publication subscribes to.
    pub(in crate::vm) fn readers(&mut self) -> &mut crate::code_dependencies::CodeReaders {
        &mut self.readers
    }
}
