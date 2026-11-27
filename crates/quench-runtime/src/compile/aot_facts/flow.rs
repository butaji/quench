//! The register dataflow of one function (`dataflow::solve` over its basic blocks): each
//! register's fact at each point, and what the function does that the closed-world facts read.

use super::blocks::Blocks;
use super::effects::{Before, Ctx, Events, step};
use crate::staged::typing;
use super::scope::Scope;
use super::value::{Word, Key, Origin, PrototypeBase};
use crate::bytecode::{ControlEdge, Function, Successors, Traits, WideInstruction, used_registers};
use crate::dataflow::{Dataflow, solve};
use crate::staged::{Comparison, Operation};
use crate::typed_ir::{Cell, Classes, Elements, Length, Representation, Type, language_bit};
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
    /// Whether a store through a key the analysis cannot name may write a named property.
    pub(super) member_wild: bool,
    /// Per register, the Number representation it holds at every read and write.
    pub(super) numbers: Box<[Option<Representation>]>,
    /// The named properties stored into a function's `prototype`, in code order.
    pub(super) prototype_writes: Vec<(PrototypeBase, u32)>,
    /// Per named field read or store, by PC, the class of the object it reads or stores (`effects::field_owner`).
    pub(super) field_owners: std::collections::BTreeMap<u32, crate::typed_ir::Classes>,
    /// The register cells of the function at every point, for the tiers that lower it (`RegisterCells`).
    pub(super) registers: RegisterCells,
    /// The keys the settled cells of the function were read from: binding loads, field reads (a class's own,
    /// or the name's), element reads of an array field. Its parameters, receiver and callees' returns are
    /// added by `AotFacts` (`dependencies`).
    pub(super) reads: std::collections::BTreeSet<Key>,
    /// How many parameters the function binds from its arguments one to one: `None` for a rest,
    /// generator or async function, whose parameters the calls do not type (`types::parameters`).
    pub(super) arity: Option<u16>,
}

/// The cells a function's registers hold at every point, kept compactly: each reachable block's cells at
/// entry, and each instruction's register writes with the cells written. The cell of a register before
/// the instruction at `pc` is its last write earlier in `pc`'s block, else its cell at the block's entry.
#[derive(Default)]
pub struct RegisterCells {
    /// By block, its first PC and its registers' cells at entry, in PC order; a block no path reaches is absent.
    entries: Vec<(u32, Box<[crate::typed_ir::Cell]>)>,
    /// By PC, in order, the registers the instruction there writes, with their cells after it.
    writes: Vec<(u32, Box<[(u16, crate::typed_ir::Cell)]>)>,
}

impl RegisterCells {
    /// The cell `register` holds just before the instruction at `pc`: none where no path reaches `pc`.
    pub fn at(&self, pc: u32, register: u16) -> Option<crate::typed_ir::Cell> {
        let block = self.entries.partition_point(|(start, _)| *start <= pc).checked_sub(1)?;
        let (start, entry) = &self.entries[block];
        let from = self.writes.partition_point(|(at, _)| *at < *start);
        let to = self.writes.partition_point(|(at, _)| *at < pc);
        let written = self.writes[from..to].iter().rev()
            .find_map(|(_, cells)| cells.iter().find(|(written, _)| *written == register).map(|(_, cell)| *cell));
        written.or_else(|| entry.get(usize::from(register)).copied())
    }

    /// The cells of every register at the entry of the block that starts at `pc`, where one does.
    pub fn entry(&self, pc: u32) -> Option<&[crate::typed_ir::Cell]> {
        self.entries.binary_search_by_key(&pc, |(start, _)| *start).ok().map(|index| &*self.entries[index].1)
    }
}

/// The dataflow problem of one function: its registers' facts over its basic blocks.
struct Solver<'a> {
    ctx: &'a Ctx<'a>,
    function: &'a Function,
    blocks: &'a Blocks,
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
    let mut solver = Solver { ctx: &ctx, function, blocks: &blocks };
    let inputs = solve(&mut solver);
    let mut record = Record::new(ctx.registers);
    for (block, input) in inputs.into_iter().enumerate() {
        if let Some(state) = input {
            record.registers.entries.push((blocks.ranges[block].start as u32, state.iter().map(|word| word.cell).collect()));
            solver.replay(block, state, &mut record);
        }
    }
    record.registers.entries.sort_by_key(|(start, _)| *start);
    record.registers.writes.sort_by_key(|(pc, _)| *pc);
    let arity = (!function.rest && !function.is_generator && !function.is_async).then_some(function.params);
    record.finish(arity)
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
    type State = Vec<Word>;

    fn blocks(&self) -> usize {
        self.blocks.ranges.len()
    }

    fn edges(&self, block: usize) -> &[usize] {
        &self.blocks.targets[block]
    }

    fn entry_state(&self, block: usize) -> Option<Vec<Word>> {
        (block == 0).then(|| self.entry_facts())
    }

    fn join(&self, into: &mut Vec<Word>, incoming: &Vec<Word>) -> bool {
        let mut changed = false;
        for (slot, &fact) in into.iter_mut().zip(incoming) {
            let joined = slot.join(fact);
            changed |= joined != *slot;
            *slot = joined;
        }
        changed
    }

    fn transfer(&mut self, block: usize, state: &Vec<Word>) -> Vec<Word> {
        let mut out = state.clone();
        for pc in self.blocks.ranges[block].clone() {
            step(self.ctx, pc, self.function.instruction_at(pc), &mut out);
        }
        out
    }

    /// An exception edge may leave the block before any of its writes, so it carries no fact. A normal
    /// edge carries the fact its block's conditional jump proves on it (`Solver::refine`). A backedge (to a block
    /// no later than its source: every loop has one) settles the integer ranges it carries (`typing::settle`),
    /// so a loop's header is joined within finitely many rounds.
    fn transfer_edge_at(&self, block: usize, index: usize, out: &Vec<Word>) -> Vec<Word> {
        match self.blocks.kinds[block][index] {
            ControlEdge::Handler => vec![Word::UNKNOWN; out.len()],
            ControlEdge::Normal => {
                let target = self.blocks.targets[block][index];
                let mut edge = self.refine(block, target, out);
                if target <= block {
                    for word in &mut edge { word.cell.ty = typing::settle(word.cell.ty); }
                }
                edge
            }
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
    /// type the pass before found (`Types::param`). A parameter the body keeps as a local stays unknown:
    /// only its promoted register holds the argument at entry (`Function::promoted_register`).
    fn entry_facts(&self) -> Vec<Word> {
        let mut state = vec![Word::UNKNOWN; self.ctx.registers];
        if let Some(types) = self.ctx.types {
            for index in 0..self.function.params {
                if let Some(register) = self.function.promoted_register(index) {
                    state[usize::from(register)] = Word::of_cell(types.param(self.ctx.id, index));
                }
            }
        }
        state
    }

    /// Replays `block` from its input `state`, recording what its instructions do.
    fn replay(&self, block: usize, mut state: Vec<Word>, record: &mut Record) {
        for pc in self.blocks.ranges[block].clone() {
            let instruction = self.function.instruction_at(pc);
            record.observe(self.ctx, instruction, pc, &state);
            let events = step(self.ctx, pc, instruction, &mut state);
            record.commit(instruction, &state, events);
            let written: Box<[_]> = instruction.written_registers().map(|register| (u16::from(register), state[usize::from(register)].cell)).collect();
            if !written.is_empty() { record.registers.writes.push((pc as u32, written)); }
        }
    }
}

/// What a replay gathers over a function's reachable code.
struct Record {
    calls: Vec<Call>,
    writes: Vec<(Key, Word)>,
    wild_owner: bool,
    wild_global: bool,
    wild_member: bool,
    prototype_writes: Vec<(PrototypeBase, u32)>,
    /// Per register, the join of the facts it is defined with.
    defs: Vec<Option<Word>>,
    /// Per register, whether a read saw a value that is not a Number.
    unsafe_reads: Vec<bool>,
    field_owners: std::collections::BTreeMap<u32, crate::typed_ir::Classes>,
    registers: RegisterCells,
    reads: std::collections::BTreeSet<Key>,
}

impl Record {
    fn new(registers: usize) -> Self {
        Self {
            calls: Vec::new(),
            writes: Vec::new(),
            wild_owner: false,
            wild_global: false,
            wild_member: false,
            prototype_writes: Vec::new(),
            defs: vec![None; registers],
            unsafe_reads: vec![false; registers],
            field_owners: std::collections::BTreeMap::new(),
            registers: RegisterCells::default(),
            reads: std::collections::BTreeSet::new(),
        }
    }

    /// Notes the reads and the calls `instruction` makes, from the facts before it.
    fn observe(&mut self, ctx: &Ctx<'_>, instruction: WideInstruction, pc: usize, state: &[Word]) {
        for register in used_registers(instruction) {
            if !state[usize::from(register)].is_number() {
                self.unsafe_reads[usize::from(register)] = true;
            }
        }
        // An element read of an array a field holds reads that field's elements (`Key::Element`).
        if instruction.op() == crate::bytecode::Op::GetIndex
            && let Some(Origin::Member(atom) | Origin::MemberOn { atom, .. }) = state[usize::from(instruction.register_b())].origin
        {
            self.reads.insert(Key::Element(atom));
        }
        if let Some(owner) = super::effects::field_owner(ctx, instruction, state) {
            let joined = self.field_owners.entry(pc as u32).or_insert(crate::typed_ir::Classes::Never);
            *joined = joined.join(owner.cell.classes);
        }
        let traits = instruction.op().traits();
        let construct = traits.has(Traits::CONSTRUCTION);
        // Every call form the staged decoder reads (`staged::Invocation`): a register call, a known
        // function's, a method site's, a construction; not an eval, `super(...)` or an array of arguments.
        if let Some(invocation) = crate::staged::Invocation::of(instruction, ctx.decode) {
            let (target, callee) = match invocation.callee {
                crate::staged::Callee::Register(register) => (Target::Register, state[usize::from(register)]),
                crate::staged::Callee::Known(function) => (Target::Known(u32::from(function)), Word::UNKNOWN),
                crate::staged::Callee::Method { atom, .. } => (Target::Method(atom), Word::UNKNOWN),
            };
            let arguments: Vec<Word> = invocation.arguments.registers(ctx.records).map(|register| state[usize::from(register)]).collect();
            let receiver = match invocation.receiver {
                crate::staged::Receiver::Register(register) => state[usize::from(register)],
                crate::staged::Receiver::This { path: None } if !ctx.is_root =>
                    ctx.types.map_or(Word::UNKNOWN, |types| Word::of_cell(types.this(ctx.id))),
                crate::staged::Receiver::This { path: Some((atom, _)) } => match ctx.types {
                    Some(_) => super::effects::this_member(ctx, atom),
                    None => Word::UNKNOWN,
                },
                // A construction's receiver is the object it allocates: an instance of the class it constructs.
                crate::staged::Receiver::Allocated => match callee.cell.ty.body_index() {
                    Some(class) if construct => Word::instance(class),
                    _ => Word::of(Type::DIRECT_OBJECT_FAMILY),
                },
                _ => Word::UNKNOWN,
            };
            // An array method that may change its array (`push`, `splice`, ...) makes the arrays the receiver names hold
            // the arguments, and an unknown length (`Scope::is_array_mutator`).
            let method = match (invocation.callee, callee.origin) {
                (crate::staged::Callee::Method { atom, .. }, _) => Some(atom),
                (crate::staged::Callee::Register(_), Some(Origin::Member(atom) | Origin::MemberOn { atom, .. })) => Some(atom),
                _ => None,
            };
            if method.is_some_and(|atom| ctx.scope.is_array_mutator(atom)) {
                let stored = arguments.iter().fold(Cell::NEVER, |cell, argument| cell.join(argument.cell));
                let elements = Elements { ty: Type::NEVER, classes: Classes::Never, length: Length::Any };
                let written = Cell { ty: stored.ty, classes: stored.classes, elements };
                self.writes.push((Key::Element(receiver.elements_key()), Word::of_cell(written)));
            }
            self.calls.push(Call { pc: pc as u32, callee, target, construct, receiver, arguments });
        }
    }

    /// Notes the writes `instruction` made and the facts it defined, from the facts after it.
    fn commit(&mut self, instruction: WideInstruction, state: &[Word], events: Events) {
        self.wild_owner |= events.wild_owner;
        self.wild_global |= events.wild_global;
        self.wild_member |= events.wild_member;
        self.writes.extend(events.writes);
        self.prototype_writes.extend(events.prototype_write);
        for register in instruction.written_registers() {
            let index = usize::from(register);
            self.defs[index] = Some(self.defs[index].map_or(state[index], |def| def.join(state[index])));
        }
        // The key a defined value was read from, where its word names one (`Origin`).
        if let Some(defined) = instruction.defined_register() {
            match state[usize::from(defined)].origin {
                Some(Origin::Binding(key)) => { self.reads.insert(key); }
                Some(Origin::Member(atom)) => { self.reads.insert(Key::Member(atom)); }
                Some(Origin::MemberOn { class, instance, atom, .. }) => {
                    self.reads.insert(Key::MemberOn { class, instance, atom });
                    self.reads.insert(Key::MemberAny(atom));
                }
                _ => {}
            }
        }
    }

    fn finish(self, arity: Option<u16>) -> FunctionFlow {
        let numbers: Box<[Option<Representation>]> = self
            .defs
            .iter()
            .zip(&self.unsafe_reads)
            .map(|(def, &unsafe_read)| if unsafe_read { None } else { def.and_then(Word::representation) })
            .collect();
        FunctionFlow {
            calls: self.calls,
            writes: self.writes,
            wild_owner: self.wild_owner,
            global_wild: self.wild_global,
            member_wild: self.wild_member,
            numbers,
            prototype_writes: self.prototype_writes,
            field_owners: self.field_owners,
            registers: self.registers,
            reads: self.reads,
            arity,
        }
    }
}
