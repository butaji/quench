//! The facts the closed-world analyses carry: what a register holds, and which binding a write
//! reaches.

use crate::typed_ir::{Cell, ElementClass, Elements, Representation, Type};

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
    /// The elements of the arrays of an element class: every indexed store into an array of that class (`types`).
    Element(ElementClass),
}

impl Key {
    /// The element class an array stored into this binding or field joins (`Word::link`). A field's (`Member`,
    /// `MemberAny`, `MemberOn`) is the field's name; a global's, a slot's and a return's is its own, so an array held
    /// there keeps its class; an element's is the unnamed class, as an array read out of an element has no origin.
    pub(super) fn elements_class(self) -> ElementClass {
        match self {
            Self::Member(atom) | Self::MemberAny(atom) | Self::MemberOn { atom, .. } => ElementClass::Field(atom),
            Self::Global(atom) => ElementClass::Global(atom),
            Self::Slot { owner, slot } => ElementClass::Slot { owner, slot },
            Self::Return(function) => ElementClass::Return(function),
            Self::Element(_) => ElementClass::Unnamed,
        }
    }
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
    /// An object a host constructor makes (`new Error(...)`, a builtin the program binds to nothing): fresh, and an instance of
    /// no program class, so unseen code that holds it reaches no class's own stores through it (`escapes::Reach`).
    Foreign,
    /// The array a literal or an array constructor allocates at `pc` of `function` (`effects::value_of`): its own element
    /// class, which only a store or a call joins to another (`Word::link`).
    Alloc { function: u32, pc: u32 },
    /// The argument parameter `index` of `function` holds in a register on entry (`flow::Solver::entry_facts`): its array's
    /// class is the parameter's, which a resolved call joins to the argument's (`types::entry_links`).
    Parameter { function: u32, index: u16 },
    /// The `this` of function `0` (`effects::this_fact`): its array's class is the receiver's, as a parameter's is.
    Receiver(u32),
    /// A method `atom` read on class `function`'s static object (`instance: false`) or prototype;
    /// `guess` where the class is the type solve's (`Classes::Set { proven: false }`), not a `new`'s.
    MemberOn { class: u32, instance: bool, atom: u32, guess: bool },
}

impl Origin {
    /// The key a value read from this origin was read from, where the origin names one: a binding, or the named property a
    /// read takes (`Key::Member`, or a class's own method `Key::MemberOn`). `ClassIds::of_word` splits a closure by it.
    pub(super) fn key(self) -> Option<Key> {
        match self {
            Self::Binding(key) => Some(key),
            Self::Member(atom) => Some(Key::Member(atom)),
            Self::MemberOn { class, instance, atom, .. } => Some(Key::MemberOn { class, instance, atom }),
            _ => None,
        }
    }
}

/// An accessor a definition makes: `get atom()` (a getter) or `set atom(v)` (`setter`), on the object of class `site`
/// (its class, and whether the instances' or the static object's) where the solve proves one class, and by the function it
/// names where the solve knows it, at instruction `pc` (`accessors::accessor_definition`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Accessor {
    pub(super) site: Option<(u32, bool)>,
    pub(super) atom: u32,
    pub(super) setter: bool,
    pub(super) function: Option<u32>,
    pub(super) pc: u32,
}

/// What a `prototype` read was read off: a closure, or a binding the census resolves to one. `Closure` holds the closure's
/// class id (`ClassIds::of_word`), its body unless the body is constructed under several keys.
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

    /// The element class of the array `self` holds: the key it was read from (`Key::elements_class`), the field it was
    /// read off, or the allocation it was made at; the unnamed class where no origin names one. Every store into an
    /// array names its class by this key, and the class's cell is read back through it (`effects::settled`).
    pub(super) fn elements_key(self) -> ElementClass {
        match self.origin {
            Some(Origin::Member(atom) | Origin::MemberOn { atom, .. }) => ElementClass::Field(atom),
            Some(Origin::Binding(key)) => key.elements_class(),
            Some(Origin::Alloc { function, pc }) => ElementClass::Allocation { function, pc },
            Some(Origin::Parameter { function, index }) => ElementClass::Parameter { function, index },
            Some(Origin::Receiver(function)) => ElementClass::Receiver(function),
            // A closure of one body with no origin (its allocation, or a join of its reads) names its body's class, so a
            // store of it into a field joins that class, not the unnamed one every unresolved value shares.
            None => self.cell.ty.body_index().map_or(ElementClass::Unnamed, ElementClass::Closure),
            _ => ElementClass::Unnamed,
        }
    }

    /// The link a store of `self` into the element class `class` makes: `self`'s class joins `class` when `self` may hold
    /// an array (its elements are not `Elements::NEVER`). The array is read back from the destination with that
    /// destination's origin, so the two classes must be one: a store through either reaches the other's elements.
    /// `None` where `self` holds no array or already is of `class`.
    pub(super) fn link(self, class: ElementClass) -> Option<(ElementClass, ElementClass)> {
        let from = self.elements_key();
        (self.cell.elements != Elements::NEVER && from != class).then_some((from, class))
    }

    /// The links a join of `self` with `other` makes (`flow::Solver::join_words`): each word whose class is not the joined
    /// word's joins it, since a store or read through the joined word names that class. A join of one class makes no link.
    pub(super) fn join_links(self, other: Self) -> impl Iterator<Item = (ElementClass, ElementClass)> {
        let joined = self.join(other).elements_key();
        [self, other].into_iter().filter_map(move |word| word.link(joined))
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

    /// The representation a register holding this word at every point has (`Cell::representation`).
    pub(super) fn representation(self) -> Option<Representation> {
        self.cell.representation()
    }
}
