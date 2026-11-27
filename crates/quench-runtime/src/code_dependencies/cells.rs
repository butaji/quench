//! Weak readers of fact cells: code that trusts a cell (`typed_ir::Cell`) subscribes to the cell's
//! `FactKey`, and an edge that moves the cell up the lattice retires every reader of that key
//! (`heap::Dependencies::widen_cell`). The table is `Readers`, the one reader table code uses.

use super::Readers;
use crate::typed_ir::FactKey;

/// The readers of each fact cell's key.
pub type CellReaders = Readers<FactKey>;
