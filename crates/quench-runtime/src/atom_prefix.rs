use crate::bytecode::{Atom, AtomTable};

/// The atom names a compilation extends. Atom ids below `len` are the host's, so the
/// compiled program's names, values and property sites compare identically with it.
pub(crate) trait AtomPrefix {
    fn len(&self) -> usize;
    fn name(&self, atom: Atom) -> &str;
    /// The prefix atom whose text is exactly `name`. An atom whose UTF-16 has no lossless
    /// host spelling keeps its id but is found by no name.
    fn find(&self, name: &str) -> Option<Atom>;
    /// The prefix's names, which the compiled program reads until its host adopts it.
    fn table(&self) -> AtomTable;
    /// Whether the host runs specialized programs, which a program extending it follows.
    fn specialized(&self) -> bool;
    /// Optional producer-owned obligations; ordinary execution prefixes retain none.
    #[cfg(feature = "aot")]
    fn native_source_requests(&self) -> Option<&std::cell::RefCell<std::collections::BTreeSet<super::NativeSourceKey>>> { None }
    #[cfg(feature = "aot")]
    fn native_global_script_calls(&self) -> &[(String, String)] { &[] }

}

/// A standalone program numbers its atoms from zero.
pub(crate) struct NoAtomPrefix;

impl AtomPrefix for NoAtomPrefix {
    fn len(&self) -> usize {
        0
    }

    fn name(&self, atom: Atom) -> &str {
        unreachable!("atom {atom} lies outside the empty prefix")
    }

    fn find(&self, _name: &str) -> Option<Atom> {
        None
    }

    fn table(&self) -> AtomTable {
        AtomTable::default()
    }

    fn specialized(&self) -> bool {
        false
    }
}

