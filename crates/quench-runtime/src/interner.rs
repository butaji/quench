//! String and symbol interner for the HIR/object model.
//!
//! Interning property names, method names, and well-known symbols gives the
//! runtime stable `Symbol` handles that can be compared in O(1) and used as
//! keys in shapes and hash maps.

use std::collections::HashMap;

/// Opaque handle to an interned string or symbol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Symbol(pub u32);

impl Symbol {
    /// A reserved symbol meaning "no such property".
    pub const NONE: Symbol = Symbol(u32::MAX);
}

/// Bidirectional string interner.
#[derive(Debug, Default)]
pub struct StringInterner {
    strings: Vec<String>,
    index: HashMap<String, Symbol>,
}

impl StringInterner {
    /// Create an empty interner.
    pub fn new() -> Self {
        Self {
            strings: Vec::new(),
            index: HashMap::new(),
        }
    }

    /// Intern a string and return its stable symbol.
    pub fn intern(&mut self, s: impl Into<String>) -> Symbol {
        let s = s.into();
        if let Some(&sym) = self.index.get(&s) {
            return sym;
        }
        let id = self.strings.len() as u32;
        let sym = Symbol(id);
        self.index.insert(s.clone(), sym);
        self.strings.push(s);
        sym
    }

    /// Resolve a symbol back to its string.
    pub fn resolve(&self, sym: Symbol) -> Option<&str> {
        self.strings.get(sym.0 as usize).map(|s| s.as_str())
    }

    /// Total number of interned strings.
    pub fn len(&self) -> usize {
        self.strings.len()
    }

    /// Whether the interner is empty.
    pub fn is_empty(&self) -> bool {
        self.strings.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_intern_same_symbol() {
        let mut interner = StringInterner::new();
        let a = interner.intern("foo");
        let b = interner.intern("foo");
        assert_eq!(a, b);
        assert_eq!(interner.len(), 1);
    }

    #[test]
    fn test_intern_different_symbols() {
        let mut interner = StringInterner::new();
        let a = interner.intern("foo");
        let b = interner.intern("bar");
        assert_ne!(a, b);
        assert_eq!(interner.len(), 2);
    }

    #[test]
    fn test_resolve() {
        let mut interner = StringInterner::new();
        let sym = interner.intern("hello");
        assert_eq!(interner.resolve(sym), Some("hello"));
        assert_eq!(interner.resolve(Symbol::NONE), None);
    }
}
