//! The register dataflow of one function (`dataflow::solve` over its basic blocks): each
//! register's fact at each point, and what the function does that the closed-world facts read.

#[cfg(not(stencil_build))]
#[allow(unused_imports)] use crate::staged::{InvocationFormsExt};
mod bounds;
mod refine;

use self::bounds::{compared_bounds, mask_bounds, relational_bounds, stored_bounds};
use super::blocks::Blocks;
use super::effects::{Ctx, settled, step};
use super::record::Record;
use super::registers::{RegisterCells, Source};
use crate::staged::typing;
use super::scope::Scope;
use super::shape::Shape;
use super::value::{Accessor, Key, Origin, Word};
use crate::bytecode::{ControlEdge, Function, Successors, WideInstruction};
use crate::dataflow::{BitSet, Dataflow, solve};
use super::quiet::Quiet;
use crate::staged::Operation;
use crate::typed_ir::{Cell, Classes, ElementClass, Representation, WIDENING_ROUNDS};

/// What a call names as its callee (`staged::Callee`): a register, whose fact the census resolves; a
/// function of the program the record names; or a method of its receiver, by name.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Target {
    Register,
    Known(u32),
    Method(u32),
    /// A named access of `atom` on the receiver: its getter (a read), or its setter with the stored value as the argument
    /// (`setter`). Its callees are the accessors the census proves for the receiver's classes (`types::targets`); none where
    /// it proves none, and then the access is no call.
    Accessor { atom: u32, setter: bool },
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
    /// Whether the call passes no receiver (`staged::Receiver::Undefined`): a sloppy callee then binds `this` to the global
    /// object (`types::Types::receivers`).
    pub(super) plain: bool,
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
    /// Whether an instruction keys an access into a value that may be the global object (`effects::keyed_global`): a
    /// computed access the closed world's call graph cannot see through.
    pub(super) global_keyed: bool,
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
    /// Per element access (a read or a store), by PC, the view of the array its base holds (`typed_ir::View`).
    pub(super) element_views: std::collections::BTreeMap<u32, crate::typed_ir::View>,
    /// The capture slots the function reads from its parent's environment, as (owner, slot) (`Record::observe`).
    pub(super) captures: std::collections::BTreeSet<(u32, u16)>,
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
    /// Whether the function is strict code (`Function::strict`, which class bodies and Modules set): a plain call binds its
    /// `this` to `undefined`, not to the global object, so only a sloppy function's `this` may be the global object (`types`).
    pub(super) strict: bool,
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
    /// The words each block's settled solve entered it with (`None` for a block no path reaches): the lengths pass replays
    /// the blocks from them (`length_settle::settle`).
    pub(super) entries: Vec<Option<Vec<Word>>>,
    /// Per pc of an element store, the array it writes and the indices it touches (`record::ElementStore`).
    pub(super) element_stores: std::collections::BTreeMap<u32, super::record::ElementStore>,
    /// The accessors the function defines, in PC order (`accessors::accessor_definition`); each names its object's class where proven.
    pub(super) accessors: Vec<Accessor>,
    /// The `extends` links the function makes: `(class, parent)` for each intrinsic `setPrototypeOf` that links two class
    /// prototypes (`accessors::heritage_link`). `types::Types::holder` walks them.
    pub(super) parents: Vec<(u32, u32)>,
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
}

/// The facts of function `id`, whose record is `function`.
pub(super) fn analyze_function(id: u32, function: &Function, (scope, decode, records): (&Scope, crate::staged::DecodeContext<'_>, &[crate::bytecode::Register]),
    resolved: &std::collections::BTreeMap<Key, u32>, accessors: &std::collections::BTreeSet<u32>, types: Option<&super::types::Types>) -> FunctionFlow {
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
        accessors,
    };
    let thresholds = typing::thresholds(relational_bounds(function, scope));
    let rounds = vec![std::cell::Cell::new(0); blocks.ranges.len()];
    let in_register = bound_parameters(function);
    let mut solver = Solver { ctx: &ctx, function, blocks: &blocks, thresholds, headers: loop_headers(&blocks), rounds, joined: Default::default(), in_register: &in_register };
    let fixpoint = solve(&mut solver);
    let inputs = solver.narrow(fixpoint);
    let entries = inputs.iter().map(|input| input.as_ref().map(|state| state.words.clone())).collect();
    let mut record = Record::new(ctx.registers);
    record.joined(solver.joined.take());
    for (block, input) in inputs.into_iter().enumerate() {
        if let Some(state) = input {
            let cells = state.words.iter().map(|word| settled(&ctx, *word)).collect();
            let sources = state.words.iter().map(|word| Source::of(word.origin)).collect();
            record.registers.push_entry(blocks.ranges[block].start as u32, cells, sources);
            solver.replay(block, state.words, &mut record);
        }
    }
    record.registers.sort();
    let arity = arity_of(function);
    let returns_out = ctx.is_root || function.is_generator || function.is_async;
    let mut flow = record.finish(arity, returns_out, in_register, function.strict);
    flow.entries = entries;
    flow
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
pub(super) fn whole_literals(function: &Function) -> Box<[bool]> {
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
        (block == 0).then(|| Arrival { block, words: self.entry_facts() })
    }

    /// Joins `incoming` into the input of `into.block`. At a loop header, once its input has changed `WIDENING_ROUNDS`
    /// times, a range that grows is widened to the nearest threshold (`typing::widen`), so the header reaches a fixed
    /// bound in finitely many joins; elsewhere the join is exact. `narrow` then tightens what widening over-approximated.
    fn join(&self, into: &mut Arrival, incoming: &Arrival) -> bool {
        let widen = self.headers.contains(into.block) && self.rounds[into.block].get() >= WIDENING_ROUNDS;
        let words = self.join_words(&mut into.words, &incoming.words, widen);
        if words {
            let rounds = &self.rounds[into.block];
            rounds.set(rounds.get().saturating_add(1));
        }
        words
    }

    fn transfer(&mut self, block: usize, state: &Arrival) -> Arrival {
        let ctx = self.ctx;
        let mut words = state.words.clone();
        for pc in self.blocks.ranges[block].clone() {
            let instruction = self.function.instruction_at(pc);
            step(ctx, pc, instruction, &mut words);
        }
        Arrival { block, words }
    }

    /// An exception edge may leave the block before any of its writes, so it carries no fact. A normal edge carries
    /// the fact its block's conditional jump proves on it (`Solver::refine`).
    fn transfer_edge_at(&self, block: usize, index: usize, out: &Arrival) -> Arrival {
        let target = self.blocks.targets[block][index];
        let words = match self.blocks.kinds[block][index] {
            ControlEdge::Handler => vec![Word::UNKNOWN; out.words.len()],
            ControlEdge::Normal => self.refine(block, target, &out.words),
        };
        Arrival { block: target, words }
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
                    }
                    slot => *slot = Some(edge),
                }
            }
        }
        next
    }
}

/// The jump target of a conditional jump (`JumpIf`, or the fused `CompareJump`); `None` for any other instruction.
pub(super) fn conditional_target(instruction: WideInstruction) -> Option<usize> {
    matches!(Operation::<u64, ()>::of(instruction), Some(Operation::JumpIf { .. } | Operation::CompareJump { .. }))
        .then(|| instruction.jump_target() as usize)
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
    fn replay(&self, block: usize, mut state: Vec<Word>, record: &mut Record) {
        let mut masks: Vec<Option<i64>> = vec![None; state.len()];
        let ctx = self.ctx;
        for pc in self.blocks.ranges[block].clone() {
            let instruction = self.function.instruction_at(pc);
            record.observe(ctx, instruction, pc, &state);
            compared_bounds(ctx, instruction, &state, record);
            let events = step(ctx, pc, instruction, &mut state);
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
