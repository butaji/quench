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
//!    Where the instruction's meaning has a fast arm that a register's own word decides
//!    (`staged::iteration`: `RequireIteratorResult`, `ToPropertyKey`, `ForInNext` over its state's memo) or the backing's layout words
//!    (`staged::elements::append_in_room`: a one-element `AppendArray`), the run is that arm's heap row
//!    first (`Step::Fast`, `plan_fast`): its hit hops past the helper step to the next instruction,
//!    and its miss enters the helper step laid out behind it, the slow arm, so the generic step is
//!    only the operation's remainder.
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
use crate::bytecode::{InstructionField, Op, Traits};
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
    /// `Fast` in a region whose heap rows read snapshots (`settle`): a helper's effects would not
    /// reach them, so the slow arm is the leave at the record, which the interpreter runs.
    Guard(Heap),
    /// The region leaves at the record.
    Leave,
    /// The activation completes with the register the row reads (a body's `Return`).
    Return,
    /// A call, made by the call planner's chain (a body's `Call`).
    Call(CallShape),
}

impl Step {
    /// Whether the run makes the step helper's call, alone or as the slow arm of a fast row: the
    /// helper may allocate or run guest code, which no heap snapshot a region took sees.
    pub(super) fn runs_helper(self) -> bool {
        matches!(self, Self::Helper | Self::Fast(_))
    }

    /// Whether the run reads the dispatch payload: a helper step calls through it, and a guarded
    /// fast row reads the tables its heap row names.
    pub(super) fn reads_payload(self) -> bool {
        self.runs_helper() || matches!(self, Self::Guard(_))
    }

    /// Whether control continues at the next instruction: a leave and a return end the run.
    pub(super) fn falls(self) -> bool {
        !matches!(self, Self::Leave | Self::Return)
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

/// The heap row that is the fast arm of `instruction`, where its meaning has one that a register's
/// word (`staged::iteration`) or the backing's own layout words (`elements::append_in_room`) decide.
fn fast_arm(instruction: WideInstruction, known: Known) -> Option<Heap> {
    match instruction.op().class() {
        Class::SetFunctionName => names::of(known, instruction).map(names::Name::heap),
        Class::RequireIteratorResult => Some(Heap::Object),
        Class::ToPropertyKey => Some(Heap::Key),
        Class::AppendArray => appends_one(instruction).then_some(Heap::Append),
        Class::ForInNext => Some(Heap::ForIn),
        _ => None,
    }
}

/// Whether an `AppendArray` is one element (not a spread, not elisions) defined at the literal's
/// length: the case a capacity word decides.
fn appends_one(instruction: WideInstruction) -> bool {
    instruction.array_length() == 0
        && instruction.optional_register_b().is_some()
        && instruction.boolean_field(InstructionField::C) == Some(false)
}

/// The registers a fast arm reads: its operand in `a`, and for an append the array it writes
/// through, the way a store names its array (`Decoded::base`).
fn fast_operands(instruction: WideInstruction, fast: Heap) -> (Option<Source>, Option<Register>) {
    match fast {
        Heap::Add { .. } => adds::operands(instruction),
        Heap::Name { .. } => (Some(Source::Register(instruction.register_a())), None),
        Heap::Append => (instruction.optional_register_b().map(Source::Register), Some(instruction.register_a())),
        _ => (Some(Source::Register(instruction.register_b())), None),
    }
}

/// The lowering of `instruction`, where helpers are `helpers` and `known` holds the sites' facts: a
/// helper step (behind its fast arm, where it has one) where the helper table has the step helper's
/// address, else the leave; an add (`adds`) is a fast arm of its own, whose miss is the helper step
/// where the instruction has one and the leave where it has none (`Step::Guard`).
pub(super) fn lowering(instruction: WideInstruction, known: Known, helpers: LaneHelpers) -> Step {
    let addressed = call_plan::helper_address(helpers, LaneHelper::Step).is_some();
    let in_place = addressed && steps_in_place(instruction);
    match (fast_arm(instruction, known).or_else(|| adds::of(known, instruction).map(adds::Add::heap)), in_place) {
        (Some(fast @ Heap::Add { .. }), false) => Step::Guard(fast),
        (_, false) => Step::Leave,
        (Some(fast), true) => Step::Fast(fast),
        (None, true) => Step::Helper,
    }
}

/// The row of an instruction at `pc` that has no numeric or heap row: a `Nop` is the jump to the
/// next instruction, the rest their step. A helper step names the register it writes, and a fast
/// arm the register it reads; an add row also names its transition and the atom it holds unnamed.
pub(super) fn decode_step(pc: u32, instruction: WideInstruction, known: Known, helpers: LaneHelpers) -> Decoded {
    match instruction.op().class() {
        Class::Nop => Decoded::number(Kind::Jump, None, None, None, Some(pc + 1)),
        _ => {
            let step = lowering(instruction, known, helpers);
            let helped = step.runs_helper();
            let written = if helped { instruction.written_registers().next() } else { None };
            let target = (helped && instruction.op().control_flow_layout().jumps()).then(|| instruction.jump_target());
            let (read, base) = match step {
                Step::Fast(fast) | Step::Guard(fast) => fast_operands(instruction, fast),
                _ => (None, None),
            };
            let add = adds::of(known, instruction).filter(|add| matches!(step, Step::Fast(fast) | Step::Guard(fast) if fast == add.heap()));
            let name = names::of(known, instruction).filter(|name| matches!(step, Step::Fast(fast) | Step::Guard(fast) if fast == name.heap()));
            Decoded { target, a: read, base, add, name, atom: add.map(|_| instruction.atom_index()), ..Decoded::step(step, written) }
        }
    }
}

/// A helper step, alone or behind a fast arm, may write any heap value, which the snapshots a heap row reads (the entry
/// record's captures, fields and array views) would not see: where the region has a heap row its
/// helper steps leave instead. A fast arm stays: its hit stores as a `Store` row does, in the
/// backing the views read live, and only its slow arm becomes the leave (`Step::Guard`).
pub(super) fn settle(decoded: &mut [Decoded]) {
    if decoded.iter().any(|row| row.heap.is_some_and(Heap::reads_snapshot)) {
        for row in decoded.iter_mut() {
            row.step = match row.step {
                Some(Step::Fast(fast)) => Some(Step::Guard(fast)),
                Some(step) if step.runs_helper() => Some(Step::Leave),
                step => step,
            };
        }
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
        Step::Leave => leave(pc),
        Step::Helper => plan_helper(layout, pc, decoded.target, helpers),
        Step::Fast(Heap::Add { .. }) => adds::plan(decoded, layout, pc, Some(plan_helper(layout, pc, None, helpers)?)),
        Step::Guard(Heap::Add { .. }) => adds::plan(decoded, layout, pc, None),
        Step::Fast(Heap::Name { .. }) => names::plan(decoded, layout, pc, Some(plan_helper(layout, pc, None, helpers)?)),
        Step::Guard(Heap::Name { .. }) => names::plan(decoded, layout, pc, None),
        Step::Fast(fast) => plan_fast(decoded, fast, layout, pc, Some(plan_helper(layout, pc, decoded.target, helpers)?)),
        Step::Guard(fast) => plan_fast(decoded, fast, layout, pc, None),
        Step::Return => {
            let slot = operand(&layout.homes, decoded.a).1;
            Ok(Planned { stop: Some(Stop { pc, returns: Some(slot) }), ..Planned::single(control(Kind::Jump)?, [0; HOLES], None, None) })
        }
        Step::Call(shape) => plan_call(layout, shape, helpers),
    }
}

/// The row that ends the region at `pc`, where the interpreter runs the instruction.
pub(super) fn leave(pc: u32) -> Result<Planned, String> {
    Ok(Planned::single(call::stencil(CallRow::Here)?, [0; HOLES], None, Some(pc)))
}

/// The chain of a helper step at `pc`, which also branches to `target` where its instruction jumps.
fn plan_helper(layout: &Layout, pc: u32, target: Option<u32>, helpers: LaneHelpers) -> Result<Planned, String> {
    let address = call_plan::helper_address(helpers, LaneHelper::Step).ok_or("step: no helper address")?;
    let (spills, reloads) = (layout.spills(pc), layout.spills(pc + 1));
    let mut row = chained(call_plan::plan_step(pc, address, &spills, &reloads)?, spills.len(), None)?;
    // The step helper answers completed, branched or missed: its (status, word) exit is the leave at the record too.
    row.exits = vec![Exit::At(pc); row.exits.len()];
    match target {
        Some(target) => branching(row, layout, (pc, target)),
        None => Ok(row),
    }
}

/// The run of a fast arm at `pc`: the heap row `fast` over the operand's frame slot, whose hit hops
/// past the rest of the run to the next instruction, and behind it the slow arm (`slow`: the helper
/// step) the row leaves into where its proof fails, or, with none (`Step::Guard`), the leave at `pc`
/// the row's own miss makes. An operand or result held outside
/// its frame slot (a pinned register holds a Number, which no fast arm takes but an append's
/// element) has the slow arm alone, or the leave. An append's frame slot is its array's, a Number element read
/// from wherever it lives; any other arm writes its destination's.
fn plan_fast(decoded: &Decoded, fast: Heap, layout: &Layout, pc: u32, slow: Option<Planned>) -> Result<Planned, String> {
    let homes = &layout.homes;
    let appends = fast == Heap::Append;
    let (read, read_hole) = operand(homes, decoded.a);
    let slot = if appends { decoded.base } else { decoded.dst };
    let (written, written_hole) = operand(homes, slot.map(Source::Register));
    if written != Place::Mem || (read != Place::Mem && !appends) {
        return slow.map_or_else(|| leave(pc), Ok);
    }
    let row = HeapRow { heap: fast, dst: written, a: read, b: Place::Mem, entry: false };
    let mut run = Planned::single(base_rows::stencil(row)?, [written_hole, read_hole, 0], Some(pc + 1), slow.is_none().then_some(pc));
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
pub(super) fn behind(mut fast: Planned, mut slow: Planned) -> Planned {
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

    /// The facts of a program that has recorded nothing: no add, no store in place, no fold.
    fn known(program: &ResidualProgram) -> Known<'_> {
        Known { program, stores: None, reads: None, adds: None, names: None, invalidation: None }
    }

    fn empty_program() -> ResidualProgram {
        crate::Engine::specialize("0", "known.js").expect("a program")
    }

    /// Every opcode has a lowering, and the helper steps are exactly the in-place instructions and
    /// the call and construction helpers' own.
    #[test]
    fn every_opcode_is_lowered() {
        let process = crate::vm::compiled::process_lane_helpers::<crate::test_host::TestHost>();
        let program = empty_program();
        let mut counts = [0usize; 2];
        for index in 0..Op::COUNT {
            let op = Op::from_index(index).expect("an opcode");
            let step = lowering(instruction(op), known(&program), process);
            counts[usize::from(step.runs_helper())] += 1;
            if op.traits().has(Traits::LANE_IN_PLACE) && instruction(op).written_registers().count() <= 1 {
                assert!(step.runs_helper(), "{op:?}");
            }
        }
        assert_eq!(counts[0] + counts[1], Op::COUNT);
        assert!(counts[1] > 0 && counts[0] > 0);
    }

    /// Without a helper table that has addresses (a linked or symbolic one) an instruction leaves:
    /// still lowered, never refused.
    #[test]
    fn without_helper_addresses_every_opcode_leaves() {
        let program = empty_program();
        for index in 0..Op::COUNT {
            let op = Op::from_index(index).expect("an opcode");
            assert_eq!(lowering(instruction(op), known(&program), LaneHelpers::Ahead), Step::Leave, "{op:?}");
        }
    }

    /// The opcodes whose only lowering is the generic helper step, each for the reason beside it:
    /// no fast arm a register's word, a shape or a backing the row can read decides, so the helper
    /// is their whole operation, not a slow arm.
    const RESIDUAL: [Op; 19] = [
        // The define adds a property: where its site recorded the transition (`Known::adds`) its staged row (`staged::add`) is the fast arm (`adds`) and this helper its slow arm; with none recorded, the helper is the whole operation.
        Op::DefineField,
        // The key is converted (guest code) and the property defined through the shape's slow path.
        Op::DefineComputedField,
        // The private atom is the running environment's home's (`Vm::private_name_atom`), resolved per environment and so no site fact; the add is then a shape transition.
        Op::DefinePrivateField,
        // The brand check resolves the private atom through the environment, then asks the shape (`Vm::has_private_slot`).
        Op::CheckPrivate,
        // The brand test resolves the private atom through the environment, then asks the shape.
        Op::PrivateIn,
        // Brands the object by a shape transition keyed by the home's private atom (`private_slots`); the home's declaration is VM state.
        Op::MarkPrivateName,
        // Declares a fresh private atom in the home's private environment: VM state, no shape or register fact.
        Op::DeclarePrivateName,
        // Names any function the site's recorded edge (`names`) does not: its general naming also records that edge.
        Op::SetFunctionName,
        // Builds the name string from a key and a prefix: allocates.
        Op::SetFunctionNameKey,
        // Edits VM-held function provenance only.
        Op::SetFunctionDebugName,
        // Defines through the sparse-or-dense element store, which allocates and grows the backing.
        Op::DefineArrayElement,
        // A spread iterates (guest code) and elisions set a sparse length; only the one-element append has a fast arm (the capacity word), and this helper is its slow arm, which grows the backing.
        Op::AppendArray,
        // Enumerates the source's keys and runs guest code (getters, proxies).
        Op::CopyDataProperties,
        // Allocates the iterator record and may run guest code.
        Op::GetIterator,
        // Takes the key snapshot: allocates the state, memoized by VM-held for-in state.
        Op::ForInStart,
        // The fast arm (`staged::iteration::for_in_next`) steps a state whose memo proves the snapshot current; this helper is its slow arm, which takes any other step and the exhausted one's jump.
        Op::ForInNext,
        // Calls `return` on the iterator: guest code.
        Op::IteratorClose,
        // Pushes onto the frame's VM-held active-iterator stack.
        Op::IteratorCleanupPush,
        // Pops the frame's VM-held active-iterator stack.
        Op::IteratorCleanupPop,
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
                    let row = decode_step(0, instruction, known(&program), process);
                    if row.step == Some(Step::Helper) {
                        assert!(RESIDUAL.contains(&instruction.op()), "{path:?}: {:?} takes the generic helper", instruction.op());
                        assert_eq!(row.target.is_some(), instruction.op().control_flow_layout().jumps(), "{:?}", instruction.op());
                    }
                }
            }
        }
    }

    /// A one-element append is a fast row over the backing's capacity word; a spread, an elision count
    /// or no element leave the generic helper step alone.
    #[test]
    fn a_one_element_append_has_a_fast_row_and_a_spread_the_helper_alone() {
        use crate::bytecode::encode_optional_register;
        let process = crate::vm::compiled::process_lane_helpers::<crate::test_host::TestHost>();
        let program = empty_program();
        let append = |element: Option<u16>, spread: u16, elisions: u32| {
            lowering(WideInstruction::new(Op::AppendArray, 1, encode_optional_register(element), spread, elisions), known(&program), process)
        };
        assert_eq!(append(Some(2), 0, 0), Step::Fast(Heap::Append));
        assert_eq!(append(Some(2), 1, 0), Step::Helper);
        assert_eq!(append(Some(2), 0, 1), Step::Helper);
        assert_eq!(append(None, 0, 2), Step::Helper);
        let row = decode_step(0, WideInstruction::new(Op::AppendArray, 1, encode_optional_register(Some(2)), 0, 0), known(&program), process);
        assert!(matches!(row.a, Some(Source::Register(2))) && row.base == Some(1) && row.dst.is_none());
    }

    /// Where a region's heap rows read snapshots a helper's effects would not reach, a fast arm stays
    /// and its slow arm becomes the leave, while a helper step leaves; elsewhere both are kept.
    #[test]
    fn a_snapshot_region_keeps_its_fast_arms_and_leaves_its_helper_steps() {
        let rows = |reader: bool| {
            let mut rows = vec![Decoded::step(Step::Fast(Heap::Append), None), Decoded::step(Step::Helper, None)];
            if reader {
                rows.push(Decoded::heap(Heap::Field, Some(0), None, None));
            }
            settle(&mut rows);
            [rows[0].step, rows[1].step]
        };
        assert_eq!(rows(true), [Some(Step::Guard(Heap::Append)), Some(Step::Leave)]);
        assert_eq!(rows(false), [Some(Step::Fast(Heap::Append)), Some(Step::Helper)]);
        assert!(Step::Guard(Heap::Append).reads_payload() && !Step::Guard(Heap::Append).runs_helper());
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
        let program = empty_program();
        let row = decode_step(7, instruction(Op::Nop), known(&program), LaneHelpers::Ahead);
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
                    let row = decode_step(0, instruction, known(&program), process);
                    assert!(row.step.is_some() || row.target.is_some(), "{path:?}: {:?}", instruction.op());
                    instructions += 1;
                }
            }
        }
        assert!(fixtures > 0 && instructions > 0);
    }
}
