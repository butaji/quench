use crate::value::Value;

/// The well-known symbols (ECMA-262 Table 1), in their installation order on `Symbol`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WellKnownSymbol {
    AsyncDispose,
    AsyncIterator,
    Dispose,
    HasInstance,
    IsConcatSpreadable,
    Iterator,
    Match,
    MatchAll,
    Metadata,
    Replace,
    Search,
    Species,
    Split,
    ToPrimitive,
    ToStringTag,
    Unscopables,
}

impl WellKnownSymbol {
    pub(crate) const ALL: [Self; 16] = [
        Self::AsyncDispose,
        Self::AsyncIterator,
        Self::Dispose,
        Self::HasInstance,
        Self::IsConcatSpreadable,
        Self::Iterator,
        Self::Match,
        Self::MatchAll,
        Self::Metadata,
        Self::Replace,
        Self::Search,
        Self::Species,
        Self::Split,
        Self::ToPrimitive,
        Self::ToStringTag,
        Self::Unscopables,
    ];

    /// The property name on `Symbol`, indexed by discriminant (`ALL` order); the description is
    /// `Symbol.` and this name.
    const NAMES: [&'static str; Self::ALL.len()] = [
        "asyncDispose",
        "asyncIterator",
        "dispose",
        "hasInstance",
        "isConcatSpreadable",
        "iterator",
        "match",
        "matchAll",
        "metadata",
        "replace",
        "search",
        "species",
        "split",
        "toPrimitive",
        "toStringTag",
        "unscopables",
    ];

    pub(crate) fn name(self) -> &'static str {
        Self::NAMES[self as usize]
    }
}

/// The realm-independent well-known symbol values, one slot per `WellKnownSymbol`, empty
/// until `Symbol` is installed.
#[derive(Clone, Default)]
pub struct WellKnownSymbols(pub(super) [Option<Value>; WellKnownSymbol::ALL.len()]);

impl WellKnownSymbols {
    #[inline(always)]
    pub fn get(&self, symbol: WellKnownSymbol) -> Option<Value> {
        self.0[symbol as usize]
    }
    pub(crate) fn insert(&mut self, symbol: WellKnownSymbol, value: Value) {
        self.0[symbol as usize] = Some(value);
    }
    /// The symbol whose `Symbol` property name is `name`.
    pub(crate) fn named(&self, name: &str) -> Option<Value> {
        WellKnownSymbol::ALL
            .into_iter()
            .find(|symbol| symbol.name() == name)
            .and_then(|symbol| self.get(symbol))
    }
    /// The installed symbols with their names, in `WellKnownSymbol::ALL` order.
    pub(crate) fn entries(&self) -> impl Iterator<Item = (&'static str, Value)> + '_ {
        WellKnownSymbol::ALL
            .into_iter()
            .filter_map(|symbol| self.get(symbol).map(|value| (symbol.name(), value)))
    }
    pub(crate) fn values(&self) -> impl Iterator<Item = Value> + '_ {
        self.0.iter().flatten().copied()
    }
    pub(crate) fn clear(&mut self) {
        self.0 = [None; WellKnownSymbol::ALL.len()];
    }
}

impl std::ops::Index<WellKnownSymbol> for WellKnownSymbols {
    type Output = Value;
    fn index(&self, symbol: WellKnownSymbol) -> &Value {
        self.0[symbol as usize]
            .as_ref()
            .expect("well-known symbols are installed with Symbol")
    }
}
