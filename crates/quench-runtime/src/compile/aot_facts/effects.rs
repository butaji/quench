//! What one instruction does to a function's register facts, and to the bindings it writes.

use super::numeric;
use super::scope::Scope;
use super::value::{Word, Key, Origin, PrototypeBase, UNNAMED_ARRAYS};
use crate::typed_ir::{Cell, Classes, Elements, Length};
use crate::bytecode::{Operand, OperandKind, Traits, WideInstruction};
use crate::typed_ir::Type;
use crate::staged::protocol_literals::LiteralStep;
use crate::staged::{Allocation, DecodeContext, NumberOperation, Operation, Protocol};

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
}

/// What an instruction does beyond its registers: the bindings it writes, and whether it makes
/// writes the analysis cannot follow.
#[derive(Default)]
pub(super) struct Events {
    pub(super) writes: Vec<(Key, Word)>,
    pub(super) wild_global: bool,
    pub(super) wild_owner: bool,
    /// Whether a store through a key the analysis cannot name may write a named property.
    pub(super) wild_member: bool,
    /// The change the instruction makes to a named property or a key the analysis cannot name (`shape`).
    pub(super) shape: Option<super::shape::Shape>,
}

/// Whether the instruction at a point may run guest code with the objects it touches: a call or construction, a getter,
/// setter or proxy trap, a conversion that runs `valueOf`. It is the operation's effect row (`Effects::may_call_js`),
/// met with what the facts before it (`state`) prove: a Number or binary operation over Numbers runs none, and a
/// captured or environment binding's read runs none. A store through `this` is not counted: it is the constructor's
/// own store, whose setter `class_layout` does not model. Whatever does not decode runs guest code.
pub(super) fn runs_guest_code(ctx: &Ctx<'_>, instruction: WideInstruction, state: &[Word]) -> bool {
    if crate::staged::Invocation::of(instruction, ctx.decode).is_some() {
        return true;
    }
    let operation = Operation::<(), ()>::decode_completed(instruction, ctx.decode).map(|(operation, _)| operation).or_else(|| decoded(instruction));
    match operation {
        None => true,
        Some(Operation::SetField { base: None, .. }) => false,
        Some(Operation::LoadEnvLocal { .. } | Operation::LoadCapture { .. }) => false,
        Some(Operation::Number(number)) => !numbers_over_numbers(ctx.scope, state, number),
        Some(Operation::Binary { left, right, .. }) => !(is_number(ctx.scope, state, left) && is_number(ctx.scope, state, right)),
        Some(operation) => operation.effects().may_call_js,
    }
}

/// Whether the operands of a Number operation are Numbers on the facts before it, so its miss runs no guest code.
fn numbers_over_numbers(scope: &Scope, state: &[Word], number: NumberOperation) -> bool {
    match number {
        NumberOperation::Arithmetic { left, right, .. } | NumberOperation::Remainder { left, right, .. } | NumberOperation::Extreme { left, right, .. }
        | NumberOperation::Bitwise { left, right, .. } | NumberOperation::Imul { left, right, .. } => is_number(scope, state, left) && is_number(scope, state, right),
        NumberOperation::Unary { source, .. } | NumberOperation::Math { source, .. } | NumberOperation::BitwiseNot { source, .. } => is_number(scope, state, source),
        NumberOperation::IncDec { source, .. } | NumberOperation::ToNumeric { source, .. } => state[usize::from(source)].is_number(),
        _ => false,
    }
}

/// Whether operand `operand` is a Number on the facts before it.
fn is_number(scope: &Scope, state: &[Word], operand: Operand) -> bool {
    operand_fact(scope, state, operand).is_number()
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
    // A call whose callees the solve knows returns the join of their returns (`types::Types::call`).
    if let Some(types) = ctx.types && let Some(cell) = types.call(ctx.id, pc as u32) {
        value = Word::of_cell(cell);
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
            .join(Elements { ty: element.ty, classes: element.classes, length: Length::Never });
    }
    let defined = instruction.defined_register();
    for register in instruction.written_registers() {
        state[usize::from(register)] = if Some(register) == defined { value } else { Word::UNKNOWN };
    }
    events
}

/// Whether `instruction` reads a global name plainly (`LoadName`, `LoadNameCall`).
fn is_global_load(instruction: WideInstruction) -> bool {
    let traits = instruction.op().traits();
    traits.has(Traits::GLOBAL_NAME_ACCESS) && traits.has(Traits::NAME_LOAD)
}

/// The fact of a load from `key`: the closure the census proved there, else the binding.
/// In a typed pass a binding's load takes the join of its writes (`types::Types::binding`).
fn resolved(ctx: &Ctx<'_>, key: Key) -> Word {
    ctx.resolved.get(&key).copied().map_or_else(
        || Word { origin: Some(Origin::Binding(key)), ..Word::of_cell(ctx.types.map_or(Cell::ANY, |types| types.cell(key))) },
        Word::closure)
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
        if ctx.scope.prototype == Some(atom) && let Some(prototype) = owner.and_then(prototype_base) {
            return Word::from(Origin::Prototype(prototype));
        }
        let cell = member_type(ctx, base_fact(ctx, base, state), atom);
        return match owner.and_then(Word::class_site) {
            Some((owner, instance, guess)) => Word { cell, ..Word::member_on(owner, instance, atom, guess) },
            None => Word::member(atom, cell),
        };
    }
    // A nested lookup (`base.first.second`) delivers its last field's value.
    if matches!(instruction.op().class(), crate::bytecode::Class::GetField) && (instruction.result_flags() & !crate::bytecode::SET_THIS_REGISTER == 0 || instruction.returns_from_frame())
        && let Some(crate::bytecode::FieldLookup::Site(index)) = instruction.field_lookup()
        && let Some((base, first, second)) = ctx.scope.site_paths.get(index).copied().flatten()
    {
        let cell = member_type(ctx, base_fact(ctx, base, state), first);
        return match second {
            Some(second) => Word::member(second, member_type(ctx, Some(Word::of_cell(cell)), second)),
            None => Word::member(first, cell),
        };
    }
    if instruction.op() == crate::bytecode::Op::Construct {
        let callee = state.get(usize::from(instruction.register_b())).copied().unwrap_or(Word::UNKNOWN);
        if let Some(class) = callee.cell.ty.body_index() { return Word::instance(class); }
        // `new` defines an object whatever it constructs: a constructor's non-object return is replaced by the
        // object it allocated.
        if ctx.types.is_some() { return Word::of(Type::language(crate::ValueType::Object)); }
    }
    if is_global_load(instruction) {
        let atom = instruction.atom_index();
        events.wild_global |= ctx.scope.is_global_object(atom);
        return resolved(ctx, Key::Global(atom));
    }
    // A closure record decodes under its allocation's proof (`Decoder::Literal`), not as an operation.
    if let Some((_, Allocation::Closure { function })) = Allocation::decode(instruction, &[]) {
        return Word::closure(function);
    }
    // An array of `length` elements: its length is fixed by its literal, and its element type is what the
    // definitions that follow join in (`step`). A literal that leaves an index undefined holds a hole, which reads `undefined`.
    if ctx.types.is_some() && let Some((_, Allocation::Array { length, .. })) = Allocation::decode(instruction, &[]) {
        let hole = if ctx.whole_literals.get(pc).copied().unwrap_or(false) { Type::NEVER } else { Type::language(crate::ValueType::Undefined) };
        return Word::of_cell(Cell {
            ty: Type::language(crate::ValueType::Object),
            classes: Classes::Many,
            elements: Elements { ty: hole, classes: Classes::Never, length: Length::Fixed(length) },
        });
    }
    // A `Binary` record, whatever operation it decodes to (none over a constant operand, a numeric one over
    // registers): its operator and operands are the record's.
    // An increment or decrement defines a Number, or a BigInt off one.
    if ctx.types.is_some() && instruction.op() == crate::bytecode::Op::IncDec {
        let source = state[usize::from(instruction.register_b())].cell.ty;
        return Word::of(numeric::step(source, instruction.boolean_flag() == Some(true)));
    }
    if ctx.types.is_some() && instruction.op() == crate::bytecode::Op::Binary {
        return binary_value(ctx.scope, state, (instruction.operand_b(), instruction.operand_c()), instruction.binary_operator());
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
        Some(Operation::Number(number)) => number_value(ctx.scope, state, number),
        // The operations whose result is a Boolean, whatever their operands: an equality, a negation, a
        // `typeof` test or an `instanceof`.
        Some(Operation::Equality { .. } | Operation::LogicalNot { .. } | Operation::TypeofTest { .. } | Operation::InstanceOf { .. }) =>
            Word::of(Type::language(crate::ValueType::Boolean)),
        Some(Operation::Void { .. }) => Word::of(Type::language(crate::ValueType::Undefined)),
        // An indexed read of the array a field holds: the field's element type, or `undefined` for a hole or an
        // index past its length.
        _ if ctx.types.is_some() && instruction.op() == crate::bytecode::Op::GetIndex =>
            element_value(ctx, state[usize::from(instruction.register_b())], state[usize::from(instruction.register_c())]),
        Some(Operation::ElementRead { base, key, .. }) if ctx.types.is_some() =>
            element_value(ctx, state[usize::from(base)], operand_fact(ctx.scope, state, key)),
        Some(Operation::Binary { left, right, operator, .. }) => binary_value(ctx.scope, state, (left, right), operator),
        _ => Word::UNKNOWN,
    }
}

/// The type and class a read of field `atom` off `owner` takes (`types::Types::field`): the class the
/// stores into that field of `owner`'s class wrote (`Key::MemberOn`), else every store of the name's
/// (`Key::Member`); any value in the passes that find call targets.
fn member_type(ctx: &Ctx<'_>, owner: Option<Word>, atom: u32) -> Cell {
    let Some(types) = ctx.types else { return Cell::ANY };
    let named = types.cell(Key::Member(atom));
    // An owner that holds no object yet (the solve's bottom, or only `null`) reads none: its classes come with a later pass.
    if owner.is_some_and(|owner| owner.cell.classes == Classes::Never) { return Cell { classes: Classes::Never, ..named }; }
    owner.and_then(Word::class_site).and_then(|(class, instance, _)| types.cell_on(class, instance, atom)).unwrap_or(named)
}

/// The object a named field read or store at `instruction` reads or stores: a `GetField`'s base (a nested
/// lookup's last owner, the value of its first field), a `SetField`'s base or the function's `this`.
pub(super) fn field_owner(ctx: &Ctx<'_>, instruction: WideInstruction, state: &[Word]) -> Option<Word> {
    if matches!(instruction.op().class(), crate::bytecode::Class::GetField) {
        return match instruction.field_lookup()? {
            crate::bytecode::FieldLookup::Atom { base, .. } => base_fact(ctx, base, state),
            crate::bytecode::FieldLookup::Site(index) => {
                let (base, first, second) = ctx.scope.site_paths.get(index).copied().flatten()?;
                let owner = base_fact(ctx, base, state);
                if second.is_none() { return owner; }
                Some(Word::of_cell(member_type(ctx, owner, first)))
            }
        };
    }
    match decoded(instruction) {
        Some(Operation::SetField { base: Some(base), .. }) => state.get(usize::from(base)).copied(),
        Some(Operation::SetField { base: None, .. }) if !ctx.is_root => Some(this_fact(ctx)),
        _ => None,
    }
}

/// The fact of a field lookup's base: its register's, or the function's own `this`.
fn base_fact(ctx: &Ctx<'_>, base: crate::bytecode::FieldBase, state: &[Word]) -> Option<Word> {
    match base.register_index() {
        Some(register) => state.get(usize::from(register)).copied(),
        None if base == crate::bytecode::FieldBase::THIS && !ctx.is_root => Some(this_fact(ctx)),
        None => None,
    }
}

/// The fact of a read of field `atom` off the function's own `this` (a receiver `this.task` of a method call):
/// its class's own field where the solve names `this`'s class (`member_type`).
pub(super) fn this_member(ctx: &Ctx<'_>, atom: u32) -> Word {
    let this = (!ctx.is_root).then(|| this_fact(ctx));
    Word::of_cell(member_type(ctx, this, atom))
}

/// The fact of the function's `this`: the join of its receivers' types and classes (`types::Types::this`).
pub(super) fn this_fact(ctx: &Ctx<'_>) -> Word {
    ctx.types.map_or(Word::UNKNOWN, |types| Word::of_cell(types.this(ctx.id)))
}

/// What a register of fact `base` names whose `prototype` a read takes: a closure, or a binding.
fn prototype_base(base: Word) -> Option<PrototypeBase> {
    if let Some(function) = base.cell.ty.body_index() { return Some(PrototypeBase::Closure(function)); }
    match base.origin? {
        Origin::Binding(key) => Some(PrototypeBase::Binding(key)),
        _ => None,
    }
}

/// The fact of an operand: its register's, or the type of its constant.
pub(super) fn operand_fact(scope: &Scope, state: &[Word], operand: Operand) -> Word {
    match (operand.register_index(), operand.kind()) {
        (Some(register), _) => state[usize::from(register)],
        (None, Some(OperandKind::Constant)) => Word::of(scope.constant_type(usize::from(operand.payload()))),
        _ => Word::UNKNOWN,
    }
}

/// The fact of the Number a numeric operation defines, when its operands are Numbers: the
/// operation of an operand that is not one may run guest code or yield a string.
fn number_value(scope: &Scope, state: &[Word], number: NumberOperation) -> Word {
    let fact = |operand: Operand| operand_fact(scope, state, operand);
    // An operand no value reaches yet (`Type::NEVER`, the type solve's bottom) is a Number among the
    // others: what reaches it later is joined in a later pass, and an operation over it defines none yet.
    let numeric = |fact: Word| fact.cell.ty.is_subtype(Type::NUMBER);
    let both = |left: Operand, right: Operand| numeric(fact(left)) && numeric(fact(right));
    let one = |source: Operand| numeric(fact(source));
    let register = |source: u16| state[usize::from(source)];
    let reached = |operands: &[Word]| operands.iter().all(|fact| fact.cell.ty != Type::NEVER);
    let operands: Vec<Word> = match number {
        NumberOperation::Arithmetic { left, right, .. } | NumberOperation::Remainder { left, right, .. } | NumberOperation::Extreme { left, right, .. }
        | NumberOperation::Bitwise { left, right, .. } | NumberOperation::Imul { left, right, .. } => vec![fact(left), fact(right)],
        NumberOperation::Unary { source, .. } | NumberOperation::Math { source, .. } | NumberOperation::BitwiseNot { source, .. } => vec![fact(source)],
        NumberOperation::IncDec { source, .. } | NumberOperation::ToNumeric { source, .. } => vec![register(source)],
        _ => Vec::new(),
    };
    if !reached(&operands) && operands.iter().all(|fact| numeric(*fact)) { return Word::of(Type::NEVER); }
    let ty = |operand: Operand| fact(operand).cell.ty;
    match number {
        NumberOperation::Arithmetic { left, right, operation, .. } if both(left, right) =>
            Word::of(numeric::arithmetic(operation, ty(left), ty(right))),
        NumberOperation::Remainder { left, right, .. } if both(left, right) => Word::of(numeric::remainder(ty(left), ty(right))),
        NumberOperation::Extreme { left, right, .. } if both(left, right) => Word::of(Type::NUMBER),
        NumberOperation::Bitwise { left, right, operation, .. } if both(left, right) =>
            Word::of(numeric::bitwise(operation, ty(left), ty(right))),
        NumberOperation::Imul { left, right, .. } if both(left, right) => Word::of(Type::INT32),
        NumberOperation::Unary { source, .. } | NumberOperation::Math { source, .. } if one(source) => Word::of(Type::NUMBER),
        NumberOperation::BitwiseNot { source, .. } if one(source) => Word::of(Type::INT32),
        NumberOperation::IncDec { source, decrement, .. } if numeric(register(source)) =>
            Word::of(numeric::step(register(source).cell.ty, decrement)),
        NumberOperation::ToNumeric { source, .. } if numeric(register(source)) => register(source),
        _ => Word::UNKNOWN,
    }
}

/// The fact of an indexed read of `base` at `index`: the elements `base`'s cell names (`Elements::element`), joined with
/// those no field names (`Types::unnamed_elements`), and with `undefined` unless the index is within a fixed length
/// (`within_length`), where a hole may read. Any value where the types are not solved yet.
fn element_value(ctx: &Ctx<'_>, base: Word, index: Word) -> Word {
    let Some(types) = ctx.types else { return Word::UNKNOWN };
    let elements = base.cell.elements.join(types.unnamed_elements());
    let element = elements.element();
    if within_length(elements.length, index.cell.ty) {
        Word::of_cell(element)
    } else {
        Word::of_cell(element.join(Cell::of(Type::language(crate::ValueType::Undefined))))
    }
}

/// Whether an index of type `index` stays within an array of length `length`: a fixed length, and an integer range
/// inside `[0, length)`.
fn within_length(length: Length, index: Type) -> bool {
    match (length, index.int_range()) {
        (Length::Fixed(count), Some(range)) => range.min >= 0 && range.max < i64::from(count),
        _ => false,
    }
}

/// The write that makes the length of the arrays `key` names (`Key::Element`) unknown: a length write, a method that
/// changes an array, an index store past the length, a `delete`.
fn length_change(key: u32) -> (Key, Word) {
    let unknown = Elements { ty: Type::NEVER, classes: Classes::Never, length: Length::Any };
    (Key::Element(key), Word::of_cell(Cell { ty: Type::NEVER, classes: Classes::Never, elements: unknown }))
}

/// The fact of what a `Binary` of `operator` defines: a comparison a Boolean whatever its operands; otherwise the
/// operator's type over the operands' types (`numeric::binary_type`: an integer range over integer ranges, a Number
/// or an int32 over Numbers, a BigInt or a string where the operands are those), else anything. An operand no value
/// reaches yet (the type solve's bottom) makes none either.
fn binary_value(scope: &Scope, state: &[Word], (left, right): (Operand, Operand), operator: u32) -> Word {
    use crate::vm::operations::binary_operator as op;
    let (left, right) = (operand_fact(scope, state, left).cell.ty, operand_fact(scope, state, right).cell.ty);
    if left == Type::NEVER || right == Type::NEVER {
        return if matches!(operator, op::In | op::Instanceof) { Word::UNKNOWN } else { Word::of(Type::NEVER) };
    }
    if matches!(operator, op::Equality | op::Inequality | op::StrictEquality | op::StrictInequality
        | op::LessThan | op::LessEqualThan | op::GreaterThan | op::GreaterEqualThan) {
        return Word::of(Type::language(crate::ValueType::Boolean));
    }
    numeric::binary_type(operator, left, right).map_or(Word::UNKNOWN, Word::of)
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
    member_stores(ctx, instruction, operation, state, events);
}

/// Records the named property stores `instruction` makes (`Key::Member`), with the facts they store; a store through a
/// key that is no Number may name any property (`Events::wild_member`).
fn member_stores(ctx: &Ctx<'_>, instruction: WideInstruction, operation: Option<Operation<(), ()>>, state: &[Word], events: &mut Events) {
    if let Some(Operation::Protocol { step: Protocol::Literal(LiteralStep::DefinePropertyRecord { value, .. }), .. }) = operation {
        let target = state[usize::from(instruction.register_b())];
        let key = state[usize::from(instruction.register_c())];
        if let (Some(atom), Some((class, instance, false))) = (key.atom_key(), target.class_site()) {
            events.writes.push((Key::MemberOn { class, instance, atom }, state[usize::from(value)]));
        }
    }
    match operation {
        // An indexed store of a Number into the array `base` names (`SetIndex`): its value joins the elements of the
        // field the array was read from (`Key::Element`), and its length becomes unknown unless the index is within it.
        Some(Operation::ElementWrite { value, base, key, .. }) if state[usize::from(key)].is_number() => {
            let (array, index, stored) = (state[usize::from(base)], state[usize::from(key)], state[usize::from(value)]);
            let length = array.cell.elements.length;
            let length = if length == Length::Never || within_length(length, index.cell.ty) { Length::Never } else { Length::Any };
            let elements = Elements { ty: Type::NEVER, classes: Classes::Never, length };
            events.writes.push((Key::Element(array.elements_key()), Word::of_cell(Cell { ty: stored.cell.ty, classes: stored.cell.classes, elements })));
        }
        // A store through a computed key that is no Number, a helper or a `delete` may store any value into any array
        // and change its length: every element read joins the cell of the arrays no field names.
        Some(Operation::ElementWrite { .. } | Operation::IndexWrite { .. } | Operation::IndexSetHelper { .. } | Operation::Delete { .. }) => {
            events.wild_member = true;
            events.writes.push((Key::Element(UNNAMED_ARRAYS), Word::of_cell(Cell::ANY)));
        }
        Some(Operation::SetField { value, atom, base, .. }) => {
            events.writes.push((Key::Member(atom), state[usize::from(value)]));
            let owner = match base { Some(base) => state[usize::from(base)], None if !ctx.is_root => this_fact(ctx), None => Word::UNKNOWN };
            if ctx.scope.length == Some(atom) {
                events.writes.push(length_change(owner.elements_key()));
            }
            // A store into an object of a known class (`Word::class_site`) is that class's own field, as a class
            // body's method is (`DefinePropertyRecord`): a method on its prototype (`C.prototype.m = f`), or a data
            // field of its instances (`this.link = link` in `C`), which a read off an instance of `C` resolves per class.
            // A store into an object of no one class may land in any class's instance (`Key::MemberAny`).
            let key = match owner.class_site() { Some((class, true, _)) => Key::MemberOn { class, instance: true, atom }, _ => Key::MemberAny(atom) };
            events.writes.push((key, state[usize::from(value)]));
        }
        Some(Operation::StoreField { atom, .. } | Operation::FieldStoreMiss { atom, .. } | Operation::FieldAdd { atom, .. }) => {
            events.writes.push((Key::Member(atom), Word::UNKNOWN));
            events.writes.push((Key::MemberAny(atom), Word::UNKNOWN));
            if ctx.scope.length == Some(atom) {
                events.writes.push(length_change(UNNAMED_ARRAYS));
            }
        }
        _ if instruction.op().is_field_store() && !matches!(operation, Some(Operation::SetField { .. })) => {
            events.writes.push((Key::Member(instruction.atom_index()), Word::UNKNOWN));
            events.writes.push((Key::MemberAny(instruction.atom_index()), Word::UNKNOWN));
            if ctx.scope.length == Some(instruction.atom_index()) {
                events.writes.push(length_change(UNNAMED_ARRAYS));
            }
        }
        _ => {}
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
        Some(
            Operation::ElementWrite { .. }
            | Operation::IndexWrite { .. }
            | Operation::IndexSetHelper { .. }
            | Operation::Delete { .. },
        ) => events.wild_global = true,
        Some(Operation::LoadThis { .. }) if ctx.is_root => events.wild_global = true,
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
