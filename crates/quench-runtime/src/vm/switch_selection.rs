//! What a `switch` selects: a string switch's case among its constants
//! (`StringCaseIndex`), and the entry a case table enters (`WasmBranchTable`), which a
//! string switch and a Wasm `br_table` share.
use super::*;

impl Vm {
    /// The position among `cases`, string constants, of the first strictly equal to
    /// `value`, or `cases.len()` when none is (`StringCaseIndex`). A program's equal string
    /// constants share one cell, so the first match by identity or by text is the first case
    /// whose `===` test holds.
    #[inline]
    pub fn string_case_index(&self, value: Value, cases: &[Value]) -> u32 {
        let position = self.heap.string(value).and_then(|text| {
            cases.iter().position(|case| {
                *case == value || self.heap.string(*case).is_some_and(|case| case == text)
            })
        });
        position.unwrap_or(cases.len()) as u32
    }

    /// The entry a case table takes past the first, the PC already naming that one: its
    /// selector's low 32 bits, any past the cases the default entry after them.
    pub(in crate::vm) fn branch_table_case(&self, f: usize, i: WideInstruction) -> usize {
        self.read(f, i.register_a()).wasm_bits32().min(i.imm()) as usize
    }
}
