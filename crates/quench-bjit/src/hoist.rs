//! Moves a stencil's cold exit tails off its end so the hot tail ends it.
use crate::{HOLE_CODE, JCC_ESCAPE, JCC_NEAR, JCC_REL32_LEN, JMP_OPCODE_LEN, JMP_REL32_LEN, JMP_REL32_OPCODE, NEXT, NOP4, REL32_LEN, Reloc, UNIT_LEN};

/// The high nibble mask of the second opcode byte of a `jcc rel32`.
const NIBBLE: u8 = 0xf0;
/// A `jcc rel32`'s displacement starts after its two opcode bytes.
const JCC_OPCODE_LEN: usize = 2;

/// The exit units after the hot tail: where the hot part ends and how many `nop4; jmp rel32`
/// units follow, when the stencil has exactly that shape.
fn units(code: &[u8], relocs: &[Reloc]) -> Option<(usize, usize)> {
    let next = relocs.iter().find(|r| r.pc_relative && r.namespace == HOLE_CODE && r.index == NEXT)?;
    let hot_end = next.offset as usize + REL32_LEN;
    let tail = code.len() - hot_end;
    let units = tail / UNIT_LEN;
    let unit_ok = |k: usize| {
        let at = hot_end + k * UNIT_LEN;
        code[at..at + NOP4.len()] == NOP4 && unit_reloc(relocs, hot_end, k).is_some()
    };
    (tail != 0 && tail % UNIT_LEN == 0 && (0..units).all(unit_ok)).then_some((hot_end, units))
}

fn unit_reloc(relocs: &[Reloc], hot_end: usize, k: usize) -> Option<&Reloc> {
    let at = hot_end + k * UNIT_LEN + NOP4.len() + JMP_OPCODE_LEN;
    relocs.iter().find(|r| r.pc_relative && r.offset as usize == at)
}

/// The jump at `p` in the hot part: its length and where its displacement starts.
fn jump_at(code: &[u8], p: usize) -> Option<(usize, usize)> {
    match (code[p], code[p + 1] & NIBBLE) {
        (JCC_ESCAPE, JCC_NEAR) => Some((JCC_REL32_LEN, p + JCC_OPCODE_LEN)),
        (JMP_REL32_OPCODE, _) => Some((JMP_REL32_LEN, p + JMP_OPCODE_LEN)),
        _ => None,
    }
}

/// Moves the exit tails off the end: a frameless stencil's cold exits follow its hot `jmp next`
/// as units of `nop4; jmp rel32`. Each jump in the hot part that lands on a unit becomes a jump
/// relocated to that exit directly, and the units are cut, so the hot tail ends the stencil and
/// adjacency drops it. A stencil of any other shape is kept whole.
pub(crate) fn hoist_exits(mut code: Vec<u8>, relocs: Vec<Reloc>) -> (Vec<u8>, Vec<Reloc>) {
    let Some((hot_end, count)) = units(&code, &relocs) else { return (code, relocs) };
    let relocated = |at: usize| relocs.iter().any(|r| (r.offset as usize..r.offset as usize + REL32_LEN).contains(&at));
    let mut added = Vec::new();
    for p in 0..hot_end.saturating_sub(JMP_REL32_LEN) {
        let Some((length, disp_at)) = jump_at(&code, p) else { continue };
        if relocated(disp_at) || relocated(p) {
            continue;
        }
        let disp = i32::from_le_bytes(code[disp_at..disp_at + REL32_LEN].try_into().expect("a displacement"));
        let target = (p + length) as i64 + i64::from(disp);
        let landed = (0..count).find(|k| (hot_end + k * UNIT_LEN) as i64 == target);
        let Some(exit) = landed.and_then(|k| unit_reloc(&relocs, hot_end, k)) else { continue };
        added.push(Reloc { offset: disp_at as u32, ..exit.clone() });
        code[disp_at..disp_at + REL32_LEN].fill(0);
    }
    code.truncate(hot_end);
    let mut kept: Vec<Reloc> = relocs.into_iter().filter(|r| (r.offset as usize) < hot_end).collect();
    kept.extend(added);
    (code, kept)
}
