use crate::bytecode::Atom;
use crate::value::Value;

/// The semantic identity of an object property key.
///
/// Ordinary shapes currently store string keys as atoms for compact
/// transitions. This type is the authority at semantic edges: symbol keys
/// cannot be confused with strings, and private names have their own
/// namespace even when display text happens to match.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PropertyKey {
    String(Atom),
    Symbol(Value),
    /// A private name: the class home whose evaluation declared it and its name, the
    /// brand (`PrivateBrand`) an object's private element is stored under. The home is a
    /// value a live shape keeps alive (`gc_value`).
    Private(Value, Atom),
}

/// One hash write per key: the shape and proof memos probe by (shape, key) on every
/// generic lookup, where a derived hash wrote the discriminant and each field in turn. Equal
/// keys write equal words; distinct keys may share one, as any hash allows.
impl std::hash::Hash for PropertyKey {
    #[inline(always)]
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        state.write_u64(match *self {
            Self::String(atom) => u64::from(atom),
            Self::Symbol(value) => value.to_bits().rotate_left(17) ^ 1,
            Self::Private(home, atom) => home.to_bits() ^ (u64::from(atom) << 32) ^ 2,
        });
    }
}

impl PropertyKey {
    #[inline(always)] pub fn string(atom: Atom) -> Self {
        Self::String(atom)
    }

    #[inline(always)]
    pub fn symbol(value: Value) -> Self {
        Self::Symbol(value)
    }

    #[inline(always)]
    pub(crate) fn private(home: Value, atom: Atom) -> Self {
        Self::Private(home, atom)
    }

    /// The key's one-word image (K-ENTRY's key32): a string's atom below `1 << 30`, a
    /// symbol's record id with bit 30 set (symbols are records, so no slot index can name one). `None` for a private name and any key outside
    /// those ranges, which the fact table does not hold.
    #[inline(always)]
    pub(crate) fn key32(self) -> Option<u32> {
        const SYMBOL_CLASS: u32 = 1 << 30;
        match self {
            Self::String(atom) => (atom < SYMBOL_CLASS).then_some(atom),
            Self::Symbol(value) => value.direct_symbol_index().filter(|index| *index < SYMBOL_CLASS).map(|index| index | SYMBOL_CLASS),
            Self::Private(..) => None,
        }
    }

    /// The value a shape naming this key keeps alive: a symbol key's symbol, a private
    /// name's class home.
    pub(crate) fn gc_value(self) -> Option<Value> {
        match self {
            Self::Symbol(value) | Self::Private(value, _) => Some(value),
            Self::String(_) => None,
        }
    }

    pub(crate) fn symbol_value(self) -> Option<Value> {
        match self {
            Self::Symbol(value) => Some(value),
            Self::String(_) | Self::Private(..) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::PropertyKey;
    use crate::bytecode::Atom;
    use crate::value::Value;

    #[test]
    fn namespaces_do_not_alias() {
        let atom = Atom::from(7_u8);
        assert_ne!(PropertyKey::string(atom), PropertyKey::private(Value::NULL, atom));
        assert_ne!(
            PropertyKey::symbol(Value::number(7.0)),
            PropertyKey::string(atom)
        );
    }

    #[test]
    fn only_symbol_keys_project_back_to_values() {
        let symbol = Value::number(3.0);
        assert_eq!(PropertyKey::symbol(symbol).symbol_value(), Some(symbol));
        assert_eq!(PropertyKey::string(Atom::from(3_u8)).symbol_value(), None);
    }
}
