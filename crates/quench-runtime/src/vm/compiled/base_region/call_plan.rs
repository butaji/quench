//! Planning a `Call` for the b-jit: from the instruction and its site's call target record, one
//! chain of stencil parts (`quench-bjit/stencils/call_rows.rs` holds the rows and their register
//! and stack contract; `quench_bjit::call` names them).
//!
//! The chain, in layout order:
//!
//! 1. one `Spill` row per pinned register live at the call, writing it to its frame slot as a
//!    canonical tagged word (the callee and the helpers read the frame);
//! 2. when the target's entry cell is stable (`Choice::Direct`), the direct row for the call's
//!    argument count and receiver binding, whose record (`CallRecord`) the chain owns;
//! 3. the helper row (`lane_body_call`), the direct row's refusal path and the whole call
//!    otherwise;
//! 4. one `reload_*` row per pinned register live after the call (every pinned register is dead
//!    across a call row).
//!
//! Successors are chain-local: a part's `taken` is the index of the part it branches to, and
//! `parts.len()` means the part after the chain (the next instruction). A part's `exits`, by
//! exit kind, say which leave the linker patches in: `Leave::At(pc)` is a leave at `pc` that
//! does not spill (the spills already ran), `Leave::Status` is `quench_bjit::call::LEAVE_STATUS`
//! (the (status, word) in the first two pinned ints is the leave's result). The chain's record
//! lives in the chain: the code that uses the chain must not outlive it.
//!
//! Direct admission reads the cell's facts word once at plan time (`CellFacts`: its registers,
//! room, params and entry kind must describe the callee this site recorded) and the cell's
//! validity word becomes the row's expected word, so a republish or withdrawal that bumps the
//! validity sends the call to the helper row, which resolves the target anew.

use super::SLOT;
use super::super::direct_callee::DirectCallee;
use super::super::entry_cell::capture_word;
use super::super::{FRAME_BYTES, LaneHelper, LaneHelpers};
use crate::bytecode::{ImmediateLayout, Register, WideInstruction};
use crate::staged::call::{CellFacts, ENTRY_ACTIVATES, ENTRY_CELL};
use quench_bjit::call::{self, CallRow};
use quench_bjit::call_record::{CallRecord, MAX_DIRECT_ARGC};
use quench_bjit::numeric::{self, Kind, Place};
use quench_bjit::{HOLES, Stencil, padded_holes};

/// What a `Call` instruction names, decoded once: the record the helper takes (`call_pc`), the
/// callee and receiver registers, the first argument register, the argument count and the
/// register that receives the result.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct CallShape {
    pub(super) call_pc: u32,
    pub(super) callee: Register,
    pub(super) this: Register,
    pub(super) window: Register,
    pub(super) argc: u16,
    pub(super) result: Register,
}

impl CallShape {
    pub(super) fn of(call_pc: u32, instruction: WideInstruction) -> Self {
        let imm = instruction.imm();
        Self {
            call_pc,
            callee: instruction.register_b(),
            this: instruction.register_c(),
            window: ImmediateLayout::call_window_base(imm),
            argc: ImmediateLayout::argument_count(imm),
            result: instruction.result_register(),
        }
    }
}

/// What the call site's target record says of its callee, read once (`DirectFacts::of`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct DirectFacts {
    pub(super) callee: u64,
    pub(super) cell: usize,
    pub(super) validity: u64,
    pub(super) facts: CellFacts,
    pub(super) descriptor: usize,
    pub(super) captures: u64,
    /// The global object a sloppy callee binds an `undefined` or `null` receiver to.
    pub(super) global_this: Option<u64>,
    pub(super) registers: u16,
    /// A construction site's forwarding record: not a plain call.
    pub(super) construction: bool,
    pub(super) frame_bytes: u64,
}

impl DirectFacts {
    /// The facts of `callee`, whose cell's validity and facts words are read now.
    ///
    /// # Safety
    /// `callee.cell` is the address of the callee function's live entry cell.
    pub(super) unsafe fn of(callee: &DirectCallee) -> Self {
        // SAFETY: the caller's contract; the words are at the designed cell's offsets.
        let word = |offset: i32| unsafe { ((callee.cell + offset as usize) as *const u64).read() };
        Self {
            callee: callee.callee.to_bits(),
            cell: callee.cell,
            validity: word(ENTRY_CELL.validity_offset),
            facts: CellFacts::of_word(word(ENTRY_CELL.facts_offset)),
            descriptor: std::ptr::from_ref(callee).addr(),
            captures: capture_word(callee.capture_values, callee.capture_len),
            global_this: callee.sloppy.map(|global| global.to_bits()),
            registers: callee.registers,
            construction: callee.forwarder.is_some(),
            frame_bytes: FRAME_BYTES as u64,
        }
    }

    /// Whether the cell's facts describe this callee as a plain activation that takes `argc`
    /// arguments, so the stencil's admission (validity and room) is all that is left to check.
    fn describes(&self, argc: u16) -> bool {
        let facts = self.facts;
        facts.registers == self.registers && facts.room == self.registers && facts.params >= argc && u64::from(facts.entry) == ENTRY_ACTIVATES
    }
}

/// Why a call takes the helper row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Declined {
    /// The site recorded no stable target.
    NoTarget,
    /// More arguments than the direct rows pass (`MAX_DIRECT_ARGC`).
    Arguments,
    /// A construction, which the helper row's `lane_body_call` does not make either: the
    /// planner for `Construct` chooses.
    Construction,
    /// A callee with no registers has no window the room check can refuse.
    Window,
    /// The cell's facts do not describe the callee (not yet published, or another function's).
    Facts,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Choice {
    Direct,
    Helper(Declined),
}

/// The row class of a call: direct when its target's cell is stable and describes the callee,
/// else the helper row, with the first reason it cannot be direct.
pub(super) fn choose(shape: &CallShape, direct: Option<&DirectFacts>) -> Choice {
    let Some(facts) = direct else { return Choice::Helper(Declined::NoTarget) };
    let declined = [
        (usize::from(shape.argc) > MAX_DIRECT_ARGC, Declined::Arguments),
        (facts.construction, Declined::Construction),
        (facts.registers == 0, Declined::Window),
        (!facts.describes(shape.argc), Declined::Facts),
    ];
    declined.into_iter().find(|(declined, _)| *declined).map_or(Choice::Direct, |(_, why)| Choice::Helper(why))
}

/// The leave a part's exit hole is patched to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Leave {
    /// Leave at this pc; nothing is spilled (the chain's spills already ran).
    At(u32),
    /// `LEAVE_STATUS`: the (status, word) the part left with.
    Status,
}

/// One stencil of a chain with its hole values, chain-local `taken` successor and exits by kind.
pub(super) struct CallPart {
    pub(super) stencil: &'static Stencil,
    pub(super) holes: [u64; HOLES],
    pub(super) taken: Option<usize>,
    pub(super) exits: Vec<Leave>,
}

/// A planned call: its parts, the record the direct row reads, and the choice that made them.
pub(super) struct CallChain {
    pub(super) parts: Vec<CallPart>,
    pub(super) record: Option<Box<CallRecord>>,
    pub(super) choice: Choice,
}

/// Everything a call is planned from: the pinned registers live at the call (spilled before it)
/// and live after it (reloaded after it), each with the register whose frame slot it uses.
pub(super) struct CallPlan<'a> {
    pub(super) shape: CallShape,
    pub(super) direct: Option<&'a DirectFacts>,
    /// The address of `lane_body_call` (`helper_address`).
    pub(super) helper: usize,
    pub(super) spills: &'a [(Place, Register)],
    pub(super) reloads: &'a [(Place, Register)],
}

/// The address of `helper` in a process helper table; none for a linked or symbolic one.
pub(super) fn helper_address(helpers: LaneHelpers, helper: LaneHelper) -> Option<usize> {
    match helpers {
        LaneHelpers::Process(table) => Some(table[helper as usize].addr()),
        _ => None,
    }
}

fn slot(register: Register) -> u64 {
    u64::from(register) * SLOT
}

fn part(stencil: &'static Stencil, holes: [u64; HOLES], exits: Vec<Leave>) -> CallPart {
    CallPart { stencil, holes, taken: None, exits }
}

fn spill_part(place: Place, register: Register) -> Result<CallPart, String> {
    let row = numeric::Row { kind: Kind::Spill, dst: Place::Mem, a: place, b: Place::Mem, speculate: false };
    Ok(part(numeric::stencil(row)?, padded_holes([slot(register)]), Vec::new()))
}

fn reload_part(place: Place, register: Register) -> Result<CallPart, String> {
    Ok(part(call::stencil(CallRow::Reload(place))?, padded_holes([slot(register)]), Vec::new()))
}

fn helper_part(shape: &CallShape, helper: usize) -> Result<CallPart, String> {
    if helper == 0 {
        return Err("call: no helper address".to_string());
    }
    Ok(part(call::stencil(CallRow::Helper)?, padded_holes([u64::from(shape.call_pc), helper as u64]), vec![Leave::At(shape.call_pc), Leave::Status]))
}

/// The direct row and the record it reads, whose address is its hole 0.
fn direct_part(shape: &CallShape, facts: &DirectFacts) -> Result<(CallPart, Box<CallRecord>), String> {
    let record = Box::new(CallRecord {
        callee: facts.callee,
        cell: facts.cell as u64,
        expected: facts.validity,
        descriptor: facts.descriptor as u64,
        captures: facts.captures,
        global_this: facts.global_this.unwrap_or_default(),
        frame_bytes: facts.frame_bytes,
        callee_register: shape.callee,
        this_register: shape.this,
        args_register: shape.window,
        result_register: shape.result,
    });
    let argc = u8::try_from(shape.argc).map_err(|e| e.to_string())?;
    let stencil = call::stencil(CallRow::Direct { argc, sloppy: facts.global_this.is_some() })?;
    Ok((part(stencil, padded_holes([std::ptr::from_ref(&*record).addr() as u64]), vec![Leave::Status]), record))
}

/// Rows a direct row's `taken` skips: itself and the helper row after it.
const ROWS_PAST_DIRECT: usize = 2;

/// The chain of a call (see the module doc for its layout and successors).
pub(super) fn plan_call(plan: &CallPlan) -> Result<CallChain, String> {
    let mut parts = plan.spills.iter().map(|&(place, register)| spill_part(place, register)).collect::<Result<Vec<_>, _>>()?;
    let choice = choose(&plan.shape, plan.direct);
    let direct = match (choice, plan.direct) {
        (Choice::Direct, Some(facts)) => Some(direct_part(&plan.shape, facts)?),
        _ => None,
    };
    let mut record = None;
    if let Some((mut row, owned)) = direct {
        // Past the direct row and the helper row that follows it: the first reload, or the next instruction.
        row.taken = Some(parts.len() + ROWS_PAST_DIRECT);
        parts.push(row);
        record = Some(owned);
    }
    parts.push(helper_part(&plan.shape, plan.helper)?);
    for &(place, register) in plan.reloads {
        parts.push(reload_part(place, register)?);
    }
    Ok(CallChain { parts, record, choice })
}

#[cfg(test)]
mod tests {
    use super::*;
    use quench_bjit::call_record as protocol;

    const SHAPE: CallShape = CallShape { call_pc: 9, callee: 1, this: 2, window: 3, argc: 2, result: 6 };

    fn facts(registers: u16, params: u16) -> DirectFacts {
        let cell = CellFacts { registers, room: registers, params, entry: ENTRY_ACTIVATES as u8 };
        DirectFacts { callee: 7, cell: 0x1000, validity: 5, facts: cell, descriptor: 0x2000, captures: 0, global_this: None, registers, construction: false, frame_bytes: 32 }
    }

    #[test]
    fn a_stable_cell_that_describes_the_callee_is_called_directly() {
        assert_eq!(choose(&SHAPE, Some(&facts(4, 2))), Choice::Direct);
        assert_eq!(choose(&SHAPE, Some(&facts(4, 5))), Choice::Direct, "fewer arguments than params");
    }

    #[test]
    fn every_other_call_takes_the_helper_with_its_reason() {
        let helper = |why| Choice::Helper(why);
        assert_eq!(choose(&SHAPE, None), helper(Declined::NoTarget));
        let many = CallShape { argc: MAX_DIRECT_ARGC as u16 + 1, ..SHAPE };
        assert_eq!(choose(&many, Some(&facts(4, 9))), helper(Declined::Arguments));
        assert_eq!(choose(&SHAPE, Some(&DirectFacts { construction: true, ..facts(4, 2) })), helper(Declined::Construction));
        assert_eq!(choose(&SHAPE, Some(&facts(0, 2))), helper(Declined::Window));
        assert_eq!(choose(&SHAPE, Some(&facts(4, 1))), helper(Declined::Facts), "more arguments than params");
        let unpublished = DirectFacts { facts: CellFacts { room: 9, ..facts(4, 2).facts }, ..facts(4, 2) };
        assert_eq!(choose(&SHAPE, Some(&unpublished)), helper(Declined::Facts), "the cell describes another window");
        let other_kind = DirectFacts { facts: CellFacts { entry: 1, ..facts(4, 2).facts }, ..facts(4, 2) };
        assert_eq!(choose(&SHAPE, Some(&other_kind)), helper(Declined::Facts), "not a plain activation");
    }

    fn plan<'a>(direct: Option<&'a DirectFacts>, spills: &'a [(Place, Register)], reloads: &'a [(Place, Register)]) -> CallPlan<'a> {
        CallPlan { shape: SHAPE, direct, helper: 0x4000, spills, reloads }
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn a_direct_chain_is_spills_direct_helper_reloads_with_the_direct_row_skipping_the_helper() {
        let (spills, reloads) = ([(Place::Pin(0), 1), (Place::PinF(2), 5)], [(Place::Pin(0), 1)]);
        let direct = facts(4, 2);
        let chain = plan_call(&plan(Some(&direct), &spills, &reloads)).expect("a plan");
        assert_eq!(chain.choice, Choice::Direct);
        assert_eq!(chain.parts.len(), spills.len() + 2 + reloads.len());
        let (direct_at, helper_at) = (spills.len(), spills.len() + 1);
        assert_eq!(chain.parts[direct_at].taken, Some(helper_at + 1), "completion skips the helper row");
        assert_eq!(chain.parts[direct_at].exits, [Leave::Status]);
        assert_eq!(chain.parts[helper_at].exits, [Leave::At(SHAPE.call_pc), Leave::Status]);
        assert_eq!(chain.parts[helper_at].holes[..2], [u64::from(SHAPE.call_pc), 0x4000]);
        let record = chain.record.expect("the direct row's record");
        assert_eq!(chain.parts[direct_at].holes[0], std::ptr::from_ref(&*record).addr() as u64);
        assert_eq!((record.callee, record.cell, record.expected, record.result_register), (7, 0x1000, 5, 6));
        assert_eq!(chain.parts[spills.len() + 2].holes[0], SLOT);
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn a_helper_chain_has_no_record_and_its_helper_row_is_last_before_the_reloads() {
        let chain = plan_call(&plan(None, &[], &[(Place::PinF(0), 3)])).expect("a plan");
        assert_eq!((chain.choice, chain.record.is_none(), chain.parts.len()), (Choice::Helper(Declined::NoTarget), true, 2));
        assert!(chain.parts.iter().all(|part| part.taken.is_none()));
        let nowhere = CallPlan { helper: 0, ..plan(None, &[], &[]) };
        assert!(plan_call(&nowhere).is_err(), "a helper row needs its address");
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn a_sloppy_callee_takes_the_sloppy_row_with_its_global() {
        let sloppy = DirectFacts { global_this: Some(0xabc), ..facts(4, 2) };
        let chain = plan_call(&plan(Some(&sloppy), &[], &[])).expect("a plan");
        assert_eq!(chain.record.expect("a record").global_this, 0xabc);
        let name = CallRow::Direct { argc: 2, sloppy: true }.name().expect("a row");
        assert!(std::ptr::eq(chain.parts[0].stencil, call::stencil(CallRow::Direct { argc: 2, sloppy: true }).expect(&name)));
    }

    /// The call protocol the stencils are compiled with (`quench_bjit::call_record`,
    /// `stencils/layout.rs`) is the runtime's own.
    #[test]
    fn call_protocol_matches_the_runtime() {
        use super::super::super as compiled;
        use compiled::entry_cell::{EntryCell, LANE_BODY_NOT_ENTERED};
        #[allow(dead_code)]
        mod stencil_layout {
            include!("../../../../../quench-bjit/stencils/layout.rs");
        }
        use stencil_layout as mirror;
        assert_eq!(
            [compiled::LANE_CALL_COMPLETED, compiled::LANE_CALL_MISSED, compiled::LANE_CALL_LEFT, compiled::LANE_CALL_TERMINATED, compiled::LaneExit::Callee.word()],
            [protocol::LANE_CALL_COMPLETED, protocol::LANE_CALL_MISSED, protocol::LANE_CALL_LEFT, protocol::LANE_CALL_TERMINATED, protocol::CALLEE_EXIT]
        );
        assert_eq!(
            [compiled::RETURN_STATUS_BASE, compiled::LANE_BODY_RETURNED, LANE_BODY_NOT_ENTERED, compiled::NATIVE_JS_STOPPED, compiled::TERMINATED_STATUS as u32],
            [protocol::GENERAL_RETURN_STATUS_BASE, protocol::LANE_BODY_RETURNED, protocol::LANE_BODY_NOT_ENTERED, protocol::NATIVE_JS_STOPPED, protocol::TERMINATED_STATUS]
        );
        assert_eq!(protocol::REGISTER_BYTES, std::mem::size_of::<crate::value::Value>());
        let dispatch = |offsets: [usize; 7]| offsets;
        assert_eq!(
            dispatch([
                std::mem::offset_of!(compiled::JsStepDispatch, vm), std::mem::offset_of!(compiled::JsStepDispatch, residual), std::mem::offset_of!(compiled::JsStepDispatch, lane_context),
                std::mem::offset_of!(compiled::JsStepDispatch, native_top), std::mem::offset_of!(compiled::JsStepDispatch, native_end),
                std::mem::offset_of!(compiled::JsStepDispatch, shadow_top), std::mem::offset_of!(compiled::JsStepDispatch, shadow_end),
            ]),
            [mirror::JS_STEP_VM, mirror::JS_STEP_RESIDUAL, mirror::JS_STEP_LANE_CONTEXT, mirror::JS_STEP_NATIVE_TOP, mirror::JS_STEP_NATIVE_END, mirror::JS_STEP_SHADOW_TOP, mirror::JS_STEP_SHADOW_END]
        );
        use crate::vm::dispatch_fast::LaneRecoverySpace;
        assert_eq!(
            [std::mem::offset_of!(EntryCell, entry), std::mem::offset_of!(EntryCell, recovery), std::mem::offset_of!(EntryCell, body), std::mem::offset_of!(LaneRecoverySpace, values), std::mem::offset_of!(LaneRecoverySpace, activations)],
            [mirror::CELL_ENTRY_AT, mirror::CELL_RECOVERY_AT, mirror::CELL_BODY_AT, mirror::RECOVERY_VALUES_AT, mirror::RECOVERY_ACTIVATIONS_AT]
        );
    }
}
