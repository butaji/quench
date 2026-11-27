//! The register dataflow of one function (`dataflow::solve` over its basic blocks): each
//! register's fact at each point, and what the function does that the closed-world facts read.

use super::blocks::Blocks;
use super::effects::{Before, Ctx, settled, step};
use super::lengths::{self, Lengths};
use super::record::Record;
use super::registers::{RegisterCells, Source};
use crate::staged::typing;
use super::scope::Scope;
use super::shape::Shape;
use super::value::{Key, Origin, Word};
use crate::bytecode::{ControlEdge, Function, Operand, OperandKind, Successors, WideInstruction, used_registers};
use crate::dataflow::{BitSet, Dataflow, solve};
use super::quiet::Quiet;
use crate::staged::{Comparison, NumberOperation, Operation};
use crate::typed_ir::{Cell, Classes, ElementClass, Elements, IntRange, Representation, Type, WIDENING_ROUNDS, language_bit};
use crate::vm::operations::NumberBitwise;
use crate::ValueType;

/// What a call names as its callee (`staged::Callee`): a register, whose fact the census resolves; a
/// function of the program the record names; or a method of its receiver, by name.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Target {
    Register,
    Known(u32),
    Method(u32),
}

/// A call or a `new` a function makes, with its callee's fact and its arguments' facts.
pub(super) struct Call {
    pub(super) pc: u32,
    /// The fact of the callee register: for a `Target::Register` call only, unknown otherwise.
    pub(super) callee: Word,
    pub(super) target: Target,
    pub(super) construct: bool,
    /// The fact of the `this` it passes its callee (`staged::Receiver`): a register's, the caller's own
    /// `this` or a field of it, an allocated object's; unknown for any other receiver.
    pub(super) receiver: Word,
    /// The facts of the arguments it passes (`staged::Invocation::arguments`), in order.
    pub(super) arguments: Vec<Word>,
}

/// What one function contributes to the closed-world facts.
pub(super) struct FunctionFlow {
    pub(super) calls: Vec<Call>,
    /// Every binding write, with the fact of the value written.
    pub(super) writes: Vec<(Key, Word)>,
    /// Whether a write the analysis cannot follow reaches the function's own slots.
    pub(super) wild_owner: bool,
    /// Whether a write the analysis cannot follow reaches the globals.
    pub(super) global_wild: bool,
    /// Whether a store through a non-Number key may write a name of the global object (`effects::writes_global_name`).
    pub(super) computed_global: bool,
    /// Whether the function may make the global object reachable as a value (`Events::global_source`, sloppy plain calls,
    /// `call`/`apply`/`bind`). The proofs of a program with no such source keep the global object unreached.
    pub(super) global_source: bool,
    /// The callee of each call the function makes with no receiver: its function id, if it names one, else None.
    pub(super) plain_calls: Vec<Option<u32>>,
    /// Whether the function reads its `this` (a load, a store or a field read of it, or a method of it).
    pub(super) reads_this: bool,
    /// Whether a store through a key the analysis cannot name may write a named property.
    pub(super) member_wild: bool,
    /// Per register, the Number representation it holds at every read and write.
    pub(super) numbers: Box<[Option<Representation>]>,
    /// Per named field read or store, by PC, the class of the object it reads or stores (`effects::field_owner`).
    pub(super) field_owners: std::collections::BTreeMap<u32, crate::typed_ir::Classes>,
    /// The register cells of the function at every point, for the tiers that lower it (`RegisterCells`).
    pub(super) registers: RegisterCells,
    /// The keys the settled cells of the function were read from: binding loads, field reads (a class's own,
    /// or the name's), element reads of an array's class. Its parameters, receiver and callees' returns are
    /// added by `AotFacts` (`dependencies`).
    pub(super) reads: std::collections::BTreeSet<Key>,
    /// The element-class links the stores and calls make (`Word::link`): an array joins its destination's class.
    pub(super) links: Vec<(crate::typed_ir::ElementClass, crate::typed_ir::ElementClass)>,
    /// Whether the function writes elements of arrays it cannot name (`effects::Events::wild_elements`).
    pub(super) wild_elements: bool,
    /// The changes its instructions and calls make to named properties, in PC order (`shape`).
    pub(super) shapes: Vec<Shape>,
    /// The PCs, in order, of the instructions that may run guest code with the objects they touch (`effects::runs_guest_code`).
    pub(super) guest_runs: Vec<u32>,
    /// The proofs a guest-run point may hold (`quiet::Quiet`), by PC, in order.
    pub(super) quiet: Vec<(u32, Quiet)>,
    /// How many parameters the function binds from its arguments one to one: `None` for a rest,
    /// generator or async function, whose parameters the calls do not type (`types::parameters`).
    pub(super) arity: Option<u16>,
    /// Per parameter, whether its argument is in a register at entry (`bound_parameters`): its array is then the
    /// parameter's own element class, which a resolved call links to the argument's (`types::entry_links`).
    pub(super) in_register: Box<[bool]>,
    /// Whether the function's return value leaves the closed world: the program's root (the host reads its result),
    /// an async function (its result is a promise a runtime reaction reads) or a generator (`next()` hands it out).
    /// `types::escapes` takes its returns as positions the solve cannot follow.
    pub(super) returns_out: bool,
    /// The values the function hands to code the solve cannot follow: each value it throws, and every register at
    /// a `yield` or `await`, which the runtime carries to a consumer or a promise reaction (`types::escapes`).
    pub(super) handed: Vec<Word>,
    /// The functions whose closure the function defines in a register. Where a value's identity is lost (a function
    /// alternative with no body), any of them may be the closure it holds (`types::escapes`).
    pub(super) closures: Vec<u32>,
    /// The integer bounds the function puts on the keys it stores to and compares against, in replay order (`Record::bounds`):
    /// each constant a relational jump compares a key's value with, and each mask bound a store writes. `Types::of` widens
    /// the key's ranges to them, so a field that counts up to a constant it is compared with settles at that constant.
    pub(super) bounds: Vec<(Key, i64)>,
    /// The length relations each pc holds before its instruction (`lengths::Settled`), where any holds.
    pub(super) lengths: lengths::Settled,
}

/// The dataflow problem of one function: its registers' facts over its basic blocks.
struct Solver<'a> {
    ctx: &'a Ctx<'a>,
    function: &'a Function,
    blocks: &'a Blocks,
    /// The integer bounds a loop header widens its ranges to (`typing::thresholds`): the bounds the relational jumps compare against.
    thresholds: Vec<i64>,
    /// The blocks a backedge enters: the loop headers, where a join widens (`join`).
    headers: BitSet,
    /// Per block, how many joins changed its input: past `WIDENING_ROUNDS` a loop header widens on every change.
    rounds: Vec<std::cell::Cell<u8>>,
    /// The element-class links the joins make of words with different origins (`Word::join_links`): the record keeps them.
    joined: std::cell::RefCell<std::collections::BTreeSet<(ElementClass, ElementClass)>>,
    /// Per parameter, whether its argument is in a register at entry (`bound_parameters`).
    in_register: &'a [bool],
}

/// The state a block starts with, or an edge carries: the words of the registers, and the block the state flows into,
/// so a join there knows whether it is a loop header and how often it has changed (`Solver::join`).
#[derive(Clone, PartialEq)]
struct Arrival {
    block: usize,
    words: Vec<Word>,
    /// The length relations the state holds (`lengths`), intersected at a join.
    lengths: Lengths,
}

/// The facts of function `id`, whose record is `function`.
pub(super) fn analyze_function(id: u32, function: &Function, (scope, decode, records): (&Scope, crate::staged::DecodeContext<'_>, &[crate::bytecode::Register]),
    resolved: &std::collections::BTreeMap<Key, u32>, types: Option<&super::types::Types>) -> FunctionFlow {
    let successors = Successors::of_code(function);
    let blocks = Blocks::of(&successors, function.code.len());
    let whole = whole_literals(function);
    let ctx = Ctx {
        id,
        parameter_end: function.parameter_end_pc as usize,
        is_root: function.parent.is_none(),
        scope,
        resolved,
        registers: usize::from(function.registers),
        whole_literals: &whole[..],
        types,
        decode,
        records,
    };
    let thresholds = typing::thresholds(relational_bounds(function, scope));
    let rounds = vec![std::cell::Cell::new(0); blocks.ranges.len()];
    let in_register = bound_parameters(function);
    let mut solver = Solver { ctx: &ctx, function, blocks: &blocks, thresholds, headers: loop_headers(&blocks), rounds, joined: Default::default(), in_register: &in_register };
    let fixpoint = solve(&mut solver);
    let inputs = solver.narrow(fixpoint);
    let mut record = Record::new(ctx.registers);
    record.joined(solver.joined.take());
    for (block, input) in inputs.into_iter().enumerate() {
        if let Some(state) = input {
            let cells = state.words.iter().map(|word| settled(&ctx, *word)).collect();
            let sources = state.words.iter().map(|word| Source::of(word.origin)).collect();
            record.registers.push_entry(blocks.ranges[block].start as u32, cells, sources);
            solver.replay(block, state.words, state.lengths, &mut record);
        }
    }
    record.registers.sort();
    let arity = arity_of(function);
    let returns_out = ctx.is_root || function.is_generator || function.is_async;
    record.finish(arity, returns_out, in_register)
}

/// How many parameters `function` binds from its arguments one to one (`FunctionFlow::arity`).
fn arity_of(function: &Function) -> Option<u16> {
    (!function.rest && !function.is_generator && !function.is_async).then_some(function.params)
}

/// Per parameter, whether its argument is in a register at entry: a parameter bound one to one that the body promotes to a
/// register (`Function::promoted_register`). A function that reads its `arguments` object reaches every argument through
/// it, which the solve does not follow, so none of its parameters is one (their arrays are the unnamed class).
fn bound_parameters(function: &Function) -> Box<[bool]> {
    let reads_arguments = (0..function.code.len()).any(|pc| function.instruction_at(pc).op().reads_arguments());
    let one_to_one = arity_of(function).is_some() && !reads_arguments;
    (0..function.params).map(|index| one_to_one && function.promoted_register(index).is_some()).collect()
}

/// The integer constants the function's relational jumps compare a value against (`typing::thresholds`): the ones
/// whose operand is a constant. A register operand's bound is the state's, which the narrowing pass tightens to.
fn relational_bounds(function: &Function, scope: &Scope) -> Vec<i64> {
    (0..function.code.len())
        .filter_map(|pc| match Operation::<u64, ()>::of(function.instruction_at(pc))? {
            Operation::CompareJump { comparison: Comparison::Relation { .. }, left, right, .. } => Some([left, right]),
            _ => None,
        })
        .flatten()
        .filter(|operand| matches!(operand.kind(), Some(crate::bytecode::OperandKind::Constant)))
        .filter_map(|operand| scope.constant_type(usize::from(operand.payload())).int_range())
        .flat_map(|range| [range.min, range.max])
        .collect()
}

/// The blocks a backedge enters (a normal edge to a block no later than its source): every loop has one, and every cycle
/// of the graph passes through one, so widening at these blocks alone makes the solve finite.
fn loop_headers(blocks: &Blocks) -> BitSet {
    let mut headers = BitSet::with_capacity(blocks.ranges.len());
    for (block, (targets, kinds)) in blocks.targets.iter().zip(&blocks.kinds).enumerate() {
        for (&target, kind) in targets.iter().zip(kinds) {
            if matches!(kind, ControlEdge::Normal) && target <= block { headers.insert(target); }
        }
    }
    headers
}

/// Per PC, whether the `MakeArray` there is a literal whose `DefineArrayElement` records follow it back to back and
/// define every index in order (`allocation::literal_elements`). Such an array holds no hole: an elision leaves its index
/// undefined, so it is not whole.
fn whole_literals(function: &Function) -> Box<[bool]> {
    (0..function.code.len()).map(|pc| {
        let make = function.instruction_at(pc);
        make.op() == crate::bytecode::Op::MakeArray && (0..make.array_length()).all(|index| {
            let at = pc + 1 + index;
            at < function.code.len() && {
                let define = function.instruction_at(at);
                define.op() == crate::bytecode::Op::DefineArrayElement
                    && define.register_b() == make.result_register()
                    && define.imm() as usize == index
            }
        })
    }).collect()
}

impl Dataflow for Solver<'_> {
    type State = Arrival;

    fn blocks(&self) -> usize {
        self.blocks.ranges.len()
    }

    fn edges(&self, block: usize) -> &[usize] {
        &self.blocks.targets[block]
    }

    fn entry_state(&self, block: usize) -> Option<Arrival> {
        (block == 0).then(|| Arrival { block, words: self.entry_facts(), lengths: Lengths::default() })
    }

    /// Joins `incoming` into the input of `into.block`. At a loop header, once its input has changed `WIDENING_ROUNDS`
    /// times, a range that grows is widened to the nearest threshold (`typing::widen`), so the header reaches a fixed
    /// bound in finitely many joins; elsewhere the join is exact. `narrow` then tightens what widening over-approximated.
    /// The length relations join by intersection, which only shrinks them, so they need no widening.
    fn join(&self, into: &mut Arrival, incoming: &Arrival) -> bool {
        let widen = self.headers.contains(into.block) && self.rounds[into.block].get() >= WIDENING_ROUNDS;
        let words = self.join_words(&mut into.words, &incoming.words, widen);
        if words {
            let rounds = &self.rounds[into.block];
            rounds.set(rounds.get().saturating_add(1));
        }
        let related = into.lengths.meet(&incoming.lengths);
        words || related
    }

    fn transfer(&mut self, block: usize, state: &Arrival) -> Arrival {
        let ctx = self.ctx;
        let mut words = state.words.clone();
        let mut lengths = state.lengths.clone();
        for pc in self.blocks.ranges[block].clone() {
            let instruction = self.function.instruction_at(pc);
            lengths::advance(ctx, instruction, &mut lengths, &mut words, |words| step(ctx, pc, instruction, words));
        }
        Arrival { block, words, lengths }
    }

    /// An exception edge may leave the block before any of its writes, so it carries no fact. A normal edge carries
    /// the fact its block's conditional jump proves on it (`Solver::refine`).
    fn transfer_edge_at(&self, block: usize, index: usize, out: &Arrival) -> Arrival {
        let target = self.blocks.targets[block][index];
        let (words, lengths) = match self.blocks.kinds[block][index] {
            ControlEdge::Handler => (vec![Word::UNKNOWN; out.words.len()], Lengths::default()),
            ControlEdge::Normal => (self.refine(block, target, &out.words), self.refine_lengths(block, target, out)),
        };
        Arrival { block: target, words, lengths }
    }

    /// The length relations the normal edge from `block` to `target` carries: those of `out`, and the ones the block's
    /// conditional jump proves on that edge (`lengths::refine`), decided as `refine` decides its edge.
    fn refine_lengths(&self, block: usize, target: usize, out: &Arrival) -> Lengths {
        let last = self.blocks.ranges[block].end - 1;
        let instruction = self.function.instruction_at(last);
        let fallthrough = last + 1;
        let edge = self.blocks.ranges[target].start;
        match conditional_target(instruction) {
            Some(jump) if jump != fallthrough && (edge == jump || edge == fallthrough) =>
                lengths::refine(self.ctx, instruction, edge == jump, &out.words, &out.lengths),
            _ => out.lengths.clone(),
        }
    }
}

/// What a conditional jump's test proves about the value it tests, on one of its edges.
#[derive(Clone, Copy)]
enum Refinement {
    /// The value is `null` or `undefined` (or, by a loose test, an object an IsHTMLDDA cell may be).
    Nullish,
    /// The value is neither `null` nor `undefined`.
    NotNullish,
}

impl Solver<'_> {
    /// The state the normal edge from `block` to `target` carries, from `block`'s output `out`. Edge-fact
    /// law: the jump at the end of a block proves a fact about the register it tests on each of its two
    /// edges, and that fact is proven at the test, not assumed. The refined word is carried on the value
    /// from the edge on, and every read after the edge trusts it, so `x.f` after `x != null` is one load
    /// (the true edge carries `x: C`, not `C | Null`). An edge the test proves nothing on, or a jump whose
    /// operands the state does not name with certainty, carries `out` as it is.
    fn refine(&self, block: usize, target: usize, out: &Vec<Word>) -> Vec<Word> {
        let last = self.blocks.ranges[block].end - 1;
        let instruction = self.function.instruction_at(last);
        let Some(jump) = conditional_target(instruction) else { return out.clone() };
        let fallthrough = last + 1;
        let edge = self.blocks.ranges[target].start;
        // A jump to the next instruction has one edge that two indices name: no test tells them apart.
        if jump == fallthrough || (edge != jump && edge != fallthrough) { return out.clone(); }
        if let Some(refined) = self.refine_ranges(instruction, out, edge == jump) { return refined; }
        let Some((register, fact)) = edge_fact(instruction, out, edge == jump) else { return out.clone() };
        let word = out[register];
        let mut refined = out.clone();
        refined[register] = Word { cell: refine_cell(word.cell, fact), origin: word.origin };
        refined
    }

    /// The state an edge of a relational jump carries when its two operands are integer ranges: each keeps the range
    /// the comparison leaves it on that edge (`typing::relation_edge`). `None` where the jump compares no two integer
    /// ranges, one register against itself, or no value reaches the edge (the edge then keeps the state it has).
    fn refine_ranges(&self, instruction: WideInstruction, out: &Vec<Word>, to_target: bool) -> Option<Vec<Word>> {
        let Some(Operation::CompareJump { comparison: Comparison::Relation { relation, .. }, left, right, negate }) =
            Operation::<u64, ()>::of(instruction) else { return None };
        let operands = [left, right];
        if left.register_index().is_some() && left.register_index() == right.register_index() { return None; }
        let env = Before { ctx: self.ctx, state: out };
        let [Some(left_range), Some(right_range)] = operands.map(|operand| typing::operand_cell(&env, operand).ty.int_range())
            else { return None };
        // The jump is taken where the comparison fails when `negate`, so its target's edge holds the relation false.
        let [left_kept, right_kept] = typing::relation_edge(relation, to_target != negate, left_range, right_range)?;
        let mut refined = out.clone();
        for (operand, range) in operands.into_iter().zip([left_kept, right_kept]) {
            if let Some(register) = operand.register_index() {
                let word = refined[usize::from(register)];
                refined[usize::from(register)] = Word { cell: Cell { ty: Type::int(range), ..word.cell }, origin: word.origin };
            }
        }
        Some(refined)
    }
}

impl Solver<'_> {
    /// Joins the words `incoming` holds into `into`, register by register; whether `into` changed. Where `widen`, a
    /// word's integer range is widened (`typing::widen`) instead of joined, and its other parts are joined.
    fn join_words(&self, into: &mut [Word], incoming: &[Word], widen: bool) -> bool {
        let mut changed = false;
        for (slot, &fact) in into.iter_mut().zip(incoming) {
            self.joined.borrow_mut().extend(slot.join_links(fact));
            let joined = slot.join(fact);
            let next = if widen {
                Word { cell: Cell { ty: typing::widen(&self.thresholds, slot.cell.ty, joined.cell.ty), ..joined.cell }, ..joined }
            } else {
                joined
            };
            changed |= next != *slot;
            *slot = next;
        }
        changed
    }

    /// One descending pass over the fixpoint `inputs` (narrowing): each block's input is recomputed as the join of the
    /// edges the fixpoint's inputs send it, without widening, so a widened range tightens to what its edges carry (a
    /// counter widened to int32's span becomes `[0, n]` under `i < n`). `inputs` is a post-fixpoint, so each edge's state
    /// is at most the one it had, and the result still bounds every reachable state.
    fn narrow(&mut self, inputs: Vec<Option<Arrival>>) -> Vec<Option<Arrival>> {
        let mut next: Vec<Option<Arrival>> = (0..inputs.len()).map(|block| self.entry_state(block)).collect();
        for (block, input) in inputs.iter().enumerate() {
            let Some(state) = input else { continue };
            let out = self.transfer(block, state);
            for index in 0..self.blocks.targets[block].len() {
                let edge = self.transfer_edge_at(block, index, &out);
                match &mut next[edge.block] {
                    Some(known) => {
                        self.join_words(&mut known.words, &edge.words, false);
                        known.lengths.meet(&edge.lengths);
                    }
                    slot => *slot = Some(edge),
                }
            }
        }
        next
    }
}

/// The jump target of a conditional jump (`JumpIf`, or the fused `CompareJump`); `None` for any other instruction.
fn conditional_target(instruction: WideInstruction) -> Option<usize> {
    matches!(Operation::<u64, ()>::of(instruction), Some(Operation::JumpIf { .. } | Operation::CompareJump { .. }))
        .then(|| instruction.jump_target() as usize)
}

/// The fact a conditional jump's test proves about the register it tests, on its target edge (`to_target`)
/// or its fallthrough, from the state `state` the jump leaves. Recognized with certainty only:
/// - `JumpIf` on a register `r` is a truth test. Its truthy edge proves `r` not nullish; its falsy edge may
///   be `null`, `undefined`, `false`, `0` or `""`, so it proves nothing.
/// - `CompareJump` with an equality operator (`==`, `===`, `!=`, `!==`) of one register against a register
///   whose word is exactly `null` or `undefined`. The equal edge proves `r` nullish. The unequal edge of a
///   loose test proves `r` not nullish; a strict unequal edge may still be the other nullish value, so it proves nothing.
/// Every other jump, and a comparison of two registers neither of which is a nullish constant, is `None`.
fn edge_fact(instruction: WideInstruction, state: &[Word], to_target: bool) -> Option<(usize, Refinement)> {
    match Operation::<u64, ()>::of(instruction)? {
        Operation::JumpIf { register, taken } => {
            // The target is taken where `r` is truthy when `taken`, and where it is falsy when not.
            let truthy = if to_target { taken } else { !taken };
            truthy.then_some((usize::from(register), Refinement::NotNullish))
        }
        // The jump is taken where the comparison fails (`negate`), so the target's edge holds the comparison false.
        Operation::CompareJump { comparison: Comparison::Equality { strict, negated }, left, right, negate: true } => {
            let holds = !to_target;
            let equal = holds != negated;
            let (left, right) = (usize::from(left.register_index()?), usize::from(right.register_index()?));
            let register = match (is_nullish_constant(state[left]), is_nullish_constant(state[right])) {
                (false, true) => left,
                (true, false) => right,
                _ => return None,
            };
            if equal {
                // A loose `== null` also holds of a legacy heap cell an IsHTMLDDA object is, so only a type that
                // holds no such cell proves the value nullish.
                (strict || !may_be_legacy_nullish(state[register].cell.ty)).then_some((register, Refinement::Nullish))
            } else {
                (!strict).then_some((register, Refinement::NotNullish))
            }
        }
        _ => None,
    }
}

/// The integer range of a program constant operand (`Scope::constant_type`); `None` for a register.
fn constant_range(ctx: &Ctx<'_>, operand: Operand) -> Option<IntRange> {
    if matches!(operand.kind(), Some(OperandKind::Constant)) { ctx.scope.constant_type(usize::from(operand.payload())).int_range() } else { None }
}

/// The key a value read from `origin` is the binding or named property of (`Record::commit` reads keys the same way).
fn origin_key(origin: Option<Origin>) -> Option<Key> {
    match origin? {
        Origin::Binding(key) => Some(key),
        Origin::Member(atom) => Some(Key::Member(atom)),
        Origin::MemberOn { class, instance, atom, .. } => Some(Key::MemberOn { class, instance, atom }),
        _ => None,
    }
}

/// The bounds a relational jump puts on the key of its register operand, where its other operand is a constant: each
/// end of the constant's range (`Record::bounds`). A field compared with `this.count < 1000` gets 1000 under its key.
fn compared_bounds(ctx: &Ctx<'_>, instruction: WideInstruction, state: &[Word], record: &mut Record) {
    let Some(Operation::CompareJump { comparison: Comparison::Relation { .. }, left, right, .. }) = Operation::<u64, ()>::of(instruction) else { return };
    for (operand, other) in [(left, right), (right, left)] {
        let Some(register) = operand.register_index() else { continue };
        let Some(range) = constant_range(ctx, other) else { continue };
        let Some(key) = origin_key(state[usize::from(register)].origin) else { continue };
        record.bounds.extend([range.min, range.max].map(|bound| (key, bound)));
    }
}

/// Marks the registers `instruction` writes: a `&` with a nonnegative constant operand holds that constant's mask bound
/// (`typing::mask_bound`), and any other write clears the mark. Only the bytecode decides the marks, so they agree in every pass.
fn mask_bounds(ctx: &Ctx<'_>, instruction: WideInstruction, masks: &mut [Option<i64>]) {
    for register in instruction.written_registers() { masks[usize::from(register)] = None; }
    if let Some(Operation::Number(NumberOperation::Bitwise { destination, left, right, operation: NumberBitwise::And })) = Operation::<u64, ()>::of(instruction) {
        masks[usize::from(destination)] = [left, right].into_iter().find_map(|operand| typing::mask_bound(constant_range(ctx, operand)?));
    }
}

/// The mask bound a store writes to the keys it stores to: a value the instruction reads that holds one (`mask_bounds`).
fn stored_bounds(instruction: WideInstruction, writes: &[(Key, Word)], masks: &[Option<i64>], record: &mut Record) {
    let Some(bound) = used_registers(instruction).into_iter().find_map(|register| masks[usize::from(register)]) else { return };
    record.bounds.extend(writes.iter().map(|&(key, _)| (key, bound)));
}

/// Whether a word is exactly `null` or exactly `undefined`: its type holds no other value, so it is that constant.
fn is_nullish_constant(word: Word) -> bool {
    word.cell.ty == Type::language(ValueType::Null) || word.cell.ty == Type::language(ValueType::Undefined)
}

/// Whether a value of type `ty` may be a legacy heap cell (an IsHTMLDDA object) that a loose `== null` holds of:
/// any type outside `null`, `undefined`, Number and Boolean, except a direct object, which no such cell is.
fn may_be_legacy_nullish(ty: Type) -> bool {
    let plain = language_bit(ValueType::Null) | language_bit(ValueType::Undefined)
        | language_bit(ValueType::Number) | language_bit(ValueType::Boolean);
    ty.direct_object_part().is_none() && ty.may_be_languages(Type::languages(!plain))
}

/// The cell a value of cell `cell` holds on an edge proving `fact`. A nullish value holds no object and no array.
fn refine_cell(cell: Cell, fact: Refinement) -> Cell {
    match fact {
        Refinement::NotNullish => Cell { ty: cell.ty.not_nullish(), ..cell },
        Refinement::Nullish => Cell { ty: cell.ty.nullish_part(), classes: Classes::Never, elements: Elements::NEVER },
    }
}

impl Solver<'_> {
    /// The facts at entry: every register unknown, but a promoted parameter holds its argument, whose
    /// type the pass before found (`Types::param`), with the parameter's element class where the argument is in
    /// the register (`in_register`). A parameter the body keeps as a local stays unknown: only its promoted register
    /// holds the argument at entry (`Function::promoted_register`).
    fn entry_facts(&self) -> Vec<Word> {
        let mut state = vec![Word::UNKNOWN; self.ctx.registers];
        if let Some(types) = self.ctx.types {
            for index in 0..self.function.params {
                if let Some(register) = self.function.promoted_register(index) {
                    let origin = self.in_register[usize::from(index)].then_some(Origin::Parameter { function: self.ctx.id, index });
                    state[usize::from(register)] = Word { cell: types.param(self.ctx.id, index), origin };
                }
            }
        }
        state
    }

    /// Replays `block` from its input `state`, recording what its instructions do. The mask bounds the block's
    /// registers hold (`mask_bounds`) start empty at each block: they follow from the bytecode alone, not from the facts.
    fn replay(&self, block: usize, mut state: Vec<Word>, mut lengths: Lengths, record: &mut Record) {
        let mut masks: Vec<Option<i64>> = vec![None; state.len()];
        let ctx = self.ctx;
        for pc in self.blocks.ranges[block].clone() {
            let instruction = self.function.instruction_at(pc);
            record.observe(ctx, instruction, pc, &state);
            compared_bounds(ctx, instruction, &state, record);
            record.lengths.push(pc as u32, lengths.relations());
            let events = lengths::advance(ctx, instruction, &mut lengths, &mut state, |words| step(ctx, pc, instruction, words));
            stored_bounds(instruction, &events.writes, &masks, record);
            mask_bounds(self.ctx, instruction, &mut masks);
            record.commit(instruction, &state, events);
            let written: Box<[_]> = instruction.written_registers().map(|register| {
                let word = state[usize::from(register)];
                (u16::from(register), settled(self.ctx, word), Source::of(word.origin))
            }).collect();
            if !written.is_empty() { record.registers.push_writes(pc as u32, written); }
        }
    }
}
