//! The lowering of an instruction no stencil row takes: every instruction the interpreter
//! handles has one, so a region is never refused for the instruction it holds.
//!
//! 1. A row (`decode`): the numeric and heap rows, instantiated from the staged operations.
//! 2. A helper step: the interpreter's own operation made in place by a lane helper
//!    (`LaneHelper::Step`: `(vm, residual, lane_context, call_pc) -> LANE_CALL_*`, the row
//!    `quench_bjit::call::CallRow::Helper`) between the spills of the live pinned registers and
//!    their reloads (`call_plan::plan_step`), for the instructions whose general step only
//!    continues or throws (`Traits::LANE_IN_PLACE`) and writes at most one register. The helper
//!    answers completed or missed, so both of the row's exits are the leave at the instruction:
//!    a miss, where the helper made nothing or raised, leaves there for the interpreter.
//! 3. A leave (`CallRow::Here`): the region ends at the instruction and the interpreter runs it,
//!    for every other instruction: jumps and branches the rows do not take, throws,
//!    suspensions, scope entries, and any instruction that carries result flags, which a helper
//!    cannot give back.
//!
//! A whole function's body (`function`) also has the two steps that are the body's own: its
//! `Return` completes the activation, and its `Call` is the call planner's chain
//! (`call_plan::plan_call`): the direct entry-cell row where the site recorded a stable callee,
//! the helper row otherwise, between the spills and reloads. A loop region makes no activation,
//! so its returns (and its calls that return a result) are leaves; its plain calls are the chain
//! with the helper row alone (`calls_in_place`, `LaneHelper::RegionCall`), or the callee's rows
//! where the stitch inlines it (`function::parts`): a call that completes
//! continues the region, one that missed leaves at the call, and one that left the lane in its
//! callee's activation, or ended in termination, ends the run with the word that says so
//! (`LaneExit::Callee`, `LaneExit::Terminated`), the registers already spilled.
//!
//! Which class an instruction is in follows from the opcode catalog's traits and layouts
//! (`lowering`), never from a table of opcodes here.

use super::*;
use super::call_plan::{self, CallChain, CallPart, CallPlan, CallShape, Choice};
use super::callees::chains;
use super::super::{LaneHelper, LaneHelpers};
use crate::bytecode::Traits;
use quench_bjit::HOLES;
use quench_bjit::call::{self, CallRow};

/// How the b-jit runs an instruction no row takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Step {
    /// `LaneHelper::Step` makes the record in place and the region continues.
    Helper,
    /// The region leaves at the record.
    Leave,
    /// The activation completes with the register the row reads (a body's `Return`).
    Return,
    /// A call, made by the call planner's chain (a body's `Call`, a loop region's plain call).
    Call(CallShape),
}

impl Step {
    /// Whether control continues at the next instruction: a leave and a return end the run.
    pub(super) fn falls(self) -> bool {
        !matches!(self, Self::Leave | Self::Return)
    }
}

/// Whether `instruction` is made in place by `LaneHelper::Step`.
fn steps_in_place(instruction: WideInstruction) -> bool {
    instruction.op().traits().has(Traits::LANE_IN_PLACE) && instruction.result_flags() == 0 && instruction.written_registers().count() <= 1
}

/// Whether a loop region makes `instruction` by its call row (`Step::Call`): a call a body's chain makes
/// (`callees::chains`) that completes no activation, where the helper table has the region call
/// helper's address. Any other call leaves, as the rest of the instructions no row takes.
pub(super) fn calls_in_place(instruction: WideInstruction, helpers: LaneHelpers) -> bool {
    chains(instruction) && instruction.result_flags() == 0 && call_plan::helper_address(helpers, LaneHelper::RegionCall).is_some()
}

/// The lowering of `instruction`, where helpers are `helpers`: a helper step where the helper
/// table has the step helper's address, else the leave.
pub(super) fn lowering(instruction: WideInstruction, helpers: LaneHelpers) -> Step {
    let addressed = call_plan::helper_address(helpers, LaneHelper::Step).is_some();
    if addressed && steps_in_place(instruction) { Step::Helper } else { Step::Leave }
}

/// The row of an instruction at `pc` that has no numeric or heap row: a `Nop` is the jump to the
/// next instruction, the rest their step. A helper step names the register it writes.
pub(super) fn decode_step(pc: u32, instruction: WideInstruction, helpers: LaneHelpers) -> Decoded {
    match instruction.op().class() {
        Class::Nop => Decoded::number(Kind::Jump, None, None, None, Some(pc + 1)),
        Class::Call if calls_in_place(instruction, helpers) => Decoded::step(Step::Call(CallShape::of(pc, instruction)), Some(instruction.result_register())),
        _ => {
            let step = lowering(instruction, helpers);
            let written = if step == Step::Helper { instruction.written_registers().next() } else { None };
            Decoded::step(step, written)
        }
    }
}

/// A helper step may write any heap value, which the snapshots a heap row reads (the entry
/// record's captures, fields and array views) would not see: where the region has a heap row its
/// helper steps leave instead.
pub(super) fn settle(rows: &mut [&mut Decoded]) {
    if rows.iter().any(|row| row.heap.is_some_and(Heap::reads_snapshot)) {
        rows.iter_mut().filter(|row| row.step == Some(Step::Helper)).for_each(|row| row.step = Some(Step::Leave));
    }
}

/// The stencil of a control row: it reads and writes no operand.
pub(super) fn control(kind: Kind) -> Result<&'static Stencil, String> {
    numeric::stencil(numeric::Row { kind, dst: Place::Mem, a: Place::Mem, b: Place::Mem, speculate: true })
}

impl Planned {
    /// The row of a chain part, its exits and no rows of its own; a part's successor is a part of
    /// the chain it belongs to.
    fn of_part(part: CallPart) -> Self {
        Self { branch: part.taken.map(Branch::Part), exits: part.exits, ..Self::single(part.stencil, part.holes, None, None) }
    }
}

/// A chain as one planned row: the row after the `spills` that lead it is the row itself, the
/// spills run before it and the rest after it, all at the same PC.
fn chained(parts: Vec<CallPart>, spills: usize, owned: Option<Box<quench_bjit::call_record::CallRecord>>) -> Result<Planned, String> {
    let mut parts = parts.into_iter().map(Planned::of_part);
    let before: Vec<Planned> = parts.by_ref().take(spills).collect();
    let mut row = parts.next().ok_or("chain: no row")?;
    row.pre = before;
    row.post = parts.collect();
    row.owned = owned;
    Ok(row)
}

/// The plan of a step at row `pc`: the leave row, the helper chain, the completion of the
/// activation or the call's chain.
pub(super) fn plan_step(decoded: &Decoded, layout: &Layout, pc: u32, helpers: LaneHelpers) -> Result<Planned, String> {
    match decoded.step.ok_or("plan_step: no step")? {
        Step::Leave => Ok(Planned::single(call::stencil(CallRow::Here)?, [0; HOLES], None, Some(pc))),
        Step::Helper => plan_helper(layout, pc, helpers),
        Step::Return => {
            let slot = operand(&layout.homes, decoded.a).1;
            Ok(Planned { stop: Some(Stop { pc, returns: Some(slot) }), ..Planned::single(control(Kind::Jump)?, [0; HOLES], None, None) })
        }
        Step::Call(shape) => plan_call(layout, shape, pc, helpers),
    }
}

/// The chain of a helper step at row `pc`, which names the caller's record it makes.
fn plan_helper(layout: &Layout, pc: u32, helpers: LaneHelpers) -> Result<Planned, String> {
    let address = call_plan::helper_address(helpers, LaneHelper::Step).ok_or("step: no helper address")?;
    let (spills, reloads, record) = (layout.spills(pc), layout.spills(pc + 1), layout.record(pc));
    let mut row = chained(call_plan::plan_step(record, address, &spills, &reloads)?, spills.len(), None)?;
    // The step helper answers completed or missed: its (status, word) exit is the leave at the record too.
    row.exits = vec![Exit::At(record); row.exits.len()];
    Ok(row)
}

/// The chain of the call `shape` names at row `pc`: spills, the direct row where the site's target
/// is stable, the helper row, reloads. The direct row's record is the row's `owned`.
fn plan_call(layout: &Layout, shape: CallShape, pc: u32, helpers: LaneHelpers) -> Result<Planned, String> {
    let helper = call_plan::helper_address(helpers, layout.ending.call_helper()).ok_or("call: no helper address")?;
    let (spills, reloads) = (layout.spills(pc), layout.spills(pc + 1));
    let plan = CallPlan { shape, direct: layout.callee(shape.call_pc), helper, spills: &spills, reloads: &reloads };
    let CallChain { parts, record, choice } = call_plan::plan_call(&plan)?;
    note(|totals, _| if choice == Choice::Direct { totals.direct_calls += 1 } else { totals.helper_calls += 1 });
    chained(parts, spills.len(), record)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bytecode::Op;

    fn instruction(op: Op) -> WideInstruction {
        WideInstruction::new(op, 1, 2, 3, 0)
    }

    /// Every opcode has a lowering, and the helper steps are exactly the in-place instructions and
    /// the call and construction helpers' own.
    #[test]
    fn every_opcode_is_lowered() {
        let process = crate::vm::compiled::process_lane_helpers::<crate::test_host::TestHost>();
        let mut counts = [0usize; 2];
        for index in 0..Op::COUNT {
            let op = Op::from_index(index).expect("an opcode");
            let step = lowering(instruction(op), process);
            counts[usize::from(step == Step::Helper)] += 1;
            if op.traits().has(Traits::LANE_IN_PLACE) && instruction(op).written_registers().count() <= 1 {
                assert_eq!(step, Step::Helper, "{op:?}");
            }
        }
        assert_eq!(counts[0] + counts[1], Op::COUNT);
        assert!(counts[1] > 0 && counts[0] > 0);
    }

    /// Without a helper table that has addresses (a linked or symbolic one) an instruction leaves:
    /// still lowered, never refused.
    #[test]
    fn without_helper_addresses_every_opcode_leaves() {
        for index in 0..Op::COUNT {
            let op = Op::from_index(index).expect("an opcode");
            assert_eq!(lowering(instruction(op), LaneHelpers::Ahead), Step::Leave, "{op:?}");
        }
    }

    #[test]
    fn a_nop_is_the_jump_to_the_next_instruction() {
        let row = decode_step(7, instruction(Op::Nop), LaneHelpers::Ahead);
        assert_eq!((row.kind, row.target, row.step.is_none()), (Kind::Jump, Some(8), true));
    }

    /// The planner refuses no instruction of the lane fixtures: each decodes to a step (or the
    /// jump a `Nop` is), whatever the program holds.
    #[test]
    fn nothing_in_the_lane_fixtures_is_refused() {
        let process = crate::vm::compiled::process_lane_helpers::<crate::test_host::TestHost>();
        let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/lanes");
        let (mut fixtures, mut instructions) = (0, 0);
        for entry in std::fs::read_dir(&directory).expect("the lane fixtures") {
            let path = entry.expect("a directory entry").path();
            if !path.extension().is_some_and(|extension| extension == "js") {
                continue;
            }
            let source = std::fs::read_to_string(&path).expect("a fixture");
            let program = crate::Engine::specialize(&source, "lane-fixture.js").unwrap_or_else(|errors| panic!("{path:?}: {errors:?}"));
            fixtures += 1;
            for index in 0..program.functions.len() {
                let function = program.function(index);
                for packed in &function.code {
                    let instruction = crate::vm::dispatch_fast::decoded_instruction(function, *packed);
                    let row = decode_step(0, instruction, process);
                    assert!(row.step.is_some() || row.target.is_some(), "{path:?}: {:?}", instruction.op());
                    instructions += 1;
                }
            }
        }
        assert!(fixtures > 0 && instructions > 0);
    }
}
