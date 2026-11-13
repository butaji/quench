use crate::bytecode::Atom;
use crate::heap::PrivateAtom;
use crate::value::Value;

/// The semantic identity of an object property key.
///
/// Ordinary shapes currently store string keys as atoms for compact
/// transitions. This type is the authority at semantic edges: symbol keys
/// cannot be confused with strings, and private names have their own
/// namespace even when display text happens to match.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum PropertyKey {
    String(Atom),
    Symbol(Value),
    /// A private field or brand of one class evaluation (`PrivateAtom`): a shape slot like any
    /// other, but never a key of reflection. Own-key listing, descriptors, enumeration, proxies'
    /// traps and structured copies name only string and symbol keys, so no operation of the
    /// language reaches a private key but the private-name operations themselves.
    Private(PrivateAtom),
}

impl PropertyKey {
    pub(crate) fn string(atom: Atom) -> Self {
        Self::String(atom)
    }

    pub(crate) fn symbol(value: Value) -> Self {
        Self::Symbol(value)
    }

    #[cfg(test)]
    pub(crate) fn private(atom: PrivateAtom) -> Self {
        Self::Private(atom)
    }

    pub(crate) fn symbol_value(self) -> Option<Value> {
        match self {
            Self::Symbol(value) => Some(value),
            Self::String(_) | Self::Private(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::PropertyKey;
    use crate::bytecode::Atom;
    use crate::heap::PrivateAtom;
    use crate::value::Value;

    #[test]
    fn namespaces_do_not_alias() {
        let atom: Atom = 7;
        assert_ne!(PropertyKey::string(atom), PropertyKey::private(PrivateAtom::from_raw(atom)));
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
