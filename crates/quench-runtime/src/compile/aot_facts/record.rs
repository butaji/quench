//! What the register solve gathers over a function's reachable code, once its input facts settle: the calls it makes,
//! the bindings and properties it writes, and the cells and sources its registers hold at each point (`RegisterCells`).
//! `FunctionFlow` is the record's result.

use super::effects::{Ctx, Events, decoded, is_global_load, root_this, runs_guest_code, settled, this_fact};
use super::escapes::method_atom;
use super::quiet::{self, Quiet};
use super::flow::{Call, FunctionFlow, Target};
use super::registers::RegisterCells;
use super::shape::{self, Shape};
use super::value::{Accessor, Key, Origin, Word};
use crate::bytecode::{Traits, WideInstruction, used_registers};
use crate::staged::effects::{Affine, Indices};
use crate::typed_ir::{Cell, Classes, ElementClass, Elements, Length, Representation, Type, View};

/// The array an element store writes and the indices it touches, read before the store: the array's class as the census
/// keys the store (`Key::Element`), and the indices (`indices_of`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ElementStore {
    pub(crate) class: ElementClass,
    pub(crate) indices: Indices,
}

/// The indices an element store through `index` touches in `function`. A Number that is parameter `p` on entry
/// (`Origin::Parameter`, which a write to the register retires, and a join with any other origin drops) is the one
/// index `p`: `Span { start: p, len: 1 }`. Any other index may be any index: `Whole`.
pub(super) fn indices_of(function: u32, index: Word) -> Indices {
    match index.origin {
        Some(Origin::Parameter { function: owner, index: parameter }) if owner == function && index.is_number() =>
            Indices::Span { start: Affine::parameter(parameter), len: Affine::constant(1) },
        _ => Indices::Whole,
    }
}

/// What a replay gathers over a function's reachable code.
pub(super) struct Record {
    calls: Vec<Call>,
    writes: Vec<(Key, Word)>,
    wild_owner: bool,
    wild_global: bool,
    computed_global: bool,
    global_keyed: bool,
    global_source: bool,
    plain_calls: Vec<Option<u32>>,
    reads_this: bool,
    wild_member: bool,
    /// Per register, the join of the facts it is defined with.
    defs: Vec<Option<Word>>,
    /// Per register, whether a read saw a value that is not a Number.
    unsafe_reads: Vec<bool>,
    field_owners: std::collections::BTreeMap<u32, crate::typed_ir::Classes>,
    /// Per element access (a read or a store), by PC, the view of the array its base holds before it (`settled`).
    element_views: std::collections::BTreeMap<u32, View>,
    /// The capture slots the function reads from its parent's environment (depth 0), as (owner, slot).
    captures: std::collections::BTreeSet<(u32, u16)>,
    pub(super) registers: RegisterCells,
    reads: std::collections::BTreeSet<Key>,
    links: Vec<(ElementClass, ElementClass)>,
    wild_elements: bool,
    shapes: Vec<Shape>,
    handed: Vec<Word>,
    closures: Vec<u32>,
    guest_runs: Vec<u32>,
    quiet: Vec<(u32, Quiet)>,
    /// The integer bounds the replay puts on keys, in replay order (`flow::Solver::replay`, `FunctionFlow::bounds`).
    pub(super) bounds: Vec<(Key, i64)>,
    /// Per pc of an element store (`ElementWrite`), the array it writes and the indices it touches (`ElementStore`).
    element_stores: std::collections::BTreeMap<u32, ElementStore>,
    accessors: Vec<Accessor>,
    parents: Vec<(u32, u32)>,
}

impl Record {
    pub(super) fn new(registers: usize) -> Self {
        Self {
            calls: Vec::new(),
            writes: Vec::new(),
            wild_owner: false,
            wild_global: false,
            computed_global: false,
            global_keyed: false,
            global_source: false,
            plain_calls: Vec::new(),
            reads_this: false,
            wild_member: false,
            defs: vec![None; registers],
            unsafe_reads: vec![false; registers],
            field_owners: std::collections::BTreeMap::new(),
            element_views: std::collections::BTreeMap::new(),
            captures: std::collections::BTreeSet::new(),
            registers: RegisterCells::default(),
            reads: std::collections::BTreeSet::new(),
            links: Vec::new(),
            wild_elements: false,
            shapes: Vec::new(),
            handed: Vec::new(),
            closures: Vec::new(),
            guest_runs: Vec::new(),
            quiet: Vec::new(),
            bounds: Vec::new(),
            element_stores: std::collections::BTreeMap::new(),
            accessors: Vec::new(),
            parents: Vec::new(),
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
        // An element store writes the elements of its array's class, at the indices its index register holds.
        if let Some(crate::staged::Operation::ElementWrite { base, key, .. }) = decoded(instruction) {
            let (array, index) = (state[usize::from(base)], state[usize::from(key)]);
            self.element_stores.insert(pc as u32, ElementStore { class: array.elements_key(), indices: indices_of(ctx.id, index) });
        }
        // An element read reads the elements of its array's class (`Key::Element`, `ElementClasses`).
        if instruction.op() == crate::bytecode::Op::GetIndex {
            self.reads.insert(Key::Element(state[usize::from(instruction.register_b())].elements_key()));
        }
        // An element access (a read or a store) keeps the view of the array its base holds, so a tier reads a typed array's
        // element by its kind (`FunctionFacts::element_view`).
        let element_base = match decoded(instruction) {
            Some(crate::staged::Operation::ElementWrite { base, .. }) => Some(base),
            _ if instruction.op() == crate::bytecode::Op::GetIndex => Some(instruction.register_b()),
            _ => None,
        };
        if let Some(base) = element_base {
            let view = settled(ctx, state[usize::from(base)]).elements.view;
            let joined = self.element_views.entry(pc as u32).or_insert(View::None);
            *joined = joined.join(view);
        }
        // A capture of the parent's environment reads the parent's slot: its cell is the slot's settled cell, which the
        // function depends on as a `Key::Slot` (`AotFacts::captured_slot_cell`).
        if let Some(crate::staged::Operation::LoadCapture { depth, slot, .. }) = decoded(instruction)
            && let Some(owner) = ctx.scope.owner(ctx.id, depth)
        {
            self.reads.insert(Key::Slot { owner, slot });
            if depth == 0 {
                self.captures.insert((owner, slot));
            }
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
                crate::staged::Receiver::This { path: None } => if ctx.is_root { root_this(ctx) } else { this_fact(ctx) },
                crate::staged::Receiver::This { path: Some((atom, _)) } => match ctx.types {
                    Some(_) => super::effects::this_member(ctx, atom),
                    None => Word::UNKNOWN,
                },
                // A construction's receiver is the object it allocates: an instance of the class it constructs.
                crate::staged::Receiver::Allocated => match callee.cell.ty.body_index() {
                    Some(class) if construct => Word::instance(class),
                    _ => Word::of(Type::DIRECT_OBJECT_FAMILY),
                },
                // A call with no receiver passes `undefined`: a sloppy callee's `this` is the global object, which `Call::plain`
                // makes of the callee's `this` (`types::Types::receivers`). The receiver of any other form may be the global object.
                crate::staged::Receiver::Undefined => Word::UNKNOWN,
                _ => Word::of_cell(Cell::ANY.maybe_global()),
            };
            let plain = matches!(invocation.receiver, crate::staged::Receiver::Undefined);
            let call = Call { pc: pc as u32, callee, target, construct, receiver, plain, arguments };
            let method = method_atom(&call);
            // An array mutator (`push`, `splice`, ...) makes the arrays its receiver names hold its arguments, and an unknown
            // length (`Scope::is_array_mutator`).
            if let Some(atom) = method.filter(|&atom| ctx.scope.is_array_mutator(atom)) {
                let stored = call.arguments.iter().fold(Cell::NEVER, |cell, argument| cell.join(argument.cell));
                let elements = Elements { ty: Type::NEVER, classes: Classes::Never, length: Length::Any, view: View::None, holes: false };
                let written = Cell { ty: stored.ty, classes: stored.classes, elements, global: stored.global };
                self.writes.push((Key::Element(call.receiver.elements_key()), Word::of_cell(written)));
                // A builtin mutator on an array runs no code, so its receiver is no escape (`escapes`). The arrays it stores are
                // read back by an element read, which takes the unnamed class, as an element store's do (`Operation::ElementWrite`).
                if call.receiver.cell.elements != Elements::NEVER && ctx.scope.is_array_builtin(atom) {
                    self.links.extend(call.arguments.iter().filter_map(|argument| argument.link(ElementClass::Unnamed)));
                }
            }
            if matches!(invocation.receiver, crate::staged::Receiver::This { .. }) {
                self.reads_this = true;
            }
            if !matches!(invocation.receiver, crate::staged::Receiver::Register(_) | crate::staged::Receiver::This { .. } | crate::staged::Receiver::Allocated) {
                self.plain_calls.push(match call.target {
                    Target::Known(function) => Some(function),
                    Target::Register => call.callee.cell.ty.body_index(),
                    Target::Method(_) | Target::Accessor { .. } => None,
                });
            }
            if method.is_some_and(|atom| ctx.scope.passes_this(atom)) {
                self.global_source = true;
            }
            self.shapes.extend(shape::of_call(ctx, pc as u32, &call));
            if let Some(proof) = quiet::of_call(ctx.scope, &call) {
                self.quiet.push((pc as u32, proof));
            }
            if let Some(link) = super::accessors::heritage_link(ctx, &call) {
                self.parents.push(link);
            }
            self.calls.push(call);
        }
        // A named field read or store may run the accessor its receiver's class defines for the name (`Target::Accessor`).
        if let Some(access) = super::accessors::accessor_call(ctx, pc as u32, instruction, state) {
            self.calls.push(access);
        }
    }

    /// Notes the writes `instruction` made and the facts it defined, from the facts after it.
    pub(super) fn commit(&mut self, instruction: WideInstruction, state: &[Word], events: Events) {
        self.wild_owner |= events.wild_owner;
        self.wild_global |= events.wild_global;
        self.computed_global |= events.computed_store;
        self.global_keyed |= events.global_keyed;
        self.global_source |= events.global_source;
        self.wild_member |= events.wild_member;
        self.wild_elements |= events.wild_elements;
        self.links.extend(events.writes.iter().filter_map(|(key, word)| word.link(key.elements_class())).chain(events.links));
        self.writes.extend(events.writes);
        self.accessors.extend(events.accessors);
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

    pub(super) fn finish(mut self, arity: Option<u16>, returns_out: bool, in_register: Box<[bool]>, strict: bool) -> FunctionFlow {
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
            global_keyed: self.global_keyed,
            global_source: self.global_source,
            plain_calls: self.plain_calls,
            reads_this: self.reads_this,
            member_wild: self.wild_member,
            wild_elements: self.wild_elements,
            numbers,
            field_owners: self.field_owners,
            element_views: self.element_views,
            captures: self.captures,
            registers: self.registers,
            reads: self.reads,
            links: self.links,
            shapes: self.shapes,
            guest_runs: self.guest_runs,
            quiet: self.quiet,
            arity,
            in_register,
            returns_out,
            strict,
            handed: self.handed,
            closures: self.closures,
            bounds: self.bounds,
            entries: Vec::new(),
            element_stores: self.element_stores,
            accessors: self.accessors,
            parents: self.parents,
        }
    }
}
