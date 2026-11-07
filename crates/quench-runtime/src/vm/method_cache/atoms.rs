//! The atoms method caches may name, and the compiled code that holds an atom unnamed.

use crate::bytecode::Atom;

/// A bit per atom.
#[derive(Clone, Debug, Default)]
pub(in crate::vm) struct AtomBits {
    words: Vec<u64>,
}

impl AtomBits {
    #[inline(always)]
    fn position(atom: Atom) -> (usize, u64) {
        ((atom / u64::BITS) as usize, 1 << (atom % u64::BITS))
    }

    #[inline(always)]
    pub(in crate::vm) fn contains(&self, atom: Atom) -> bool {
        let (word, bit) = Self::position(atom);
        self.words.get(word).is_some_and(|bits| bits & bit != 0)
    }

    /// Sets `atom`'s bit; whether it was clear.
    fn insert(&mut self, atom: Atom) -> bool {
        let (word, bit) = Self::position(atom);
        if word >= self.words.len() {
            self.words.resize(word + 1, 0);
        }
        let new = self.words[word] & bit == 0;
        self.words[word] |= bit;
        new
    }
}

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
    readers: crate::code_dependencies::CodeReaders,
}

impl MethodCacheAtoms {
    #[inline(always)]
    pub(in crate::vm) fn contains(&self, atom: Atom) -> bool {
        self.bits.contains(atom)
    }

    /// Names `atom`, retiring the code that held it unnamed.
    pub(in crate::vm) fn name(&mut self, atom: Atom) {
        if self.bits.insert(atom) {
            self.readers.retire(atom);
        }
    }

    /// Unnames every atom: no code held one named, so none retires.
    pub(in crate::vm) fn clear(&mut self) {
        self.bits.words.fill(0);
    }

    /// The names now, for a lowering that folds them (`compiled::OnlineFacts`).
    pub(in crate::vm) fn bits(&self) -> &AtomBits {
        &self.bits
    }

    /// The bit words, for generated code that tests a name (`emit_js_own_field_set`).
    pub(in crate::vm) fn words(&self) -> &[u64] {
        &self.bits.words
    }

    /// The code that holds atoms unnamed, which a publication subscribes to.
    pub(in crate::vm) fn readers(&mut self) -> &mut crate::code_dependencies::CodeReaders {
        &mut self.readers
    }
}
