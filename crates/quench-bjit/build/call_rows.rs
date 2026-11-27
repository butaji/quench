//! The invocations of `stencils/call_rows.rs`'s `direct_row!` and `reload_row!` macros, generated from the pinned
//! places (`codes.rs`) instead of typed one per row. `build.rs` writes them to the file `QUENCH_STENCIL_CALL_ROWS`
//! names, which `call_rows.rs` includes in place of its own lists. Only the fixed-rows shard compiles them.

use crate::library::{DOUBLE_PINS, INT_PINS, PIN, PINF};

/// The most arguments a direct call row takes: its frame's window slots past the pinned leading ones (`DirectFrame::argument_words`).
const DIRECT_ARGC_MAX: usize = 6;

/// The modes a direct call row is built in: its name's suffix and its receiver binding (`call_rows.rs`'s
/// `STRICT`, `SLOPPY`, `LEXICAL`).
const DIRECT_MODES: [(&str, &str); 3] = [("strict", "STRICT"), ("sloppy", "SLOPPY"), ("lexical", "LEXICAL")];

/// The identity checks a direct row is built with: its name's suffix (only the unchecked variant has one) and whether it checks the callee register.
const DIRECT_CHECKS: [(&str, bool); 2] = [("", true), ("_unchecked", false)];

/// Every `direct_row!` (each argument count, mode and identity check), then a `reload_row!` for every pinned place, named by it.
/// Each direct row with arguments has a `_window` twin that reads no places (`PLACED` false): the planner names it when no
/// leading argument is pinned, which is the common call. A call with no arguments has only the general row.
pub fn invocations() -> String {
    let mut out = String::new();
    for argc in 0..=DIRECT_ARGC_MAX {
        for (mode, sloppy) in DIRECT_MODES {
            for (suffix, checked) in DIRECT_CHECKS {
                out += &format!("direct_row!(call_direct_a{argc}_{mode}{suffix}, {argc}, {sloppy}, {checked}, true);\n");
                if argc > 0 {
                    out += &format!("direct_row!(call_direct_a{argc}_{mode}{suffix}_window, {argc}, {sloppy}, {checked}, false);\n");
                }
            }
        }
    }
    for place in (PIN..PIN + INT_PINS).chain(PINF..PINF + DOUBLE_PINS) {
        out += &format!("reload_row!(reload_{place}, {place});\n");
    }
    out
}
