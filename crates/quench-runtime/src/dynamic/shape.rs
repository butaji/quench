//! QuickJS shapes: shared prototype + property names + flags.

use super::atom::Atom;
use super::jsvalue::JsValue;

pub const PROP_CONFIGURABLE: u8 = 1 << 0;
pub const PROP_WRITABLE: u8 = 1 << 1;
pub const PROP_ENUMERABLE: u8 = 1 << 2;
pub const PROP_CWE: u8 = PROP_CONFIGURABLE | PROP_WRITABLE | PROP_ENUMERABLE;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct ShapeId(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Prop {
    pub atom: Atom,
    pub flags: u8,
}

/// Shared hidden class. Objects with the same `ShapeId` share this layout.
#[derive(Clone, Debug, PartialEq)]
pub struct Shape {
    pub proto: JsValue,
    pub props: Box<[Prop]>,
    pub extensible: bool,
}

impl Shape {
    pub fn empty(proto: JsValue) -> Self {
        Self {
            proto,
            props: Box::new([]),
            extensible: true,
        }
    }

    pub fn slot(&self, atom: Atom) -> Option<u32> {
        self.props.iter().position(|p| p.atom == atom).map(|i| i as u32)
    }
}

#[derive(Clone, Debug, Default)]
pub struct ShapeTable {
    shapes: Vec<Shape>,
}

impl ShapeTable {
    pub fn intern(&mut self, shape: Shape) -> ShapeId {
        if let Some(i) = self.shapes.iter().position(|s| *s == shape) {
            return ShapeId(i as u32);
        }
        let id = ShapeId(self.shapes.len() as u32);
        self.shapes.push(shape);
        id
    }

    pub fn get(&self, id: ShapeId) -> Option<&Shape> {
        self.shapes.get(id.0 as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::{Prop, Shape, ShapeTable, PROP_CWE};
    use crate::dynamic::atom::{Atom, AtomTable};
    use crate::dynamic::jsvalue::JsValue;

    #[test]
    fn two_objects_share_one_shape() {
        let mut atoms = AtomTable::default();
        let x = atoms.intern("x");
        let shape = Shape {
            proto: JsValue::Null,
            props: Box::new([Prop {
                atom: x,
                flags: PROP_CWE,
            }]),
            extensible: true,
        };
        let mut table = ShapeTable::default();
        let a = table.intern(shape.clone());
        let b = table.intern(shape);
        assert_eq!(a, b);
        assert_eq!(table.get(a).unwrap().slot(x), Some(0));
        assert_eq!(table.get(a).unwrap().slot(Atom::NULL), None);
    }
}
