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
//!    for every other instruction: jumps and branches the rows do not take, returns, throws,
//!    suspensions, scope entries and the whole call family (a loop region makes no guest
//!    activation), and any instruction that carries result flags, which a helper cannot give back.
//!
//! Which class an instruction is in follows from the opcode catalog's traits and layouts
//! (`lowering`), never from a table of opcodes here.

use super::*;
use super::call_plan::{self, CallPart};
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
}

/// Whether `instruction` is made in place by `LaneHelper::Step`.
fn steps_in_place(instruction: WideInstruction) -> bool {
    instruction.op().traits().has(Traits::LANE_IN_PLACE) && instruction.result_flags() == 0 && instruction.written_registers().count() <= 1
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
    if instruction.op().class() == Class::Nop {
        return Decoded::number(Kind::Jump, None, None, None, Some(pc + 1));
    }
    let step = lowering(instruction, helpers);
    let written = if step == Step::Helper { instruction.written_registers().next() } else { None };
    Decoded::step(step, written)
}

/// A helper step may write any heap value, which the snapshots a heap row reads (the entry
/// record's captures, fields and array views) would not see: where the region has a heap row its
/// helper steps leave instead.
pub(super) fn settle(decoded: &mut [Decoded]) {
    if decoded.iter().any(|row| row.heap.is_some()) {
        decoded.iter_mut().filter(|row| row.step == Some(Step::Helper)).for_each(|row| row.step = Some(Step::Leave));
    }
}

impl Planned {
    /// The row of a chain part, its exits and no rows of its own.
    fn of_part(part: CallPart) -> Self {
        Self { stencil: part.stencil, holes: part.holes, target: None, exits: part.exits, pre: Vec::new(), post: Vec::new() }
    }
}

/// The plan of a step at `pc`: the leave row, or the helper row with its spills before and its
/// reloads after, as the rows that run before and after it at the same PC.
pub(super) fn plan_step(step: Step, layout: &Layout, pc: u32, helpers: LaneHelpers) -> Result<Planned, String> {
    if step == Step::Leave {
        return Ok(Planned::single(call::stencil(CallRow::Here)?, [0; HOLES], None, Some(pc)));
    }
    let address = call_plan::helper_address(helpers, LaneHelper::Step).ok_or("step: no helper address")?;
    let (spills, reloads) = (layout.spills(pc), layout.spills(pc + 1));
    let mut parts = call_plan::plan_step(pc, address, &spills, &reloads)?.into_iter().map(Planned::of_part);
    let before: Vec<Planned> = parts.by_ref().take(spills.len()).collect();
    let mut row = parts.next().ok_or("step: no helper row")?;
    // The step helper answers completed or missed: its (status, word) exit is the leave at the record too.
    row.exits = vec![Exit::At(pc); row.exits.len()];
    row.pre = before;
    row.post = parts.collect();
    Ok(row)
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
