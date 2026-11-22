//! The indirect calls of the b-jit's Wasm functions: a `Call::IndirectTarget` and the
//! `Call::Reference` after it, stitched as one `call_indirect` row (`quench-bjit/stencils/
//! wasm_rows_indirect.rs`) that does the target's checks and the call. The target places only a
//! move of the element index into its register, which the call reads.
//!
//! The fusion is safe only when the two are adjacent (the target at `pc - 1`, its register read by
//! the call at `pc`) and no branch lands on the call: otherwise the checks would run later than the
//! target's, after instructions that may trap or write. Both conditions return `None`, so the
//! function has no rows and keeps its other tier.
//!
//! A child of `wasm_stitch` (`#[path]`), so it names `wasm_stitch`'s private helpers
//! through `super`. The site's data comes from the artifact's analysis (`site`).

#[allow(unused_imports)]
use crate::ext_traits::*;
use quench_runtime::vm::{CONTEXT_CALL_REFERENCE, CONTEXT_INSTANCE, CONTEXT_MEMORY_LENGTH, CONTEXT_TABLE_WORDS, Plan, Slot};
use super::{IndirectSite, Planned, World, leak, row, slot};
use quench_runtime::bytecode::WideInstruction;
use quench_runtime::heap::{TableTarget, TableWords};
use quench_runtime::staged::wasm_call::Call;
use quench_runtime::staged::wasm_step::Step;
use quench_bjit::padded_holes;
use quench_bjit::wasm as codes;

// The row's stencil reads these layouts from `quench_bjit::wasm` (`wasm_rows_indirect.rs`), so each
// is asserted against the runtime's own.
const _: () = assert!(TableTarget::NO_ENTRY == codes::NO_ENTRY);
const _: () = assert!(size_of::<TableTarget>() == codes::TARGET_BYTES);
const _: () = assert!(TableTarget::SIGNATURE as usize == codes::TARGET_SIGNATURE_AT);
const _: () = assert!(TableTarget::ENTRY as usize == codes::TARGET_ENTRY_AT);
const _: () = assert!(TableTarget::INSTANCE as usize == codes::TARGET_INSTANCE_AT);
const _: () = assert!(TableTarget::BODY as usize == codes::TARGET_BODY_AT);
const _: () = assert!(TableWords::BASE as usize == codes::TABLE_WORDS_BASE_AT);
const _: () = assert!(TableWords::LENGTH as usize == codes::TABLE_WORDS_LENGTH_AT);
// `CallContext`'s offsets, which the row does not take in holes.
const _: () = assert!(CONTEXT_TABLE_WORDS as usize == codes::CONTEXT_TABLE_WORDS_AT);
const _: () = assert!(CONTEXT_CALL_REFERENCE as usize == codes::CONTEXT_CALL_REFERENCE_AT);
const _: () = assert!(CONTEXT_INSTANCE as usize == codes::CONTEXT_INSTANCE_AT);
const _: () = assert!(CONTEXT_MEMORY_LENGTH as usize == codes::CONTEXT_MEMORY_LENGTH_AT);

/// Whether `instruction` is a `Call::Reference`.
fn is_reference(instruction: WideInstruction) -> bool {
    matches!(Step::of(instruction.op(), instruction.imm()), Some(Step::Call(Call::Reference)))
}

/// Whether `instruction` loads a constant: the lowering places the call's signature constant
/// between the target and the call.
fn is_constant(instruction: WideInstruction) -> bool {
    quench_runtime::staged::wasm_instance::Instance::of(instruction.op()) == Some(quench_runtime::staged::wasm_instance::Instance::Constant)
}

/// The `Call::Reference` the target at `pc` feeds: the first instruction after it that loads no
/// constant, reading the target's register as `b`, with no constant in between overwriting that
/// register and nothing branching into the span (so the call's row runs the target's checks
/// exactly when the interpreter would).
fn call_of(pc: usize, instructions: &[WideInstruction]) -> Option<usize> {
    let target = instructions[pc];
    let call_pc = (pc + 1..instructions.len()).find(|&at| !is_constant(instructions[at]))?;
    let call = instructions[call_pc];
    let clobbered = instructions[pc + 1..call_pc].iter().any(|constant| constant.a() == target.a());
    if !is_reference(call) || call.b() != target.a() || clobbered {
        return None;
    }
    // Conservative: any instruction whose target lies in the span (a branch, or a branch
    // table's entry) refuses. A non-branch instruction whose field reads so refuses too.
    let span = pc + 1..=call_pc;
    if instructions.iter().any(|other| span.contains(&(other.jump_target() as usize))) {
        return None;
    }
    Some(call_pc)
}

/// Whether `instruction` is a `Call::IndirectTarget`.
fn is_target(instruction: WideInstruction) -> bool {
    matches!(Step::of(instruction.op(), instruction.imm()), Some(Step::Call(Call::IndirectTarget)))
}

/// The rows of the indirect target at `pc` (`Call::IndirectTarget`): a move of its element index
/// (register `c`) into its register `a`, which the call after it (`call_of`) reads as `b`. The checks are
/// the call's row's, so the target places nothing else.
pub(crate) fn target(pc: usize, instructions: &[WideInstruction]) -> Option<Vec<Planned>> {
    let instruction = instructions[pc];
    call_of(pc, instructions)?;
    Some(vec![row((codes::MOVE, 0, codes::W64, codes::REGISTER), [slot(instruction.a()), slot(instruction.c()), 0], None)])
}

/// The row of the indirect call at `pc` (`Call::Reference`): the target before it, past
/// constants, must feed this call (`call_of`).
pub(crate) fn reference(pc: usize, instructions: &[WideInstruction], world: &World<'_>) -> Option<Vec<Planned>> {
    let instruction = instructions[pc];
    // The target this call reads: the nearest target before it, past constants only.
    let target_pc = (0..pc).rev().find(|&at| !is_constant(instructions[at]))?;
    (is_target(instructions[target_pc]) && call_of(target_pc, instructions) == Some(pc)).then_some(())?;
    let site = (world.indirect)(pc)?;
    let destination = u32::try_from(slot(instruction.result_register())).ok()?;
    let window = u32::try_from(slot(instruction.call_window().base)).ok()?;
    let index = u32::try_from(slot(instruction.b())).ok()?;
    // The packing `wasm_rows_indirect.rs` reads (its header).
    let holes = [
        pack(destination, window),
        pack(index, site.table),
        pack(site.canonical, site.signature),
        u64::from(site.site),
        0,
    ];
    let name = leak(codes::wasm_name((codes::CALL_INDIRECT, 0, codes::W64, codes::REGISTER)));
    Some(vec![Planned { stencil: name, holes: padded_holes(holes), taken: None }])
}

/// Two 32-bit values in one hole: `low` in bits 0..32, `high` in 32..64.
fn pack(low: u32, high: u32) -> u64 {
    u64::from(low) | u64::from(high) << 32
}

/// The indirect site of the `Call::Reference` at `pc`, from the artifact's own analysis: the
/// binding and type the function's full plan gives the reference (`Slot::IndirectReference`), the
/// site id the artifact numbered for it (`reference_site_ids`, sorted by pc), and the canonical word
/// of the program's type (`canonical`). `None` where any of them is missing.
pub(crate) fn site(
    plan: &Plan,
    sites: &[(usize, usize)],
    canonical: &dyn Fn(u32) -> Option<u64>,
    pc: usize,
) -> Option<IndirectSite> {
    let Slot::IndirectReference { signature, table } = plan.operand(pc) else {
        return None;
    };
    let at = sites.binary_search_by_key(&pc, |(at, _)| *at).ok()?;
    Some(IndirectSite {
        table,
        signature,
        canonical: u32::try_from(canonical(signature)?).ok()?,
        site: u32::try_from(sites[at].1).ok()?,
    })
}
