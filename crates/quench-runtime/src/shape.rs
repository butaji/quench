//! Shape-based object layout.
//!
//! A `Shape` describes the property layout of an object: the ordered list of
//! property names (as interned symbols). Shapes are immutable and shared.
//!
//! Design constraints:
//! - Shapes are immutable and reference-counted.
//! - No transition tree edges are stored inside the shape.
//! - When a property is added, the runtime looks up `(current_shape, name)` in
//!   a global shape cache. If no child shape exists, a new one is created and
//!   cached.
//! - Objects use inline slots (inside the object header) plus a flat
//!   out-of-line array for overflow. Only when there are "too many" properties
//!   does the object fall back to a hash map.
//! - No polymorphic inline caches.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::interner::Symbol;

/// Shape identifier. Stable for the lifetime of the shape interner.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct ShapeId(pub u32);

/// Handle to a shared, immutable shape.
pub type ShapeRef = Rc<Shape>;

/// Describes the property layout of an object.
pub struct Shape {
    /// Stable shape identifier.
    pub id: ShapeId,
    /// Parent shape (the layout before the last property was added).
    pub parent: Option<ShapeRef>,
    /// The property symbol added by this transition, if any.
    pub added: Symbol,
    /// Ordered property symbols; includes all ancestors plus `added`.
    pub property_names: Vec<Symbol>,
}

impl Shape {
    /// Number of properties described by this shape.
    pub fn len(&self) -> usize {
        self.property_names.len()
    }

    /// Whether this shape describes no properties.
    pub fn is_empty(&self) -> bool {
        self.property_names.is_empty()
    }

    /// Look up the property offset for `prop`.
    pub fn find_offset(&self, prop: Symbol) -> Option<usize> {
        self.property_names.iter().position(|&p| p == prop)
    }

    /// Iterate property symbols in layout order.
    pub fn property_names(&self) -> &[Symbol] {
        &self.property_names
    }
}

impl std::fmt::Debug for Shape {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Shape")
            .field("id", &self.id.0)
            .field("property_names", &self.property_names)
            .field("len", &self.property_names.len())
            .finish()
    }
}

/// Global shape cache entry: `(parent_shape_id, added_symbol) -> child_shape`.
///
/// The cache is held inside the `ShapeInterner`. In a production runtime this
/// would be a weak table so unreachable shapes can be collected; here we use
/// `Rc` for simplicity and clear the table when the realm is reset.
#[derive(Debug)]
pub struct ShapeInterner {
    next_id: RefCell<u32>,
    cache: RefCell<HashMap<(ShapeId, Symbol), ShapeRef>>,
    root: ShapeRef,
}

impl ShapeInterner {
    /// Create a shape interner with a single root shape.
    pub fn new() -> Self {
        let root = Rc::new(Shape {
            id: ShapeId(0),
            parent: None,
            added: Symbol::NONE,
            property_names: Vec::new(),
        });
        let mut cache = HashMap::new();
        cache.insert((ShapeId(0), Symbol::NONE), Rc::clone(&root));
        Self {
            next_id: RefCell::new(1),
            cache: RefCell::new(cache),
            root,
        }
    }

    /// Return the root shape (no properties).
    pub fn root(&self) -> ShapeRef {
        Rc::clone(&self.root)
    }

    /// Get or create a child shape by adding `prop` to `shape`.
    pub fn add_property(&self, shape: &ShapeRef, prop: Symbol) -> ShapeRef {
        if prop == Symbol::NONE {
            return Rc::clone(shape);
        }
        let key = (shape.id, prop);
        {
            let cache = self.cache.borrow();
            if let Some(child) = cache.get(&key) {
                return Rc::clone(child);
            }
        }
        let id = ShapeId(*self.next_id.borrow());
        *self.next_id.borrow_mut() += 1;
        let mut property_names = shape.property_names.clone();
        property_names.push(prop);
        let child = Rc::new(Shape {
            id,
            parent: Some(Rc::clone(shape)),
            added: prop,
            property_names,
        });
        self.cache.borrow_mut().insert(key, Rc::clone(&child));
        child
    }

    /// Ensure a shape with the given properties exists.
    pub fn shape_for(&self, props: &[Symbol]) -> ShapeRef {
        let mut shape = self.root();
        for &prop in props {
            shape = self.add_property(&shape, prop);
        }
        shape
    }

    /// Clear the cache (used on realm reset).
    pub fn clear(&self) {
        self.cache.borrow_mut().clear();
        self.cache
            .borrow_mut()
            .insert((ShapeId(0), Symbol::NONE), Rc::clone(&self.root));
        *self.next_id.borrow_mut() = 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interner::StringInterner;

    #[test]
    fn test_shape_transitions_share() {
        let mut interner = StringInterner::new();
        let a = interner.intern("a");
        let b = interner.intern("b");

        let shapes = ShapeInterner::new();
        let s1 = shapes.add_property(&shapes.root(), a);
        let s1 = shapes.add_property(&s1, b);
        let s2 = shapes.add_property(&shapes.root(), a);
        let s2 = shapes.add_property(&s2, b);
        assert!(Rc::ptr_eq(&s1, &s2));

        assert_eq!(s1.find_offset(a), Some(0));
        assert_eq!(s1.find_offset(b), Some(1));
    }

    #[test]
    fn test_shape_for() {
        let mut interner = StringInterner::new();
        let x = interner.intern("x");
        let y = interner.intern("y");

        let shapes = ShapeInterner::new();
        let s1 = shapes.shape_for(&[x, y]);
        let s2 = shapes.shape_for(&[x, y]);
        assert!(Rc::ptr_eq(&s1, &s2));
        assert_eq!(s1.len(), 2);
    }
}
