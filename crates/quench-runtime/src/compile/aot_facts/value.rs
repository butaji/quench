//! The facts the closed-world analyses carry: what a register holds, and which binding a write
//! reaches.

use crate::typed_ir::{Cell, Representation, Type};

/// The elements key of the arrays no field names (an alias, a local, an array an opaque store reaches): every
/// element read joins its cell (`types::Types::unnamed_elements`), so no array's element type or length is
/// trusted past a store that names no field.
pub(super) const UNNAMED_ARRAYS: u32 = u32::MAX;

/// A binding: a global name (by atom), or a slot of the environment or frame of `owner`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Key {
    Global(u32),
    Slot { owner: u32, slot: u16 },
    /// The named property `atom` of any object: every store of that name, wherever it lands.
    Member(u32),
    /// A method `atom` stored on class `class`'s static object (`instance: false`) or its prototype.
    MemberOn { class: u32, instance: bool, atom: u32 },
    /// The named property `atom` of an object of no one class the solve names (`Classes::Many`): a store a
    /// read off an instance of any class may see, so a class's own field (`MemberOn`) joins it.
    MemberAny(u32),
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
    /// A method `atom` read on class `function`'s static object (`instance: false`) or prototype;
    /// `guess` where the class is the type solve's (`Classes::Set { proven: false }`), not a `new`'s.
    MemberOn { class: u32, instance: bool, atom: u32, guess: bool },
}

/// What a `prototype` read was read off: a closure, or a binding the census resolves to one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum PrototypeBase {
    Closure(u32),
    Binding(Key),
}

/// A word of the solve's abstract stage: what a register holds at a point, as one cell of the fact
/// lattice (`typed_ir::Cell`: its type, the classes of its objects, what its arrays keep), and, when it
/// was read from a binding or a named property, where from (`Origin`), which the census follows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Word {
    pub(super) cell: Cell,
    pub(super) origin: Option<Origin>,
}

impl Word {
    /// Any value, from nowhere the census follows.
    pub(super) const UNKNOWN: Self = Self { cell: Cell::ANY, origin: None };

    /// A value of type `ty`, its classes from the type alone (`Cell::of`).
    pub(super) fn of(ty: Type) -> Self {
        Self::of_cell(Cell::of(ty))
    }

    /// A value of cell `cell`, from nowhere the census follows.
    pub(super) const fn of_cell(cell: Cell) -> Self {
        Self { cell, origin: None }
    }

    /// Any value, read from `origin`.
    pub(super) const fn from(origin: Origin) -> Self {
        Self { cell: Cell::ANY, origin: Some(origin) }
    }

    /// The closure of function `function`.
    pub(super) fn closure(function: u32) -> Self {
        Self::of(Type::function_body(function))
    }

    /// The property-key constant `atom`.
    pub(super) const fn atom(atom: u32) -> Self {
        Self::from(Origin::Atom(atom))
    }

    /// An instance of class `class` (`new`): a direct object, the construction's own result.
    pub(super) fn instance(class: u32) -> Self {
        Self { cell: Cell::instance(class), origin: Some(Origin::Instance(class)) }
    }

    /// The value of field `atom` read off some object, of cell `cell` (`types::Types::cell`).
    pub(super) const fn member(atom: u32, cell: Cell) -> Self {
        Self { cell, origin: Some(Origin::Member(atom)) }
    }

    /// The method `atom` read on class `class`'s static object (`instance: false`) or prototype,
    /// a guess where the class is the solve's (`Origin::MemberOn::guess`).
    pub(super) const fn member_on(class: u32, instance: bool, atom: u32, guess: bool) -> Self {
        Self::from(Origin::MemberOn { class, instance, atom, guess })
    }

    /// The class object `self` is: a closure's constructor (`instance: false`), or a prototype or
    /// instance of one class (`instance: true`); with whether the class is the solve's guess. A closed
    /// union of two or more classes names no one class (`Classes::ids` lists them).
    pub(super) fn class_site(self) -> Option<(u32, bool, bool)> {
        if let Some(class) = self.cell.ty.body_index() {
            return Some((class, false, false));
        }
        match self.origin {
            Some(Origin::Prototype(PrototypeBase::Closure(class)) | Origin::Instance(class)) => Some((class, true, false)),
            _ => self.cell.classes.single().map(|(class, proven)| (class, true, !proven)),
        }
    }

    /// The `Key::Element` an element store into the array `self` names: the field the array was read from, else
    /// `UNNAMED_ARRAYS`.
    pub(super) fn elements_key(self) -> u32 {
        match self.origin {
            Some(Origin::Member(atom) | Origin::MemberOn { atom, .. }) => atom,
            _ => UNNAMED_ARRAYS,
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
        self.cell.ty != Type::NEVER && self.cell.ty.is_subtype(Type::NUMBER)
    }

    /// The word both describe: the join of their cells, from the origin both share.
    pub(super) fn join(self, other: Self) -> Self {
        Self { cell: self.cell.join(other.cell), origin: if self.origin == other.origin { self.origin } else { None } }
    }

    /// The word of the Number `value`: an int32 unless it is not one, or is `-0`.
    pub(super) fn number(value: f64) -> Self {
        Self::of(if crate::value::exact_int32(value).is_some() { Type::INT32 } else { Type::NUMBER })
    }

    /// The representation a register holding this word at every point has (`Cell::representation`).
    pub(super) fn representation(self) -> Option<Representation> {
        self.cell.representation()
    }
}
