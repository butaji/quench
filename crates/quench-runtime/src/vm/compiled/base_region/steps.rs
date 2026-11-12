//! The lowering of an instruction no stencil row takes: every instruction the interpreter
//! handles has one, so a region is never refused for the instruction it holds.
//!
//! 1. A row (`decode`): the numeric and heap rows, instantiated from the staged operations.
//! 2. A helper step: the interpreter's own operation made in place by a lane helper
//!    (`LaneHelper::Step`: `(vm, residual, lane_context, call_pc) -> LANE_CALL_*`, the row
//!    `quench_bjit::call::CallRow::Helper`) between the spills of the live pinned registers and
//!    their reloads (`call_plan::plan_step`), for the instructions whose general step only
//!    continues, jumps or throws (`Traits::LANE_IN_PLACE`) and writes at most one register. The
//!    helper answers completed, branched or missed, so both of the row's exits are the leave at
//!    the instruction: a miss, where the helper made nothing or raised, leaves there for the
//!    interpreter. A step that jumps (`ForInNext`) has its target as the row's branch: the helper
//!    answers branched where the interpreter's step took it, which continues at the reloads of the
//!    registers live at the target (`branching`).
//!    A name store or `typeof` no row takes (`globals`) is the VM's own operation (`staged::CallOut`),
//!    made in place by `LaneHelper::VmStep` in the same chain (`Step::CallOut`).
//! 3. A leave (`CallRow::Here`): the region ends at the instruction and the interpreter runs it,
//!    for every other instruction: jumps and branches the rows do not take, throws,
//!    suspensions, scope entries, and any instruction that carries result flags, which a helper
//!    cannot give back.
//!
//! A whole function's body (`function`) also has the two steps that are the body's own: its
//! `Return` completes the activation, and its `Call` is the call planner's chain
//! (`call_plan::plan_call`): the direct entry-cell row where the site recorded a stable callee,
//! the helper row otherwise, between the spills and reloads. A loop region makes no activation,
//! so its calls and returns are leaves.
//!
//! Which class an instruction is in follows from the opcode catalog's traits and layouts
//! (`lowering`), never from a table of opcodes here.

use super::*;
use super::call_plan::{self, CallChain, CallPart, CallPlan, CallShape, Choice};
use super::super::{LaneHelper, LaneHelpers};
use crate::bytecode::{Op, Traits};
use quench_bjit::HOLES;
use quench_bjit::call::{self, CallRow};

/// How the b-jit runs an instruction no row takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Step {
    /// `LaneHelper::Step` makes the record in place and the region continues.
    Helper,
    /// `LaneHelper::VmStep` makes the record's call-out in place (`staged::CallOut`) and the region continues.
    CallOut,
    /// The region leaves at the record.
    Leave,
    /// The activation completes with the register the row reads (a body's `Return`).
    Return,
    /// A call, made by the call planner's chain (a body's `Call`).
    Call(CallShape),
}

impl Step {
    /// Whether control continues at the next instruction: a leave and a return end the run.
    pub(super) fn falls(self) -> bool {
        !matches!(self, Self::Leave | Self::Return)
    }

    /// Whether a lane helper makes the record: it may write any heap value and run guest code.
    pub(super) fn runs_helper(self) -> bool {
        matches!(self, Self::Helper | Self::CallOut)
    }
}

/// The opcodes the b-jit does not compile, by opcode index: the Wasm opcodes (catalog names that
/// begin `Wasm`). A Wasm function has its own compiler (`dispatch_wasm`, the Wasm tiers), which
/// owns the linear memory, tables and typed registers these opcodes read, so a function holding
/// one is not a b-jit body (`covers`), and a loop region holding one leaves at it.
const EXCLUDED: [bool; Op::COUNT] = {
    const PREFIX: &[u8] = b"Wasm";
    let mut table = [false; Op::COUNT];
    let mut index = 0;
    while index < Op::COUNT {
        let name = Op::NAMES[index].as_bytes();
        let mut at = 0;
        let mut named = name.len() >= PREFIX.len();
        while named && at < PREFIX.len() {
            named = name[at] == PREFIX[at];
            at += 1;
        }
        table[index] = named;
        index += 1;
    }
    table
};

/// Whether the b-jit compiles a function of these instructions: none is an excluded opcode.
pub(in crate::vm::compiled) fn covers(code: &[WideInstruction]) -> bool {
    code.iter().all(|instruction| !EXCLUDED[instruction.op() as usize])
}

/// Whether `instruction` is made in place by `LaneHelper::Step`.
fn steps_in_place(instruction: WideInstruction) -> bool {
    instruction.op().traits().has(Traits::LANE_IN_PLACE) && instruction.result_flags() == 0 && instruction.written_registers().count() <= 1
}

/// Whether `instruction` is a global name store or `typeof`, a call-out the VM makes in place
/// (`LaneHelper::VmStep`) that writes at most one register.
fn calls_out(instruction: WideInstruction) -> bool {
    let call = crate::staged::Operation::<(), ()>::decode(instruction, |_| None).and_then(|row| row.call_out());
    instruction.result_flags() == 0 && matches!(call, Some((crate::staged::CallOut::GlobalWrite { .. } | crate::staged::CallOut::GlobalTypeof { .. }, _)))
}

/// The lowering of `instruction`, where helpers are `helpers`: a helper step where the helper
/// table has the step helper's address, a call-out where it has the VM step helper's, else the leave.
pub(super) fn lowering(instruction: WideInstruction, helpers: LaneHelpers) -> Step {
    let addressed = |helper| call_plan::helper_address(helpers, helper).is_some();
    if addressed(LaneHelper::Step) && steps_in_place(instruction) {
        Step::Helper
    } else if addressed(LaneHelper::VmStep) && calls_out(instruction) {
        Step::CallOut
    } else {
        Step::Leave
    }
}

/// The row of an instruction at `pc` that has no numeric or heap row: a `Nop` is the jump to the
/// next instruction, the rest their step. A helper step names the register it writes.
pub(super) fn decode_step(pc: u32, instruction: WideInstruction, helpers: LaneHelpers) -> Decoded {
    match instruction.op().class() {
        Class::Nop => Decoded::number(Kind::Jump, None, None, None, Some(pc + 1)),
        _ => {
            let step = lowering(instruction, helpers);
            let written = if step.runs_helper() { instruction.written_registers().next() } else { None };
            let target = (step == Step::Helper && instruction.op().control_flow_layout().jumps()).then(|| instruction.jump_target());
            Decoded { target, ..Decoded::step(step, written) }
        }
    }
}

/// A helper step may write any heap value, which the snapshots a heap row reads (the entry
/// record's captures, fields and array views) would not see, and run guest code that changes a
/// Stable fact a region holds (the global object's layout): where the region has such a row its
/// helper steps leave instead.
pub(super) fn settle(decoded: &mut [Decoded]) {
    if decoded.iter().any(|row| row.heap.is_some_and(Heap::reads_snapshot) || row.global.is_some()) {
        decoded.iter_mut().filter(|row| row.step.is_some_and(Step::runs_helper)).for_each(|row| row.step = Some(Step::Leave));
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

/// The plan of a step at `pc`: the leave row, the helper chain, the completion of the activation
/// or the call's chain.
pub(super) fn plan_step(decoded: &Decoded, layout: &Layout, pc: u32, helpers: LaneHelpers) -> Result<Planned, String> {
    match decoded.step.ok_or("plan_step: no step")? {
        Step::Leave => Ok(Planned::single(call::stencil(CallRow::Here)?, [0; HOLES], None, Some(pc))),
        Step::Helper => plan_helper(layout, (pc, decoded.target), (LaneHelper::Step, helpers)),
        Step::CallOut => plan_helper(layout, (pc, decoded.target), (LaneHelper::VmStep, helpers)),
        Step::Return => {
            let slot = operand(&layout.homes, decoded.a).1;
            Ok(Planned { stop: Some(Stop { pc, returns: Some(slot) }), ..Planned::single(control(Kind::Jump)?, [0; HOLES], None, None) })
        }
        Step::Call(shape) => plan_call(layout, shape, helpers),
    }
}

/// The chain of a step at `pc` that `helper` makes, which also branches to `target` where its instruction jumps.
fn plan_helper(layout: &Layout, (pc, target): (u32, Option<u32>), (helper, helpers): (LaneHelper, LaneHelpers)) -> Result<Planned, String> {
    let address = call_plan::helper_address(helpers, helper).ok_or("step: no helper address")?;
    let (spills, reloads) = (layout.spills(pc), layout.spills(pc + 1));
    let mut row = chained(call_plan::plan_step(pc, address, &spills, &reloads)?, spills.len(), None)?;
    // The step helper answers completed, branched or missed: its (status, word) exit is the leave at the record too.
    // A call-out may run guest code, which leaves the body through that exit.
    if helper == LaneHelper::Step {
        row.exits = vec![Exit::At(pc); row.exits.len()];
    }
    match target {
        Some(target) => branching(row, layout, (pc, target)),
        None => Ok(row),
    }
}

/// The jump to `pc`, a part of a run whose NEXT is not where the run goes on.
fn jump_to(pc: u32) -> Result<Planned, String> {
    Ok(Planned::single(control(Kind::Jump)?, [0; HOLES], Some(pc), None))
}

/// The reloads of the pinned registers live at `pc`, then the jump to `pc`: the branch of a run
/// that restores the registers its destination expects.
fn landing(layout: &Layout, pc: u32) -> Result<Vec<Planned>, String> {
    let reloads = layout.spills(pc).into_iter().map(|(place, register)| call_plan::reload_part(place, register).map(Planned::of_part));
    reloads.chain(std::iter::once(jump_to(pc))).collect()
}

/// A helper step's run made to branch: its fall-through (the reloads of the next instruction's
/// registers) jumps to the next instruction, and the TAKEN exit of the helper row (a step that took
/// its jump, `LANE_CALL_BRANCHED`) lands at `target` after the fall-through, as a part's NEXT is the
/// part laid out after it.
fn branching(mut row: Planned, layout: &Layout, (pc, target): (u32, u32)) -> Result<Planned, String> {
    row.post.push(jump_to(pc + 1)?);
    let taken = row.pre.len() + 1 + row.post.len();
    row.post.extend(landing(layout, target)?);
    row.branch = Some(Branch::Part(taken));
    Ok(row)
}

/// The chain of the call `shape` names: spills, the direct row where the site's target is stable,
/// the helper row, reloads. The direct row's record is the row's `owned`.
fn plan_call(layout: &Layout, shape: CallShape, helpers: LaneHelpers) -> Result<Planned, String> {
    let helper = call_plan::helper_address(helpers, LaneHelper::Call).ok_or("call: no helper address")?;
    let (spills, reloads) = (layout.spills(shape.call_pc), layout.spills(shape.call_pc + 1));
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

    /// The opcodes whose only lowering is the generic helper step: no staged fast arm exists for
    /// them (each allocates, runs guest code, or edits VM-held state no row reads), so the helper
    /// is their whole operation, not a slow arm.
    const RESIDUAL: [Op; 20] = [
        Op::DefineField, Op::DefineComputedField, Op::DefinePrivateField, Op::CheckPrivate, Op::PrivateIn, Op::MarkPrivateName,
        Op::SetFunctionName, Op::SetFunctionNameKey, Op::SetFunctionDebugName, Op::DefineArrayElement, Op::AppendArray,
        Op::ToPropertyKey, Op::CopyDataProperties, Op::GetIterator, Op::ForInStart, Op::ForInNext, Op::RequireIteratorResult,
        Op::IteratorClose, Op::IteratorCleanupPush, Op::IteratorCleanupPop,
    ];

    /// No function of the lane fixtures holds a generic helper step beyond the listed residual
    /// opcodes, and a helper step that jumps names its target (its row branches).
    #[test]
    fn lane_fixtures_use_the_generic_helper_only_for_residual_opcodes() {
        let process = crate::vm::compiled::process_lane_helpers::<crate::test_host::TestHost>();
        let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/lanes");
        for entry in std::fs::read_dir(&directory).expect("the lane fixtures") {
            let path = entry.expect("a directory entry").path();
            if !path.extension().is_some_and(|extension| extension == "js") {
                continue;
            }
            let source = std::fs::read_to_string(&path).expect("a fixture");
            let program = crate::Engine::specialize(&source, "lane-fixture.js").unwrap_or_else(|errors| panic!("{path:?}: {errors:?}"));
            for index in 0..program.functions.len() {
                let function = program.function(index);
                for packed in &function.code {
                    let instruction = crate::vm::dispatch_fast::decoded_instruction(function, *packed);
                    let row = decode_step(0, instruction, process);
                    if row.step == Some(Step::Helper) {
                        assert!(RESIDUAL.contains(&instruction.op()), "{path:?}: {:?} takes the generic helper", instruction.op());
                        assert_eq!(row.target.is_some(), instruction.op().control_flow_layout().jumps(), "{:?}", instruction.op());
                    }
                }
            }
        }
    }

    /// A function holding a Wasm opcode is no b-jit body; the catalog's other opcodes are covered.
    #[test]
    fn wasm_opcodes_are_excluded_by_the_table() {
        for index in 0..Op::COUNT {
            let op = Op::from_index(index).expect("an opcode");
            assert_eq!(!covers(&[instruction(op)]), Op::NAMES[index].starts_with("Wasm"), "{op:?}");
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
