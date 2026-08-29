//! QuickJS atoms: interned strings as `u32`. Half the range is immediate integers.

use std::collections::HashMap;

/// 32-bit interned name. `0` is `JS_ATOM_NULL`. Immediates occupy the high half.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct Atom(pub u32);

impl Atom {
    pub const NULL: Self = Self(0);
    const IMMEDIATE: u32 = 1 << 31;

    pub fn from_u32(n: u32) -> Option<Self> {
        if n >= Self::IMMEDIATE {
            return None;
        }
        Some(Self(n | Self::IMMEDIATE))
    }

    pub fn as_u32(self) -> Option<u32> {
        if self.0 & Self::IMMEDIATE != 0 {
            Some(self.0 & !Self::IMMEDIATE)
        } else {
            None
        }
    }
}

/// Runtime-owned intern table. Comparison of interned names is integer equality.
#[derive(Clone, Debug, Default)]
pub struct AtomTable {
    by_str: HashMap<Box<str>, Atom>,
    by_id: Vec<Box<str>>,
}

impl AtomTable {
    pub fn intern(&mut self, s: &str) -> Atom {
        if let Some(atom) = self.by_str.get(s) {
            return *atom;
        }
        let id = (self.by_id.len() as u32) + 1;
        let atom = Atom(id);
        let owned: Box<str> = s.into();
        self.by_str.insert(owned.clone(), atom);
        self.by_id.push(owned);
        atom
    }

    pub fn get(&self, atom: Atom) -> Option<&str> {
        if atom.as_u32().is_some() {
            return None;
        }
        self.by_id.get(atom.0.wrapping_sub(1) as usize).map(|s| &**s)
    }
}

#[cfg(test)]
mod tests {
    use super::{Atom, AtomTable};

    #[test]
    fn intern_is_identity() {
        let mut t = AtomTable::default();
        let a = t.intern("length");
        let b = t.intern("length");
        assert_eq!(a, b);
        assert_eq!(t.get(a), Some("length"));
    }

    #[test]
    fn immediate_int_atoms() {
        let a = Atom::from_u32(12).unwrap();
        assert_eq!(a.as_u32(), Some(12));
        assert_ne!(a, Atom::NULL);
    }
}
