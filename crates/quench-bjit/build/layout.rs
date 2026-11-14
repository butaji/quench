//! Row layout at build time. LLVM places the cold exits of a row after its hot `jmp HOLE_NEXT`,
//! which the stitcher could then not drop when the next row follows. Each row is decoded, its
//! cold `jmp HOLE` trampolines folded into the branches that reach them (`jcc L; ..; L: jmp
//! EXIT` becomes `jcc EXIT`), and its code split in two: the hot part (the entry run and the
//! run ending in `jmp HOLE_NEXT`, so that jump is the hot part's last instruction) and the cold
//! part (every other run). Both are re-encoded by `iced_x86`'s block encoder, which widens a
//! short branch where its target moved. The stitcher places every row's cold part after all hot
//! parts, so a branch between the two parts is a rel32 whose relocation names the target's
//! offset in the row's own code (`LOCAL`).
//!
//! An instruction with a relocation (a hole branch, a `movabs`, a 32-bit immediate) is emitted
//! as its own bytes, so its relocation offsets move with it unchanged. A row that does not
//! decode into runs ending in unconditional jumps, or does not re-encode, keeps its built bytes.

use iced_x86::{BlockEncoder, BlockEncoderOptions, Decoder, DecoderOptions, Instruction, InstructionBlock, Mnemonic};
use std::ops::Range;

/// A relocation as `build.rs` reads it: (offset, class, hole index, addend).
pub(crate) type Reloc = (u16, u8, u8, i32);

use crate::class::{LOCAL as LOCAL_INDEX, NEXT, REL32 as SUCCESSOR};

/// The hole index of a successor relocation to the row's own code (`class::LOCAL`), as a relocation tuple carries it.
pub(crate) const LOCAL: u8 = LOCAL_INDEX as u8;

const BITNESS: u32 = 64;
/// Where the decoder puts the built code: not zero, which the encoder reads as "no address".
const ORIGINAL_BASE: u64 = 1 << 20;
/// Where the encoder puts the parts: far enough apart that a branch between them stays rel32.
const HOT_BASE: u64 = 0;
const COLD_BASE: u64 = 1 << 30;
const REL32_BYTES: usize = 4;
/// The pc-relative addend of a rel32 field that ends its instruction.
#[cfg(test)]
const REL32_ADDEND: i32 = -(REL32_BYTES as i32);
const JMP_REL32: u8 = 0xe9;
const JMP_LEN: usize = 1 + REL32_BYTES;
const JCC_ESCAPE: u8 = 0x0f;
const JCC_NEAR_BASE: u8 = 0x80;
const CONDITION_MASK: u8 = 0x0f;
const JCC_NEAR: std::ops::RangeInclusive<u8> = JCC_NEAR_BASE..=JCC_NEAR_BASE + CONDITION_MASK;
const JCC_LEN: usize = 2 + REL32_BYTES;
/// Instructions that end a row or leave it by something other than a direct jump.
const NOT_A_ROW_BODY: [Mnemonic; 14] = [
    Mnemonic::Ret, Mnemonic::Retf, Mnemonic::Iretq, Mnemonic::Ud2, Mnemonic::Hlt, Mnemonic::Int3, Mnemonic::Call, Mnemonic::Jmp,
    Mnemonic::Loop, Mnemonic::Loope, Mnemonic::Loopne, Mnemonic::Jrcxz, Mnemonic::Jecxz, Mnemonic::Jcxz,
];

/// A row's code split into its hot part (`..hot_len`, ending in `jmp HOLE_NEXT`) and cold part.
pub(crate) struct Layout {
    pub(crate) code: Vec<u8>,
    pub(crate) relocs: Vec<Reloc>,
    pub(crate) hot_len: usize,
}

#[derive(Clone, Copy, PartialEq)]
enum Flow {
    Plain,
    /// An unconditional jump: to an instruction of the row, or (`None`) through a hole.
    Jump(Option<u64>),
    /// A conditional branch, as `Jump`.
    Branch(Option<u64>),
}

struct Op {
    ip: u64,
    bytes: Vec<u8>,
    /// The relocations inside this instruction, as offsets from its start.
    relocs: Vec<Reloc>,
    instr: Instruction,
    flow: Flow,
}

impl Op {
    fn target(&self) -> Option<u64> {
        match self.flow {
            Flow::Jump(target) | Flow::Branch(target) => target,
            Flow::Plain => None,
        }
    }

    fn is_continuation(&self) -> bool {
        self.flow == Flow::Jump(None) && self.relocs.iter().any(|r| r.1 == SUCCESSOR && u32::from(r.2) == NEXT)
    }
}

/// The flow of a hole instruction, read from its bytes.
fn hole_flow(bytes: &[u8]) -> Flow {
    match bytes {
        [JMP_REL32, ..] if bytes.len() == JMP_LEN => Flow::Jump(None),
        [JCC_ESCAPE, condition, ..] if JCC_NEAR.contains(condition) && bytes.len() == JCC_LEN => Flow::Branch(None),
        _ => Flow::Plain,
    }
}

fn flow_of(instr: &Instruction, bytes: &[u8], hole: bool) -> Option<Flow> {
    if hole {
        return Some(hole_flow(bytes));
    }
    if instr.is_jmp_short_or_near() {
        return Some(Flow::Jump(Some(instr.near_branch_target())));
    }
    if instr.is_jcc_short_or_near() {
        return Some(Flow::Branch(Some(instr.near_branch_target())));
    }
    (!NOT_A_ROW_BODY.contains(&instr.mnemonic())).then_some(Flow::Plain)
}

/// The row's instructions, in address order, or `None` where it does not decode.
fn decode(code: &[u8], relocs: &[Reloc]) -> Option<Vec<Op>> {
    let mut decoder = Decoder::with_ip(BITNESS, code, ORIGINAL_BASE, DecoderOptions::NONE);
    let mut ops = Vec::new();
    while decoder.can_decode() {
        let instr = decoder.decode();
        if instr.is_invalid() {
            return None;
        }
        let start = usize::try_from(instr.ip() - ORIGINAL_BASE).ok()?;
        let bytes = code[start..start + instr.len()].to_vec();
        let inside: Vec<Reloc> = relocs.iter().filter(|r| (start..start + bytes.len()).contains(&(r.0 as usize))).map(|r| (r.0 - start as u16, r.1, r.2, r.3)).collect();
        let flow = flow_of(&instr, &bytes, !inside.is_empty())?;
        ops.push(Op { ip: instr.ip(), bytes, relocs: inside, instr, flow });
    }
    Some(ops)
}

/// The hole (index, addend) of a run that is a lone `jmp HOLE` other than `HOLE_NEXT`.
fn trampoline_hole(ops: &[Op], at: usize) -> Option<(u8, i32)> {
    let after_a_jump = at > 0 && matches!(ops[at - 1].flow, Flow::Jump(_));
    let reloc = ops[at].relocs.first()?;
    (after_a_jump && ops[at].flow == Flow::Jump(None) && reloc.1 == SUCCESSOR && u32::from(reloc.2) != NEXT).then_some((reloc.2, reloc.3))
}

/// `op`'s branch, going through `hole` itself.
fn through_hole(op: &Op, (index, addend): (u8, i32)) -> Op {
    let (bytes, flow) = match op.flow {
        Flow::Branch(_) => {
            let condition = if op.bytes[0] == JCC_ESCAPE { op.bytes[1] } else { op.bytes[0] } & CONDITION_MASK;
            (vec![JCC_ESCAPE, JCC_NEAR_BASE | condition, 0, 0, 0, 0], Flow::Branch(None))
        }
        _ => (vec![JMP_REL32, 0, 0, 0, 0], Flow::Jump(None)),
    };
    let field = u16::try_from(bytes.len() - REL32_BYTES).expect("a short instruction");
    Op { ip: op.ip, relocs: vec![(field, SUCCESSOR, index, addend)], bytes, flow, instr: op.instr }
}

/// Folds each trampoline into the branches that reach it, and drops the ones nothing reaches.
fn fold_trampolines(ops: &mut Vec<Op>) {
    let holes: Vec<(u64, (u8, i32))> = (0..ops.len()).filter_map(|at| Some((ops[at].ip, trampoline_hole(ops, at)?))).collect();
    for op in ops.iter_mut() {
        let hole = op.target().and_then(|target| holes.iter().find(|(ip, _)| *ip == target));
        if let Some((_, hole)) = hole {
            *op = through_hole(op, *hole);
        }
    }
    let reached: Vec<u64> = ops.iter().filter_map(Op::target).collect();
    ops.retain(|op| !holes.iter().any(|(ip, _)| *ip == op.ip) || reached.contains(&op.ip));
}

/// The runs of instructions that end in an unconditional jump, as ranges of `ops`; `None` where
/// the code does not end in one (it would fall off its end).
fn runs(ops: &[Op]) -> Option<Vec<Range<usize>>> {
    let mut runs = Vec::new();
    let mut start = 0;
    for at in (0..ops.len()).filter(|at| matches!(ops[*at].flow, Flow::Jump(_))) {
        runs.push(start..at + 1);
        start = at + 1;
    }
    (start == ops.len() && !runs.is_empty()).then_some(runs)
}

/// The hot and cold instruction indexes: the entry run and the run ending in `jmp HOLE_NEXT`
/// (the only one), then the rest in address order.
fn placement(ops: &[Op], runs: &[Range<usize>]) -> Option<[Vec<usize>; 2]> {
    let mut continuing = (0..runs.len()).filter(|&k| ops[runs[k].end - 1].is_continuation());
    let next = continuing.next().filter(|_| continuing.next().is_none())?;
    let hot_runs: Vec<usize> = if next == 0 { vec![0] } else { vec![0, next] };
    let flatten = |hot: bool| -> Vec<usize> { (0..runs.len()).filter(|k| hot_runs.contains(k) == hot).flat_map(|k| runs[k].clone()).collect() };
    Some([flatten(true), flatten(false)])
}

/// One encoded part: where it starts in the row's code, its bytes, and where each of its
/// instructions (`ops[k]` for `k` in `ops`) starts in them.
struct Region {
    start: usize,
    ops: Vec<usize>,
    bytes: Vec<u8>,
    offsets: Vec<u32>,
    first: Vec<usize>,
}

/// The encoder's instructions for `indexes`: a hole instruction as its bytes (one `db` each, only
/// the first carrying the address a branch names), and where each op's instructions start.
fn instructions(ops: &[Op], indexes: &[usize]) -> (Vec<Instruction>, Vec<usize>) {
    let (mut list, mut first) = (Vec::new(), Vec::new());
    for &k in indexes {
        first.push(list.len());
        if ops[k].relocs.is_empty() {
            list.push(ops[k].instr);
            continue;
        }
        for (n, byte) in ops[k].bytes.iter().enumerate() {
            let mut data = Instruction::with_declare_byte_1(*byte);
            data.set_ip(if n == 0 { ops[k].ip } else { 0 });
            list.push(data);
        }
    }
    (list, first)
}

fn encode(ops: &[Op], placement: [Vec<usize>; 2]) -> Option<Vec<Region>> {
    let parts: Vec<(Vec<usize>, Vec<Instruction>, Vec<usize>)> = placement.into_iter().filter(|p| !p.is_empty()).map(|p| {
        let (list, first) = instructions(ops, &p);
        (p, list, first)
    }).collect();
    let blocks: Vec<InstructionBlock> = parts.iter().zip([HOT_BASE, COLD_BASE]).map(|((_, list, _), base)| InstructionBlock::new(list, base)).collect();
    let results = BlockEncoder::encode_slice(BITNESS, &blocks, BlockEncoderOptions::RETURN_NEW_INSTRUCTION_OFFSETS).ok()?;
    let mut start = 0;
    let mut regions = Vec::new();
    for ((indexes, _, first), result) in parts.into_iter().zip(results) {
        regions.push(Region { start, ops: indexes, offsets: result.new_instruction_offsets, first, bytes: result.code_buffer });
        start += regions.last()?.bytes.len();
    }
    Some(regions)
}

/// The encoded row: addresses of every op in the new code, and which part holds it.
struct Placed<'a> {
    ops: &'a [Op],
    address: Vec<usize>,
    region_of: Vec<usize>,
    code: Vec<u8>,
}

impl<'a> Placed<'a> {
    fn new(ops: &'a [Op], regions: &[Region]) -> Option<Self> {
        let mut placed = Placed { ops, address: vec![0; ops.len()], region_of: vec![0; ops.len()], code: regions.iter().flat_map(|r| r.bytes.iter().copied()).collect() };
        for (n, region) in regions.iter().enumerate() {
            for (k, &op) in region.ops.iter().enumerate() {
                let offset = region.offsets[region.first[k]];
                placed.address[op] = region.start + usize::try_from(offset).ok().filter(|_| offset != u32::MAX)?;
                placed.region_of[op] = n;
            }
        }
        Some(placed)
    }

    /// The relocations of `ops[k]`, which spans `start..end` of the new code: a hole
    /// instruction's own, moved; a branch into the other part gets a `LOCAL` one.
    fn relocs_of(&self, k: usize, start: usize, end: usize) -> Option<Vec<Reloc>> {
        let op = &self.ops[k];
        if !op.relocs.is_empty() {
            return op.relocs.iter().map(|r| Some((u16::try_from(start + r.0 as usize).ok()?, r.1, r.2, r.3))).collect();
        }
        let Some(target) = op.target() else { return Some(Vec::new()) };
        let target = self.ops.binary_search_by_key(&target, |o| o.ip).ok()?;
        if self.region_of[target] == self.region_of[k] {
            return Some(Vec::new());
        }
        if hole_flow(&self.code[start..end]) == Flow::Plain {
            return None;
        }
        Some(vec![(u16::try_from(end - REL32_BYTES).ok()?, SUCCESSOR, LOCAL, i32::try_from(self.address[target]).ok()?)])
    }
}

/// The new code and relocations of the encoded parts, once the hot part ends in `jmp HOLE_NEXT`.
fn assemble(ops: &[Op], regions: &[Region]) -> Option<Layout> {
    let placed = Placed::new(ops, regions)?;
    let mut relocs = Vec::new();
    for region in regions {
        for (j, &k) in region.ops.iter().enumerate() {
            let end = region.ops.get(j + 1).map_or(region.start + region.bytes.len(), |&next| placed.address[next]);
            relocs.extend(placed.relocs_of(k, placed.address[k], end)?);
        }
    }
    relocs.sort_unstable();
    let hot_len = regions[0].bytes.len();
    let continues = relocs.iter().any(|r| r.1 == SUCCESSOR && u32::from(r.2) == NEXT && r.0 as usize + REL32_BYTES == hot_len);
    // The encoder pre-filled each widened branch's displacement; its `LOCAL` relocation owns the field.
    let mut code = placed.code;
    for at in relocs.iter().filter(|r| r.2 == LOCAL).map(|r| r.0 as usize) {
        code[at..at + REL32_BYTES].fill(0);
    }
    continues.then_some(Layout { code, relocs, hot_len })
}

/// The row with its cold exits folded and its code laid out hot part then cold part, or `None`
/// where it is not of the shape this handles (it then keeps its built bytes).
pub(crate) fn reorder(code: &[u8], relocs: &[Reloc]) -> Option<Layout> {
    let mut ops = decode(code, relocs)?;
    fold_trampolines(&mut ops);
    let run_list = runs(&ops)?;
    let parts = placement(&ops, &run_list)?;
    assemble(&ops, &encode(&ops, parts)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXIT0: u8 = 2;
    const TAKEN: u8 = 1;
    const JE_REL8: u8 = 0x74;
    const JE: [u8; 2] = [JCC_ESCAPE, 0x84];
    const JNE: [u8; 2] = [JCC_ESCAPE, 0x85];
    const NOP: u8 = 0x90;
    const INC_EAX: [u8; 2] = [0xff, 0xc0];
    const HOLE_JMP: [u8; JMP_LEN] = [JMP_REL32, 0, 0, 0, 0];
    const NO_FIELD: [u8; REL32_BYTES] = [0; REL32_BYTES];

    fn successor(offset: u16, hole: u8) -> Reloc {
        (offset, SUCCESSOR, hole, REL32_ADDEND)
    }

    /// `jne L; nop; jmp NEXT; L: jmp EXIT` becomes `jne EXIT; nop; jmp NEXT`: no cold part.
    #[test]
    fn a_cold_trampoline_folds_into_its_branch() {
        let code = [&JNE[..], &6i32.to_le_bytes(), &[NOP], &HOLE_JMP, &HOLE_JMP].concat();
        let layout = reorder(&code, &[successor(8, 0), successor(13, EXIT0)]).expect("the pattern");
        assert_eq!(layout.code, [&JNE[..], &NO_FIELD, &[NOP], &HOLE_JMP].concat());
        assert_eq!(layout.relocs, [successor(2, EXIT0), successor(8, 0)]);
        assert_eq!(layout.hot_len, layout.code.len());
    }

    /// A short branch to code behind `jmp NEXT`: the cold code moves to the cold part and the
    /// branch widens to rel32 with a `LOCAL` relocation to it.
    #[test]
    fn a_short_branch_widens_and_the_cold_block_moves() {
        let code = [&[JE_REL8, 6, NOP][..], &HOLE_JMP, &INC_EAX, &HOLE_JMP].concat();
        let layout = reorder(&code, &[successor(4, 0), successor(11, EXIT0)]).expect("the pattern");
        let hot = [&JE[..], &NO_FIELD, &[NOP], &HOLE_JMP].concat();
        assert_eq!(layout.code, [&hot[..], &INC_EAX, &HOLE_JMP].concat());
        assert_eq!(layout.hot_len, hot.len());
        assert_eq!(layout.relocs, [(2, SUCCESSOR, LOCAL, hot.len() as i32), successor(8, 0), successor(15, EXIT0)]);
    }

    /// A cold block between `jmp NEXT` and the trampoline: the trampoline folds away and the
    /// block moves behind the hot part.
    #[test]
    fn a_cold_block_between_the_continuation_and_the_trampoline_moves() {
        let code = [&JE[..], &19i32.to_le_bytes(), &JNE, &6i32.to_le_bytes(), &[NOP], &HOLE_JMP, &INC_EAX, &HOLE_JMP, &HOLE_JMP].concat();
        let relocs = [successor(14, 0), successor(21, TAKEN), successor(26, EXIT0)];
        let layout = reorder(&code, &relocs).expect("the pattern");
        let hot = [&JE[..], &NO_FIELD, &JNE, &NO_FIELD, &[NOP], &HOLE_JMP].concat();
        assert_eq!(layout.code, [&hot[..], &INC_EAX, &HOLE_JMP].concat());
        let cold_start = hot.len() as i32;
        assert_eq!(layout.relocs, [successor(2, EXIT0), (8, SUCCESSOR, LOCAL, cold_start), successor(14, 0), successor(21, TAKEN)]);
    }

    /// A row that returns, or has no continuation jump, keeps its built bytes.
    #[test]
    fn other_rows_are_left_alone() {
        assert!(reorder(&[0xc3], &[]).is_none());
        assert!(reorder(&[&[NOP][..], &HOLE_JMP].concat(), &[successor(2, EXIT0)]).is_none());
    }
}
