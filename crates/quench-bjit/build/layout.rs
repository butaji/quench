//! Row layout at build time: a cold `jmp HOLE` trampoline LLVM places after the hot
//! `jmp HOLE_NEXT` is folded into the branches that reach it, so the row ends with the
//! continuation jump and the stitcher can drop it when the next row follows.
//!
//! `jcc L; ...; jmp NEXT; L: jmp EXIT` becomes `jcc EXIT; ...; jmp NEXT`: the branch's rel32
//! field takes the trampoline's relocation and the trampoline is cut off the end. Nothing
//! before the cut moves, so no other offset changes. A row is rewritten only when every branch
//! that reaches a trampoline is a rel32 one (a rel8 branch cannot carry a relocation, and
//! widening it would shift the code), otherwise it is left as built.

/// A relocation as `build.rs` reads it: (offset, class, hole index, addend).
pub(crate) type Reloc = (u16, u8, u8, i32);

/// The relocation class of a pc-relative 32-bit successor hole, and the `HOLE_NEXT` index.
pub(crate) const SUCCESSOR: u8 = 1;
pub(crate) const NEXT: u32 = 0;
/// `jmp rel32`: the opcode byte and the field's width.
const JMP_REL32: u8 = 0xe9;
const REL32_BYTES: usize = 4;
const JMP_LEN: usize = 1 + REL32_BYTES;
/// `jcc rel32` is `0f 80+cc`, then the field; `jcc rel8` is `70+cc`, `jmp rel8` is `eb`.
const JCC_ESCAPE: u8 = 0x0f;
const JCC_REL32: std::ops::RangeInclusive<u8> = 0x80..=0x8f;
const JCC_REL8: std::ops::RangeInclusive<u8> = 0x70..=0x7f;
const JMP_REL8: u8 = 0xeb;
/// The pc-relative addend of a rel32 field that ends its instruction.
const REL32_ADDEND: i32 = -(REL32_BYTES as i32);

fn rel32_at(code: &[u8], field: usize) -> Option<i64> {
    let bytes = code.get(field..field + REL32_BYTES)?;
    Some(i64::from(i32::from_le_bytes(bytes.try_into().ok()?)))
}

/// The hole index of a trampoline `jmp HOLE` (not `HOLE_NEXT`) that ends `code[..end]`.
fn trampoline_ending_at(code: &[u8], relocs: &[Reloc], end: usize) -> Option<u8> {
    let start = end.checked_sub(JMP_LEN)?;
    let field = start + 1;
    let reloc = relocs.iter().find(|r| r.0 as usize == field)?;
    (code[start] == JMP_REL32 && reloc.1 == SUCCESSOR && u32::from(reloc.2) != NEXT).then_some(reloc.2)
}

/// Whether `jmp HOLE_NEXT` ends `code[..end]`.
fn continuation_ends_at(code: &[u8], relocs: &[Reloc], end: usize) -> bool {
    end >= JMP_LEN && code[end - JMP_LEN] == JMP_REL32 && relocs.iter().any(|r| r.0 as usize == end - REL32_BYTES && r.1 == SUCCESSOR && u32::from(r.2) == NEXT)
}

/// The rel32 fields of the branches in `code[..end]` that land on `target`; a field under a
/// relocation is a hole, not a displacement.
fn rel32_branches_to(code: &[u8], relocs: &[Reloc], end: usize, target: usize) -> Vec<usize> {
    let lands = |field: usize| !relocs.iter().any(|r| r.0 as usize == field) && rel32_at(code, field).is_some_and(|rel| (field + REL32_BYTES) as i64 + rel == target as i64);
    (0..end)
        .filter_map(|at| match code[at] {
            JMP_REL32 => lands(at + 1).then_some(at + 1),
            JCC_ESCAPE if JCC_REL32.contains(&code[at + 1]) => lands(at + 2).then_some(at + 2),
            _ => None,
        })
        .collect()
}

/// Whether a rel8 branch in `code[..end]` lands on `target`, which no relocation could redirect.
fn rel8_branch_to(code: &[u8], end: usize, target: usize) -> bool {
    (0..end).any(|at| (JCC_REL8.contains(&code[at]) || code[at] == JMP_REL8) && at as i64 + 2 + i64::from(code[at + 1] as i8) == target as i64)
}

/// The row with its trailing cold trampolines folded into the branches that reach them, or
/// `None` where the row has none or its layout is not exactly the pattern above.
pub(crate) fn fold_cold_trampolines(code: &[u8], relocs: &[Reloc]) -> Option<(Vec<u8>, Vec<Reloc>)> {
    let mut end = code.len();
    let mut cold = Vec::new();
    while let Some(hole) = trampoline_ending_at(code, relocs, end) {
        end -= JMP_LEN;
        cold.push((end, hole));
    }
    if cold.is_empty() || !continuation_ends_at(code, relocs, end) || cold.iter().any(|&(at, _)| rel8_branch_to(code, end, at)) {
        return None;
    }
    let mut folded = code[..end].to_vec();
    let mut kept: Vec<Reloc> = relocs.iter().copied().filter(|r| (r.0 as usize) < end).collect();
    for (at, hole) in cold {
        for field in rel32_branches_to(code, relocs, end, at) {
            folded[field..field + REL32_BYTES].fill(0);
            kept.push((u16::try_from(field).ok()?, SUCCESSOR, hole, REL32_ADDEND));
        }
    }
    kept.sort_unstable();
    Some((folded, kept))
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXIT0: u8 = 2;
    const TAKEN: u8 = 1;
    const JNE_REL32: [u8; 2] = [JCC_ESCAPE, 0x85];

    fn jmp_hole() -> [u8; JMP_LEN] {
        [JMP_REL32, 0, 0, 0, 0]
    }

    /// `jne L; nop; jmp NEXT; L: jmp EXIT0` becomes `jne EXIT0; nop; jmp NEXT`.
    #[test]
    fn a_cold_trampoline_folds_into_its_branch() {
        let displacement = 6i32.to_le_bytes();
        let code = [&JNE_REL32[..], &displacement, &[0x90], &jmp_hole(), &jmp_hole()].concat();
        let relocs = [(8, SUCCESSOR, 0, -4), (13, SUCCESSOR, EXIT0, -4)];
        let (folded, folded_relocs) = fold_cold_trampolines(&code, &relocs).expect("the pattern");
        assert_eq!(folded, [&JNE_REL32[..], &[0; 4], &[0x90], &jmp_hole()].concat());
        assert_eq!(folded_relocs, [(2, SUCCESSOR, EXIT0, -4), (8, SUCCESSOR, 0, -4)]);
    }

    /// Two trampolines behind the continuation jump, each reached by its own branch.
    #[test]
    fn every_trampoline_is_folded() {
        let (to_exit, to_taken) = (16i32.to_le_bytes(), 5i32.to_le_bytes());
        let code = [&JNE_REL32[..], &to_exit, &JNE_REL32, &to_taken, &jmp_hole(), &jmp_hole(), &jmp_hole()].concat();
        let relocs = [(13, SUCCESSOR, 0, -4), (18, SUCCESSOR, TAKEN, -4), (23, SUCCESSOR, EXIT0, -4)];
        let (folded, folded_relocs) = fold_cold_trampolines(&code, &relocs).expect("the pattern");
        assert_eq!(folded.len(), 17);
        assert_eq!(folded_relocs, [(2, SUCCESSOR, EXIT0, -4), (8, SUCCESSOR, TAKEN, -4), (13, SUCCESSOR, 0, -4)]);
    }

    /// A rel8 branch to the trampoline cannot carry a relocation: the row is left as built.
    #[test]
    fn a_short_branch_to_the_trampoline_leaves_the_row() {
        let code = [&[0x75, 0x06, 0x90][..], &jmp_hole(), &jmp_hole()].concat();
        let relocs = [(4, SUCCESSOR, 0, -4), (9, SUCCESSOR, EXIT0, -4)];
        assert!(fold_cold_trampolines(&code, &relocs).is_none());
    }

    /// No trampoline, or code after the continuation jump that is not one: nothing to fold.
    #[test]
    fn other_layouts_are_left_alone() {
        let ends_in_continuation = [&[0x90][..], &jmp_hole()].concat();
        assert!(fold_cold_trampolines(&ends_in_continuation, &[(2, SUCCESSOR, 0, -4)]).is_none());
        let cold_code = [&[0x90][..], &jmp_hole(), &[0x0f, 0x0b]].concat();
        assert!(fold_cold_trampolines(&cold_code, &[(2, SUCCESSOR, 0, -4)]).is_none());
    }
}
