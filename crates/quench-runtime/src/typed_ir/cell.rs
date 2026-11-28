//! The one fact lattice (L2 of the layered design, TOD-2445/TOD-2448): one `Cell` per value, slot,
//! parameter, return, receiver and element. A cell is the product of three lattices, joined
//! component-wise:
//!
//! - `ty`: the semantic type (`Type`), which already carries the integer range (`TypeKind::Int`), the
//!   NaN bit of a Number (`TypeKind::NotNaN` is a Number that is never NaN), nullability and function
//!   identity. A field's or an element's `ty` is the join of every store's, so its range and NaN bit
//!   are the join of the stores' and the widening of `staged::typing` keeps a grown range bounded;
//! - `classes`: the constructed classes of the objects the value may hold (`Classes`), a closed
//!   union of at most `MAX_CLASSES`, so a polymorphic call site (`this.task.run()`) is a switch on the
//!   class word;
//! - `elements`: what an array the value holds keeps in its elements (`Elements`): their type and
//!   classes, and a fixed length where no edge can change it;
//! - `sites`: the object-literal allocation sites the object may come from (`Sites`), which a copy
//!   (`Object.assign`, an extend helper) reads the keys and values of. Kept apart from `classes`: a literal
//!   is no constructed class's instance, so its site is not a class id;
//! - `identity`: which one object the value is, where the solve proves it (`Identity`): a builtin the
//!   closed world never changes, the realm's global object, or a closure of a body made once per run. A
//!   callee or a base whose identity is proven needs no compare in execution.
//!
//! Representation is derived from the cell (`Cell::representation`), never stored. A store and a read
//! form a Galois connection: a slot's cell is the join of every stored cell (α), and a read takes the
//! slot's cell (γ). Code that trusts a cell subscribes to its key (`FactKey`), and an edge that moves the
//! cell up the lattice retires every subscriber the same way (L3, `code_dependencies::cells`).
//!
//! This module is the only home of a fact type: a `struct`/`enum` named `*Fact` elsewhere is a second
//! home (census `fact_homes`).
use super::{IntRange, Representation, Type};
use crate::heap::TypedArrayKind;
use crate::ValueType;

/// The most classes a closed union holds before it is `Classes::Many`: richards' four task classes.
pub const MAX_CLASSES: usize = 4;

/// The most literal sites a closed union holds before it is `Sites::Many`.
pub const MAX_SITES: usize = 4;

/// The sorted union of two sorted, distinct lists, as a packed array of `N` with its length; `None` past `N` elements.
fn union<T: Ord + Copy + Default, const N: usize>(a: &[T], b: &[T]) -> Option<([T; N], u8)> {
    let mut out = [T::default(); N];
    let mut len = 0usize;
    let (mut i, mut j) = (0usize, 0usize);
    while i < a.len() || j < b.len() {
        let next = match (a.get(i), b.get(j)) {
            (Some(&x), Some(&y)) if x == y => { i += 1; j += 1; x }
            (Some(&x), Some(&y)) if x < y => { i += 1; x }
            (Some(_), Some(&y)) => { j += 1; y }
            (Some(&x), None) => { i += 1; x }
            (None, Some(&y)) => { j += 1; y }
            (None, None) => break,
        };
        if len == N { return None; }
        out[len] = next;
        len += 1;
    }
    Some((out, len as u8))
}

/// The classes (constructed functions, by function id) of the objects a value may hold: a flat lattice
/// widened to closed unions. `Never` holds no object (a primitive, `null`, `undefined`, or nothing yet);
/// `Set` holds instances of the listed classes only, `proven` where every one is a construction's own
/// result rather than the solve's guess through a slot; `Many` may hold any object.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Classes {
    Never,
    /// `ids[..len]`, sorted and distinct, `1 <= len <= MAX_CLASSES`.
    Set { ids: [u32; MAX_CLASSES], len: u8, proven: bool },
    Many,
}

impl Classes {
    /// Instances of class `class` alone.
    pub const fn one(class: u32, proven: bool) -> Self {
        Self::Set { ids: [class, 0, 0, 0], len: 1, proven }
    }

    /// The classes of a value of type `ty` the solve knows no construction of: none where the type holds
    /// no object (`Type::IMMEDIATE`: a Number, a Boolean, `null`, `undefined`, or nothing yet), any otherwise.
    pub fn of_type(ty: Type) -> Self {
        if ty.is_subtype(Type::IMMEDIATE) { Self::Never } else { Self::Many }
    }

    /// The classes either may hold: the union, `Many` past `MAX_CLASSES`.
    pub fn join(self, other: Self) -> Self {
        match (self, other) {
            (Self::Never, other) | (other, Self::Never) => other,
            (Self::Many, _) | (_, Self::Many) => Self::Many,
            (Self::Set { ids: a, len: la, proven: pa }, Self::Set { ids: b, len: lb, proven: pb }) => {
                match union::<u32, MAX_CLASSES>(&a[..usize::from(la)], &b[..usize::from(lb)]) {
                    Some((ids, len)) => Self::Set { ids, len, proven: pa && pb },
                    None => Self::Many,
                }
            }
        }
    }

    /// The classes as read back from a slot the solve stored them in: a guess.
    pub const fn guessed(self) -> Self {
        match self {
            Self::Set { ids, len, .. } => Self::Set { ids, len, proven: false },
            other => other,
        }
    }

    /// The listed classes of a closed union, none for `Never` or `Many`.
    pub fn ids(&self) -> &[u32] {
        match self {
            Self::Set { ids, len, .. } => &ids[..usize::from(*len)],
            _ => &[],
        }
    }

    /// The one class and whether it is proven, where the union holds exactly one.
    pub fn single(self) -> Option<(u32, bool)> {
        match self {
            Self::Set { ids, len: 1, proven } => Some((ids[0], proven)),
            _ => None,
        }
    }

    /// Whether every class listed is a construction's own result (`Set { proven }`).
    pub const fn proven(self) -> bool {
        matches!(self, Self::Set { proven: true, .. })
    }

    /// Whether an object of class `class` may be among them.
    pub fn may_hold(self, class: u32) -> bool {
        match self {
            Self::Never => false,
            Self::Set { .. } => self.ids().contains(&class),
            Self::Many => true,
        }
    }
}

/// An object-literal allocation site: the function and the PC of the allocation (`Record::note_literal`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Site {
    pub function: u32,
    pub pc: u32,
}

/// The literal allocation sites an object value may come from: a flat lattice widened to closed unions, joined like
/// `Classes`. `Never` holds no object; `Set` holds objects of the listed sites only (`sites[..len]`, sorted and distinct);
/// `Many` may hold an object of any origin, a literal or not.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Sites {
    Never,
    Set { sites: [Site; MAX_SITES], len: u8 },
    Many,
}

impl Sites {
    /// Objects of the one literal site `site` alone.
    pub fn one(site: Site) -> Self {
        let mut sites = [Site::default(); MAX_SITES];
        sites[0] = site;
        Self::Set { sites, len: 1 }
    }

    /// The sites of a value of type `ty` the solve knows no literal of: none where the type holds no object, any otherwise.
    pub fn of_type(ty: Type) -> Self {
        if ty.is_subtype(Type::IMMEDIATE) { Self::Never } else { Self::Many }
    }

    /// The sites either may hold: the union, `Many` past `MAX_SITES`.
    pub fn join(self, other: Self) -> Self {
        match (self, other) {
            (Self::Never, other) | (other, Self::Never) => other,
            (Self::Many, _) | (_, Self::Many) => Self::Many,
            (Self::Set { sites: a, len: la }, Self::Set { sites: b, len: lb }) => {
                match union::<Site, MAX_SITES>(&a[..usize::from(la)], &b[..usize::from(lb)]) {
                    Some((sites, len)) => Self::Set { sites, len },
                    None => Self::Many,
                }
            }
        }
    }

    /// The listed sites of a closed union, `None` where the value is not one (`Never` or `Many`).
    pub fn set(&self) -> Option<&[Site]> {
        match self {
            Self::Set { sites, len } => Some(&sites[..usize::from(*len)]),
            _ => None,
        }
    }
}

/// How many elements an array keeps: none yet, a fixed count no reachable edge (a length write, a
/// `push`, an index store past the end, a `delete`) changes, or any.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Length {
    Never,
    Fixed(u32),
    Any,
}

impl Length {
    pub fn join(self, other: Self) -> Self {
        match (self, other) {
            (Self::Never, other) | (other, Self::Never) => other,
            (Self::Fixed(a), Self::Fixed(b)) if a == b => self,
            _ => Self::Any,
        }
    }
}

/// Which kind of array a value holds, as the element reads of its arrays see it. A typed array's elements are its kind's
/// (`view_element_type`), whatever is stored into it, so a read of a one-kind class takes that type and no store widens it.
/// `None` holds no array at all (the solve's bottom); `Plain` holds only plain arrays, whose elements are the stores';
/// `One` holds only typed arrays of that native kind; `Mixed` holds anything else, which no element read is proven to see.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
    None,
    Plain,
    One(TypedArrayKind),
    Mixed,
}

impl View {
    /// The view of a value that may hold either: equal views stay, a bottom takes the other side, anything else is `Mixed`.
    pub fn join(self, other: Self) -> Self {
        match (self, other) {
            (Self::None, other) | (other, Self::None) => other,
            (Self::Plain, Self::Plain) => Self::Plain,
            (Self::One(a), Self::One(b)) if a == b => self,
            _ => Self::Mixed,
        }
    }

    /// The one typed-array kind every array here is, if there is one.
    pub fn kind(self) -> Option<TypedArrayKind> {
        match self {
            Self::One(kind) => Some(kind),
            _ => None,
        }
    }
}

/// The type of an element of a typed array of `kind`: a Number for the float kinds, the integer range of the
/// element for the integer kinds (ToInt32 modulo the width, the clamped byte). Float16 and the BigInt kinds have
/// no native element and no `View::One`; their type is any value.
pub fn view_element_type(kind: TypedArrayKind) -> Type {
    match kind {
        TypedArrayKind::Float32 | TypedArrayKind::Float64 => Type::NUMBER,
        TypedArrayKind::Int8 => Type::int(IntRange { min: -128, max: 127 }),
        TypedArrayKind::Int16 => Type::int(IntRange { min: -32_768, max: 32_767 }),
        TypedArrayKind::Int32 => Type::INT32,
        TypedArrayKind::Uint8 | TypedArrayKind::Uint8Clamped => Type::int(IntRange { min: 0, max: 255 }),
        TypedArrayKind::Uint16 => Type::int(IntRange { min: 0, max: 65_535 }),
        TypedArrayKind::Uint32 => Type::int(IntRange { min: 0, max: 4_294_967_295 }),
        TypedArrayKind::Float16 | TypedArrayKind::BigInt64 | TypedArrayKind::BigUint64 => Type::ANY,
    }
}

/// What the arrays a value holds keep in their elements: the join of every element store's type and classes (`ty`,
/// `classes`), whether some index within the length may hold no element (`holes`), their length (`Length`), and which
/// kind of array they are (`View`). A hole is not a stored value, so `ty` never holds `undefined` for one: a read of a
/// hole takes `undefined` (`element`), and a store of an actual `undefined` joins it into `ty`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Elements {
    pub ty: Type,
    pub classes: Classes,
    pub length: Length,
    pub view: View,
    /// Whether an index may hold no element: a literal that leaves an index out, `new Array(n)` before it is filled, a
    /// `delete`, or a store past the length. Joins by `or`: a join with a hole-free array keeps the hole.
    pub holes: bool,
}

impl Elements {
    /// No array, or none with an element yet.
    pub const NEVER: Self = Self { ty: Type::NEVER, classes: Classes::Never, length: Length::Never, view: View::None, holes: false };
    /// Arrays of anything, of any length, of any kind, with holes.
    pub const ANY: Self = Self { ty: Type::ANY, classes: Classes::Many, length: Length::Any, view: View::Mixed, holes: true };

    pub fn join(self, other: Self) -> Self {
        Self {
            ty: self.ty.join(other.ty),
            classes: self.classes.join(other.classes),
            length: self.length.join(other.length),
            view: self.view.join(other.view),
            holes: self.holes || other.holes,
        }
    }

    /// Whether every array these elements describe is a plain backing whose count of other values (`Header::others`) is zero:
    /// no hole (`holes` false), every element an int32 (`ty` within `INT32`, so no stored Number or other value), and a plain
    /// array (`View::Plain`). Each store of the class joins its type into `ty` and each hole-making op sets `holes`, so the
    /// backing holds no value `others` counts. `others` is exact only while no native path stores an uncounted value
    /// (`OTHERS_UNKNOWN`); the analysis does not see such stores, so the closed world's gate (`compile/aot_facts/gate.rs`)
    /// is what keeps this fact true for a program.
    pub fn int32_backing(self) -> bool {
        !self.holes && self.view == View::Plain && self.ty.is_subtype(Type::INT32)
    }

    /// The cell an element read takes (γ): the elements' type and classes, the type of a typed array's kind where its
    /// view is one kind, and `undefined` where the read may find a hole: the array has holes, or the index is not within
    /// its length (`in_bounds`, `typing::within_length`).
    pub fn element(self, in_bounds: bool) -> Cell {
        let ty = match self.view {
            View::One(kind) => view_element_type(kind),
            _ => self.ty,
        };
        let ty = if self.holes || !in_bounds { ty.join(Type::language(ValueType::Undefined)) } else { ty };
        Cell { ty, classes: self.classes, elements: Elements::ANY, sites: Sites::Many, global: false, prototype: true, identity: Identity::of_type(ty) }
    }
}

/// One cell of the fact lattice: a value's type, the classes of the objects it may hold and what its
/// arrays keep. The product of the three lattices, joined component-wise.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cell {
    pub ty: Type,
    pub classes: Classes,
    pub elements: Elements,
    /// The literal allocation sites of the objects the value may be (`Sites`).
    pub sites: Sites,
    /// Whether the value may be the global object. Not a lattice point of `ty` or `classes` (`Classes::Many` absorbs
    /// objects, so the global object cannot ride in them): a flag the closed world's verification reads (`aot_facts::gate`).
    pub global: bool,
    /// Whether the value may be a builtin's prototype object (`Array.prototype`, `Object.prototype`, ...): a `.prototype` or
    /// `__proto__` read off a value that is no program closure, a `getPrototypeOf` result, or a value from nowhere the solve
    /// follows (`ANY`). A flag like `global`, joined by OR; the closed world's index-property fact reads it (`aot_facts::gate`).
    pub prototype: bool,
    /// Which one object the value is (`Identity`): proven by the settled solve, else `Many` or `Never` per the type.
    pub identity: Identity,
}

impl Cell {
    /// No value yet: the solve's bottom.
    pub const NEVER: Self = Self { ty: Type::NEVER, classes: Classes::Never, elements: Elements::NEVER, sites: Sites::Never, global: false, prototype: false, identity: Identity::Never };
    /// Any value: the top.
    pub const ANY: Self = Self { ty: Type::ANY, classes: Classes::Many, elements: Elements::ANY, sites: Sites::Many, global: false, prototype: true, identity: Identity::Many };

    /// A value of type `ty` from nowhere the solve follows: its classes, sites and identity from the type alone.
    pub fn of(ty: Type) -> Self {
        let elements = if ty.is_subtype(Type::IMMEDIATE) { Elements::NEVER } else { Elements::ANY };
        Self { ty, classes: Classes::of_type(ty), elements, sites: Sites::of_type(ty), global: false, prototype: false, identity: Identity::of_type(ty) }
    }

    /// The cell with the global object among the values it may hold.
    pub const fn maybe_global(self) -> Self {
        Self { global: true, ..self }
    }

    /// The cell with a builtin's prototype object among the values it may hold (`prototype`).
    pub const fn maybe_prototype(self) -> Self {
        Self { prototype: true, ..self }
    }

    /// A value of type `ty` holding objects of `classes`.
    pub fn with_classes(ty: Type, classes: Classes) -> Self {
        Self { classes, ..Self::of(ty) }
    }

    /// A fresh instance of class `class`, a construction's own result. It is no literal's object, so it has no literal site.
    pub fn instance(class: u32) -> Self {
        Self { sites: Sites::Never, ..Self::with_classes(Type::DIRECT_OBJECT_FAMILY, Classes::one(class, true)) }
    }

    pub fn join(self, other: Self) -> Self {
        Self {
            ty: self.ty.join(other.ty),
            classes: self.classes.join(other.classes),
            elements: self.elements.join(other.elements),
            sites: self.sites.join(other.sites),
            global: self.global || other.global,
            prototype: self.prototype || other.prototype,
            identity: join_identity(self, other),
        }
    }

    /// Whether `self` is below or equal to `other` in the lattice: every value `self` describes `other` does.
    pub fn within(self, other: Self) -> bool {
        self.join(other) == other
    }

    /// The cell as read back from where the solve stored it: its classes a guess (`Classes::guessed`).
    pub fn guessed(self) -> Self {
        Self { classes: self.classes.guessed(), elements: Elements { classes: self.elements.classes.guessed(), ..self.elements }, ..self }
    }

    /// The integer range the value holds, where it is an int32 with a known range.
    pub fn range(self) -> Option<IntRange> {
        self.ty.int_range()
    }

    /// The representation a value of this cell has at every point: derived from the type, never stored.
    pub fn representation(self) -> Option<Representation> {
        [Representation::Int32, Representation::Float64].into_iter().find(|representation| representation.accepts(self.ty))
    }
}

/// A builtin the program may name: a global binding (`Math`), or a member of one (`Math.sqrt`), by atom.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BuiltinPath {
    /// The atom of the global name.
    pub global: u32,
    /// The atom of the member the global object's builtin holds (`String.fromCharCode`), None for the global itself.
    pub member: Option<u32>,
}

/// Which one object a value is: a flat lattice. `Never` holds no object (a primitive, or nothing yet); `Builtin`,
/// `GlobalObject` and `Closure` are the proven identities, and `Many` is any other object or none known.
///
/// Proven: where the settled solve keeps a `Builtin(p)`, `GlobalObject` or `Closure(b)` in a cell, the value is that builtin /
/// the realm's global object / that body's one closure object in every run (`AotFacts::builtin_identity`,
/// `AotFacts::global_object_identity`, `AotFacts::closure_identity`); every other identity the solve proposed is demoted to
/// `Many` after the solve (`identity::proven`). The lane drops its callee or base compare on the proof.
///
/// `GlobalObject` implies the cell's `global` flag (the may-fact the closed world's gate reads). The flag is kept as it is:
/// `global` says the value may be the global object, `GlobalObject` says it is, in every run.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Identity {
    Never,
    Builtin(BuiltinPath),
    /// The realm's global object, in every run: the root's `this` of a Script, the value of a by-name read of `globalThis`,
    /// or a sloppy function's `this` where every call binds it to the global object (a plain call).
    GlobalObject,
    /// The one closure object of body `0`, made once per run. Derived from the body a function type carries (`Type::body_index`).
    Closure(u32),
    Many,
}

impl Identity {
    /// The identity of a value of type `ty` the solve knows no builtin of: its closure's body where the type holds one, none
    /// where the type holds no object (`Type::IMMEDIATE`), any object otherwise.
    pub fn of_type(ty: Type) -> Self {
        match ty.body_index() {
            Some(body) => Self::Closure(body),
            None if ty.is_subtype(Type::IMMEDIATE) => Self::Never,
            None => Self::Many,
        }
    }

    /// The identities either may hold: `Never` is the unit, equal identities stay, any other pair is `Many`.
    pub fn join(self, other: Self) -> Self {
        match (self, other) {
            (Self::Never, other) | (other, Self::Never) => other,
            (left, right) if left == right => left,
            _ => Self::Many,
        }
    }
}

/// The identity of a join of two cells. `Never` is the unit only where a side holds no value at all (`Cell::NEVER`): a
/// primitive side joined with an object's identity is no one object, so `Many`.
fn join_identity(left: Cell, right: Cell) -> Identity {
    match (left.identity, right.identity) {
        (Identity::Never, Identity::Never) => Identity::Never,
        (Identity::Never, other) if left.ty == Type::NEVER => other,
        (other, Identity::Never) if right.ty == Type::NEVER => other,
        (Identity::Never, _) | (_, Identity::Never) => Identity::Many,
        (left, right) => left.join(right),
    }
}

/// The objects a store under an unknown name may reach: the constructed classes and the literal sites those objects come from,
/// joined like the components of a `Cell`. `NONE` holds no object; `ANY` may be any object. A store whose name is unknown
/// (a computed key, a copy from an object whose names the solve cannot list) writes into the holders of its destination,
/// and a read of a name is wild only where its object may be one of those holders (`Holders::overlaps`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Holders {
    pub classes: Classes,
    pub sites: Sites,
}

impl Default for Holders {
    fn default() -> Self {
        Self::NONE
    }
}

impl Holders {
    /// No object: a program with no store under an unknown name.
    pub const NONE: Self = Self { classes: Classes::Never, sites: Sites::Never };
    /// Any object: a store under an unknown name whose destination is unknown.
    pub const ANY: Self = Self { classes: Classes::Many, sites: Sites::Many };

    /// The holders of the objects of `cell`. A cell that names literal sites holds literals, which are no class's instances,
    /// so its classes are none; any other cell holds its classes.
    pub fn of(cell: Cell) -> Self {
        match cell.sites {
            Sites::Set { .. } => Self { classes: Classes::Never, sites: cell.sites },
            _ => Self { classes: cell.classes, sites: cell.sites },
        }
    }

    /// The holders either may name.
    pub fn join(self, other: Self) -> Self {
        Self { classes: self.classes.join(other.classes), sites: self.sites.join(other.sites) }
    }

    /// Whether no object is a holder.
    pub fn is_none(self) -> bool {
        self == Self::NONE
    }

    /// Whether an object both may be a holder of `self` and may be one of `other`: a class or a literal site they share, or
    /// either side's unknown (`Many`) with the other side holding anything.
    pub fn overlaps(self, other: Self) -> bool {
        let classes = match (self.classes, other.classes) {
            (Classes::Never, _) | (_, Classes::Never) => false,
            (Classes::Many, _) | (_, Classes::Many) => true,
            (mine, theirs) => mine.ids().iter().any(|&class| theirs.may_hold(class)),
        };
        let sites = match (self.sites, other.sites) {
            (Sites::Never, _) | (_, Sites::Never) => false,
            (Sites::Many, _) | (_, Sites::Many) => true,
            (mine, theirs) => mine.set().is_some_and(|sites| sites.iter().any(|site| theirs.set().is_some_and(|others| others.contains(site)))),
        };
        classes || sites
    }
}

/// The element class of an array: the key whose arrays share one set of elements. An array's elements belong to its
/// class, not to the register, field or binding it is read through: a store into any array of a class writes the
/// elements of every array of it, so an array read through another key keeps what the class's stores leave. Each key
/// an array is stored into joins its class with the destination's (`aot_facts::census::ElementClasses`), so two
/// classes are one only where the program moves an array between them; a literal is its own class until then.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ElementClass {
    /// The arrays a named property holds: every store of the name, on any object or on one class's own.
    Field(u32),
    /// The arrays global binding `atom` holds.
    Global(u32),
    /// The arrays slot `slot` of the environment or frame of function `owner` holds.
    Slot { owner: u32, slot: u16 },
    /// The arrays function `0` returns. A call's result is read as its callee's return class.
    Return(u32),
    /// The array the literal or array constructor at `pc` of function `function` allocates: its own class until a
    /// store or a call joins it to another.
    Allocation { function: u32, pc: u32 },
    /// The arrays parameter `index` of function `function` holds on entry. A resolved call joins each argument's class to
    /// it, so the callee's stores into the parameter reach the caller's array (`types::entry_links`).
    Parameter { function: u32, index: u16 },
    /// The arrays `this` of function `function` holds. A resolved call joins its receiver's class to it, as a parameter's.
    Receiver(u32),
    /// The arrays no key names: an element of an array, a value read with no origin, an argument or receiver of a call
    /// the solve does not resolve, and every array that may reach code the solve does not follow (an escaped function's).
    /// The arguments of a function that reads its `arguments` object join it too (`flow::bound_parameters`).
    Unnamed,
}

/// The key of a cell the program's whole solve keeps, which code that trusts the cell subscribes to and
/// an edge moving it retires (L3). Every key names a set of slots the run-time writes, so each edge (a
/// store, a method slot write, a global write, a length change) names the key it moves.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FactKey {
    /// Field `atom` of class `class`'s instances (`instance`) or of its constructor or prototype.
    Field { class: u32, instance: bool, atom: u32 },
    /// Field `atom` of an object of no one class: every class's instances may hold the store.
    AnyField(u32),
    /// The values function `0` returns.
    Return(u32),
    /// Global binding `atom`.
    Global(u32),
    /// Slot `slot` of the environment or frame of function `owner`.
    Slot { owner: u32, slot: u16 },
    /// The elements of the arrays of an element class: every store into an array of the class writes them, joined
    /// with the array's own cell. A store through any array of the class moves this cell, so every key of the class
    /// is retired together (`AotFacts::element_class_keys`).
    Elements(ElementClass),
    /// Parameter `index` of function `function`.
    Parameter { function: u32, index: u16 },
    /// The `this` of function `0`.
    Receiver(u32),
    /// The closure cell of body `body` in program `program` (`ProgramStore::closure_cell`): an edge fact, not a solved cell.
    /// The creation edge of the body's one closure writes it once (`Vm::note_closure_created`), and that write retires the
    /// code subscribed to the key, since a subscriber was compiled before the closure existed.
    ClosureCell { program: u32, body: u32 },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classes_join_is_a_closed_union_up_to_four() {
        let a = Classes::one(9, true);
        let b = Classes::one(12, false);
        let ab = a.join(b);
        assert_eq!(ab.ids(), &[9, 12]);
        assert!(!ab.proven());
        let all = ab.join(Classes::one(10, true)).join(Classes::one(11, true));
        assert_eq!(all.ids(), &[9, 10, 11, 12]);
        assert_eq!(all.join(Classes::one(13, true)), Classes::Many);
        assert_eq!(all.join(Classes::Never), all);
        assert_eq!(all.join(all), all);
    }

    #[test]
    fn cell_join_is_componentwise_and_monotone() {
        let int = Cell::of(Type::INT32);
        let packet = Cell::instance(14);
        let joined = int.join(packet);
        assert!(int.within(joined) && packet.within(joined));
        assert_eq!(joined.classes.single(), Some((14, true)));
        assert_eq!(Cell::NEVER.join(int), int);
        assert!(joined.within(Cell::ANY));
    }

    #[test]
    fn literal_sites_join_as_a_closed_union() {
        let site = |function, pc| Site { function, pc };
        let joined = Sites::one(site(3, 7)).join(Sites::one(site(3, 2)));
        assert_eq!(joined.set(), Some(&[site(3, 2), site(3, 7)][..]));
        let full = (0..MAX_SITES as u32 - 1).fold(Sites::one(site(3, 2)), |sites, pc| sites.join(Sites::one(site(4, pc))));
        assert_eq!(full.set().map(<[Site]>::len), Some(MAX_SITES));
        assert_eq!(full.join(Sites::one(site(9, 0))), Sites::Many);
        assert_eq!(Cell::of(Type::INT32).sites, Sites::Never);
        assert_eq!(Cell::instance(14).sites, Sites::Never);
        assert_eq!(Cell::NEVER.join(Cell::of(Type::INT32)).sites, Sites::Never);
    }

    #[test]
    fn primitives_hold_no_class() {
        assert_eq!(Cell::of(Type::INT32).classes, Classes::Never);
        assert_eq!(Cell::of(Type::ANY).classes, Classes::Many);
    }
}
