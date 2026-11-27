//! The fixpoint of `transfer` over the control-flow graph, and the replay of its rows: the one
//! analysis, which every consumer of a type, a range or a length relation reads.

#[allow(unused_imports)] use quench_runtime::staged::{OperationOperationRecordsExt};
#[allow(unused_imports)]
use crate::ext_traits::*;
use super::lengths::Relations;
use super::ranges::{self, Edge};
use super::{hold_contracts, join_types, meet_relations, set, transfer_observed, Contract, Facts, Flow, Observed, INLINE_ANALYSIS_ITEMS};
use quench_runtime::bytecode::{Constant, ControlFlowLayout, Register, ResidualProgram, WideInstruction};
use quench_runtime::typed_ir::{IntRange, Type, WIDENING_ROUNDS};
use std::cell::{Cell, RefCell};
use quench_runtime::vm::operations::NumberRelation;

/// The abstract state of a run at one point: the registers' types (a range is a type), and the
/// length relations a unit-proving run tracks beside them.
#[derive(Clone)]
pub(crate) struct Point {
    pub(crate) types: smallvec::SmallVec<[Type; INLINE_ANALYSIS_ITEMS]>,
    pub(crate) relations: Relations,
}

/// The unit a run proves the ranges of: its inlined frames, whose arguments the markers pass,
/// and the element accesses whose index it bounds.
#[derive(Clone, Copy)]
pub(crate) struct Model<'a> {
    pub(crate) program: &'a ResidualProgram,
    pub(crate) unit: &'a super::super::lane_unit::Unit,
    /// The PCs a previous body left at (`Lowering::generic`): their rows are unknown, and their
    /// instructions run guest code.
    pub(crate) generic: &'a [usize],
    /// Whether the element access at a PC reads or writes a hoisted view, whose miss leaves the
    /// body (`Lowering::hoisted_access`).
    pub(crate) dense: &'a dyn Fn(usize) -> bool,
    /// What the access at a PC does through a hoisted typed view (`Lowering::typed_access`), where it is
    /// one that runs no guest code and changes no array's length (`TypedAccess`).
    pub(crate) typed: &'a dyn Fn(usize) -> Option<TypedAccess>,
}

/// An instruction on a hoisted typed view whose effect on lengths is known (`Model::typed`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum TypedAccess {
    /// A `length` read of a register's typed array, which the view answers natively.
    Length,
    /// An element store through a typed or double view: it changes no length, and runs guest
    /// code only where its value is no Number (`Relations::step`).
    Store,
}

/// What a run assumes beyond the program text.
#[derive(Clone, Copy)]
pub(crate) struct Seeds<'a> {
    /// The guarded block input contracts the fixed point keeps.
    pub(crate) contracts: &'a [Contract],
    /// The element reads observed or known to hold integers or Numbers.
    pub(crate) observed: Observed<'a>,
    /// The registers the entry guards to a range narrower than int32 (`Lowering::guard_entering_ranges`).
    pub(crate) entering: &'a [(Register, IntRange)],
    /// The unit the run proves ranges of; a type-only run has none and tracks no relations.
    pub(crate) model: Option<Model<'a>>,
    /// The PCs (ascending) of the products no consumer reads the sign of a zero of
    /// (`Lowering::signless_products`): where operands of ranges make a negative zero possible,
    /// the result is still an integer of their product's range.
    pub(crate) signless: &'a [usize],
}

impl<'a> Seeds<'a> {
    /// A type-only run that takes `observed` reads and no contract.
    pub(crate) fn observing(observed: Observed<'a>) -> Self {
        Self { contracts: &[], observed, entering: &[], model: None, signless: &[] }
    }
}

/// The program text a run reads.
pub(crate) struct Text<'a> {
    pub(crate) entry: usize,
    pub(crate) instructions: &'a [WideInstruction],
    pub(crate) leaders: &'a quench_runtime::dataflow::BitSet,
    pub(crate) constants: &'a [Constant],
    pub(crate) written: &'a dyn Fn(WideInstruction) -> super::Writes,
    pub(crate) own_shape: &'a dyn Fn(usize) -> Option<(Register, u32)>,
    pub(crate) native_calls: bool,
    /// The exception edges native code takes (`lane_region::catches`), ascending by PC.
    pub(crate) raises: &'a [Raise],
}

/// An exception edge: the raising instruction's PC, its handler's block and the register the
/// handler binds the thrown value to, if any. The handler is entered in the state before the
/// instruction, a call's register untouched, with the bound register holding any value.
pub(crate) type Raise = (usize, usize, Option<Register>);

impl Point {
    /// The state at the entry: the checked inputs, an int32 one taking the range the entry
    /// guards where it has one.
    fn entering(initial: &[Type], seeds: Seeds<'_>) -> Self {
        let mut types: smallvec::SmallVec<[Type; INLINE_ANALYSIS_ITEMS]> = initial.iter().copied().collect();
        for &(register, range) in seeds.entering {
            if let Some(slot) = types.get_mut(usize::from(register))
                && let Some(narrowed) = slot.int_range().and_then(|known| known.intersect(range)) {
                *slot = Type::int(narrowed);
            }
        }
        let relations = seeds.model.map_or_else(Relations::default, |_| Relations::new(initial.len()));
        Self { types, relations }
    }

    /// The state on the edge of the comparison `instruction` that is taken where it is `truth`.
    /// An edge no value reaches keeps what it has: the lowering still branches to it.
    fn refine(&mut self, constants: &[Constant], instruction: WideInstruction, truth: bool) {
        let Some(relation) = NumberRelation::from_operator(instruction.binary_operator_field()) else { return };
        let operands = [instruction.operand_b(), instruction.operand_c()];
        self.relations.bound(ranges::effective_relation(relation, truth), operands);
        let Edge::Refined(refined) = Edge::of(&ranges::lookup(&self.types, constants), relation, truth, operands) else { return };
        for (operand, range) in operands.into_iter().zip(refined) {
            if let Some(register) = operand.register_index() { set(&mut self.types, register, Type::int(range)); }
        }
    }

    /// Whether the int32 arithmetic or step of `instruction` is exact in int32 here, whatever
    /// int32 operands reach it.
    pub(crate) fn overflow_free(&self, constants: &[Constant], instruction: WideInstruction) -> bool {
        ranges::exact(&ranges::lookup(&self.types, constants), instruction).is_some_and(|range| range.fits_int32())
    }

    /// Whether `register` holds a Number at this point on every path into it (`Type::NUMBER`).
    pub(crate) fn holds_number(&self, register: Register) -> bool {
        self.types.get(usize::from(register)).is_some_and(|ty| ty.is_subtype(Type::NUMBER))
    }

    /// Whether `register` holds an int32 at this point on every path into it (`Type::INT32`).
    pub(crate) fn holds_int32(&self, register: Register) -> bool {
        self.types.get(usize::from(register)).is_some_and(|ty| ty.is_subtype(Type::INT32))
    }

    /// The range of the result of the arithmetic of `instruction` where it leaves int32 but is
    /// an integer Number of `IntRange::WORD` (`ranges::index_range`).
    pub(crate) fn index_range(&self, constants: &[Constant], instruction: WideInstruction, signless: bool) -> Option<IntRange> {
        ranges::index_range(&ranges::lookup(&self.types, constants), instruction, signless)
    }

    /// Whether the product of `instruction` is no negative zero, whatever operands reach it.
    pub(crate) fn zero_free(&self, constants: &[Constant], instruction: WideInstruction) -> bool {
        ranges::zero_free(&ranges::lookup(&self.types, constants), instruction)
    }
}

impl Model<'_> {
    /// The inlined call whose marker is `pc` initializes the callee's registers: its window is
    /// unknown, and each passed argument is a copy of the caller's register.
    fn enter_frame(&self, pc: usize, point: &mut Point) {
        let Some(frame) = self.unit.markers.iter().position(|marker| *marker == pc).map(|index| self.unit.frames[index]) else { return };
        for register in frame.base..frame.base + frame.registers {
            set(&mut point.types, register, Type::ANY);
            point.relations.kill(register);
        }
        for (to, from) in frame.passed_registers(self.program.function(frame.target.function as usize)) {
            let copied = point.types.get(usize::from(from)).copied().unwrap_or(Type::ANY);
            set(&mut point.types, to, copied);
            point.relations.copy(to, from);
        }
    }
}

/// The state after the instruction at `pc` of a run, and where control goes.
fn step(point: &mut Point, text: &Text<'_>, seeds: Seeds<'_>, pc: usize) -> Flow {
    let instruction = text.instructions[pc];
    if let Some(model) = seeds.model {
        model.enter_frame(pc, point);
        // A store of a Number runs no guest code (its value and key convert without a call).
        let numeric_store = point.numeric_store(instruction);
        let lookup = ranges::lookup(&point.types, text.constants);
        point.relations.step(&model, pc, instruction, text.written, &lookup, numeric_store);
    }
    // A direct own read proves its receiver before defining its result.
    // If result and receiver alias, transfer kills that refinement.
    if let Some((register, shape)) = (text.own_shape)(pc) { set(&mut point.types, register, Type::direct_object(shape)); }
    let signless = seeds.signless.binary_search(&pc).is_ok()
        .then(|| ranges::signless_range(&ranges::lookup(&point.types, text.constants), instruction)).flatten();
    let flow = transfer_observed(&mut point.types, instruction, text.constants, text.written, text.native_calls, seeds.observed.at(pc));
    if let Some(range) = signless { set(&mut point.types, instruction.result_register(), Type::int(range)); }
    flow
}

/// The block led by `leader` run from `point`, where `visit` sees the state before each
/// instruction; the PC of its last instruction and where control goes from it.
fn run_block(point: &mut Point, text: &Text<'_>, seeds: Seeds<'_>, leader: usize,
    mut visit: impl FnMut(usize, &Point)) -> (usize, Flow) {
    let mut pc = leader;
    loop {
        visit(pc, point);
        let flow = step(point, text, seeds, pc);
        let next = pc + 1;
        if !matches!(flow, Flow::Next) || next >= text.instructions.len() || text.leaders.contains(next) { return (pc, flow); }
        pc = next;
    }
}

impl Facts {
    /// Each reachable block replayed from its row, as the fixed point ran it: `visit` sees the
    /// state before each instruction.
    pub(crate) fn replay(&self, text: &Text<'_>, seeds: Seeds<'_>, mut visit: impl FnMut(usize, &Point)) {
        for start in (0..text.instructions.len()).filter(|pc| self.at(*pc).is_some()) {
            let mut point = self.point_at(start);
            run_block(&mut point, text, seeds, start, &mut visit);
        }
    }
}

/// The fixpoint of `transfer` over the CFG, seeded by checked region-entry facts.
pub(crate) fn analyze(entry: usize, initial: &[Type], instructions: &[WideInstruction],
    leaders: &quench_runtime::dataflow::BitSet, constants: &[Constant], written: &dyn Fn(WideInstruction) -> super::Writes,
    own_shape: &dyn Fn(usize) -> Option<(Register, u32)>, native_calls: bool, raises: &[Raise], seeds: Seeds<'_>) -> Facts {
    analyze_text(&Text { entry, instructions, leaders, constants, written, own_shape, native_calls, raises }, initial, seeds)
}

/// A block's input as `solve` holds it: the row of the block led by `heads[block]`, its types
/// and the length relations the run keeps (empty where it tracks none).
#[derive(Clone, PartialEq)]
struct Row {
    block: usize,
    types: smallvec::SmallVec<[Type; INLINE_ANALYSIS_ITEMS]>,
    length_of: Vec<Option<Register>>,
    below: Vec<Option<Register>>,
    under: Vec<Option<Register>>,
    atmost: Vec<Option<Register>>,
    twice: Vec<Option<Register>>,
}

impl Row {
    fn of(block: usize, point: Point) -> Self {
        let Relations { length_of, below, under, atmost, twice, .. } = point.relations;
        Self { block, types: point.types, length_of, below, under, atmost, twice }
    }

    /// The state on entry to the block: its relations name the registers their rows do.
    fn point(&self) -> Point {
        Point { types: self.types.clone(),
            relations: Relations::of_rows(self.length_of.clone(), self.below.clone(), self.under.clone(), self.atmost.clone(), self.twice.clone()) }
    }
}

/// The fixpoint of `transfer` as a forward problem of `dataflow::solve`. Its blocks are the PCs
/// a row may be held at, ascending, so the driver's lowest-first order is the bytecode's (the
/// lowest pending leader is the next in reverse postorder, but for backedges). A block runs to
/// the first instruction that does not continue to the next or whose next leads a block, as the
/// lowering does, so where it ends depends on its state: `transfer` records the edges it took,
/// each with its own state, the exception edges of the instructions it passed included, in the
/// order the rows join them (the jump target, the fallthrough, then each raise by PC).
struct FactFlow<'t, 'a, 's> {
    text: &'t Text<'a>,
    seeds: Seeds<'s>,
    heads: Vec<usize>,
    entry: Option<Point>,
    /// How often a join changed each block's row: past `WIDENING_ROUNDS` a range that grows widens,
    /// at a loop header alone (`headers`): a block inside the loop follows the header's finite rows
    /// exactly, so the branch that guards it keeps the bound it proves.
    rounds: Vec<Cell<u8>>,
    /// The blocks a backward edge reaches, each marked as its edge is taken.
    headers: Vec<Cell<bool>>,
    reached: quench_runtime::dataflow::BitSet,
    /// Join of the entry's backedge inputs, without the unknown external input.
    backedge: smallvec::SmallVec<[Type; INLINE_ANALYSIS_ITEMS]>,
    /// The edges the last `transfer` took: their target blocks, and each one's state and whether
    /// it is a backward edge, which `transfer_edge_at` hands over.
    targets: Vec<usize>,
    sends: RefCell<Vec<(Option<Row>, bool)>>,
    raised: smallvec::SmallVec<[(usize, usize, Point); 2]>,
}

impl FactFlow<'_, '_, '_> {
    fn block_of(&self, pc: usize) -> Option<usize> {
        self.heads.binary_search(&pc).ok()
    }

    /// The edge to `pc` in the state `point`, backward where `backward`. An edge past the end
    /// reaches no row.
    fn send(&mut self, pc: usize, point: Point, backward: bool) {
        if pc >= self.text.instructions.len() { return; }
        let block = self.block_of(pc);
        debug_assert!(block.is_some(), "an edge's target PC {pc} leads no block");
        let Some(block) = block else { return };
        self.targets.push(block);
        self.sends.get_mut().push((Some(Row::of(block, point)), backward));
    }

    /// The jump at `pc` to `target`, and the fallthrough of a conditional one, each edge in the
    /// state it proves.
    fn branch(&mut self, (pc, target): (usize, usize), mut point: Point) {
        let text = self.text;
        let instruction = text.instructions[pc];
        let conditional = instruction.op().control_flow_layout() == ControlFlowLayout::ConditionalJump;
        let comparison = quench_runtime::staged::meaning::means(instruction, quench_runtime::staged::meaning::Meaning::CompareJump);
        // Only a comparison refines its edges apart; any other conditional jump sends one state
        // both ways.
        let refines = conditional && comparison;
        let mut following = refines.then(|| point.clone());
        if comparison {
            point.refine(text.constants, instruction, false);
            if let Some(following) = following.as_mut() { following.refine(text.constants, instruction, true); }
        }
        if target == text.entry && target <= pc { self.join_backedge(&point.types); }
        match following {
            Some(following) => {
                self.send(target, point, target <= pc);
                self.send(pc + 1, following, false);
            }
            None if conditional => {
                self.send(target, point.clone(), target <= pc);
                self.send(pc + 1, point, false);
            }
            None => self.send(target, point, target <= pc),
        }
    }

    /// The join of the entry's backedge inputs, without the unknown external input.
    fn join_backedge(&mut self, types: &[Type]) {
        if self.backedge.is_empty() { self.backedge.extend_from_slice(types); return; }
        for (known, incoming) in self.backedge.iter_mut().zip(types) { *known = known.join(*incoming); }
    }

    /// What a join that `changed` the row of `block` does beside: it counts toward the block's
    /// widening, and a block a previous body left at forgets its types.
    fn joined(&self, row: &mut Row, changed: bool) {
        if !changed { return; }
        let rounds = &self.rounds[row.block];
        rounds.set(rounds.get().saturating_add(1));
        if self.seeds.model.is_some_and(|model| model.generic.contains(&self.heads[row.block])) {
            row.types.fill(Type::ANY);
        }
    }
}

impl quench_runtime::dataflow::Dataflow for FactFlow<'_, '_, '_> {
    type State = Row;

    fn blocks(&self) -> usize {
        self.heads.len()
    }

    fn edges(&self, _block: usize) -> &[usize] {
        &self.targets
    }

    fn entry_state(&self, block: usize) -> Option<Row> {
        if self.heads[block] != self.text.entry { return None; }
        let point = self.entry.clone()?;
        Some(self.admit(block, Row::of(block, point)))
    }

    fn admit(&self, block: usize, mut incoming: Row) -> Row {
        hold_contracts(self.seeds.contracts, self.heads[block], &mut incoming.types);
        self.joined(&mut incoming, true);
        incoming
    }

    fn join(&self, into: &mut Row, incoming: &Row) -> bool {
        let block = into.block;
        let widening = self.headers[block].get() && self.rounds[block].get() >= WIDENING_ROUNDS;
        let mut changed = join_types(&mut into.types, &incoming.types, widening);
        changed |= meet_relations([&mut into.length_of, &mut into.below, &mut into.under, &mut into.atmost, &mut into.twice],
            [incoming.length_of.as_slice(), incoming.below.as_slice(), incoming.under.as_slice(), incoming.atmost.as_slice(), incoming.twice.as_slice()]);
        changed |= hold_contracts(self.seeds.contracts, self.heads[block], &mut into.types);
        self.joined(into, changed);
        changed
    }

    fn transfer(&mut self, block: usize, state: &Row) -> Row {
        let (text, seeds) = (self.text, self.seeds);
        self.targets.clear();
        self.sends.get_mut().clear();
        let mut point = state.point();
        let mut raised = std::mem::take(&mut self.raised);
        let reached = &mut self.reached;
        let ended = run_block(&mut point, text, seeds, self.heads[block], |pc, state| {
            reached.insert(pc);
            let first = text.raises.partition_point(|raise| raise.0 < pc);
            for &(_, target, bound) in text.raises[first..].iter().take_while(|raise| raise.0 == pc) {
                let mut caught = state.clone();
                if let Some(register) = bound {
                    set(&mut caught.types, register, Type::ANY);
                    caught.relations.kill(register);
                }
                raised.push((pc, target, caught));
            }
        });
        match ended {
            (_, Flow::Leaves) => {}
            (last, Flow::Next) => self.send(last + 1, point, false),
            (last, Flow::To(target)) => self.branch((last, target), point),
            (last, Flow::Table) => for entry in quench_runtime::bytecode::branch_table_entries(last, text.instructions[last]) {
                self.send(entry, point.clone(), entry <= last);
            },
        }
        for (pc, target, caught) in raised.drain(..) {
            self.send(target, caught, target <= pc);
        }
        self.raised = raised;
        // The edges carry their own states (`transfer_edge_at`).
        Row { block, types: smallvec::SmallVec::new(), length_of: Vec::new(), below: Vec::new(), under: Vec::new(), atmost: Vec::new(), twice: Vec::new() }
    }

    fn transfer_edge_at(&self, _block: usize, index: usize, _out: &Row) -> Row {
        let (row, backward) = std::mem::take(&mut self.sends.borrow_mut()[index]);
        let row = row.expect("each edge is taken once per transfer");
        if backward { self.headers[row.block].set(true); }
        row
    }
}

/// The PCs a row may be held at: the entry, the leaders, every jump's target and conditional
/// fallthrough, and every handler an exception edge enters.
fn heads(text: &Text<'_>) -> Vec<usize> {
    let length = text.instructions.len();
    let mut heads: quench_runtime::dataflow::BitSet = text.leaders.clone();
    let mut add = |pc: usize| if pc < length { heads.insert(pc); };
    add(text.entry);
    for (pc, instruction) in text.instructions.iter().enumerate() {
        let layout = instruction.op().control_flow_layout();
        if layout.jumps() || quench_runtime::staged::meaning::means(instruction, quench_runtime::staged::meaning::Meaning::CompareJump) { add(instruction.jump_target() as usize); }
        if layout == ControlFlowLayout::ConditionalJump { add(pc + 1); }
    }
    for &(_, target, _) in text.raises { add(target); }
    heads.iter().filter(|&pc| pc < length).collect()
}

/// `analyze` over `text`.
pub(crate) fn analyze_text(text: &Text<'_>, initial: &[Type], seeds: Seeds<'_>) -> Facts {
    let length = text.instructions.len();
    let heads = heads(text);
    let blocks = heads.len();
    let mut flow = FactFlow {
        text, seeds,
        entry: (text.entry < length).then(|| Point::entering(initial, seeds)),
        rounds: (0..blocks).map(|_| Cell::new(0)).collect(),
        headers: (0..blocks).map(|_| Cell::new(false)).collect(),
        reached: quench_runtime::dataflow::BitSet::with_capacity(length),
        backedge: smallvec::SmallVec::new(),
        targets: Vec::new(),
        sends: RefCell::new(Vec::new()),
        raised: smallvec::SmallVec::new(),
        heads,
    };
    let rows = quench_runtime::dataflow::solve(&mut flow);
    let mut facts = Facts::new(length, initial.len(), seeds);
    facts.reached = flow.reached;
    facts.backedge = flow.backedge;
    for (row, pc) in rows.into_iter().zip(flow.heads) {
        let Some(row) = row else { continue };
        facts.store_row(pc, &row.types, [&row.length_of, &row.below, &row.under, &row.atmost, &row.twice]);
    }
    facts
}
