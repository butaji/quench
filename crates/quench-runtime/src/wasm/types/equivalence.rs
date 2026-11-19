//! Structural equivalence of a module's defined types, across two declaration tables; the
//! walk visits each reachable pair of preceding groups once (`WasmTypes::equivalent`).

use super::{Edge, WasmTypes};
use rustc_hash::FxHashSet;
use wasmparser::{CompositeInnerType, FieldType, HeapType, PackedIndex, StorageType, SubType, UnpackedIndex, ValType};

/// Each reachable pair of preceding groups is visited once. Recursive edges
/// stay local, so deep declaration chains never consume the native call stack.
pub(super) struct Equivalence<'a> {
    pub(super) tables: [&'a WasmTypes; 2],
    pub(super) pending: Vec<[usize; 2]>,
    pub(super) seen: FxHashSet<[usize; 2]>,
}

impl Equivalence<'_> {
    pub(super) fn enqueue(&mut self, origins: [usize; 2]) {
        if self.seen.insert(origins) {
            self.pending.push(origins);
        }
    }

    fn indices(&mut self, indices: [UnpackedIndex; 2], origins: [usize; 2]) -> bool {
        match (
            self.tables[0].edge(indices[0], origins[0]),
            self.tables[1].edge(indices[1], origins[1]),
        ) {
            (Some(Edge::Local(left)), Some(Edge::Local(right))) => left == right,
            (
                Some(Edge::External {
                    origin: left,
                    member: left_member,
                }),
                Some(Edge::External {
                    origin: right,
                    member: right_member,
                }),
            ) if left_member == right_member => {
                self.enqueue([left, right]);
                true
            }
            _ => false,
        }
    }

    fn optional_indices(
        &mut self,
        left: Option<PackedIndex>,
        right: Option<PackedIndex>,
        origins: [usize; 2],
    ) -> bool {
        match (left, right) {
            (None, None) => true,
            (Some(left), Some(right)) => self.indices([left.unpack(), right.unpack()], origins),
            _ => false,
        }
    }

    fn value(&mut self, left: ValType, right: ValType, origins: [usize; 2]) -> bool {
        match (left, right) {
            (ValType::Ref(left), ValType::Ref(right)) => {
                left.is_nullable() == right.is_nullable()
                    && match (left.heap_type(), right.heap_type()) {
                        (HeapType::Concrete(left), HeapType::Concrete(right))
                        | (HeapType::Exact(left), HeapType::Exact(right)) => {
                            self.indices([left, right], origins)
                        }
                        (HeapType::Abstract { .. }, HeapType::Abstract { .. }) => {
                            left.heap_type() == right.heap_type()
                        }
                        _ => false,
                    }
            }
            _ => left == right,
        }
    }

    fn values(&mut self, left: &[ValType], right: &[ValType], origins: [usize; 2]) -> bool {
        left.len() == right.len()
            && left
                .iter()
                .zip(right)
                .all(|(&left, &right)| self.value(left, right, origins))
    }

    fn field(&mut self, left: &FieldType, right: &FieldType, origins: [usize; 2]) -> bool {
        left.mutable == right.mutable
            && match (left.element_type, right.element_type) {
                (StorageType::Val(left), StorageType::Val(right)) => {
                    self.value(left, right, origins)
                }
                (left, right) => left == right,
            }
    }

    pub(super) fn subtype(&mut self, left: &SubType, right: &SubType, origins: [usize; 2]) -> bool {
        let left_composite = &left.composite_type;
        let right_composite = &right.composite_type;
        left.is_final == right.is_final
            && left_composite.shared == right_composite.shared
            && self.optional_indices(left.supertype_idx, right.supertype_idx, origins)
            && self.optional_indices(
                left_composite.descriptor_idx,
                right_composite.descriptor_idx,
                origins,
            )
            && self.optional_indices(
                left_composite.describes_idx,
                right_composite.describes_idx,
                origins,
            )
            && match (&left_composite.inner, &right_composite.inner) {
                (CompositeInnerType::Func(left), CompositeInnerType::Func(right)) => {
                    self.values(left.params(), right.params(), origins)
                        && self.values(left.results(), right.results(), origins)
                }
                (CompositeInnerType::Struct(left), CompositeInnerType::Struct(right)) => {
                    left.fields.len() == right.fields.len()
                        && left
                            .fields
                            .iter()
                            .zip(&right.fields)
                            .all(|(left, right)| self.field(left, right, origins))
                }
                (CompositeInnerType::Array(left), CompositeInnerType::Array(right)) => {
                    self.field(&left.0, &right.0, origins)
                }
                (CompositeInnerType::Cont(left), CompositeInnerType::Cont(right)) => {
                    self.indices([left.0.unpack(), right.0.unpack()], origins)
                }
                _ => false,
            }
    }
}
