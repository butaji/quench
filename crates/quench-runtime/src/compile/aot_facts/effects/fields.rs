//! The named field reads and stores one instruction makes: the object a field access reads or writes, the type a read
//! takes off that object, the fact of a named read, and the stores a `SetField` or a field store writes (`Events::writes`).

use super::super::accessors::{accessor_definition, accessor_mode, site_holder};
use super::super::value::{Key, Origin, PrototypeBase, Word};
use super::{Ctx, Events, decoded, settled};
use crate::bytecode::{PropertyDefinitionMode, WideInstruction};
use crate::staged::protocol_literals::LiteralStep;
use crate::staged::typing;
use crate::staged::{Operation, Protocol};
use crate::typed_ir::{BuiltinPath, Cell, Classes, ElementClass, Elements, Holders, Identity, Length, Sites, Type, View};

/// The type and class a read of field `atom` off `owner` takes (`types::Types::field`): the class the
/// stores into that field of `owner`'s class wrote (`Key::MemberOn`), else every store of the name's
/// (`Key::Member`); any value in the passes that find call targets.
pub(super) fn member_type(ctx: &Ctx<'_>, owner: Option<Word>, atom: u32) -> Cell {
    let cell = stored_member_type(ctx, owner, atom);
    // A `prototype` or `__proto__` read off a value that is no program closure may be a builtin's prototype object
    // (`Cell::prototype`): a program closure's `prototype` is its own, never a builtin's.
    if ctx.scope.is_prototype_name(atom) && !owner.is_some_and(|owner| owner.cell.ty.body_index().is_some()) { cell.maybe_prototype() } else { cell }
}

/// `cell` (a read of field `atom` off `owner`) with the identity of a builtin member: where `owner` is the builtin global `G`
/// (`Math`), the read is `G.atom` (`Math.sqrt`). Joined with the identity the stores of the name put there (`cell`), so a
/// member the program also stores is no one builtin.
pub(super) fn member_identity(owner: Option<Word>, cell: Cell, atom: u32) -> Cell {
    match owner.map(|owner| owner.cell.identity) {
        Some(Identity::Builtin(BuiltinPath { global, member: None })) => {
            Cell { identity: Identity::Builtin(BuiltinPath { global, member: Some(atom) }).join(cell.identity), ..cell }
        }
        _ => cell,
    }
}

/// The type and class a read of field `atom` off `owner` takes from the solved stores (`member_type`).
fn stored_member_type(ctx: &Ctx<'_>, owner: Option<Word>, atom: u32) -> Cell {
    let Some(types) = ctx.types else { return Cell::ANY };
    let named = types.cell(Key::Member(atom));
    // An owner that holds no object yet (the solve's bottom, or only `null`) reads none: its classes come with a later pass.
    if owner.is_some_and(|owner| owner.cell.classes == Classes::Never) { return Cell { classes: Classes::Never, ..named }; }
    owner.and_then(Word::class_site).and_then(|site| {
        let (class, instance, _) = site_holder(ctx, site, atom);
        types.cell_on(class, instance, atom)
    }).unwrap_or(named)
}

/// The object a named field read or store at `instruction` reads or stores: a `GetField`'s base (a nested
/// lookup's last owner, the value of its first field), a `SetField`'s base or the function's `this`.
pub(in super::super) fn field_owner(ctx: &Ctx<'_>, instruction: WideInstruction, state: &[Word]) -> Option<Word> {
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
pub(in super::super) fn base_fact(ctx: &Ctx<'_>, base: crate::bytecode::FieldBase, state: &[Word]) -> Option<Word> {
    match base.register_index() {
        Some(register) => state.get(usize::from(register)).copied(),
        None if base == crate::bytecode::FieldBase::THIS && !ctx.is_root => Some(this_fact(ctx)),
        None => None,
    }
}

/// The fact of a read of field `atom` off the function's own `this` (a receiver `this.task` of a method call):
/// its class's own field where the solve names `this`'s class (`member_type`). It carries the field's origin, so
/// an array it holds is the field's element class, as a read of the field off any other object is.
pub(in super::super) fn this_member(ctx: &Ctx<'_>, atom: u32) -> Word {
    let this = (!ctx.is_root).then(|| this_fact(ctx));
    Word::member(atom, member_type(ctx, this, atom))
}

/// The fact of the function's `this`: the join of its receivers' types and classes (`types::Types::this`), and the
/// receiver's own element class, which a resolved call joins to its receiver's (`types::entry_links`).
pub(in super::super) fn this_fact(ctx: &Ctx<'_>) -> Word {
    ctx.types.map_or(Word::UNKNOWN, |types| Word { cell: types.this(ctx.id), origin: Some(Origin::Receiver(ctx.id)) })
}

/// What a register of fact `base` names whose `prototype` a read takes: a closure (its class, `ClassIds::of_word`), or a binding.
pub(super) fn prototype_base(classes: &super::super::class_ids::ClassIds, base: Word) -> Option<PrototypeBase> {
    if let Some(class) = classes.of_word(base) { return Some(PrototypeBase::Closure(class)); }
    // A binding, or a named property read off some object (`Origin::Member`, `Key::Member`): the census resolves the key to the
    // one closure it holds (`single`), so a `Flog.Class` read the same way as a global one is a class too.
    match base.origin? {
        Origin::Binding(_) | Origin::Member(_) => base.origin?.key().map(PrototypeBase::Binding),
        _ => None,
    }
}

/// The write that makes the length of the arrays of element class `class` unknown: a length write, a method that
/// changes an array, an index store past the length, a `delete`.
fn length_change(class: ElementClass) -> (Key, Word) {
    let unknown = Elements { ty: Type::NEVER, classes: Classes::Never, length: Length::Any, view: View::None, holes: true };
    (Key::Element(class), Word::of_cell(Cell { ty: Type::NEVER, classes: Classes::Never, elements: unknown, sites: Sites::Never, global: false, prototype: false, identity: Identity::Never }))
}

/// Records the named property stores `instruction` makes (`Key::Member`), with the facts they store; a store through a
/// key that is no Number may name any property, into the holders of its base (`Events::wild`).
pub(super) fn member_stores(ctx: &Ctx<'_>, pc: usize, instruction: WideInstruction, operation: Option<Operation<(), ()>>, state: &[Word], events: &mut Events) {
    events.computed_store |= writes_global_name(operation.as_ref(), state);
    if let Some(Operation::Protocol { step: Protocol::Literal(LiteralStep::DefinePropertyRecord { mode, value }), .. }) = operation {
        let target = state[usize::from(instruction.register_b())];
        let key = state[usize::from(instruction.register_c())];
        match PropertyDefinitionMode::from_word(mode).and_then(accessor_mode) {
            Some(setter) => accessor_definition(pc, setter, target, key, state[usize::from(value)], events),
            None => {
                if let (Some(atom), Some((class, instance, false))) = (key.atom_key(), target.class_site()) {
                    events.writes.push((Key::MemberOn { class, instance, atom }, state[usize::from(value)]));
                }
            }
        }
    }
    match operation {
        // A store into the array `base` names (`SetIndex`): its value joins the elements of the array's element class
        // (`Key::Element`), and the class's length changes unless the key is a Number within the length it has. A key
        // that is no Number may name a property (`wild`) or the length, so its store changes the length.
        Some(Operation::ElementWrite { value, base, key, .. }) => {
            let (array, index, stored) = (state[usize::from(base)], state[usize::from(key)], state[usize::from(value)]);
            let length = settled(ctx, array).elements.length;
            let in_bounds = index.is_number() && (length == Length::Never || typing::within_length(length, index.cell.ty));
            let length = if in_bounds { Length::Never } else { Length::Any };
            // A store the previous round proved hole-free leaves no hole, as its index is within the filled prefix.
            let hole_free = ctx.hole_free_stores.contains(&(ctx.id, pc as u32));
            let elements = Elements { ty: Type::NEVER, classes: Classes::Never, length, view: View::None, holes: !in_bounds && !hole_free };
            events.writes.push((Key::Element(array.elements_key()), Word::of_cell(Cell { ty: stored.cell.ty, classes: stored.cell.classes, elements, sites: stored.cell.sites, global: stored.cell.global, prototype: stored.cell.prototype, identity: Identity::of_type(stored.cell.ty) })));
            // The stored value is an element of the array, which reads it back with the unnamed class.
            events.links.extend(stored.link(ElementClass::Unnamed));
            // A key of no value yet (`Type::NEVER`, a pass from the bottom) names no property: a later pass sees its value.
            if !index.is_number() && index.cell.ty != Type::NEVER {
                events.wild = events.wild.join(Holders::of(array.cell));
            }
        }
        // A store whose operands the decode does not name (`IndexWrite`, `IndexSetHelper`) names no array: it may store
        // any value into an array of any class and change any length, so every class's elements are any (`wild_elements`).
        Some(Operation::IndexWrite { .. } | Operation::IndexSetHelper { .. }) => {
            events.wild = Holders::ANY;
            events.wild_elements = true;
        }
        // `delete base[key]` keeps the values the elements hold and changes the length of `base`'s class (a hole, or the
        // last element removed); it may name a property, in the holders of its base (`wild`).
        Some(Operation::Delete { base, .. }) => {
            events.wild = events.wild.join(Holders::of(state[usize::from(base)].cell));
            events.writes.push(length_change(state[usize::from(base)].elements_key()));
        }
        Some(Operation::SetField { value, atom, base, .. }) => {
            events.writes.push((Key::Member(atom), state[usize::from(value)]));
            let owner = match base { Some(base) => state[usize::from(base)], None if !ctx.is_root => this_fact(ctx), None => Word::UNKNOWN };
            events.member_bases.push((atom, Holders::of(owner.cell)));
            if ctx.scope.length == Some(atom) {
                events.writes.push(length_change(owner.elements_key()));
            }
            // A store into an object of a known class (`Word::class_site`) is that class's own field, as a class
            // body's method is (`DefinePropertyRecord`): a method on its prototype (`C.prototype.m = f`), or a data
            // field of its instances (`this.link = link` in `C`), which a read off an instance of `C` resolves per class.
            // A store into an object of no one class may land in any class's instance (`Key::MemberAny`). A store of `prototype`
            // into a constructor the solve knows is that constructor's own static property (`Key::MemberOn` with `instance: false`):
            // `copies::expand` reads the literal it stores and writes its keys as the class's prototype members.
            let constructor = if Some(atom) == ctx.scope.prototype && owner.cell.ty.body_index().is_some() { ctx.classes.of_word(owner) } else { None };
            let key = match (constructor, owner.class_site()) {
                (Some(class), _) => Key::MemberOn { class, instance: false, atom },
                (None, Some((class, true, _))) => Key::MemberOn { class, instance: true, atom },
                _ => Key::MemberAny(atom),
            };
            // A store of an accessor's name may run its setter instead of storing the value (`accessor_call`): no class's field holds it.
            let stored = if ctx.accessors.contains(&atom) { Word::UNKNOWN } else { state[usize::from(value)] };
            events.writes.push((key, stored));
        }
        Some(Operation::StoreField { atom, .. } | Operation::FieldStoreMiss { atom, .. } | Operation::FieldAdd { atom, .. }) => {
            events.writes.push((Key::Member(atom), Word::UNKNOWN));
            // The base of a store whose operands the decode does not name is unknown, so the name's object may be any.
            events.member_bases.push((atom, Holders::ANY));
            events.writes.push((Key::MemberAny(atom), Word::UNKNOWN));
            // A length store through an object the analysis does not name may resize an array of any class.
            events.wild_elements |= ctx.scope.length == Some(atom);
        }
        _ if instruction.op().is_field_store() && !matches!(operation, Some(Operation::SetField { .. })) => {
            events.writes.push((Key::Member(instruction.atom_index()), Word::UNKNOWN));
            events.member_bases.push((instruction.atom_index(), Holders::ANY));
            events.writes.push((Key::MemberAny(instruction.atom_index()), Word::UNKNOWN));
            events.wild_elements |= ctx.scope.length == Some(instruction.atom_index());
        }
        _ => {}
    }
}

/// Whether a store through a computed key (`operation`) may write a name of the global object: a key that is no Number
/// (a Number names no identifier a name read reads), into a receiver that may be the global object (`may_be_global`).
/// A store through an operation with no receiver the analysis names may be any of them.
fn writes_global_name(operation: Option<&Operation<(), ()>>, state: &[Word]) -> bool {
    match operation {
        Some(Operation::ElementWrite { base, key, .. } | Operation::Delete { base, key, .. }) =>
            !state[usize::from(*key)].is_number() && may_be_global(state[usize::from(*base)]),
        Some(Operation::IndexWrite { .. } | Operation::IndexSetHelper { .. }) => true,
        _ => false,
    }
}

/// Whether a value of cell `word` may be the global object: an object of no one known class (`Classes::Many`), as the
/// global object is no instance of a constructed class.
fn may_be_global(word: Word) -> bool {
    word.cell.classes == Classes::Many && word.cell.ty.direct_object_part().is_some()
}

/// The fact of a named read of `atom` whose type is `cell`, off `owner`: the holder's class where the owner is an instance
/// (`Word::member_on`), else the name it is read by (`Word::member`).
pub(super) fn named_word(ctx: &Ctx<'_>, owner: Option<Word>, cell: Cell, atom: u32) -> Word {
    match owner.and_then(Word::class_site) {
        Some(site) => {
            let (holder, instance, guess) = site_holder(ctx, site, atom);
            Word { cell, ..Word::member_on(holder, instance, atom, guess) }
        }
        None => Word::member(atom, cell),
    }
}
