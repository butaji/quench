//! Planning a `Call` for the b-jit: from the instruction and its site's call target record, one
//! chain of stencil parts (`quench-bjit/stencils/call_rows.rs` holds the rows and their register
//! and stack contract; `quench_bjit::call` names them).
//!
//! A call whose callee is small and monomorphic is not a chain at all: the stitch splices in the
//! callee's rows (`inline::InlineSite`), so no part is planned and the pinned registers stay where
//! they are. A call that returns its result (`return f(x)`) is followed by the row that completes
//! the activation with it.
//!
//! The chain, in layout order:
//!
//! 1. one `Spill` row per pinned register live at the call, writing it to its frame slot as a
//!    canonical tagged word (the callee and the helpers read the frame); a leading argument's spill
//!    runs here too although the direct row reads the pin itself (`CallRecord::argument_places`): the
//!    rows' refusal passes every pinned register dead, so a spill behind them would write junk;
//! 2. when the target's entry cell is stable (`Choice::Direct`), the direct row for the call's
//!    argument count and receiver binding, whose record (`CallRecord`) the chain owns;
//! 3. the helper row (`body_call_helper`), the direct row's refusal path and the whole call
//!    otherwise;
//! 4. one `reload_*` row per pinned register live after the call (every pinned register is dead
//!    across a call row).
//!
//! `plan_step` is the same chain for any instruction the helper runs in place (`steps`): spills,
//! the helper row, reloads. The calls of the other forms (`CallForm`) are this chain with their own
//! helper row and no direct row; a returning call has no reloads, the completion following it.
//!
//! Successors are chain-local: a part's `taken` is the index of the part it branches to, and
//! `parts.len()` means the part after the chain (the next instruction). A part's `exits`, by
//! exit kind, say which leave the linker patches in (`Exit`: `At(pc)` leaves at `pc` without
//! spilling, the spills already ran; `Status` is `quench_bjit::call::LEAVE_STATUS`, the (status,
//! word) in the first two pinned ints being the leave's result). The chain's record
//! lives in the chain: the code that uses the chain must not outlive it.
//!
//! Direct admission reads the cell's facts word once at plan time (`CellFacts`: its registers,
//! room, params and entry kind must describe the callee this site recorded) and the cell's
//! validity word becomes the row's expected word, so a republish or withdrawal that bumps the
//! validity sends the call to the helper row, which resolves the target anew.

#[allow(unused_imports)] use quench_runtime::staged::{InvocationFormsExt};
#[allow(unused_imports)]
use crate::ext_traits::*;
pub(crate) mod inline;
pub(crate) mod chain;
pub(crate) mod builtin;
pub(crate) mod planned;
mod shape;
mod facts;
mod direct;
pub(crate) use shape::{method_shape, CallForm, CallShape};
pub(crate) use facts::{Choice, DirectFacts};
use facts::choose;
use direct::{argument_places, decline_builtin, direct_part};
use builtin::BuiltinCall;
pub(crate) use chain::ChainRead;
use chain::chained_reads;
use super::{Exit, NO_SYMBOLS, SLOT, Symbolic};
use super::super::direct_callee::DirectCallee;
use super::super::{LaneHelper, LaneHelpers};
use quench_runtime::bytecode::{ImmediateLayout, Register, WideInstruction};
use quench_runtime::staged::call::{CellFacts, ENTRY_ACTIVATES, ENTRY_CELL};
use quench_runtime::staged::frame::{ARGC_SHIFT, NATIVE_FLAG, PLAIN_FLAG};
use quench_bjit::call::{self, CallRow, ThisRow};
use quench_bjit::call_record::{CalleeRecord, CallRecord, DirectHole, HelperHole, NumericLeafHole, MAX_DIRECT_ARGC, PLACED_ARGUMENTS};
use quench_bjit::numeric::{self, Kind, Place};
use quench_bjit::{HOLES, Stencil, padded_holes};
use crate::aot_image::Symbol;

/// One stencil of a chain with its hole values, chain-local `taken` successor and exits by kind.
pub(crate) struct CallPart {
    pub(crate) stencil: &'static Stencil,
    pub(crate) holes: [u64; HOLES],
    pub(crate) taken: Option<usize>,
    pub(crate) exits: Vec<Exit>,
    /// The holes whose run-time addresses are symbols the loader resolves, each with its symbol
    /// (`HelperHole::Symbol`, the generic row's entry and operand record).
    pub(crate) symbols: Symbolic,
}

pub(crate) use quench_runtime::vm::engine_types::OwnedCallRecord;

/// A planned call: its parts, the records its direct rows read (one per callee the site reached), and
/// the choice that made them.
pub(crate) struct CallChain {
    pub(crate) parts: Vec<CallPart>,
    pub(crate) records: Vec<OwnedCallRecord>,
    pub(crate) choice: Choice,
}

/// Everything a call is planned from: the pinned registers live at the call (spilled before it)
/// and live after it (reloaded after it), each with the register whose frame slot it uses.
pub(crate) struct CallPlan<'a> {
    pub(crate) shape: CallShape,
    /// Each callee the site reached, latest first (`Layout::callees`).
    pub(crate) direct: &'a [DirectFacts],
    /// The address of `body_call_helper` (`helper_address`).
    pub(crate) helper: HelperPlace,
    pub(crate) spills: &'a [(Place, Register)],
    pub(crate) reloads: &'a [(Place, Register)],
    /// The arguments the caller proves Numbers (bit `k`: argument `k`, `call_args::proven_arguments`), which the direct row
    /// passes on to the callee's proven-arguments entry where its guards are covered (`CallRecord::proven_args`).
    pub(crate) proven_args: u64,
    /// The site read a `Method` call's direct rows follow (`plan::method_read`): its stencil and holes. Planned only
    /// where a direct row is, after the spills; its miss goes to the helper row behind the direct rows.
    pub(crate) read: Option<(&'static Stencil, [u64; HOLES])>,
    /// The prototype-held entries of a method site (`plan::chain_reads`), each a read of the method at a receiver shape
    /// with the value its holder held when planned. Where one pairs with a callee the site reached, the chain is its reads
    /// and direct rows (`chained_reads`), and `read` is not planned.
    pub(crate) chain: Vec<ChainRead>,
    /// The builtin the callee is, where the site's native callee is one the b-jit runs on a row (`builtin::of`).
    pub(crate) builtin: Option<BuiltinCall>,
}

/// Where a helper row's address goes: the address itself in a process helper table, or the symbol
/// an image's loader resolves to it (`LaneHelpers::Ahead`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HelperPlace {
    Address(usize),
    Symbol(LaneHelper),
}

/// The helper's place for `helper` under `helpers`: none for a linked or a frameless one.
pub(crate) fn helper_address(helpers: LaneHelpers, helper: LaneHelper) -> Option<HelperPlace> {
    match helpers {
        LaneHelpers::Process(table) => Some(HelperPlace::Address(table[helper as usize].addr())),
        LaneHelpers::Ahead => Some(HelperPlace::Symbol(helper)),
        _ => None,
    }
}

fn slot(register: Register) -> u64 {
    u64::from(register) * SLOT
}

fn part(stencil: &'static Stencil, holes: [u64; HOLES], exits: Vec<Exit>) -> CallPart {
    CallPart { stencil, holes, taken: None, exits, symbols: NO_SYMBOLS }
}

pub(crate) fn spill_part(place: Place, register: Register) -> Result<CallPart, String> {
    let row = numeric::Row { kind: Kind::Spill, dst: Place::Mem, a: place, b: Place::Mem, speculate: false };
    Ok(part(numeric::stencil(row)?, padded_holes([slot(register)]), Vec::new()))
}

pub(crate) fn reload_part(place: Place, register: Register) -> Result<CallPart, String> {
    Ok(part(call::stencil(CallRow::Reload(place))?, padded_holes([slot(register)]), Vec::new()))
}

fn helper_part(call_pc: u32, helper: HelperPlace, packed: Option<[u64; 2]>) -> Result<CallPart, String> {
    let (address, symbols) = match helper {
        HelperPlace::Address(0) => return Err("call: no helper address".to_string()),
        HelperPlace::Address(address) => (address as u64, NO_SYMBOLS),
        HelperPlace::Symbol(helper) => (0, [Some((HelperHole::Helper as u8, Symbol::Helper(helper as u32))), None]),
    };
    let exits = vec![Exit::At(call_pc), Exit::Status];
    let mut holes = [0; HOLES];
    holes[HelperHole::CallPc as usize] = u64::from(call_pc);
    holes[HelperHole::Helper as usize] = address;
    let row = match packed {
        None => part(call::stencil(CallRow::Helper)?, holes, exits),
        Some([first, imm]) => {
            holes[HelperHole::PackedFirst as usize] = first;
            holes[HelperHole::PackedImm as usize] = imm;
            part(call::stencil(CallRow::HelperPacked)?, holes, exits)
        }
    };
    Ok(CallPart { symbols, ..row })
}

/// The chain of a helper step at `pc` (any instruction whose operation the helper runs in place):
/// the spills, the helper row, the reloads.
pub(crate) fn plan_step(pc: u32, helper: HelperPlace, packed: Option<[u64; 2]>, spills: &[(Place, Register)], reloads: &[(Place, Register)]) -> Result<Vec<CallPart>, String> {
    chain(spills, helper_part(pc, helper, packed)?, reloads)
}

/// The float leaf row of a numeric operation at `pc`: the leaf's address in its hole (or its symbol), the operands' places
/// (a frame slot, or a constant in its hole), and the holes: the operands' offsets or constants and the destination's
/// offset (`NumericLeafHole`, `steps::leaf_operands`). A non-Number operand leaves at the pc: nothing is written.
fn leaf_part(pc: u32, leaf: HelperPlace, operands: [Place; 2], [left, right, dst]: [u64; 3]) -> Result<CallPart, String> {
    let (address, symbols) = match leaf {
        HelperPlace::Address(0) => return Err("call: no leaf address".to_string()),
        HelperPlace::Address(address) => (address as u64, NO_SYMBOLS),
        HelperPlace::Symbol(helper) => (0, [Some((NumericLeafHole::Leaf as u8, Symbol::Helper(helper as u32))), None]),
    };
    let mut holes = [0; HOLES];
    holes[NumericLeafHole::Destination as usize] = dst;
    holes[NumericLeafHole::Left as usize] = left;
    holes[NumericLeafHole::Right as usize] = right;
    holes[NumericLeafHole::Leaf as usize] = address;
    Ok(CallPart { symbols, ..part(call::stencil(CallRow::NumericLeaf { left: operands[0], right: operands[1] })?, holes, vec![Exit::At(pc)]) })
}

/// The chain `middle` leads: the spills of the pinned registers live at it, `middle`, the reloads of those live after it.
fn chain(spills: &[(Place, Register)], middle: CallPart, reloads: &[(Place, Register)]) -> Result<Vec<CallPart>, String> {
    let mut parts = spills.iter().map(|&(place, register)| spill_part(place, register)).collect::<Result<Vec<_>, _>>()?;
    parts.push(middle);
    for &(place, register) in reloads {
        parts.push(reload_part(place, register)?);
    }
    Ok(parts)
}

/// The chain of a float leaf call at `pc` (`Math.pow`, `CallRow::NumericLeaf`): the spills, the leaf row, the reloads, as
/// `plan_step` for a helper step. `operands` and `holes` are `steps::leaf_operands`'s.
pub(crate) fn plan_leaf(pc: u32, leaf: HelperPlace, operands: [Place; 2], holes: [u64; 3], spills: &[(Place, Register)], reloads: &[(Place, Register)]) -> Result<Vec<CallPart>, String> {
    chain(spills, leaf_part(pc, leaf, operands, holes)?, reloads)
}

/// The chain of a call (see the module doc for its layout and successors). A polymorphic site's chain
/// holds one direct row per callee it reached, latest first: each refuses another callee to the next
/// (NEXT), the last to the helper row, so a site alternating between known callees calls each by its
/// entry, as the interpreter's own link does, and only an unknown one takes the helper.
pub(crate) fn plan_call(plan: &CallPlan) -> Result<CallChain, String> {
    if let Some(chain) = chained_reads(plan)? {
        return Ok(chain);
    }
    let choices: Vec<(Choice, &DirectFacts)> = plan.direct.iter().map(|facts| (choose(&plan.shape, Some(facts)), facts)).collect();
    if quench_runtime::trace::Channel::AotLanes.on() && !plan.direct.is_empty() && !choices.iter().any(|(choice, _)| *choice == Choice::Direct) {
        eprintln!("aot-lanes: call at {} declines its direct row: {:?} (form {:?}, method {})", plan.shape.call_pc, choices.first().map(|(choice, _)| *choice), plan.shape.form, plan.shape.method.is_some());
    }
    let choice = choices.iter().map(|(choice, _)| *choice).find(|choice| *choice == Choice::Direct)
        .unwrap_or_else(|| choices.first().map_or_else(|| choose(&plan.shape, None), |(choice, _)| *choice));
    let arguments = argument_places(plan);
    // The closed plan's last arm is unchecked only where every arm is a direct row: a declined arm reaches the helper row
    // through the chain, and the unchecked row would take its callee.
    let every_arm_direct = choices.iter().all(|(choice, _)| *choice == Choice::Direct);
    let mut directs = Vec::new();
    for (_, facts) in choices.into_iter().filter(|(choice, _)| *choice == Choice::Direct) {
        directs.push(direct_part(&plan.shape, facts, arguments, plan.proven_args, facts.closed && every_arm_direct)?);
    }
    // A builtin callee where no direct row stands declines the region (`decline_builtin`).
    if directs.is_empty() {
        decline_builtin(&plan.shape, plan.builtin)?;
    }
    // Every spill comes before the direct rows. A spill behind them (a pinned argument only the refusal path
    // reads) wrote what the refused row passed on: every pinned register is dead past a direct row, whose NEXT
    // passes junk (`words::dead`), so the helper took a stale register word as the argument (JetStream ML's
    // "Row index out of range": `check(j)` in a nested loop got a pointer's low bits whenever its callee's cell
    // held the stub). The direct row still reads the argument from its pin (`argument_places`).
    let mut parts = Vec::new();
    let read = plan.read.filter(|_| !directs.is_empty());
    for &(place, register) in plan.spills {
        parts.push(spill_part(place, register)?);
    }
    if let Some((stencil, holes)) = read {
        // A miss takes the helper row behind the direct rows, as a refused direct call does: the spills ran, so the frame is canonical.
        let helper = 1 + directs.len();
        parts.push(CallPart { stencil, holes, taken: None, exits: vec![Exit::Ahead(helper)], symbols: NO_SYMBOLS });
    }
    // Past every direct row and the helper row that follows them: the first reload, or the next instruction.
    let past = parts.len() + directs.len() + 1;
    let mut records = Vec::new();
    for (mut row, owned) in directs {
        row.taken = Some(past);
        parts.push(row);
        records.push(owned);
    }
    parts.push(helper_part(plan.shape.call_pc, plan.helper, None)?);
    for &(place, register) in plan.reloads {
        parts.push(reload_part(place, register)?);
    }
    Ok(CallChain { parts, records, choice })
}


#[cfg(test)]
pub(crate) mod tests;
