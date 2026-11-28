//! The register dataflow of one function (`dataflow::solve` over its basic blocks): each
//! register's fact at each point, and what the function does that the closed-world facts read.

use super::blocks::Blocks;
use super::effects::{Ctx, Events, step};
use super::scope::Scope;
use super::value::{Fact, Key, PrototypeBase};
use crate::bytecode::{ControlEdge, Function, Successors, Traits, WideInstruction, used_registers};
use crate::dataflow::{Dataflow, solve};
use crate::typed_ir::{Representation, Type};

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
    pub(super) callee: Fact,
    pub(super) target: Target,
    pub(super) construct: bool,
    /// The fact of the `this` it passes its callee (`staged::Receiver`): a register's, the caller's own
    /// `this` or a field of it, an allocated object's; unknown for any other receiver.
    pub(super) receiver: Fact,
    /// The facts of the arguments it passes (`staged::Invocation::arguments`), in order.
    pub(super) arguments: Vec<Fact>,
}

/// What one function contributes to the closed-world facts.
pub(super) struct FunctionFlow {
    pub(super) calls: Vec<Call>,
    /// Every binding write, with the fact of the value written.
    pub(super) writes: Vec<(Key, Fact)>,
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
    pub(super) field_owners: std::collections::BTreeMap<u32, super::value::Class>,
    /// How many parameters the function binds from its arguments one to one: `None` for a rest,
    /// generator or async function, whose parameters the calls do not type (`types::parameters`).
    pub(super) arity: Option<u16>,
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
    let ctx = Ctx {
        id,
        parameter_end: function.parameter_end_pc as usize,
        is_root: function.parent.is_none(),
        scope,
        resolved,
        registers: usize::from(function.registers),
        types,
        decode,
        records,
    };
    let mut solver = Solver { ctx: &ctx, function, blocks: &blocks };
    let inputs = solve(&mut solver);
    let mut record = Record::new(ctx.registers);
    for (block, input) in inputs.into_iter().enumerate() {
        if let Some(state) = input {
            solver.replay(block, state, &mut record);
        }
    }
    let arity = (!function.rest && !function.is_generator && !function.is_async).then_some(function.params);
    record.finish(arity)
}

impl Dataflow for Solver<'_> {
    type State = Vec<Fact>;

    fn blocks(&self) -> usize {
        self.blocks.ranges.len()
    }

    fn edges(&self, block: usize) -> &[usize] {
        &self.blocks.targets[block]
    }

    fn entry_state(&self, block: usize) -> Option<Vec<Fact>> {
        (block == 0).then(|| self.entry_facts())
    }

    fn join(&self, into: &mut Vec<Fact>, incoming: &Vec<Fact>) -> bool {
        let mut changed = false;
        for (slot, &fact) in into.iter_mut().zip(incoming) {
            let joined = slot.join(fact);
            changed |= joined != *slot;
            *slot = joined;
        }
        changed
    }

    fn transfer(&mut self, block: usize, state: &Vec<Fact>) -> Vec<Fact> {
        let mut out = state.clone();
        for pc in self.blocks.ranges[block].clone() {
            step(self.ctx, pc, self.function.instruction_at(pc), &mut out);
        }
        out
    }

    /// An exception edge may leave the block before any of its writes, so it carries no fact.
    fn transfer_edge_at(&self, block: usize, index: usize, out: &Vec<Fact>) -> Vec<Fact> {
        match self.blocks.kinds[block][index] {
            ControlEdge::Handler => vec![Fact::UNKNOWN; out.len()],
            ControlEdge::Normal => out.clone(),
        }
    }
}

impl Solver<'_> {
    /// The facts at entry: every register unknown, but a promoted parameter holds its argument, whose
    /// type the pass before found (`Types::param`). A parameter the body keeps as a local stays unknown:
    /// only its promoted register holds the argument at entry (`Function::promoted_register`).
    fn entry_facts(&self) -> Vec<Fact> {
        let mut state = vec![Fact::UNKNOWN; self.ctx.registers];
        if let Some(types) = self.ctx.types {
            for index in 0..self.function.params {
                if let Some(register) = self.function.promoted_register(index) {
                    state[usize::from(register)] = Fact::of_class(types.param(self.ctx.id, index), types.param_class(self.ctx.id, index));
                }
            }
        }
        state
    }

    /// Replays `block` from its input `state`, recording what its instructions do.
    fn replay(&self, block: usize, mut state: Vec<Fact>, record: &mut Record) {
        for pc in self.blocks.ranges[block].clone() {
            let instruction = self.function.instruction_at(pc);
            record.observe(self.ctx, instruction, pc, &state);
            let events = step(self.ctx, pc, instruction, &mut state);
            record.commit(instruction, &state, events);
        }
    }
}

/// What a replay gathers over a function's reachable code.
struct Record {
    calls: Vec<Call>,
    writes: Vec<(Key, Fact)>,
    wild_owner: bool,
    wild_global: bool,
    wild_member: bool,
    prototype_writes: Vec<(PrototypeBase, u32)>,
    /// Per register, the join of the facts it is defined with.
    defs: Vec<Option<Fact>>,
    /// Per register, whether a read saw a value that is not a Number.
    unsafe_reads: Vec<bool>,
    field_owners: std::collections::BTreeMap<u32, super::value::Class>,
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
        }
    }

    /// Notes the reads and the calls `instruction` makes, from the facts before it.
    fn observe(&mut self, ctx: &Ctx<'_>, instruction: WideInstruction, pc: usize, state: &[Fact]) {
        for register in used_registers(instruction) {
            if !state[usize::from(register)].is_number() {
                self.unsafe_reads[usize::from(register)] = true;
            }
        }
        if let Some(owner) = super::effects::field_owner(ctx, instruction, state) {
            let joined = self.field_owners.entry(pc as u32).or_insert(super::value::Class::Never);
            *joined = joined.join(owner.class);
        }
        let traits = instruction.op().traits();
        let construct = traits.has(Traits::CONSTRUCTION);
        // Every call form the staged decoder reads (`staged::Invocation`): a register call, a known
        // function's, a method site's, a construction; not an eval, `super(...)` or an array of arguments.
        if let Some(invocation) = crate::staged::Invocation::of(instruction, ctx.decode) {
            let (target, callee) = match invocation.callee {
                crate::staged::Callee::Register(register) => (Target::Register, state[usize::from(register)]),
                crate::staged::Callee::Known(function) => (Target::Known(u32::from(function)), Fact::UNKNOWN),
                crate::staged::Callee::Method { atom, .. } => (Target::Method(atom), Fact::UNKNOWN),
            };
            let arguments = invocation.arguments.registers(ctx.records).map(|register| state[usize::from(register)]).collect();
            let this = || ctx.types.map_or(Type::ANY, |types| types.this(ctx.id));
            let receiver = match invocation.receiver {
                crate::staged::Receiver::Register(register) => state[usize::from(register)],
                crate::staged::Receiver::This { path: None } if !ctx.is_root =>
                    Fact::of_class(this(), ctx.types.map_or(super::value::Class::Many, |types| types.this_class(ctx.id))),
                crate::staged::Receiver::This { path: Some((atom, _)) } => ctx.types.map_or(Fact::UNKNOWN,
                    |types| Fact::of_class(types.field(atom), types.class(super::value::Key::Member(atom)))),
                // A construction's receiver is the object it allocates: an instance of the class it constructs.
                crate::staged::Receiver::Allocated => match callee.ty.body_index() {
                    Some(class) if construct => Fact::instance(class),
                    _ => Fact::of(Type::DIRECT_OBJECT_FAMILY),
                },
                _ => Fact::UNKNOWN,
            };
            self.calls.push(Call { pc: pc as u32, callee, target, construct, receiver, arguments });
        }
    }

    /// Notes the writes `instruction` made and the facts it defined, from the facts after it.
    fn commit(&mut self, instruction: WideInstruction, state: &[Fact], events: Events) {
        self.wild_owner |= events.wild_owner;
        self.wild_global |= events.wild_global;
        self.wild_member |= events.wild_member;
        self.writes.extend(events.writes);
        self.prototype_writes.extend(events.prototype_write);
        for register in instruction.written_registers() {
            let index = usize::from(register);
            self.defs[index] = Some(self.defs[index].map_or(state[index], |def| def.join(state[index])));
        }
    }

    fn finish(self, arity: Option<u16>) -> FunctionFlow {
        let numbers: Box<[Option<Representation>]> = self
            .defs
            .iter()
            .zip(&self.unsafe_reads)
            .map(|(def, &unsafe_read)| if unsafe_read { None } else { def.and_then(Fact::representation) })
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
            arity,
        }
    }
}
