//! QuickJS shapes: shared prototype + property names + flags.

use super::atom::Atom;
use super::jsvalue::{JsValue, Tag};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};

pub const PROP_CONFIGURABLE: u8 = 1 << 0;
pub const PROP_WRITABLE: u8 = 1 << 1;
pub const PROP_ENUMERABLE: u8 = 1 << 2;
pub const PROP_CWE: u8 = PROP_CONFIGURABLE | PROP_WRITABLE | PROP_ENUMERABLE;

/// Dynamic adapters use the VM's canonical shape identity. Keeping the alias
/// here preserves the adapter API without creating a second shape universe.
pub use crate::identity::ShapeId;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Prop {
    pub atom: Atom,
    pub flags: u8,
}

/// Shared hidden class. Objects with the same `ShapeId` share this layout.
#[derive(Clone, Debug, PartialEq)]
pub struct Shape {
    proto: JsValue,
    props: Box<[Prop]>,
    extensible: bool,
    slot_index: HashMap<Atom, u32>,
    content_hash: u64,
}

impl Shape {
    pub fn empty(proto: JsValue) -> Self {
        Self::with_props(proto, Box::new([]), true)
    }

    /// Build a shape and derive its slot index from the canonical property
    /// sequence. Callers should use this constructor instead of maintaining
    /// a second atom-to-slot mapping.
    pub fn with_props(proto: JsValue, props: Box<[Prop]>, extensible: bool) -> Self {
        let mut slot_index = HashMap::with_capacity(props.len());
        for (index, prop) in props.iter().enumerate() {
            slot_index.entry(prop.atom).or_insert(index as u32);
        }
        let mut shape = Self {
            proto,
            props,
            extensible,
            slot_index,
            content_hash: 0,
        };
        shape.content_hash = shape.compute_content_hash();
        shape
    }

    pub fn slot(&self, atom: Atom) -> Option<u32> {
        self.slot_index.get(&atom).copied()
    }

    pub fn proto(&self) -> &JsValue {
        &self.proto
    }

    pub fn props(&self) -> &[Prop] {
        &self.props
    }

    pub fn is_extensible(&self) -> bool {
        self.extensible
    }

    /// Stable content hash used only to select an equality bucket. Equality
    /// remains authoritative, so unusual values such as NaN retain their
    /// existing observable comparison semantics.
    fn compute_content_hash(&self) -> u64 {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        hash_js_value(&self.proto, &mut hasher);
        self.extensible.hash(&mut hasher);
        self.props.len().hash(&mut hasher);
        for prop in self.props.iter() {
            prop.atom.hash(&mut hasher);
            prop.flags.hash(&mut hasher);
        }
        hasher.finish()
    }

    fn content_hash(&self) -> u64 {
        self.content_hash
    }
}

#[derive(Clone, Debug, Default)]
pub struct ShapeTable {
    shapes: Vec<Shape>,
    by_hash: HashMap<u64, Vec<u32>>,
    transitions: HashMap<(u32, Atom, u8), ShapeId>,
}

impl ShapeTable {
    pub fn intern(&mut self, shape: Shape) -> ShapeId {
        let hash = shape.content_hash();
        if let Some(candidates) = self.by_hash.get(&hash) {
            if let Some(id) = candidates.iter().copied().find(|id| {
                self.shapes
                    .get(*id as usize)
                    .is_some_and(|candidate| *candidate == shape)
            }) {
                return ShapeId(id);
            }
        }
        let id = ShapeId(self.shapes.len() as u32);
        self.shapes.push(shape);
        self.by_hash.entry(hash).or_default().push(id.0);
        id
    }

    /// Derive and memoize one property transition from an existing shape.
    /// Repeating the same `(shape, atom, flags)` event returns the same target
    /// without rebuilding or re-interning the property array.
    pub fn transition(&mut self, from: ShapeId, atom: Atom, flags: u8) -> Option<ShapeId> {
        let key = (from.0, atom, flags);
        if let Some(target) = self.transitions.get(&key).copied() {
            return Some(target);
        }
        let source = self.shapes.get(from.0 as usize)?;
        if source.slot(atom).is_some() {
            // Redefinition is a descriptor/state operation, not a new hidden
            // class transition. Keep this edge limited to property addition.
            return None;
        }
        let mut props = source.props.to_vec();
        props.push(Prop { atom, flags });
        let target = self.intern(Shape::with_props(
            source.proto.clone(),
            props.into_boxed_slice(),
            source.extensible,
        ));
        self.transitions.insert(key, target);
        Some(target)
    }

    pub fn transition_target(&self, from: ShapeId, atom: Atom, flags: u8) -> Option<ShapeId> {
        self.transitions.get(&(from.0, atom, flags)).copied()
    }

    pub fn hash(&self, id: ShapeId) -> Option<u64> {
        self.shapes.get(id.0 as usize).map(Shape::content_hash)
    }

    pub fn get(&self, id: ShapeId) -> Option<&Shape> {
        self.shapes.get(id.0 as usize)
    }
}

fn hash_js_value<H: Hasher>(value: &JsValue, hasher: &mut H) {
    value.tag().hash(hasher);
    match value.tag() {
        Tag::Int | Tag::CatchOffset => value.payload_bits().hash(hasher),
        Tag::Bool => value.as_bool().unwrap_or(false).hash(hasher),
        Tag::ShortBigInt => value.as_i64().unwrap_or(0).hash(hasher),
        Tag::Float64 => {
            let value = value.as_f64().unwrap_or(f64::NAN);
            // Match f64 equality for signed zero; NaNs are intentionally
            // canonicalized only for bucket selection, never for equality.
            let bits = if value == 0.0 {
                0
            } else if value.is_nan() {
                f64::NAN.to_bits()
            } else {
                value.to_bits()
            };
            bits.hash(hasher);
        }
        tag if tag.has_ref_count() => value.pointer().map(|(_, id)| id).hash(hasher),
        Tag::Null | Tag::Undefined | Tag::Uninitialized | Tag::Exception => {}
        _ => value.tag().hash(hasher),
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
        let shape = Shape::with_props(
            JsValue::Null,
            Box::new([Prop {
                atom: x,
                flags: PROP_CWE,
            }]),
            true,
        );
        let mut table = ShapeTable::default();
        let a = table.intern(shape.clone());
        let b = table.intern(shape);
        assert_eq!(a, b);
        assert_eq!(table.get(a).unwrap().slot(x), Some(0));
        assert_eq!(table.get(a).unwrap().slot(Atom::NULL), None);
    }

    #[test]
    fn content_hash_preserves_float_equality_edges() {
        let mut table = ShapeTable::default();
        let plus_zero = table.intern(Shape::empty(JsValue::Float64(0.0)));
        let minus_zero = table.intern(Shape::empty(JsValue::Float64(-0.0)));
        assert_eq!(plus_zero, minus_zero);

        let nan_a = table.intern(Shape::empty(JsValue::Float64(f64::NAN)));
        let nan_b = table.intern(Shape::empty(JsValue::Float64(f64::NAN)));
        assert_ne!(nan_a, nan_b);
    }

    #[test]
    fn property_transition_is_memoized_from_one_shape() {
        let mut atoms = AtomTable::default();
        let x = atoms.intern("x");
        let mut table = ShapeTable::default();
        let root = table.intern(Shape::empty(JsValue::Null));
        let first = table.transition(root, x, PROP_CWE).unwrap();
        let second = table.transition(root, x, PROP_CWE).unwrap();
        assert_eq!(first, second);
        assert_eq!(table.transition_target(root, x, PROP_CWE), Some(first));
        assert_eq!(table.get(first).unwrap().slot(x), Some(0));
        assert_ne!(table.hash(root), table.hash(first));
        assert_eq!(table.transition(first, x, PROP_CWE), None);
    }
}
