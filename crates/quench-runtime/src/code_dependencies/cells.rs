//! Weak readers of fact cells: code that trusts a cell (`typed_ir::Cell`) subscribes to the cell's
//! `FactKey`, and an edge that moves the cell up the lattice retires every reader of that key
//! (`heap::Dependencies::widen_cell`). The table is `Readers`, the one reader table code uses.

use std::{collections::BTreeMap, num::NonZeroU32};
use super::{ReaderKey, Readers};
use crate::typed_ir::FactKey;

/// Fact keys are sparse and ordered, so their list heads live in a `BTreeMap`.
impl ReaderKey for FactKey {
    type Heads = BTreeMap<FactKey, NonZeroU32>;
}

/// The readers of each fact cell's key.
pub type CellReaders = Readers<FactKey>;
