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
//!    (`staged::iteration`: `RequireIteratorResult`, `ToPropertyKey`, `ForInNext` over its state's memo) or the backing's layout words
//!    (`staged::elements::append_in_room`: a one-element `AppendArray`), or a site's recorded transition (`adds`, `names`), the run is that arm's heap row
//!    first (`Step::Fast`, `plan_fast`): its hit hops past the helper step to the next instruction,
//!    and its miss enters the helper step laid out behind it, the slow arm, so the generic step is
//!    only the operation's remainder.
//!    A name store or `typeof` no row takes (`globals`) is the VM's own operation (`staged::CallOut`),
//!    made in place by `LaneHelper::VmStep` in the same chain (`Step::CallOut`).
//! 3. A throw: `Throw` is the helper step that always raises, whose row ends the run (`Step::Throw`):
//!    the helper stashes the exception in `Vm::lane_raised` and misses, so the run leaves at the
//!    `throw`, where the lane raises the stashed error at the record. A function's own handler that
//!    covers it is the catch's jump instead (`throws::caught`, `Step::Catch`): the thrown value moves
//!    to the handler's register and control goes to its block, as `Vm::exception_handler_target`
//!    does, so the interpreter never re-executes the `throw` to find the handler.
//! 4. A leave (`CallRow::Here`): the region ends at the instruction and the interpreter runs it,
//!    for every other instruction: the suspensions `Await`, `Yield` and
//!    `YieldStar`, whose generator or async record the lane resumes in the interpreter, so no native
//!    code could continue past them; the Wasm opcodes (`Traits::WASM_OPCODE`); a call with a result flag other than the
//!    return flag and the `this` binding; a conditional jump the helper would take backward in a loop region, which has no row that
//!    polls the interrupt flag before it (a body has: `function::preludes`); and any instruction that
//!
//! A whole function's body (`function`) also has the steps that are the body's own: its `Return`
//! completes the activation, an instruction that carries the return flag is its row followed by that
//! completion with its result (`completing`), and its calls are the call planner's chains
//! (`call_plan::plan_call`, one per `CallForm`): the direct entry-cell row where a plain call's site
//! recorded a stable callee, the call, construct or step helper's row otherwise, between the spills
//! and reloads. A loop region makes no activation, so its returns (and its calls that return a
//! result) are leaves; its plain calls are the chain with the helper row alone (`calls_in_place`,
//! `LaneHelper::RegionCall`), the direct row where the site recorded a stable callee, or the callee's
//! rows where the stitch inlines it (`function::parts`): a call that completes continues the region,
//! one that missed leaves at the call, and one that left the lane in its callee's activation, or
//! whose callee left, or that ended in termination, ends the run with the (status, word) it left
//! with, which the region's entry returns as a body's does (`RegionRun::nested`,
//! `LaneExit::Terminated`), the registers already spilled.
//!
//! Which class an instruction is in follows from the opcode catalog's traits and layouts
//! (`lowering`), never from a table of opcodes here.

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;
use super::call_plan::{self, CallChain, CallForm, CallPart, CallPlan, CallShape, Choice};
use super::callees::name_records;
use super::super::{LaneHelper, LaneHelpers};
use quench_runtime::bytecode::Traits;
use quench_bjit::HOLES;
use quench_bjit::call::{self, CallRow};

pub(crate) mod lowering;
pub(crate) use lowering::{calls_in_place, decode_step, lowering, STEP_WRITES};

/// How the b-jit runs an instruction no row takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Step {
    /// `LaneHelper::Step` makes the record in place and the region continues.
    Helper,
    /// The heap row is the record's fast arm over its operand's frame slot, with `Helper` behind it
    /// as the slow arm.
    Fast(Heap),
    /// `LaneHelper::VmStep` makes the record's call-out in place (`staged::CallOut`) and the region continues.
    CallOut,
    /// `LaneHelper::AllocateStep` makes the record's fixed-shape object in place (its destination written) and the region
    /// continues; a declined allocation leaves at the record.
    Allocate,
    /// A fast arm whose miss is the leave at the record, which the interpreter runs: an add with no
    /// in-place step (`lowering`), an `instanceof` jump (`sites::instanceof_jump`).
    Guard(Heap),
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
    /// A `WasmBranchTable`: a compare tree on the selector over the table's `Jump` rows (`function::switch`).
    Switch,
}

impl Step {
    /// Whether a lane helper makes the record (the step helper alone, as the slow arm of a fast row or
    /// as the raising step, or the VM step helper's call-out): it may allocate or run guest code, which
    /// no heap snapshot a region took sees.
    pub(crate) fn runs_helper(self) -> bool {
        matches!(self, Self::Helper | Self::CallOut | Self::Allocate | Self::Fast(_) | Self::Throw)
    }

    /// Whether the run reads the dispatch payload: a helper step calls through it, and a guarded
    /// fast row reads the tables its heap row names.
    pub(crate) fn reads_payload(self) -> bool {
        self.runs_helper() || matches!(self, Self::Guard(_))
    }

    /// Whether control continues at the next instruction: a leave, a return, a throw and a catch
    /// end the run or go elsewhere.
    pub(crate) fn falls(self) -> bool {
        !matches!(self, Self::Leave | Self::Return | Self::Throw | Self::Catch)
    }
}

/// Whether the b-jit compiles a function of these instructions: none is a Wasm opcode, which the
/// catalog marks (`Traits::WASM_OPCODE`). A Wasm function has its own compiler (`dispatch_wasm`, the
/// Wasm tiers), which owns the linear memory, tables and typed registers these opcodes read, so a
/// function holding one is not a b-jit body, and a loop region holding one leaves at it.
pub(crate) fn covers(code: &[WideInstruction]) -> bool {
    code.iter().all(|instruction| !instruction.op().traits().has(Traits::WASM_OPCODE))
}

/// The stencil of a control row: it reads and writes no operand.
pub(crate) fn control(kind: Kind) -> Result<&'static Stencil, String> {
    numeric::stencil(numeric::Row { kind, dst: Place::Mem, a: Place::Mem, b: Place::Mem, speculate: true })
}

impl Planned {
    /// The row of a chain part, its exits and no rows of its own; a part's successor is a part of
    /// the chain it belongs to.
    fn of_part(part: CallPart) -> Self {
        Self { branch: part.taken.map(Branch::Part), exits: part.exits, symbolic: part.symbols, ..Self::single(part.stencil, part.holes, None, None) }
    }
}

/// A chain as one planned row: the row after the `spills` that lead it is the row itself, the
/// spills run before it and the rest after it, all at the same PC.
fn chained(parts: Vec<CallPart>, spills: usize, owned: Vec<crate::compiled::OwnedCallRecord>) -> Result<Planned, String> {
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
pub(crate) fn plan_step(decoded: &Decoded, layout: &Layout, pc: u32, helpers: LaneHelpers) -> Result<Planned, String> {
    match decoded.step.ok_or("plan_step: no step")? {
        Step::Leave => leave(pc),
        Step::Helper => plan_helper(layout, pc, decoded, (step_helper(decoded, helpers), helpers)),
        Step::CallOut => plan_helper(layout, pc, decoded, (LaneHelper::VmStep, helpers)),
        Step::Allocate => plan_helper(layout, pc, decoded, (LaneHelper::AllocateStep, helpers)),
        Step::Throw => plan_throw(layout, pc, helpers),
        Step::Catch => plan_catch(decoded, layout, pc, helpers),
        Step::Switch => Err("switch: planned as a compare tree (plan_row)".into()),
        Step::Fast(fast) => plan_guarded(decoded, fast, layout, pc, Some(plan_helper(layout, pc, decoded, (step_helper(decoded, helpers), helpers))?), helpers),
        Step::Guard(fast) => plan_guarded(decoded, fast, layout, pc, None, helpers),
        Step::Return => {
            let register = match decoded.a {
                Some(Source::Register(register)) => register,
                _ => return Err("return: no register".into()),
            };
            let mut parts = completion(layout, pc, register)?;
            let mut first = parts.remove(0);
            first.post = parts;
            Ok(first)
        }
        Step::Call(shape) => plan_call(layout, shape, pc, helpers),
    }
}

/// The helper that makes the step of `decoded`: its instruction's typed helper (`Decoded::own_step`) where the table has
/// it, else the general step.
fn step_helper(decoded: &Decoded, helpers: LaneHelpers) -> LaneHelper {
    decoded.own_step.filter(|helper| decoded.packed.is_none() && call_plan::helper_address(helpers, *helper).is_some()).unwrap_or(LaneHelper::Step)
}

/// The plan of a fast arm or guard of `fast`: an add or a name call has its own planner, the rest
/// share `plan_fast`; `chain` is the helper chain a fast arm falls back to.
fn plan_guarded(decoded: &Decoded, fast: Heap, layout: &Layout, pc: u32, chain: Option<Planned>, helpers: LaneHelpers) -> Result<Planned, String> {
    match fast {
        Heap::Add { .. } => adds::plan(decoded, layout, pc, chain),
        Heap::Name { .. } => names::plan(decoded, layout, pc, chain),
        _ => plan_fast(decoded, fast, layout, pc, chain, helpers),
    }
}

/// The row that ends the region at row `pc`, where the interpreter runs the instruction.
pub(crate) fn leave(pc: u32) -> Result<Planned, String> {
    Ok(Planned::single(call::stencil(CallRow::Here)?, [0; HOLES], None, Some(pc)))
}

/// The chain of `helper` at row `pc`, which names the caller's record it makes: the spills, the helper row and the `reloads` after it. The step helper answers
/// completed, branched or missed (nothing ran: the leave at the record), or left, where the step ran in frames it made
/// of the lane's activations (a frame-needing step of a native callee) and the body leaves past it: the row's
/// (status, word) exit.
pub(crate) fn helper_chain(layout: &Layout, pc: u32, reloads: &[(Place, Register)], (helper, helpers): (LaneHelper, LaneHelpers), packed: Option<[u64; 2]>) -> Result<Planned, String> {
    // A step the planner decoded runs the packed helper (`LaneHelper::StepPacked`), which takes its instruction as is.
    let packed = packed.filter(|_| helper == LaneHelper::Step).and_then(|words| call_plan::helper_address(helpers, LaneHelper::StepPacked).map(|address| (address, words)));
    let (address, packed) = match packed {
        Some((address, words)) => (address, Some(words)),
        None => (call_plan::helper_address(helpers, helper).ok_or("step: no helper address")?, None),
    };
    let (spills, record) = (layout.spills(pc), layout.record(pc));
    chained(call_plan::plan_step(record, address, packed, &spills, reloads)?, spills.len(), Vec::new())
}

/// The pinned registers a row at `pc` reloads before it continues: those live at the next
/// instruction, none where the row completes the activation (nothing runs after it).
fn reloads(layout: &Layout, decoded: &Decoded, pc: u32) -> Vec<(Place, Register)> {
    if decoded.completes { Vec::new() } else { layout.spills(pc + 1) }
}

/// The operands and holes of a `Math.pow` for the leaf call (`call_plan::plan_leaf`): each register operand is read from
/// its frame slot (a pinned register its spill writes to the slot, so `Mem`), each constant operand from its own place
/// and hole (`plan::operand`: `Const` or `Imm`). The destination is a register whose home the spills cover. `None` where
/// the leaf has no address (no helper table names it), or the exponent is a constant `SmallPower` names (the numeric row's
/// product), so the numeric row keeps the operation.
pub(crate) fn leaf_operands(layout: &Layout, decoded: &Decoded, helpers: LaneHelpers) -> Option<([Place; 2], [u64; 3])> {
    if call_plan::helper_address(helpers, LaneHelper::NumberPower)? == call_plan::HelperPlace::Address(0) {
        return None;
    }
    let covered = |register: Register| matches!(layout.homes.place(register), Place::Mem | Place::MemInt | Place::Pin(_) | Place::PinF(_));
    let operand = |source: Option<Source>| -> Option<(Place, u64)> {
        match source? {
            Source::Register(register) => covered(register).then(|| (Place::Mem, super::plan::operand(&layout.homes, Some(Source::Register(register))).1)),
            constant => Some(super::plan::operand(&layout.homes, Some(constant))),
        }
    };
    let dst = decoded.dst.filter(|dst| covered(*dst))?;
    let ((left, left_hole), (right, right_hole)) = (operand(decoded.a)?, operand(decoded.b)?);
    // A constant exponent `SmallPower` names is the row's own product (`power_row`), not the leaf's call.
    let small = match super::plan::operand(&layout.homes, decoded.b) {
        (Place::Imm, hole) => quench_runtime::staged::power::SmallPower::of(f64::from(hole as u32 as i32)),
        (Place::Const, word) => quench_runtime::staged::power::SmallPower::of(f64::from_bits(word)),
        _ => None,
    };
    if small.is_some() {
        return None;
    }
    Some(([left, right], [left_hole, right_hole, super::plan::operand(&layout.homes, Some(Source::Register(dst))).1]))
}

/// The chain of `Math.pow` at `pc` over `operands` and `holes` (`leaf_operands`): the leaf call (`call_plan::plan_leaf`,
/// the `NumberPower` leaf) between the spills of the live pinned registers and the reloads after it.
pub(crate) fn leaf_plan(decoded: &Decoded, layout: &Layout, pc: u32, (operands, holes): ([Place; 2], [u64; 3]), helpers: LaneHelpers) -> Result<Planned, String> {
    let leaf = call_plan::helper_address(helpers, LaneHelper::NumberPower).ok_or("pow: no leaf address")?;
    let (spills, after) = (layout.spills(pc), reloads(layout, decoded, pc));
    chained(call_plan::plan_leaf(pc, leaf, operands, holes, &spills, &after)?, spills.len(), Vec::new())
}

/// The chain of a step at `pc` that `helper` makes, which also branches to the target of its instruction where
/// it jumps.
pub(crate) fn plan_helper(layout: &Layout, pc: u32, decoded: &Decoded, helper: (LaneHelper, LaneHelpers)) -> Result<Planned, String> {
    let row = helper_chain(layout, pc, &reloads(layout, decoded, pc), helper, decoded.packed)?;
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
pub(crate) fn completing(mut run: Planned, layout: &Layout, pc: u32, result: Option<Register>) -> Result<Planned, String> {
    let web = result.ok_or("completion: the instruction writes no result")?;
    run.post.extend(completion(layout, pc, web)?);
    Ok(run)
}

/// The completion of the activation at `pc` with the value `web` holds: a pinned register is
/// spilled to its frame slot first, as the stop returns the slot. Every completion (a returning
/// instruction's, and a returning call's after its site) takes this one path.
fn completion(layout: &Layout, pc: u32, web: Register) -> Result<Vec<Planned>, String> {
    let (place, slot) = operand(&layout.homes, Some(Source::Register(web)));
    let spill = matches!(place, Place::Pin(_) | Place::PinF(_)).then(|| call_plan::spill_part(place, layout.homes.orig[usize::from(web)]).map(Planned::of_part)).transpose()?;
    let stop = Planned { stop: Some(Stop { pc, returns: Some(slot) }), ..Planned::single(control(Kind::Jump)?, [0; HOLES], None, None) };
    Ok(spill.into_iter().chain(std::iter::once(stop)).collect())
}

/// The chain of a `throw` at `pc`: the spills and the helper row, which raises, so nothing is live
/// after it and no registers are reloaded; a helper that did not raise (it ran beside a native
/// callee's frames, where it makes nothing) falls to the leave at the record, as every exit does.
fn plan_throw(layout: &Layout, pc: u32, helpers: LaneHelpers) -> Result<Planned, String> {
    // The typed raise where the table has it, else the general step's.
    let raise = if call_plan::helper_address(helpers, LaneHelper::ThrowStep).is_some() { LaneHelper::ThrowStep } else { LaneHelper::Step };
    let mut row = helper_chain(layout, pc, &[], (raise, helpers), None)?;
    row.post.push(Planned { exits: vec![Exit::At(layout.record(pc))], ..Planned::single(call::stencil(CallRow::Here)?, [0; HOLES], None, None) });
    Ok(row)
}

/// A catch at `pc`: the move of the thrown value to the handler's register, then the jump to the
/// handler's block.
fn plan_catch(decoded: &Decoded, layout: &Layout, pc: u32, helpers: LaneHelpers) -> Result<Planned, String> {
    let handler = decoded.target.ok_or("catch: no handler")?;
    let moved = Decoded::number(Kind::Move, decoded.dst, decoded.a, None, None);
    let mut row = plan(&moved, layout, pc, (true, Variant::Generic), helpers)?;
    row.post.push(jump_to(handler)?);
    Ok(row)
}

/// The word a `typeof` fast arm answers for any Number, which an operand held in a pinned register or an
/// int slot always is: the fact is the operand's representation, proved once where it was defined.
fn number_typeof(fast: Heap, names: u64) -> Option<u64> {
    use quench_runtime::staged::value::{typeof_of, typeof_test_of};
    let number = quench_runtime::typed_ir::Type::NUMBER;
    let index = typeof_of(number)?;
    match fast {
        // SAFETY: the table is the compiling VM's (`Known::typeof_names`), whose strings the VM roots.
        Heap::TypeofString => Some(unsafe { (names as *const u64).add(index as usize).read() }),
        Heap::TypeofIndex => Some(Value::integer(index as i32).to_bits()),
        Heap::TypeofIs { index: tested, negated } => Some(Value::boolean(typeof_test_of(number, u64::from(tested), negated)?).to_bits()),
        _ => None,
    }
}

/// The run of a fast arm at `pc`: the heap row `fast` over the operands' places, whose hit hops past
/// the rest of the run to the next instruction (to the completion where the instruction returns its
/// result), and behind it the slow arm (`slow`: the helper step) the row leaves into where its proof
/// fails, or, with none (`Step::Guard`), the leave at `pc` the row's own miss makes. A row that reads a
/// word of its operand's frame slot (`Heap::reads_frame_word`) with an operand held outside it has the
/// slow arm alone, or the leave, or, where the operand is a Number its representation proves, the constant
/// the arm answers for one (`number_typeof`); so does a result held outside its frame slot. An append's
/// frame slot is its array's, a Number element read from wherever it lives; any other arm writes its
/// destination's.
fn plan_fast(decoded: &Decoded, fast: Heap, layout: &Layout, pc: u32, slow: Option<Planned>, helpers: LaneHelpers) -> Result<Planned, String> {
    let homes = &layout.homes;
    let slot = if fast == Heap::Append { decoded.base } else { decoded.dst };
    let ((read, read_hole), (written, written_hole)) = (operand(homes, decoded.a), operand(homes, slot.map(Source::Register)));
    let (second, second_hole) = operand(homes, decoded.b);
    if written != Place::Mem {
        return slow.map_or_else(|| leave(pc), Ok);
    }
    if let Some(word) = number_typeof(fast, decoded.hole).filter(|_| holds_number(read)) {
        return plan(&Decoded::number(Kind::Move, decoded.dst, Some(Source::Value(word)), None, None), layout, pc, (true, Variant::Generic), helpers);
    }
    if (read != Place::Mem || second != Place::Mem) && fast.reads_frame_word() {
        return slow.map_or_else(|| leave(pc), Ok);
    }
    let row = HeapRow { heap: fast, dst: written, a: read, b: second, entry: false };
    let holes = heap_holes(decoded, homes, fast, false, [written_hole, read_hole, second_hole]);
    let behind_parts = slow.as_ref().map_or(0, |slow| 1 + slow.pre.len() + slow.post.len());
    let mut run = Planned::single(base_rows::stencil(row)?, holes, None, slow.is_none().then_some(pc));
    run.branch = Some(if decoded.completes { Branch::Part(1 + behind_parts) } else { Branch::Pc(pc + 1) });
    Ok(match slow {
        Some(slow) => {
            run.exits = vec![Exit::Ahead(1)];
            behind(run, slow)
        }
        None => run,
    })
}

/// `fast` with the run `slow` laid out behind it: the slow run's parts follow the fast row, so the
/// parts that name another part of their run are renumbered past it.
pub(crate) fn behind(mut fast: Planned, mut slow: Planned) -> Planned {
    let (lead, tail) = (std::mem::take(&mut slow.pre), std::mem::take(&mut slow.post));
    fast.owned = std::mem::take(&mut slow.owned);
    fast.post = lead.into_iter().chain(std::iter::once(slow)).chain(tail).map(|mut part| {
        part.branch = part.branch.map(|branch| match branch {
            Branch::Part(index) => Branch::Part(index + 1),
            pc @ Branch::Pc(_) => pc,
        });
        part.cold = true;
        part
    }).collect();
    fast
}

/// The chain of the call `shape` names at row `pc`: spills, the direct row where the site's target
/// is stable, the helper row, reloads. The direct row's record is the row's `owned`. A plain call's
/// helper is the ending's (`Ending::call_helper`), another form's its own (`CallForm::helper`).
fn plan_call(layout: &Layout, shape: CallShape, pc: u32, helpers: LaneHelpers) -> Result<Planned, String> {
    let helper = if shape.form == CallForm::Plain { layout.ending.call_helper() } else { shape.form.helper() };
    let helper = call_plan::helper_address(helpers, helper).ok_or("call: no helper address")?;
    let (spills, reloads) = (layout.spills(pc), if shape.returning { Vec::new() } else { layout.spills(pc + 1) });
    let proven_args = if layout.inlined(pc) || layout.callees(shape.call_pc).is_empty() { 0 } else { layout.proven_args.at(shape.call_pc as usize) };
    let read = shape.method.map(|method| super::plan::method_read(shape.this, method.cache, method.scratch)).transpose()?;
    let chain = shape.method.map_or_else(Vec::new, |method| super::plan::chain_reads(layout, shape.this, method.cache, method.scratch));
    // The holders the chain's reads answer through: the body subscribes to them, so a change of their values retires it.
    layout.holders.borrow_mut().extend(chain.iter().map(|read| read.holder));
    let plan = CallPlan { shape, direct: layout.callees(shape.call_pc), helper, spills: &spills, reloads: &reloads, proven_args, read, chain, builtin: layout.builtin(shape.call_pc) };
    let CallChain { mut parts, records: record, choice } = call_plan::plan_call(&plan)?;
    // An image's direct rows name the site's record (`name_records`).
    if helpers == LaneHelpers::Ahead && let Some(callee) = layout.callees(shape.call_pc).first() {
        name_records(&mut parts, &record, crate::aot_image::Symbol::CallRecord { function: layout.id, pc: shape.call_pc, callee: callee.function });
    }
    note(|totals, _| if choice == Choice::Direct { totals.direct_calls += 1 } else { totals.helper_calls += 1 });
    #[cfg(test)]
    if choice == Choice::Direct { super::note_fast(shape.op); }
    chained(parts, spills.len(), record)
}

#[path = "steps/branches.rs"]
pub(crate) mod branches;
use branches::{branching, jump_to};

#[cfg(test)]
pub(crate) mod tests;
