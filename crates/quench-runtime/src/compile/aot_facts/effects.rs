//! What one instruction does to a function's register facts, and to the bindings it writes.

#[cfg(not(stencil_build))]
#[allow(unused_imports)] use crate::staged::{OperationOperationExt};
mod fields;
mod strings;

pub(super) use self::fields::{base_fact, field_owner, this_fact, this_member};
#[allow(unused_imports)]
use crate::staged::AllocationAllocationExt;
use self::fields::{base_fact, member_identity, member_stores, member_type, named_word, prototype_base};
use self::strings::{string_call, string_length};
use super::scope::Scope;
use super::value::{Accessor, Word, Key, Origin};
use crate::typed_ir::{BuiltinPath, Cell, Classes, ElementClass, Elements, Holders, Identity, Length, Site, Sites, View};
use crate::bytecode::{FieldBase, FieldLookup, Operand, Register, Traits, WideInstruction};
use crate::typed_ir::Type;
use crate::staged::typing::{self, Abstract};
use crate::staged::{Allocation, Comparison, DecodeContext, NumberOperation, Operation, Protocol};
use oxc_syntax::operator::BinaryOperator;

/// The function an instruction belongs to, and where its parameters end.
pub(super) struct Ctx<'a> {
    /// The function's id: the owner of its own environment and frame slots.
    pub(super) id: u32,
    /// The PC after the parameters' initialization: stores before it bind arguments.
    pub(super) parameter_end: usize,
    /// Whether the function is a root, whose `this` is the global object in a script.
    pub(super) is_root: bool,
    pub(super) scope: &'a Scope,
    /// Bindings the census proved hold one closure, so their loads carry its fact too.
    pub(super) resolved: &'a std::collections::BTreeMap<Key, u32>,
    /// The class ids of the program's constructors (`class_ids::ClassIds`): a construction's instance and a prototype read
    /// take the class of the closure's body and the key it was read from.
    pub(super) classes: &'a super::class_ids::ClassIds,
    pub(super) registers: usize,
    /// Per PC, whether the `MakeArray` there is a literal whose element definitions cover every index, so it
    /// holds no hole (`flow::whole_literals`).
    pub(super) whole_literals: &'a [bool],
    /// The field and return types the pass before found (`types`), which this pass's field reads
    /// and calls take; `None` in the passes that find call targets, which read them as any value.
    pub(super) types: Option<&'a super::types::Types>,
    /// The program's method sites, which a method call's operands are read from (`staged::Invocation`).
    pub(super) decode: crate::staged::DecodeContext<'a>,
    /// The registers the method sites list as their arguments (`ResidualProgram::method_arguments`).
    pub(super) records: &'a [crate::bytecode::Register],
    /// The names some accessor definition defines (`accessors::names_of`): a store of one may run its setter, so it is a
    /// guest run (`runs_guest_code`), and no class's field takes the stored value (`fields::member_stores`).
    pub(super) accessors: &'a std::collections::BTreeSet<u32>,
    /// The allocations the previous round proved filled (`length_settle::Feedback`), by (function, pc): an `new Array(n)` there
    /// holds no hole.
    pub(super) filled: &'a std::collections::BTreeSet<(u32, u32)>,
    /// The element stores the previous round proved write no hole, by (function, pc) (`length_settle::Feedback`).
    pub(super) hole_free_stores: &'a std::collections::BTreeSet<(u32, u32)>,
    /// The element reads the previous round proved in bounds, by (function, pc) (`length_settle::Feedback`).
    pub(super) in_bounds_reads: &'a std::collections::BTreeSet<(u32, u32)>,
    /// The element stores the previous round proved cover a span of parameters, by (function, pc): (start, len) (`length_settle::Feedback`).
    pub(super) spans: &'a std::collections::BTreeMap<(u32, u32), (u16, u16)>,
}

/// What an instruction does beyond its registers: the bindings it writes, and whether it makes
/// writes the analysis cannot follow.
#[derive(Default)]
pub(super) struct Events {
    pub(super) writes: Vec<(Key, Word)>,
    pub(super) wild_global: bool,
    /// Whether the instruction may make the global object reachable as a value: a read of a global object's name, or
    /// the root's `this`. Sloppy plain calls and `call`/`apply`/`bind` are sources too (`flow::Record::observe`).
    pub(super) global_source: bool,
    /// Whether a store through a non-Number key may write a name of the global object (`writes_global_name`).
    pub(super) computed_store: bool,
    /// Whether the instruction keys an access into a value that may be the global object (`keyed_global`).
    pub(super) global_keyed: bool,
    pub(super) wild_owner: bool,
    /// The holders of the objects a store under a name the analysis cannot list writes into (`FunctionFlow::wild`).
    pub(super) wild: Holders,
    /// The holders of each named store's object, by name (`FunctionFlow::member_bases`).
    pub(super) member_bases: Vec<(u32, Holders)>,
    /// The element-class links the instruction's stores make beyond its `writes` (`Word::link`): a value stored as an
    /// element, whose array joins the unnamed class its element reads come back with.
    pub(super) links: Vec<(ElementClass, ElementClass)>,
    /// Whether the instruction writes elements the analysis cannot name to an array (`IndexWrite`, a length store
    /// through an object of no known origin): the arrays of every class may change, so every class's elements are any.
    pub(super) wild_elements: bool,
    /// The change the instruction makes to a named property or a key the analysis cannot name (`shape`).
    pub(super) shape: Option<super::shape::Shape>,
    /// The accessors the instruction defines on a proven class object (`accessor_definition`).
    pub(super) accessors: Vec<Accessor>,
}

/// The cell `word` holds as the settled solve reads it: its elements joined with those of its element class
/// (`Types::stored_elements`). A store into any array of the class is a store into every array of it, so an array
/// read through another register, a field or a closure keeps the length and element types the class's stores leave.
/// A value that holds no array (`Elements::NEVER`) is its own cell, as is every value before the types exist.
pub(super) fn settled(ctx: &Ctx<'_>, word: Word) -> Cell {
    match ctx.types {
        Some(types) if word.cell.elements != Elements::NEVER =>
            Cell { elements: word.cell.elements.join(types.stored_elements(word.elements_key())), ..word.cell },
        _ => word.cell,
    }
}

/// Whether the instruction at a point may run guest code with the objects it touches: a call or construction, a getter,
/// setter or proxy trap, a conversion that runs `valueOf`. It is the operation's effect row (`Effects::may_call_js`),
/// met with what the facts before it (`state`) prove: a Number or binary operation over Numbers runs none, and a
/// captured or environment binding's read runs none. A store through `this` runs none unless its name is an accessor's
/// (`Ctx::accessors`): then it may be the setter's call, which the class layout reads as a call. Whatever does not decode
/// runs guest code.
pub(super) fn runs_guest_code(ctx: &Ctx<'_>, instruction: WideInstruction, state: &[Word]) -> bool {
    if crate::staged::Invocation::of(instruction, ctx.decode).is_some() {
        return true;
    }
    let operation = Operation::<(), ()>::decode_completed(instruction, ctx.decode).map(|(operation, _)| operation).or_else(|| decoded(instruction));
    match operation {
        None => true,
        Some(Operation::SetField { atom, .. }) if ctx.accessors.contains(&atom) => true,
        Some(Operation::SetField { base: None, .. }) => false,
        Some(Operation::LoadEnvLocal { .. } | Operation::LoadCapture { .. }) => false,
        Some(Operation::Number(number)) => !numbers_over_numbers(ctx, state, number),
        Some(Operation::Binary { left, right, .. }) => !(is_number(ctx, state, left) && is_number(ctx, state, right)),
        // Strict equality runs nothing; loose equality runs nothing when an operand is null or undefined (no ToPrimitive).
        Some(Operation::Equality { strict, left, right, .. }) => !strict && !(is_nullish(ctx, state, left) || is_nullish(ctx, state, right)),
        Some(Operation::CompareJump { comparison: Comparison::Equality { strict, .. }, left, right, .. }) =>
            !strict && !(is_nullish(ctx, state, left) || is_nullish(ctx, state, right)),
        Some(Operation::CompareJump { comparison, .. }) => comparison.runs_guest_code(),
        Some(operation) => operation.effects().may_call_js,
    }
}

/// Whether the operands of a Number operation are Numbers on the facts before it, so its miss runs no guest code.
fn numbers_over_numbers(ctx: &Ctx<'_>, state: &[Word], number: NumberOperation) -> bool {
    match number {
        NumberOperation::Arithmetic { left, right, .. } | NumberOperation::Remainder { left, right, .. } | NumberOperation::Extreme { left, right, .. }
        | NumberOperation::Bitwise { left, right, .. } | NumberOperation::Imul { left, right, .. } | NumberOperation::Power { base: left, exponent: right, .. }
            => is_number(ctx, state, left) && is_number(ctx, state, right),
        NumberOperation::Unary { source, .. } | NumberOperation::Math { source, .. } | NumberOperation::BitwiseNot { source, .. } => is_number(ctx, state, source),
        NumberOperation::IncDec { source, .. } | NumberOperation::ToNumeric { source, .. } => state[usize::from(source)].is_number(),
        _ => false,
    }
}

/// Whether operand `operand` is exactly null or undefined on the facts before it.
fn is_nullish(ctx: &Ctx<'_>, state: &[Word], operand: Operand) -> bool {
    let ty = typing::operand_cell(&Before { ctx, state }, operand).ty;
    ty == Type::language(crate::ValueType::Null) || ty == Type::language(crate::ValueType::Undefined)
}

/// Whether operand `operand` is a Number on the facts before it.
fn is_number(ctx: &Ctx<'_>, state: &[Word], operand: Operand) -> bool {
    Word::of_cell(typing::operand_cell(&Before { ctx, state }, operand)).is_number()
}

/// The staged operation of `instruction`, with no constants or sites resolved.
pub(super) fn decoded(instruction: WideInstruction) -> Option<Operation<(), ()>> {
    Operation::<(), ()>::decode_completed(instruction, DecodeContext::EMPTY)
        .or_else(|| Operation::<(), ()>::decode_literal_completed_with(instruction, None, ()))
        .map(|(operation, _)| operation)
}

/// Applies `instruction` at `pc` to `state`, the facts before it, and returns its binding effects.
pub(super) fn step(ctx: &Ctx<'_>, pc: usize, instruction: WideInstruction, state: &mut [Word]) -> Events {
    let mut events = Events::default();
    let operation = decoded(instruction);
    // A typed pass reads each value through the program's tables (`Ctx::decode`): an operator with a constant
    // operand decodes only with them.
    let typed_operation = match ctx.types {
        Some(_) => Operation::<(), ()>::decode_completed(instruction, ctx.decode).map(|(operation, _)| operation).or(operation),
        None => operation,
    };
    let mut value = value_of(ctx, pc, instruction, typed_operation, state, &mut events);
    // A call whose callees the solve knows returns the join of their returns, and the array it returns is the array
    // the first callee's `Return` holds (`types::Types::call`).
    if let Some(types) = ctx.types && let Some(word) = types.call(ctx.id, pc as u32) {
        value = word;
    }
    // A return, or an operation whose result completes the frame, writes the function's result.
    if instruction.op().is_return() {
        events.writes.push((Key::Return(ctx.id), state[usize::from(instruction.register_a())]));
    } else if instruction.returns_from_frame() {
        events.writes.push((Key::Return(ctx.id), value));
    }
    // A numeric operation that stores its result into a local writes that slot with the value.
    if let Some(local) = instruction.numeric_local_target() {
        write(ctx, pc, true, &mut events, Key::Slot { owner: ctx.id, slot: local }, value);
    }
    events.shape = super::shape::of_instruction(ctx, pc, instruction, operation.as_ref(), state);
    writes(ctx, pc, instruction, operation, state, &mut events);
    // A literal's element definition joins the element type of the array it defines into (`MakeArray`).
    if instruction.op() == crate::bytecode::Op::DefineArrayElement {
        let element = state[usize::from(instruction.register_a())].cell;
        let array = usize::from(instruction.register_b());
        state[array].cell.elements = state[array].cell.elements
            .join(Elements { ty: element.ty, classes: element.classes, length: Length::Never, view: View::None, holes: false });
    }
    let defined = instruction.defined_register();
    for register in instruction.written_registers() {
        state[usize::from(register)] = if Some(register) == defined { value } else { Word::UNKNOWN };
    }
    events
}

/// Whether `instruction` reads a global name plainly (`LoadName`, `LoadNameCall`).
pub(super) fn is_global_load(instruction: WideInstruction) -> bool {
    let traits = instruction.op().traits();
    traits.has(Traits::GLOBAL_NAME_ACCESS) && traits.has(Traits::NAME_LOAD)
}

/// The fact of the root's `this`: the global object in a Script (sloppy top-level code), `undefined` in a Module, whose
/// code is strict.
pub(super) fn root_this(ctx: &Ctx<'_>) -> Word {
    let cell = if ctx.scope.root_this_is_global() {
        Cell::of(Type::language(crate::ValueType::Object)).maybe_global()
    } else {
        Cell::of(Type::language(crate::ValueType::Undefined))
    };
    Word::of_cell(cell)
}

/// The fact of a load from `key`: the closure the census proved there, else the binding. The closure keeps its origin, so
/// a construction through it takes the class of the body and key it was read from (`ClassIds::of_word`).
/// In a typed pass a binding's load takes the join of its writes (`types::Types::binding`).
fn resolved(ctx: &Ctx<'_>, key: Key) -> Word {
    ctx.resolved.get(&key).map_or_else(
        || Word { origin: Some(Origin::Binding(key)), ..Word::of_cell(ctx.types.map_or(Cell::ANY, |types| types.cell(key))) },
        |&function| Word { origin: Some(Origin::Binding(key)), ..Word::closure(function) })
}

/// The fact of the value `instruction` defines, read from the facts before it.
fn value_of(
    ctx: &Ctx<'_>,
    pc: usize,
    instruction: WideInstruction,
    operation: Option<Operation<(), ()>>,
    state: &[Word],
    events: &mut Events,
) -> Word {
    // A constant's fact is its type (`Scope::constant_type`), and the property key it names where it is a string.
    if let Some((_, index)) = Operation::<(), ()>::decode_constant_load(instruction) {
        let origin = ctx.scope.constant_atoms.get(index).copied().flatten().map(Origin::Atom);
        return Word { origin, ..Word::of(ctx.scope.constant_type(index)) };
    }
    // A method read (`o.m(...)`) also writes `this`, which takes no fact (`step`): its value is the method all the same.
    if matches!(instruction.op().class(), crate::bytecode::Class::GetField) && (instruction.result_flags() & !crate::bytecode::SET_THIS_REGISTER == 0 || instruction.returns_from_frame())
        && let Some(crate::bytecode::FieldLookup::Atom { atom, base, .. }) = instruction.field_lookup()
    {
        let owner = base.register_index().map(|register| state[usize::from(register)]);
        // `F.prototype`: the object a function's methods are stored on, off a closure (the class's
        // prototype) or a binding the census resolves to one.
        if ctx.scope.prototype == Some(atom) && let Some(prototype) = owner.and_then(|owner| prototype_base(ctx.classes, owner)) {
            return Word::from(Origin::Prototype(prototype));
        }
        if let Some(length) = string_length(ctx, base_fact(ctx, base, state), atom) {
            return Word::of(length);
        }
        let cell = member_identity(owner, member_type(ctx, base_fact(ctx, base, state), atom), atom);
        return named_word(ctx, owner, cell, atom);
    }
    // A nested lookup (`base.first.second`) delivers its last field's value.
    if matches!(instruction.op().class(), crate::bytecode::Class::GetField) && (instruction.result_flags() & !crate::bytecode::SET_THIS_REGISTER == 0 || instruction.returns_from_frame())
        && let Some(crate::bytecode::FieldLookup::Site(index)) = instruction.field_lookup()
        && let Some((base, first, second)) = ctx.scope.site_paths.get(index).copied().flatten()
    {
        let cell = member_identity(base_fact(ctx, base, state), member_type(ctx, base_fact(ctx, base, state), first), first);
        return match second {
            Some(second) => Word::member(second, member_type(ctx, Some(Word::of_cell(cell)), second)),
            None => Word::member(first, cell),
        };
    }
    if ctx.types.is_some() && let Some(array) = array_constructed(ctx, pc, instruction, state) {
        return array;
    }
    if ctx.types.is_some() && let Some(typed) = typed_array_constructed(ctx, pc, instruction, state) {
        return typed;
    }
    if ctx.types.is_some() && let Some(string) = string_call(ctx, instruction, state) {
        return string;
    }
    if instruction.op() == crate::bytecode::Op::Construct {
        let callee = state.get(usize::from(instruction.register_b())).copied().unwrap_or(Word::UNKNOWN);
        if let Some(body) = callee.cell.ty.body_index() {
            // The instance of the constructor's class (`ClassIds::of_word`), or the global object a constructor returns in its
            // place (`Types::returns_global`).
            let class = ctx.classes.of_word(callee).unwrap_or(body);
            let mut word = Word::instance(class);
            word.cell.global = ctx.types.is_some_and(|types| types.returns_global(body));
            return word;
        }
        // `new` defines an object whatever it constructs: a constructor's non-object return is replaced by the
        // object it allocated.
        if ctx.types.is_some() { return Word::of(Type::language(crate::ValueType::Object)); }
    }
    if is_global_load(instruction) {
        let atom = instruction.atom_index();
        events.wild_global |= ctx.scope.is_global_object(atom);
        events.global_source |= ctx.scope.is_global_object(atom);
        let mut word = resolved(ctx, Key::Global(atom));
        // A name the program binds to no closure reads the realm's builtin of that name, joined with what its stores put there;
        // a name the program writes is demoted by the closed world's proof (`identity::proven`).
        if !ctx.resolved.contains_key(&Key::Global(atom)) {
            word.cell.identity = Identity::Builtin(BuiltinPath { global: atom, member: None }).join(word.cell.identity);
        }
        // A global-object name (`globalThis`, `window`, ...) reads the global object itself.
        if ctx.scope.is_global_object(atom) { word.cell = word.cell.maybe_global(); }
        return word;
    }
    // A closure record decodes under its allocation's proof (`Decoder::Literal`), not as an operation.
    if let Some((_, Allocation::Closure { function })) = Allocation::decode(instruction, &[]) {
        return Word::closure(function);
    }
    // An array of `length` elements: its length is fixed by its literal, and its element type is what the
    // definitions that follow join in (`step`). A literal that leaves an index out holds a hole, which reads `undefined`.
    // The array is its own element class, named by its allocation site (`Origin::Alloc`).
    if ctx.types.is_some() && let Some((_, Allocation::Array { length, .. })) = Allocation::decode(instruction, &[]) {
        let holes = !ctx.whole_literals.get(pc).copied().unwrap_or(false);
        let cell = Cell {
            ty: Type::language(crate::ValueType::Object),
            classes: Classes::Many,
            elements: Elements { ty: Type::NEVER, classes: Classes::Never, length: Length::Fixed(length), view: View::Plain, holes },
            sites: Sites::Many,
            global: false,
            prototype: false,
            identity: Identity::Many,
        };
        return Word { cell, origin: Some(Origin::Alloc { function: ctx.id, pc: pc as u32 }) };
    }
    // Over the operands' cells (`typing`): a `Binary` record in the typed pass, whose operands are the record's own (none
    // over a constant operand, a numeric one over registers), and an indexed read by its index. An increment is defined
    // by `typing::value` like the other Number operations.
    let before = Before { ctx, state };
    // An object literal of a fixed shape is its allocation site's (`Cell::sites`): its keys and values are the site's table
    // (`Record::note_literal`), which a copy of it reads (`copies`).
    if matches!(operation, Some(Operation::Allocate { allocation: Allocation::ObjectLiteral { .. } | Allocation::ObjectPair { .. }, .. })) {
        let cell = operation.as_ref().and_then(|operation| typing::value(operation, &before)).unwrap_or(Cell::ANY);
        return Word::of_cell(Cell { sites: Sites::one(Site { function: ctx.id, pc: pc as u32 }), ..cell });
    }
    if ctx.types.is_some() && instruction.op() == crate::bytecode::Op::Binary {
        return Word::of_cell(typing::binary(&before, instruction.operand_b(), instruction.operand_c(), instruction.binary_operator()));
    }
    if instruction.op() == crate::bytecode::Op::GetIndex {
        // A key that is one atom reads the named field it spells, as `o.name` does; an array index is an element instead.
        let key = state[usize::from(instruction.register_c())].cell.ty.string_fact().and_then(|fact| fact.atom());
        if let Some(atom) = key.filter(|&atom| !ctx.scope.names_index(atom)) {
            let owner = state[usize::from(instruction.register_b())];
            if let Some(length) = string_length(ctx, Some(owner), atom) {
                return Word::of(length);
            }
            let cell = member_identity(Some(owner), member_type(ctx, Some(owner), atom), atom);
            return named_word(ctx, Some(owner), cell, atom);
        }
        let (base, index) = (settled(ctx, state[usize::from(instruction.register_b())]), state[usize::from(instruction.register_c())].cell);
        return Word::of_cell(element_read(ctx, pc, &before, base, index));
    }
    match operation {
        Some(Operation::Move { source, .. } | Operation::StoreEnvLocal { source, .. } | Operation::StoreLocalPlain { source, .. }) => {
            state[usize::from(source)]
        }
        Some(Operation::LoadEnvLocal { slot, .. } | Operation::LoadLocalPlain { slot, .. }) => {
            resolved(ctx, Key::Slot { owner: ctx.id, slot })
        }
        Some(Operation::LoadCapture { depth, slot, .. }) => ctx
            .scope
            .owner(ctx.id, depth)
            .map_or(Word::UNKNOWN, |owner| resolved(ctx, Key::Slot { owner, slot })),
        Some(Operation::Allocate { allocation: Allocation::Closure { function }, .. }) => Word::closure(function),
        // A function's `this` is the join of the receivers its calls pass (`types::Types::this`).
        Some(Operation::LoadThis { .. }) if !ctx.is_root => this_fact(ctx),
        Some(Operation::LoadThis { .. }) => root_this(ctx),
        // `+x` of a Number is `x` itself, origin included: the cell `typing::number_cell` defines for it.
        Some(Operation::Number(NumberOperation::ToNumeric { source, .. })) if typing::reached_number(state[usize::from(source)].cell) =>
            state[usize::from(source)],
        // An element read of the operation row, as the indexed `GetIndex` reads its element (`element_read`), so the previous
        // round's in-bounds proof reaches it too.
        Some(Operation::ElementRead { base, key, .. }) =>
            Word::of_cell(element_read(ctx, pc, &before, settled(ctx, state[usize::from(base)]), typing::operand_cell(&before, key))),
        _ => operation.as_ref().and_then(|operation| typing::value(operation, &before)).map_or(Word::UNKNOWN, Word::of_cell),
    }
}

/// The cell of an indexed read of `base` at `index` by the instruction at `pc` (`typing::element`): `undefined` where the array
/// may hold a hole or the index is not within its length, unless the previous round proved the read in bounds
/// (`in_bounds_reads`), which settles the index within the length.
fn element_read(ctx: &Ctx<'_>, pc: usize, before: &Before<'_, '_>, base: Cell, index: Cell) -> Cell {
    typing::element(before, base, index, ctx.in_bounds_reads.contains(&(ctx.id, pc as u32)))
}

/// The array `new Array(...)` or `Array(...)` allocates at `pc`, named by its allocation site as a literal is (`Origin::Alloc`).
/// Its holes are `undefined` where its one argument is a Number (the length), unless the previous round proved it filled
/// (`filled`), and any value where the arguments are the elements (`Array(x)` holds `x`); no argument leaves it empty. Its
/// length is unknown, as the length is not a constant here.
fn array_constructed(ctx: &Ctx<'_>, pc: usize, instruction: WideInstruction, state: &[Word]) -> Option<Word> {
    let invocation = crate::staged::Invocation::of(instruction, ctx.decode)?;
    let crate::staged::Callee::Register(register) = invocation.callee else { return None };
    if !ctx.scope.is_array_constructor(state[usize::from(register)].origin) { return None; }
    let arguments: Vec<Word> = invocation.arguments.registers(ctx.records).map(|register| state[usize::from(register)]).collect();
    let (stored, holes) = match arguments.as_slice() {
        [] => (Type::NEVER, false),
        [length] if length.is_number() => (Type::NEVER, !ctx.filled.contains(&(ctx.id, pc as u32))),
        _ => (Type::ANY, false),
    };
    let cell = Cell {
        ty: Type::language(crate::ValueType::Object),
        classes: Classes::Many,
        elements: Elements { ty: stored, classes: Classes::Never, length: Length::Any, view: View::Plain, holes },
        sites: Sites::Many,
        global: false,
        prototype: false,
        identity: Identity::Many,
    };
    Some(Word { cell, origin: Some(Origin::Alloc { function: ctx.id, pc: pc as u32 }) })
}

/// The typed array a call of a native typed-array constructor (`new Float64Array(...)`) allocates at `pc`. Whatever its
/// arguments, its kind is the constructor's, so its elements are that kind's (`View::One`) and its length is not fixed
/// (a resizable buffer's view may change length). Its element class is its own, as an array's is.
fn typed_array_constructed(ctx: &Ctx<'_>, pc: usize, instruction: WideInstruction, state: &[Word]) -> Option<Word> {
    let invocation = crate::staged::Invocation::of(instruction, ctx.decode)?;
    let crate::staged::Callee::Register(register) = invocation.callee else { return None };
    let kind = ctx.scope.typed_array_kind(state[usize::from(register)].origin)?;
    let cell = Cell {
        ty: Type::language(crate::ValueType::Object),
        classes: Classes::Many,
        elements: Elements {
            ty: crate::typed_ir::view_element_type(kind),
            classes: Classes::Never,
            length: Length::Any,
            view: View::One(kind),
            holes: false,
        },
        sites: Sites::Many,
        global: false,
        prototype: false,
        identity: Identity::Many,
    };
    Some(Word { cell, origin: Some(Origin::Alloc { function: ctx.id, pc: pc as u32 }) })
}

/// The registers of the facts before an instruction, read as the operands of `staged::typing`. A constant's cell is the
/// type of its value (`Scope::constant_type`); the unnamed elements are the types' (`Types::unnamed_elements`).
pub(super) struct Before<'a, 'b> {
    pub(super) ctx: &'a Ctx<'b>,
    pub(super) state: &'a [Word],
}

impl Abstract for Before<'_, '_> {
    fn register(&self, register: Register) -> Cell { settled(self.ctx, self.state[usize::from(register)]) }
    fn constant(&self, index: usize) -> Cell { Cell::of(self.ctx.scope.constant_type(index)) }
    fn unnamed_elements(&self) -> Option<Elements> { self.ctx.types.map(|types| types.unnamed_elements()) }
}

/// Records the bindings `instruction` writes, from the facts before it, and the writes it makes
/// that the analysis cannot follow.
fn writes(
    ctx: &Ctx<'_>,
    pc: usize,
    instruction: WideInstruction,
    operation: Option<Operation<(), ()>>,
    state: &[Word],
    events: &mut Events,
) {
    unfollowed_writes(ctx, instruction, operation, events);
    binding_stores(ctx, pc, operation, state, events);
    member_stores(ctx, pc, instruction, operation, state, events);
    events.global_keyed |= keyed_global(ctx, instruction, operation, state);
}

/// Whether the instruction reaches into a value that may be the global object, where the closed world's call graph cannot
/// follow: a computed read, write or delete of it, a for-in or an `in` test over it (the key may name any global), and a
/// named read or store of it (the property may be a global function, read back and called). `gate::verify` rejects the
/// closed world when a reached instruction does one of these.
fn keyed_global(ctx: &Ctx<'_>, instruction: WideInstruction, operation: Option<Operation<(), ()>>, state: &[Word]) -> bool {
    let flagged = |register: Register| state[usize::from(register)].cell.global;
    if instruction.op() == crate::bytecode::Op::GetIndex {
        return flagged(instruction.register_b());
    }
    if matches!(instruction.op().class(), crate::bytecode::Class::GetField) {
        return match instruction.field_lookup() {
            Some(FieldLookup::Atom { base, .. }) => named_base_global(ctx, base, state),
            Some(FieldLookup::Site(index)) => ctx.scope.site_paths.get(index).copied().flatten().is_none_or(|(base, ..)| named_base_global(ctx, base, state)),
            None => false,
        };
    }
    match operation {
        Some(Operation::ElementRead { base, .. } | Operation::ElementWrite { base, .. } | Operation::Delete { base, .. } | Operation::EnumeratedGet { base, .. }) => flagged(base),
        Some(Operation::SetField { base, .. } | Operation::StoreField { base, .. }) => named_base_global(ctx, base.map_or(FieldBase::THIS, FieldBase::register), state),
        Some(Operation::IndexWrite { .. } | Operation::IndexSetHelper { .. } | Operation::IndexGetHost { .. }) => true,
        Some(Operation::Protocol { step: Protocol::ForInStart, operands, .. }) => operands.into_iter().flatten().any(flagged),
        Some(Operation::CompareJump { comparison: Comparison::Operator(operator), right, .. }) =>
            operator == BinaryOperator::In as u32 && right.register_index().is_some_and(flagged),
        _ => instruction.op() == crate::bytecode::Op::Binary
            && instruction.binary_operator() == BinaryOperator::In as u32
            && instruction.operand_c().register_index().is_some_and(flagged),
    }
}

/// Whether the object a named access reads or stores through `base` may be the global object: its fact (`base_fact`) may
/// be. The root's `this` has no fact (`base_fact`): it is the global object of a Script, and `undefined` in a Module; any
/// other base with no fact may be the global object.
fn named_base_global(ctx: &Ctx<'_>, base: FieldBase, state: &[Word]) -> bool {
    match base_fact(ctx, base, state) {
        Some(word) => word.cell.global,
        None => base != FieldBase::THIS || ctx.scope.root_this_is_global(),
    }
}

/// Marks the writes the analysis cannot follow: name operations other than plain loads and
/// global stores (`with`, `delete`, sloppy declarations), and writes through a computed key, which may name a global or an element of an arguments object.
fn unfollowed_writes(ctx: &Ctx<'_>, instruction: WideInstruction, operation: Option<Operation<(), ()>>, events: &mut Events) {
    let traits = instruction.op().traits();
    let global_store = matches!(operation, Some(Operation::GlobalWrite { .. }));
    if traits.has(Traits::NAME_OPERATION)
        && !traits.has(Traits::NAME_LOAD)
        && !traits.has(Traits::ENVIRONMENT_CLONE)
        && !global_store
    {
        events.wild_global = true;
        events.wild_owner = true;
    }
    match operation {
        Some(Operation::LoadThis { .. }) if ctx.is_root && ctx.scope.root_this_is_global() => {
            events.wild_global = true;
            events.global_source = true;
        }
        _ => {}
    }
}

/// Records the binding stores `operation` makes, with the facts they store.
fn binding_stores(ctx: &Ctx<'_>, pc: usize, operation: Option<Operation<(), ()>>, state: &[Word], events: &mut Events) {
    match operation {
        // A captured or global binding is no parameter: a store of it stays as the flow proves
        // it even in the prologue, where a register store may be overwritten by an argument.
        Some(Operation::StoreEnvLocal { source, slot, .. }) => {
            write(ctx, pc, true, events, Key::Slot { owner: ctx.id, slot }, state[usize::from(source)]);
        }
        Some(Operation::StoreLocalPlain { source, slot, .. }) => {
            write(ctx, pc, false, events, Key::Slot { owner: ctx.id, slot }, state[usize::from(source)]);
        }
        Some(Operation::StoreCapture { source, depth, slot }) => match ctx.scope.owner(ctx.id, depth) {
            Some(owner) => write(ctx, pc, true, events, Key::Slot { owner, slot }, state[usize::from(source)]),
            None => {
                events.wild_global = true;
                events.wild_owner = true;
            }
        },
        Some(Operation::GlobalWrite { source, atom, .. }) => {
            write(ctx, pc, true, events, Key::Global(atom), state[usize::from(source)]);
        }
        Some(
            Operation::SetField { atom, .. }
            | Operation::StoreField { atom, .. }
            | Operation::FieldStoreMiss { atom, .. }
            | Operation::FieldAdd { atom, .. },
        ) => write(ctx, pc, true, events, Key::Global(atom), Word::UNKNOWN),
        _ => {}
    }
}

/// Records a write of `fact` to `key` at `pc`; a store before the parameters end binds an argument.
fn write(ctx: &Ctx<'_>, pc: usize, stable: bool, events: &mut Events, key: Key, fact: Word) {
    let fact = if !stable && pc < ctx.parameter_end { Word::UNKNOWN } else { fact };
    events.writes.push((key, fact));
}

