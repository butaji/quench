//! The invocations of `stencils/call_rows.rs`'s `direct_row!` and `reload_row!` macros, generated from the pinned
//! places (`codes.rs`) instead of typed one per row. `build.rs` writes them to the file `QUENCH_STENCIL_CALL_ROWS`
//! names, which `call_rows.rs` includes in place of its own lists. Only the fixed-rows shard compiles them.

use crate::library::{DOUBLE_PINS, INT_PINS, PIN, PINF};

/// The most arguments a direct call row takes: the argument registers `p0..p3` of the row ABI (`rows.rs`).
const DIRECT_ARGC_MAX: usize = 3;

/// The modes a direct call row is built in: its name's suffix and its receiver binding (`call_rows.rs`'s
/// `STRICT`, `SLOPPY`, `LEXICAL`).
const DIRECT_MODES: [(&str, &str); 3] = [("strict", "STRICT"), ("sloppy", "SLOPPY"), ("lexical", "LEXICAL")];

/// Every `direct_row!` (each argument count in both modes), then a `reload_row!` for every pinned place, named by it.
pub fn invocations() -> String {
    let direct = (0..=DIRECT_ARGC_MAX).flat_map(|argc| DIRECT_MODES.map(|(mode, sloppy)| format!("direct_row!(call_direct_a{argc}_{mode}, {argc}, {sloppy});\n")));
    let pinned = (PIN..PIN + INT_PINS).chain(PINF..PINF + DOUBLE_PINS);
    direct.chain(pinned.map(|place| format!("reload_row!(reload_{place}, {place});\n"))).collect()
}
