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

use super::super::{CONTEXT_CALL_REFERENCE, CONTEXT_INSTANCE, CONTEXT_MEMORY_LENGTH, CONTEXT_TABLE_WORDS, Plan, Slot};
use super::{IndirectSite, Planned, World, leak, row, slot};
use crate::bytecode::WideInstruction;
use crate::heap::{TableTarget, TableWords};
use crate::staged::wasm_call::Call;
use crate::staged::wasm_step::Step;
use quench_bjit::padded_holes;
use quench_bjit::wasm as codes;

// The row's stencil hard-codes these runtime layouts (`wasm_rows_indirect.rs`).
const _: () = assert!(TableTarget::NO_ENTRY == u32::MAX);
const _: () = assert!(size_of::<TableTarget>() == 24);
const _: () = assert!(TableTarget::SIGNATURE == 0 && TableTarget::ENTRY == 4 && TableTarget::INSTANCE == 8);
const _: () = assert!(TableTarget::BODY == 16);
const _: () = assert!(TableWords::BASE == 0 && TableWords::LENGTH == 8);

/// Whether `instruction` is a `Call::Reference`.
fn is_reference(instruction: WideInstruction) -> bool {
    matches!(Step::of(instruction.op(), instruction.imm()), Some(Step::Call(Call::Reference)))
}

/// Whether `instruction` is a `Call::IndirectTarget`.
fn is_target(instruction: WideInstruction) -> bool {
    matches!(Step::of(instruction.op(), instruction.imm()), Some(Step::Call(Call::IndirectTarget)))
}

/// The rows of the indirect target at `pc` (`Call::IndirectTarget`): a move of its element index
/// (register `c`) into its register `a`, which the call at `pc + 1` reads as `b`. The checks are
/// the call's row's, so the target places nothing else.
pub(super) fn target(pc: usize, instructions: &[WideInstruction]) -> Option<Vec<Planned>> {
    let instruction = instructions[pc];
    let call = *instructions.get(pc + 1)?;
    if !is_reference(call) || call.b() != instruction.a() {
        return None;
    }
    Some(vec![row((codes::MOVE, 0, codes::W64, codes::REGISTER), [slot(instruction.a()), slot(instruction.c()), 0], None)])
}

/// The row of the indirect call at `pc` (`Call::Reference`): the target before it at `pc - 1`
/// must read into this call's register `b`, and nothing may branch to `pc`.
pub(super) fn reference(pc: usize, instructions: &[WideInstruction], world: &World<'_>) -> Option<Vec<Planned>> {
    let instruction = instructions[pc];
    let target = *instructions.get(pc.checked_sub(1)?)?;
    if !is_target(target) || target.a() != instruction.b() {
        return None;
    }
    // Conservative: any instruction whose target is `pc` (a branch, or a branch table's entry)
    // refuses. A non-branch instruction whose field reads as `pc` refuses too, which is safe.
    if instructions.iter().any(|other| other.jump_target() as usize == pc) {
        return None;
    }
    let site = (world.indirect)(pc)?;
    let destination = u32::try_from(slot(instruction.result_register())).ok()?;
    let window = u32::try_from(slot(instruction.call_window().base)).ok()?;
    let index = u32::try_from(slot(instruction.b())).ok()?;
    let context = |offset: i32| u16::try_from(offset).ok().map(u64::from);
    let (words, callout) = (context(CONTEXT_TABLE_WORDS)?, context(CONTEXT_CALL_REFERENCE)?);
    let (instance, length) = (context(CONTEXT_INSTANCE)?, context(CONTEXT_MEMORY_LENGTH)?);
    // The packing `wasm_rows_indirect.rs` reads (its header).
    let holes = [
        pack(destination, window),
        pack(index, site.table),
        pack(site.canonical, site.signature),
        u64::from(site.site) | words << 32 | callout << 48,
        instance << 16 | length << 32,
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
pub(in crate::vm::compiled) fn site(
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
