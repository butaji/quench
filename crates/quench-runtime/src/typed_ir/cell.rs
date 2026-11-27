//! The one fact lattice (L2 of the layered design, TOD-2445/TOD-2448): one `Cell` per value, slot,
//! parameter, return, receiver and element. A cell is the product of three lattices, joined
//! component-wise:
//!
//! - `ty`: the semantic type (`Type`), which already carries the integer range (`TypeKind::Int`),
//!   nullability and function identity;
//! - `classes`: the constructed classes of the objects the value may hold (`Classes`), a closed
//!   union of at most `MAX_CLASSES`, so a polymorphic call site (`this.task.run()`) is a switch on the
//!   class word;
//! - `elements`: what an array the value holds keeps in its elements (`Elements`): their type and
//!   classes, and a fixed length where no edge can change it.
//!
//! Representation is derived from the cell (`Cell::representation`), never stored. A store and a read
//! form a Galois connection: a slot's cell is the join of every stored cell (α), and a read takes the
//! slot's cell (γ). Code that trusts a cell subscribes to its key (`FactKey`), and an edge that moves the
//! cell up the lattice retires every subscriber the same way (L3, `code_dependencies::cells`).
//!
//! This module is the only home of a fact type: a `struct`/`enum` named `*Fact` elsewhere is a second
//! home (census `fact_homes`).
use super::{IntRange, Representation, Type};

/// The most classes a closed union holds before it is `Classes::Many`: richards' four task classes.
pub const MAX_CLASSES: usize = 4;

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
                let mut ids = [0u32; MAX_CLASSES];
                let mut len = 0usize;
                let (mut i, mut j) = (0usize, 0usize);
                let (la, lb) = (usize::from(la), usize::from(lb));
                while i < la || j < lb {
                    let next = match (a[..la].get(i), b[..lb].get(j)) {
                        (Some(&x), Some(&y)) if x == y => { i += 1; j += 1; x }
                        (Some(&x), Some(&y)) if x < y => { i += 1; x }
                        (Some(_), Some(&y)) => { j += 1; y }
                        (Some(&x), None) => { i += 1; x }
                        (None, Some(&y)) => { j += 1; y }
                        (None, None) => break,
                    };
                    if len == MAX_CLASSES { return Self::Many; }
                    ids[len] = next;
                    len += 1;
                }
                Self::Set { ids, len: len as u8, proven: pa && pb }
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

/// What the arrays a value holds keep in their elements: the join of every element store's type and
/// classes, a hole read as `undefined` included, and their length (`Length`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Elements {
    pub ty: Type,
    pub classes: Classes,
    pub length: Length,
}

impl Elements {
    /// No array, or none with an element yet.
    pub const NEVER: Self = Self { ty: Type::NEVER, classes: Classes::Never, length: Length::Never };
    /// Arrays of anything, of any length.
    pub const ANY: Self = Self { ty: Type::ANY, classes: Classes::Many, length: Length::Any };

    pub fn join(self, other: Self) -> Self {
        Self { ty: self.ty.join(other.ty), classes: self.classes.join(other.classes), length: self.length.join(other.length) }
    }

    /// The cell an element read takes (γ): the elements' type and classes. An index the range of which
    /// the length does not cover may read a hole: the reader joins `undefined` itself.
    pub fn element(self) -> Cell {
        Cell { ty: self.ty, classes: self.classes, elements: Elements::ANY }
    }
}

/// One cell of the fact lattice: a value's type, the classes of the objects it may hold and what its
/// arrays keep. The product of the three lattices, joined component-wise.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cell {
    pub ty: Type,
    pub classes: Classes,
    pub elements: Elements,
}

impl Cell {
    /// No value yet: the solve's bottom.
    pub const NEVER: Self = Self { ty: Type::NEVER, classes: Classes::Never, elements: Elements::NEVER };
    /// Any value: the top.
    pub const ANY: Self = Self { ty: Type::ANY, classes: Classes::Many, elements: Elements::ANY };

    /// A value of type `ty` from nowhere the solve follows: its classes from the type alone.
    pub fn of(ty: Type) -> Self {
        let elements = if ty.is_subtype(Type::IMMEDIATE) { Elements::NEVER } else { Elements::ANY };
        Self { ty, classes: Classes::of_type(ty), elements }
    }

    /// A value of type `ty` holding objects of `classes`.
    pub fn with_classes(ty: Type, classes: Classes) -> Self {
        Self { classes, ..Self::of(ty) }
    }

    /// A fresh instance of class `class`, a construction's own result.
    pub fn instance(class: u32) -> Self {
        Self::with_classes(Type::DIRECT_OBJECT_FAMILY, Classes::one(class, true))
    }

    pub fn join(self, other: Self) -> Self {
        Self { ty: self.ty.join(other.ty), classes: self.classes.join(other.classes), elements: self.elements.join(other.elements) }
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
    /// The elements of the arrays field `atom` holds: the elements of `atom`'s element class (every store into an
    /// array of that class writes them, `census::ElementClasses`), joined with the array's own cell.
    Elements(u32),
    /// The elements of the arrays of the unnamed class (`UNNAMED_ARRAYS`): the arrays no field names, and every array
    /// that flowed between one and a field. A store through an alias of such an array writes this cell, so a
    /// subscriber of it retires on that store. Joined with the array's own cell, as `Elements` is.
    UnnamedElements,
    /// Parameter `index` of function `function`.
    Parameter { function: u32, index: u16 },
    /// The `this` of function `0`.
    Receiver(u32),
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
    fn primitives_hold_no_class() {
        assert_eq!(Cell::of(Type::INT32).classes, Classes::Never);
        assert_eq!(Cell::of(Type::ANY).classes, Classes::Many);
    }
}
