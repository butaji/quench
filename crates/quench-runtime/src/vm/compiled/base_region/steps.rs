//! The lowering of an instruction no stencil row takes: every instruction the interpreter
//! handles has one, so a region is never refused for the instruction it holds.
//!
//! 1. A row (`decode`): the numeric and heap rows, instantiated from the staged operations.
//! 2. A helper step: the interpreter's own operation made in place by a lane helper
//!    (`LaneHelper::Step`: `(vm, residual, lane_context, call_pc) -> LANE_CALL_*`, the row
//!    `quench_bjit::call::CallRow::Helper`) between the spills of the live pinned registers and
//!    their reloads (`call_plan::plan_step`), for the instructions whose general step only
//!    continues, jumps or throws (`Op::steps_in_body`: the lane's in-place steps, and the scope,
//!    operator, constant, name, literal, element, field and class steps that may also replace the
//!    frame's environment, after which the helper drops the lane's capture caches) and writes at
//!    most `STEP_WRITES` registers. The helper answers completed, branched or missed,
//!    so both of the row's exits are the leave at the instruction: a miss, where the helper made
//!    nothing or raised, leaves there for the interpreter. A step that jumps (`ForInNext`) has its
//!    target as the row's branch: the helper answers branched where the interpreter's step took
//!    it, which continues at the reloads of the registers live at the target (`branching`).
//!    Where the instruction's meaning has a fast arm that a register's own word decides
//!    (`staged::iteration`: `RequireIteratorResult`, `ToPropertyKey`), the run is that arm's heap row
//!    first (`Step::Fast`, `plan_fast`): its hit hops past the helper step to the next instruction,
//!    and its miss enters the helper step laid out behind it, the slow arm, so the generic step is
//!    only the operation's remainder.
//! 3. A throw: `Throw` is the helper step that always raises, whose row ends the run (`Step::Throw`):
//!    the helper stashes the exception in `Vm::lane_raised` and misses, so the run leaves at the
//!    `throw`, where the lane raises the stashed error at the record. A function's own handler that
//!    covers it is the catch's jump instead (`throws::caught`, `Step::Catch`): the thrown value moves
//!    to the handler's register and control goes to its block, as `Vm::exception_handler_target`
//!    does, so the interpreter never re-executes the `throw` to find the handler.
//! 4. A leave (`CallRow::Here`): the region ends at the instruction and the interpreter runs it,
//!    for every other instruction: the suspensions `Await`, `Yield` and
//!    `YieldStar`, whose generator or async record the lane resumes in the interpreter, so no native
//!    code could continue past them; the Wasm opcodes (`EXCLUDED`); a call with a result flag other than the
//!    return flag and the `this` binding; a conditional jump the helper would take backward in a loop region, which has no row that
//!    polls the interrupt flag before it (a body has: `function::preludes`); and any instruction that
//!    carries result flags the body does not complete with.
//!
//! A whole function's body (`function`) also has the steps that are the body's own: its `Return`
//! completes the activation, an instruction that carries the return flag is its row followed by that
//! completion with its result (`completing`), and its calls are the call planner's chains
//! (`call_plan::plan_call`, one per `CallForm`): the direct entry-cell row where a plain call's site
//! recorded a stable callee, the call, construct or step helper's row otherwise, between the spills
//! and reloads. A loop region makes no activation, so its calls and returns are leaves.
//!
//! Which class an instruction is in follows from the opcode catalog's traits and layouts
//! (`lowering`), never from a table of opcodes here.

use super::*;
use super::call_plan::{self, CallChain, CallPart, CallPlan, CallShape, Choice};
use super::callees::chains;
use super::super::{LaneHelper, LaneHelpers};
use crate::bytecode::Op;
use quench_bjit::HOLES;
use quench_bjit::call::{self, CallRow};

/// How the b-jit runs an instruction no row takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Step {
    /// `LaneHelper::Step` makes the record in place and the region continues.
    Helper,
    /// The heap row is the record's fast arm over its operand's frame slot, with `Helper` behind it
    /// as the slow arm.
    Fast(Heap),
    /// `LaneHelper::VmStep` makes the record's call-out in place (`staged::CallOut`) and the region continues.
    CallOut,
    /// The region leaves at the record.
    Leave,
    /// The activation completes with the register the row reads (a body's `Return`).
    Return,
    /// A call, made by the call planner's chain (a body's `Call`, a loop region's plain call).
    Call(CallShape),
    /// A `throw` no handler of the function takes natively: the helper stashes the exception and
    /// the run leaves at the record (`plan_throw`).
    Throw,
    /// A `throw` a handler of the function covers (`throws::caught`): the thrown value goes to the
    /// handler's register (the row's destination, from its operand) and control to its block (the
    /// row's branch).
    Catch,
}

impl Step {
    /// Whether the run makes the step helper's call, alone, as the slow arm of a fast row or as the
    /// raising step: the helper may allocate or run guest code, which no heap snapshot a region took
    /// sees.
    pub(super) fn runs_helper(self) -> bool {
        matches!(self, Self::Helper | Self::Fast(_) | Self::Throw)
    }

    /// Whether control continues at the next instruction: a leave, a return, a throw and a catch
    /// end the run or go elsewhere.
    pub(super) fn falls(self) -> bool {
        !matches!(self, Self::Leave | Self::Return | Self::Throw | Self::Catch)
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

/// The most registers a step the helper makes writes: its result, and the `this` a name call
/// defines beside its callee.
pub(super) const STEP_WRITES: usize = 2;

/// Whether `instruction` is made in place by `LaneHelper::Step`: no result flag but a numeric operator's local target,
/// which the interpreter's step writes to the frame's local slot (`Vm::step`'s `numeric_local_target`) and defines no
/// register for (`written_registers`).
fn steps_in_place(instruction: WideInstruction) -> bool {
    let flags = instruction.result_flags();
    instruction.op().steps_in_body() && (flags == 0 || instruction.writes_numeric_local() && flags == crate::bytecode::NUMERIC_LOCAL_TARGET)
        && instruction.written_registers().count() <= STEP_WRITES
}

/// The heap row that is the fast arm of a `Unary` that is a `typeof` string, index or test
/// (`staged::value::immediate_typeof_index`), where the test's name index fits its hole.
fn typeof_arm(operator: u32) -> Option<Heap> {
    match operator {
        crate::bytecode::UNARY_TYPEOF_INDEX => return Some(Heap::TypeofIndex),
        crate::vm::coercion::TYPEOF => return Some(Heap::TypeofString),
        _ => {}
    }
    let (index, negated) = crate::bytecode::typeof_test(operator)?;
    Some(Heap::TypeofIs { index: u8::try_from(index).ok()?, negated })
}

/// The heap row that is the fast arm of `instruction`, where its meaning has one its operand
/// register's word decides (`staged::iteration`, `staged::value`); all read register B.
fn fast_arm(instruction: WideInstruction) -> Option<Heap> {
    match instruction.op().class() {
        Class::RequireIteratorResult => Some(Heap::Object),
        Class::ToPropertyKey => Some(Heap::Key),
        Class::Unary => typeof_arm(instruction.unary_operator()),
        _ => None,
    }
}

/// The lowering of `instruction`, where helpers are `helpers`: where the helper table has the step
/// helper's address, a `throw` is the raising step, and the other steps made in place are helper
/// steps (behind their fast arm, where they have one); every other instruction, and every one
/// without the address, leaves.
pub(super) fn lowering(instruction: WideInstruction, helpers: LaneHelpers) -> Step {
    let addressed = call_plan::helper_address(helpers, LaneHelper::Step).is_some();
    match fast_arm(instruction) {
        _ if !addressed => Step::Leave,
        _ if instruction.op().raises() => Step::Throw,
        _ if !steps_in_place(instruction) => Step::Leave,
        Some(fast) => Step::Fast(fast),
        None => Step::Helper,
    }
}

/// Whether `instruction` at `pc` may jump backward, which only a row that polls the interrupt flag
/// may do: a body polls before every row that branches back (`function::preludes`), a region only
/// at its own backedge.
fn branches_back(pc: u32, instruction: WideInstruction) -> bool {
    instruction.op().control_flow_layout().jumps() && instruction.jump_target() <= pc
}

/// The row of an instruction at `pc` that has no numeric or heap row: a `Nop` is the jump to the
/// next instruction, the rest their step. `polls` where a row before it polls the interrupt flag,
/// as a body's do, so a step that jumps backward may be made in place. A helper step names the register it writes, and a fast
/// arm the register it reads.
pub(super) fn decode_step(pc: u32, instruction: WideInstruction, (helpers, polls): (LaneHelpers, bool)) -> Decoded {
    match instruction.op().class() {
        Class::Nop => Decoded::number(Kind::Jump, None, None, None, Some(pc + 1)),
        Class::Call if calls_in_place(instruction, helpers) => Decoded::step(Step::Call(CallShape::of(pc, instruction)), Some(instruction.result_register())),
        _ => {
            let step = if branches_back(pc, instruction) && !polls { Step::Leave } else { lowering(instruction, helpers) };
            let helped = step.runs_helper();
            let mut written = instruction.written_registers().filter(|_| helped);
            let target = (helped && instruction.op().control_flow_layout().jumps()).then(|| instruction.jump_target());
            let read = matches!(step, Step::Fast(_)).then(|| Source::Register(instruction.register_b()));
            let result = written.next();
            Decoded { target, a: read, also: written.next(), ..Decoded::step(step, result) }
        }
    }
}

/// A helper step, alone or behind a fast arm, may write any heap value, which the snapshots a heap row reads (the entry
/// record's captures, fields and array views) would not see: where the region has a heap row its
/// helper steps leave instead.
pub(super) fn settle(decoded: &mut [Decoded]) {
    if decoded.iter().any(|row| row.heap.is_some_and(Heap::reads_snapshot)) {
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

/// The plan of a step at `pc`: the leave row, the helper chain, a fast arm's run, the completion of
/// the activation or the call's chain.
pub(super) fn plan_step(decoded: &Decoded, layout: &Layout, pc: u32, helpers: LaneHelpers) -> Result<Planned, String> {
    match decoded.step.ok_or("plan_step: no step")? {
        Step::Leave => Ok(Planned::single(call::stencil(CallRow::Here)?, [0; HOLES], None, Some(pc))),
        Step::Helper => plan_helper(layout, pc, decoded, helpers),
        Step::Throw => plan_throw(layout, pc, helpers),
        Step::Catch => plan_catch(decoded, layout, pc, helpers),
        Step::Fast(fast) => plan_fast(decoded, fast, layout, pc, helpers),
        Step::Return => {
            let slot = operand(&layout.homes, decoded.a).1;
            Ok(Planned { stop: Some(Stop { pc, returns: Some(slot) }), ..Planned::single(control(Kind::Jump)?, [0; HOLES], None, None) })
        }
        Step::Call(shape) => plan_call(layout, shape, pc, helpers),
    }
}

/// The step helper's chain at `pc`: the spills, the helper row and the `reloads` after it. The helper answers
/// completed, branched or missed (nothing ran: the leave at the record), or left, where the step ran in frames it made
/// of the lane's activations (a frame-needing step of a native callee) and the body leaves past it: the row's
/// (status, word) exit.
fn helper_chain(layout: &Layout, pc: u32, reloads: &[(Place, Register)], helpers: LaneHelpers) -> Result<Planned, String> {
    let address = call_plan::helper_address(helpers, LaneHelper::Step).ok_or("step: no helper address")?;
    let spills = layout.spills(pc);
    chained(call_plan::plan_step(pc, address, &spills, reloads)?, spills.len(), None)
}

/// The pinned registers a row at `pc` reloads before it continues: those live at the next
/// instruction, none where the row completes the activation (nothing runs after it).
fn reloads(layout: &Layout, decoded: &Decoded, pc: u32) -> Vec<(Place, Register)> {
    if decoded.completes { Vec::new() } else { layout.spills(pc + 1) }
}

/// The chain of a helper step at `pc`, which also branches to the target of its instruction where
/// it jumps.
fn plan_helper(layout: &Layout, pc: u32, decoded: &Decoded, helpers: LaneHelpers) -> Result<Planned, String> {
    let row = helper_chain(layout, pc, &reloads(layout, decoded, pc), helpers)?;
    match decoded.target {
        Some(target) => branching(row, layout, (pc, target)),
        None => Ok(row),
    }
}

/// `run`, whose instruction carried the return flag, followed by the completion of the activation
/// with the result it wrote: the pinned register that holds the result written to its frame slot
/// first (a leave writes back only the registers live at the instruction, and the result is the
/// instruction's own definition), then the row that completes with that slot, recording the
/// instruction's PC as the completion, as a body's `Return` does.
pub(super) fn completing(mut run: Planned, layout: &Layout, pc: u32, result: Option<Register>) -> Result<Planned, String> {
    let web = result.ok_or("completion: the instruction writes no result")?;
    let (place, slot) = operand(&layout.homes, Some(Source::Register(web)));
    if matches!(place, Place::Pin(_) | Place::PinF(_)) {
        run.post.push(Planned::of_part(call_plan::spill_part(place, layout.homes.orig[usize::from(web)])?));
    }
    run.post.push(Planned { stop: Some(Stop { pc, returns: Some(slot) }), ..Planned::single(control(Kind::Jump)?, [0; HOLES], None, None) });
    Ok(run)
}

/// The chain of a `throw` at `pc`: the spills and the helper row, which raises, so nothing is live
/// after it and no registers are reloaded; a helper that did not raise (it ran beside a native
/// callee's frames, where it makes nothing) falls to the leave at the record, as every exit does.
fn plan_throw(layout: &Layout, pc: u32, helpers: LaneHelpers) -> Result<Planned, String> {
    let mut row = helper_chain(layout, pc, &[], helpers)?;
    row.post.push(Planned { exits: vec![Exit::At(pc)], ..Planned::single(call::stencil(CallRow::Here)?, [0; HOLES], None, None) });
    Ok(row)
}

/// A catch at `pc`: the move of the thrown value to the handler's register, then the jump to the
/// handler's block.
fn plan_catch(decoded: &Decoded, layout: &Layout, pc: u32, helpers: LaneHelpers) -> Result<Planned, String> {
    let handler = decoded.target.ok_or("catch: no handler")?;
    let moved = Decoded::number(Kind::Move, decoded.dst, decoded.a, None, None);
    let mut row = plan(&moved, layout, pc, true, helpers)?;
    row.post.push(jump_to(handler)?);
    Ok(row)
}

/// The word a `typeof` fast arm answers for any Number, which an operand held in a pinned register or an
/// int slot always is: the fact is the operand's representation, proved once where it was defined.
fn number_typeof(fast: Heap, names: u64) -> Option<u64> {
    match fast {
        // SAFETY: the table is the compiling VM's (`Known::typeof_names`), whose strings the VM roots.
        Heap::TypeofString => Some(unsafe { (names as *const u64).add(crate::staged::value::TYPEOF_NUMBER as usize).read() }),
        Heap::TypeofIndex => Some(Value::integer(crate::staged::value::TYPEOF_NUMBER as i32).to_bits()),
        Heap::TypeofIs { index, negated } => Some(Value::boolean((u64::from(index) == crate::staged::value::TYPEOF_NUMBER) != negated).to_bits()),
        _ => None,
    }
}

/// The run of a fast arm at `pc`: the heap row `fast` over the operands' places, whose hit hops past
/// the rest of the run to the next instruction (to the completion where the instruction returns its
/// result), and behind it the helper step the row leaves into where its proof fails. A row that reads a
/// word of its operand's frame slot (`Heap::reads_frame_word`) with an operand held outside it has the
/// helper step alone, or, where the operand is a Number its representation proves, the constant the arm
/// answers for one (`number_typeof`); so does a result held outside its frame slot.
fn plan_fast(decoded: &Decoded, fast: Heap, layout: &Layout, pc: u32, helpers: LaneHelpers) -> Result<Planned, String> {
    let slow = plan_helper(layout, pc, decoded, helpers)?;
    let homes = &layout.homes;
    let ((read, read_hole), (written, written_hole)) = (operand(homes, decoded.a), operand(homes, decoded.dst.map(Source::Register)));
    let (second, second_hole) = operand(homes, decoded.b);
    if written != Place::Mem {
        return Ok(slow);
    }
    if let Some(word) = number_typeof(fast, decoded.hole).filter(|_| matches!(read, Place::Pin(_) | Place::PinF(_) | Place::MemInt)) {
        return plan(&Decoded::number(Kind::Move, decoded.dst, Some(Source::Value(word)), None, None), layout, pc, true, helpers);
    }
    if (read != Place::Mem || second != Place::Mem) && fast.reads_frame_word() {
        return Ok(slow);
    }
    let row = HeapRow { heap: fast, dst: written, a: read, b: Place::Mem, entry: false };
    let holes = heap_holes(decoded, homes, fast, false, [written_hole, read_hole, second_hole]);
    let parts = 2 + slow.pre.len() + slow.post.len();
    let mut run = Planned::single(base_rows::stencil(row)?, holes, None, None);
    run.branch = Some(if decoded.completes { Branch::Part(parts) } else { Branch::Pc(pc + 1) });
    run.exits = vec![Exit::Ahead(1)];
    Ok(behind(run, slow))
}

/// `fast` with the run `slow` laid out behind it: the slow run's parts follow the fast row, so the
/// parts that name another part of their run are renumbered past it.
fn behind(mut fast: Planned, mut slow: Planned) -> Planned {
    let (lead, tail) = (std::mem::take(&mut slow.pre), std::mem::take(&mut slow.post));
    fast.owned = slow.owned.take();
    fast.post = lead.into_iter().chain(std::iter::once(slow)).chain(tail).map(|mut part| {
        part.branch = part.branch.map(|branch| match branch {
            Branch::Part(index) => Branch::Part(index + 1),
            pc @ Branch::Pc(_) => pc,
        });
        part
    }).collect();
    fast
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
    let helper = call_plan::helper_address(helpers, shape.form.helper()).ok_or("call: no helper address")?;
    let (spills, reloads) = (layout.spills(shape.call_pc), if shape.returning { Vec::new() } else { layout.spills(shape.call_pc + 1) });
    let plan = CallPlan { shape, direct: layout.callee(shape.call_pc), helper, spills: &spills, reloads: &reloads };
    let CallChain { parts, record, choice } = call_plan::plan_call(&plan)?;
    note(|totals, _| if choice == Choice::Direct { totals.direct_calls += 1 } else { totals.helper_calls += 1 });
    chained(parts, spills.len(), record)
}

#[cfg(test)]
mod tests;
