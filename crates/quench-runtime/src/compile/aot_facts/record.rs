//! What the register solve gathers over a function's reachable code, once its input facts settle: the calls it makes,
//! the bindings and properties it writes, and the cells and sources its registers hold at each point (`RegisterCells`).
//! `FunctionFlow` is the record's result.

use super::effects::{Ctx, Events};
use super::flow::{Call, FunctionFlow, Target};
use super::registers::RegisterCells;
use super::value::{Key, Origin, PrototypeBase, Word};
use crate::bytecode::{Traits, WideInstruction, used_registers};
use crate::typed_ir::{Cell, Classes, Elements, Length, Representation, Type};

/// What a replay gathers over a function's reachable code.
pub(super) struct Record {
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
    pub(super) registers: RegisterCells,
    reads: std::collections::BTreeSet<Key>,
}

impl Record {
    pub(super) fn new(registers: usize) -> Self {
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
    pub(super) fn observe(&mut self, ctx: &Ctx<'_>, instruction: WideInstruction, pc: usize, state: &[Word]) {
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
    pub(super) fn commit(&mut self, instruction: WideInstruction, state: &[Word], events: Events) {
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

    pub(super) fn finish(self, arity: Option<u16>) -> FunctionFlow {
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
