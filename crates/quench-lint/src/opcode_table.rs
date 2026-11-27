//! One opcode table: its rows live in `quench-opcodes/src/table.rs`, and every consumer takes them
//! from there (the runtime by `with_opcode_table!`, build scripts and the lint by
//! `quench_opcodes::ROWS`). A second invocation of a table macro elsewhere is a second table, or
//! the text a consumer parses instead of the data, so every `opcodes!` invocation with parentheses outside the home counts
//! for `opcode-table`. Like `guest-name`, `oversized.txt` holds no allowance for one.

use crate::shape::Measures;

/// The measure every stray table invocation is recorded under, by file.
pub const MEASURE: &str = "opcode-table ";
/// The one file the table lives in.
const HOME: &str = "quench-opcodes/src/table.rs";
/// The text of a table invocation, spelled apart so this file does not match itself.
const INVOCATION: &str = concat!("opcodes", "!(");

/// Records, by `opcode-table <file>`, the table invocations a file outside the home spells.
pub fn scan(file: &str, source: &str, measures: &mut Measures) {
    let found = invocations(file, source);
    if found > 0 {
        measures.insert(format!("{MEASURE}{file}"), found);
    }
}

/// The table invocations `source` spells; none in the home.
fn invocations(file: &str, source: &str) -> usize {
    if file.ends_with(HOME) { 0 } else { source.matches(INVOCATION).count() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_home_spells_the_table() {
        let source = format!("{INVOCATION}$; Nop => Effect::PURE);");
        assert_eq!(invocations("crates/quench-runtime/src/bytecode.rs", &source), 1);
        assert_eq!(invocations("crates/quench-opcodes/src/table.rs", &source), 0);
        assert_eq!(invocations("crates/a.rs", "with_opcode_table!(define_ops);"), 0);
    }
}
