//! What the register solve gathers over a function's reachable code, once its input facts settle: the calls it makes,
//! the bindings and properties it writes, and the cells and sources its registers hold at each point (`RegisterCells`).
//! `FunctionFlow` is the record's result.

use super::effects::{Ctx, Events, decoded, is_global_load, runs_guest_code};
use super::quiet::{self, Quiet};
use super::flow::{Call, FunctionFlow, Target};
use super::registers::RegisterCells;
use super::shape::{self, Shape};
use super::value::{Key, Origin, Word};
use crate::bytecode::{Traits, WideInstruction, used_registers};
use crate::typed_ir::{Cell, Classes, ElementClass, Elements, Length, Representation, Type};

/// What a replay gathers over a function's reachable code.
pub(super) struct Record {
    calls: Vec<Call>,
    writes: Vec<(Key, Word)>,
    wild_owner: bool,
    wild_global: bool,
    computed_global: bool,
    global_source: bool,
    plain_calls: Vec<Option<u32>>,
    reads_this: bool,
    wild_member: bool,
    /// Per register, the join of the facts it is defined with.
    defs: Vec<Option<Word>>,
    /// Per register, whether a read saw a value that is not a Number.
    unsafe_reads: Vec<bool>,
    field_owners: std::collections::BTreeMap<u32, crate::typed_ir::Classes>,
    pub(super) registers: RegisterCells,
    reads: std::collections::BTreeSet<Key>,
    links: Vec<(ElementClass, ElementClass)>,
    wild_elements: bool,
    shapes: Vec<Shape>,
    handed: Vec<Word>,
    closures: Vec<u32>,
    guest_runs: Vec<u32>,
    quiet: Vec<(u32, Quiet)>,
}

impl Record {
    pub(super) fn new(registers: usize) -> Self {
        Self {
            calls: Vec::new(),
            writes: Vec::new(),
            wild_owner: false,
            wild_global: false,
            computed_global: false,
            global_source: false,
            plain_calls: Vec::new(),
            reads_this: false,
            wild_member: false,
            defs: vec![None; registers],
            unsafe_reads: vec![false; registers],
            field_owners: std::collections::BTreeMap::new(),
            registers: RegisterCells::default(),
            reads: std::collections::BTreeSet::new(),
            links: Vec::new(),
            wild_elements: false,
            shapes: Vec::new(),
            handed: Vec::new(),
            closures: Vec::new(),
            guest_runs: Vec::new(),
            quiet: Vec::new(),
        }
    }

    /// Notes the element-class links the solve's joins made (`Word::join_links`): a word of two origins joins the unnamed class.
    pub(super) fn joined(&mut self, links: std::collections::BTreeSet<(ElementClass, ElementClass)>) {
        self.links.extend(links);
    }

    /// Notes the reads and the calls `instruction` makes, from the facts before it.
    pub(super) fn observe(&mut self, ctx: &Ctx<'_>, instruction: WideInstruction, pc: usize, state: &[Word]) {
        // A thrown value leaves for a handler or the host; a yield or await carries its live registers to a consumer
        // or a promise reaction. Either hands them to code no recorded call reaches.
        if let Some(crate::staged::Operation::Throw { source }) = crate::staged::Operation::<u64, ()>::of(instruction) {
            self.handed.push(state[usize::from(source)]);
        }
        if matches!(instruction.op(), crate::bytecode::Op::Yield | crate::bytecode::Op::YieldStar | crate::bytecode::Op::Await) {
            self.handed.extend_from_slice(state);
        }
        for register in used_registers(instruction) {
            if !state[usize::from(register)].is_number() {
                self.unsafe_reads[usize::from(register)] = true;
            }
        }
        // An element read reads the elements of its array's class (`Key::Element`, `ElementClasses`).
        if instruction.op() == crate::bytecode::Op::GetIndex {
            self.reads.insert(Key::Element(state[usize::from(instruction.register_b())].elements_key()));
        }
        if runs_guest_code(ctx, instruction, state) {
            self.guest_runs.push(pc as u32);
        }
        if matches!(decoded(instruction), Some(crate::staged::Operation::LoadThis { .. } | crate::staged::Operation::SetField { base: None, .. })) {
            self.reads_this = true;
        }
        self.note_quiet(ctx, instruction, pc as u32);
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
            // A receiver or an argument is read back by the callee with no origin (`this`, a parameter), so its array joins the
            // unnamed class: the callee's reads of it see the stores this call's callee makes.
            self.links.extend(receiver.link(ElementClass::Unnamed));
            self.links.extend(arguments.iter().filter_map(|argument| argument.link(ElementClass::Unnamed)));
            let call = Call { pc: pc as u32, callee, target, construct, receiver, arguments };
            if matches!(invocation.receiver, crate::staged::Receiver::This { .. }) {
                self.reads_this = true;
            }
            if !matches!(invocation.receiver, crate::staged::Receiver::Register(_) | crate::staged::Receiver::This { .. } | crate::staged::Receiver::Allocated) {
                self.plain_calls.push(match call.target {
                    Target::Known(function) => Some(function),
                    Target::Register => call.callee.cell.ty.body_index(),
                    Target::Method(_) => None,
                });
            }
            if method.is_some_and(|atom| ctx.scope.passes_this(atom)) {
                self.global_source = true;
            }
            self.shapes.extend(shape::of_call(ctx, pc as u32, &call));
            if let Some(proof) = quiet::of_call(ctx.scope, &call) {
                self.quiet.push((pc as u32, proof));
            }
            self.calls.push(call);
        }
    }

    /// Notes the writes `instruction` made and the facts it defined, from the facts after it.
    pub(super) fn commit(&mut self, instruction: WideInstruction, state: &[Word], events: Events) {
        self.wild_owner |= events.wild_owner;
        self.wild_global |= events.wild_global;
        self.computed_global |= events.computed_store;
        self.global_source |= events.global_source;
        self.wild_member |= events.wild_member;
        self.wild_elements |= events.wild_elements;
        self.links.extend(events.writes.iter().filter_map(|(key, word)| word.link(key.elements_class())).chain(events.links));
        self.writes.extend(events.writes);
        self.shapes.extend(events.shape);
        for register in instruction.written_registers() {
            let index = usize::from(register);
            self.defs[index] = Some(self.defs[index].map_or(state[index], |def| def.join(state[index])));
            // A register written with a closure of known identity materializes that closure.
            if let Some(function) = state[index].cell.ty.body_index() { self.closures.push(function); }
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

    /// Notes the proofs `instruction` at `pc` may hold that it runs no guest code: a global name read (`Declared`, and
    /// `Intrinsic` for a builtin the table names) unless it is an invocation, and a read of field off `this` (`OwnField`).
    fn note_quiet(&mut self, ctx: &Ctx<'_>, instruction: WideInstruction, pc: u32) {
        if is_global_load(instruction) && crate::staged::Invocation::of(instruction, ctx.decode).is_none() {
            let atom = instruction.atom_index();
            self.quiet.push((pc, Quiet::Declared(atom)));
            if ctx.scope.builtin(None, atom).is_some() {
                self.quiet.push((pc, Quiet::Intrinsic { object: None, name: atom }));
            }
        }
        if matches!(instruction.op().class(), crate::bytecode::Class::GetField)
            && let Some(crate::bytecode::FieldLookup::Atom { atom, base, .. }) = instruction.field_lookup()
            && base == crate::bytecode::FieldBase::THIS
        {
            self.quiet.push((pc, Quiet::OwnField(atom)));
            self.reads_this = true;
        }
    }

    pub(super) fn finish(mut self, arity: Option<u16>, returns_out: bool) -> FunctionFlow {
        // Blocks are replayed in index order, so the PCs are sorted here: `class_layout` searches them.
        self.shapes.sort_by_key(|shape| shape.pc);
        self.guest_runs.sort_unstable();
        self.quiet.sort_by_key(|(pc, _)| *pc);
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
            computed_global: self.computed_global,
            global_source: self.global_source,
            plain_calls: self.plain_calls,
            reads_this: self.reads_this,
            member_wild: self.wild_member,
            wild_elements: self.wild_elements,
            numbers,
            field_owners: self.field_owners,
            registers: self.registers,
            reads: self.reads,
            links: self.links,
            shapes: self.shapes,
            guest_runs: self.guest_runs,
            quiet: self.quiet,
            arity,
            returns_out,
            handed: self.handed,
            closures: self.closures,
        }
    }
}
