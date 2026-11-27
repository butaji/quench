//! The facts the closed-world analyses carry: what a register holds, and which binding a write
//! reaches.

use crate::typed_ir::{Representation, Type};

/// A binding: a global name (by atom), or a slot of the environment or frame of `owner`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Key {
    Global(u32),
    Slot { owner: u32, slot: u16 },
    /// The named property `atom` of any object: every store of that name, wherever it lands.
    Member(u32),
    /// A method `atom` stored on class `class`'s static object (`instance: false`) or its prototype.
    MemberOn { class: u32, instance: bool, atom: u32 },
    /// The value function `0` returns: every return it makes (`types`).
    Return(u32),
    /// The elements of the arrays field `0` holds: every indexed store through a read of that field (`types`).
    Element(u32),
}

/// Where a register's value was read from, which the census follows to the closure it names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Origin {
    /// The value the binding held when the register was loaded from it.
    Binding(Key),
    /// The value of a named property `atom` read off some object (a method read before its call).
    Member(u32),
    /// A property-key constant, by the atom its text interns to.
    Atom(u32),
    /// The `prototype` property of a function (`PrototypeBase`): the object its methods are stored
    /// on. Off a closure (`PrototypeBase::Closure`) it is the prototype of that class.
    Prototype(PrototypeBase),
    /// An instance of class `function` (`new`).
    Instance(u32),
    /// A method `atom` read on class `function`'s static object (`instance: false`) or prototype.
    MemberOn { class: u32, instance: bool, atom: u32 },
}

/// What a `prototype` read was read off: a closure, or a binding the census resolves to one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum PrototypeBase {
    Closure(u32),
    Binding(Key),
}

/// What a register is known to hold at a point: its type in the one fact lattice
/// (`typed_ir::Type`: a closure's body, int32, Number, any value) and, when it was read from a
/// binding or a named property, where from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Fact {
    pub(super) ty: Type,
    pub(super) origin: Option<Origin>,
}

impl Fact {
    /// Any value, from nowhere the census follows.
    pub(super) const UNKNOWN: Self = Self::of(Type::ANY);

    /// A value of type `ty`.
    pub(super) const fn of(ty: Type) -> Self {
        Self { ty, origin: None }
    }

    /// Any value, read from `origin`.
    pub(super) const fn from(origin: Origin) -> Self {
        Self { ty: Type::ANY, origin: Some(origin) }
    }

    /// The closure of function `function`.
    pub(super) const fn closure(function: u32) -> Self {
        Self::of(Type::function_body(function))
    }

    /// The property-key constant `atom`.
    pub(super) const fn atom(atom: u32) -> Self {
        Self::from(Origin::Atom(atom))
    }

    /// An instance of class `class` (`new`): a direct object.
    pub(super) const fn instance(class: u32) -> Self {
        Self { ty: Type::DIRECT_OBJECT_FAMILY, origin: Some(Origin::Instance(class)) }
    }

    /// The value of field `atom` read off some object, of type `ty` (`types::Types::field`).
    pub(super) const fn member(atom: u32, ty: Type) -> Self {
        Self { ty, origin: Some(Origin::Member(atom)) }
    }

    /// The method `atom` read on class `class`'s static object (`instance: false`) or prototype.
    pub(super) const fn member_on(class: u32, instance: bool, atom: u32) -> Self {
        Self::from(Origin::MemberOn { class, instance, atom })
    }

    /// The class object `self` is: a closure's constructor (`instance: false`), or a prototype or
    /// instance of one (`instance: true`).
    pub(super) fn class_site(self) -> Option<(u32, bool)> {
        if let Some(class) = self.ty.body_index() {
            return Some((class, false));
        }
        match self.origin? {
            Origin::Prototype(PrototypeBase::Closure(class)) | Origin::Instance(class) => Some((class, true)),
            _ => None,
        }
    }

    /// The atom `self` is the property-key constant of, if it is one.
    pub(super) fn atom_key(self) -> Option<u32> {
        match self.origin? {
            Origin::Atom(atom) => Some(atom),
            _ => None,
        }
    }

    pub(super) fn is_number(self) -> bool {
        self.ty != Type::NEVER && self.ty.is_subtype(Type::NUMBER)
    }

    /// The fact both describe: the join of their types, from the origin both share.
    pub(super) fn join(self, other: Self) -> Self {
        Self { ty: self.ty.join(other.ty), origin: if self.origin == other.origin { self.origin } else { None } }
    }

    /// The fact of the Number `value`: an int32 unless it is not one, or is `-0`.
    pub(super) fn number(value: f64) -> Self {
        Self::of(if crate::value::exact_int32(value).is_some() { Type::INT32 } else { Type::NUMBER })
    }

    /// The representation a register holding this fact at every point has.
    pub(super) fn representation(self) -> Option<Representation> {
        [Representation::Int32, Representation::Float64].into_iter().find(|representation| representation.accepts(self.ty))
    }
}
