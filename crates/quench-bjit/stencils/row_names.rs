//! The heap rows' codes (the catalog in `names` mode, `row_catalog.rs`), and the ranges and last rows the stencils,
//! the runtime and the lib's tests read as codes. Mounted by the stencils (`codes.rs` re-exports it under
//! `stencil_build`), by the lib (`numeric.rs`), and read by the runtime through `numeric::row_names`. `build.rs` mounts it beside the library, which re-exports it.

use super::TYPED_KINDS;

catalog!(row_catalog(names));

/// The first and last staged codes: one heap-space run after the parse-int row.
pub const STAGED_FIRST: u8 = STAGED_REMAINDER;
pub const STAGED_LAST: u8 = STAGED_STRICT_NE;
/// The last row of each typed family.
pub const TYPED_INDEX_LAST: u8 = TYPED_INDEX_ROW + TYPED_KINDS - 1;
pub const TYPED_STORE_LAST: u8 = TYPED_STORE_ROW + TYPED_KINDS - 1;
