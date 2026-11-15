//! The iterator kinds of the heap's iterator records: what an iterator is, and what its helpers and
//! consumers do.

use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum IteratorKind {
    Array,
    ArrayKeys,
    ArrayValues,
    ArrayEntries,
    String,
    MapKeys,
    MapValues,
    MapEntries,
    SetValues,
    SetEntries,
    Protocol,
    RegExpStringMatchAll,
    IntlSegments,
    Map,
    Filter,
    Take,
    Drop,
    FlatMap,
    Concat,
    Zip,
    Generator,
    AsyncFromSync,
    AsyncGenerator,
}
impl IteratorKind {
    #[inline]
    pub(crate) fn is_array_iterator(self) -> bool {
        matches!(
            self,
            Self::Array | Self::ArrayKeys | Self::ArrayValues | Self::ArrayEntries
        )
    }

    #[inline]
    pub(crate) fn is_collection_iterator(self) -> bool {
        matches!(
            self,
            Self::MapKeys | Self::MapValues | Self::MapEntries | Self::SetValues | Self::SetEntries
        )
    }
}
#[derive(Clone, Debug)]
pub(crate) enum IteratorHelper {
    RegExpStringMatchAll {
        input: Value, // The string matched, the value each step passes to `exec`.
        global: bool,
        unicode: bool,
    },
    Map {
        callback: Value,
        index: usize,
    },
    Filter {
        callback: Value,
        index: usize,
    },
    Take {
        remaining: f64,
    },
    Drop {
        remaining: f64,
    },
    FlatMap {
        callback: Value,
        index: usize,
        inner: Option<Value>,
    },
    Concat {
        items: Vec<Value>,
        methods: Vec<Value>,
        opened: Vec<Option<Value>>,
        next_item: usize,
        active: Option<Value>,
    },
    Zip {
        iterators: Vec<Value>,
        padding: Vec<Value>,
        mode: IteratorZipMode,
        keys: Option<Vec<Value>>,
        opened: Vec<bool>,
        done: bool,
    },
}
#[derive(Clone, Copy, Debug)]
pub(crate) enum IteratorZipMode {
    Shortest,
    Longest,
    Strict,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum IteratorConsumer {
    Reduce,
    ToArray,
    ForEach,
    Every,
    Find,
    Some,
}
