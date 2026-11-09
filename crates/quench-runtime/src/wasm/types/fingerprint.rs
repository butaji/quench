//! A declaration's fingerprint: what its equivalence compares, short of following edges to
//! earlier groups. Equivalent declarations share it, so a pool compares a declaration only
//! with the few others of its fingerprint, not with every declaration it holds.

use super::WasmTypes;
use std::hash::{Hash, Hasher};
use wasmparser::{CompositeInnerType, FieldType, HeapType, StorageType, SubType, ValType};

impl WasmTypes {
    /// The fingerprint of declaration `index`: its position and its group's shapes.
    pub(crate) fn fingerprint(&self, index: usize) -> Option<u64> {
        let &(group, member) = self.0.members.get(index)?;
        let mut hasher = rustc_hash::FxHasher::default();
        (member, self.0.shapes[group as usize]).hash(&mut hasher);
        Some(hasher.finish())
    }
}

/// The hash of a group's length and shapes, which every member's fingerprint shares: a
/// module's one recursion group of thousands of types is hashed once, not per lookup.
pub(super) fn group_shapes(group: &[SubType]) -> u64 {
    let mut hasher = rustc_hash::FxHasher::default();
    group.len().hash(&mut hasher);
    group.iter().for_each(|ty| subtype(ty, &mut hasher));
    hasher.finish()
}

fn subtype(ty: &SubType, hasher: &mut impl Hasher) {
    let composite = &ty.composite_type;
    (ty.is_final, composite.shared, ty.supertype_idx.is_some()).hash(hasher);
    (composite.descriptor_idx.is_some(), composite.describes_idx.is_some()).hash(hasher);
    match &composite.inner {
        CompositeInnerType::Func(function) => {
            (0u8, function.params().len()).hash(hasher);
            function.params().iter().chain(function.results()).for_each(|value| value_type(*value, hasher));
        }
        CompositeInnerType::Struct(structure) => {
            (1u8, structure.fields.len()).hash(hasher);
            structure.fields.iter().for_each(|field| field_type(field, hasher));
        }
        CompositeInnerType::Array(array) => {
            2u8.hash(hasher);
            field_type(&array.0, hasher);
        }
        CompositeInnerType::Cont(_) => 3u8.hash(hasher),
    }
}

fn field_type(field: &FieldType, hasher: &mut impl Hasher) {
    field.mutable.hash(hasher);
    match field.element_type {
        StorageType::Val(value) => value_type(value, hasher),
        packed => packed.hash(hasher),
    }
}

/// A value type, with a reference to a declaration reduced to its kind: the declaration's own
/// equivalence decides the rest.
fn value_type(value: ValType, hasher: &mut impl Hasher) {
    match value {
        ValType::Ref(reference) => {
            reference.is_nullable().hash(hasher);
            match reference.heap_type() {
                HeapType::Abstract { shared, ty } => (0u8, shared, ty).hash(hasher),
                HeapType::Concrete(_) => 1u8.hash(hasher),
                HeapType::Exact(_) => 2u8.hash(hasher),
            }
        }
        other => other.hash(hasher),
    }
}
