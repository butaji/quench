//! Module type facts retain recursive group boundaries and source index order.
//! Executable signatures are projections, not replacements for declarations.

use rustc_hash::FxHashSet;
use std::rc::Rc;
use wasmparser::{
    CompositeInnerType, FieldType, FuncType, HeapType, PackedIndex, RecGroup, StorageType, SubType,
    UnpackedIndex, ValType,
};

#[derive(Clone, Debug, Default)]
pub struct WasmTypes {
    groups: Rc<[Vec<SubType>]>,
}

impl WasmTypes {
    pub fn from_groups(groups: impl IntoIterator<Item = RecGroup>) -> Self {
        Self {
            groups: groups
                .into_iter()
                .map(|group| group.into_types().collect())
                .collect(),
        }
    }

    /// Embedding inputs without a binary type section declare singleton,
    /// final, unshared function groups, just like ordinary function types.
    pub fn from_functions(functions: impl IntoIterator<Item = FuncType>) -> Self {
        Self {
            groups: functions
                .into_iter()
                .map(|function| vec![SubType::func(function, false)])
                .collect(),
        }
    }

    pub(crate) fn struct_fields(&self, index: u32) -> Option<&[FieldType]> {
        let composite = &self.get(index as usize)?.composite_type;
        if composite.shared {
            return None;
        }
        let CompositeInnerType::Struct(structure) = &composite.inner else {
            return None;
        };
        Some(&structure.fields)
    }

    pub(crate) fn descriptor_type(&self, index: u32) -> Option<super::WasmType> {
        let (group, member) = self.locate(index as usize)?;
        let composite = &group[member].composite_type;
        if composite.shared || !matches!(composite.inner, CompositeInnerType::Struct(_)) {
            return None;
        }
        let origin = index as usize - member;
        let descriptor = match self.edge(composite.descriptor_idx?.unpack(), origin)? {
            Edge::Local(member) => origin + member,
            Edge::External { origin, member } => origin + member,
        };
        let index = u32::try_from(descriptor).ok()?;
        self.struct_fields(index)?;
        Some(super::WasmType::Reference {
            kind: super::WasmReferenceKind::DeclaredGc { index, exact: true },
            nullable: true,
        })
    }

    pub(crate) fn array_field(&self, index: u32) -> Option<FieldType> {
        let composite = &self.get(index as usize)?.composite_type;
        if composite.shared
            || composite.descriptor_idx.is_some()
            || composite.describes_idx.is_some()
        {
            return None;
        }
        match &composite.inner {
            CompositeInnerType::Array(array) => Some(array.0),
            _ => None,
        }
    }

    pub fn groups(&self) -> &[Vec<SubType>] {
        &self.groups
    }

    fn locate(&self, mut index: usize) -> Option<(&[SubType], usize)> {
        for group in self.groups.iter() {
            if index < group.len() {
                return Some((group, index));
            }
            index -= group.len();
        }
        None
    }

    pub fn get(&self, index: usize) -> Option<&SubType> {
        let (group, index) = self.locate(index)?;
        group.get(index)
    }

    /// Equality of closed defined types: group members are positional, local
    /// edges are relative, and edges to prior groups compare their definitions.
    /// This deliberately differs from equality of infinitely unfolded trees.
    pub fn equivalent(&self, index: usize, other: &Self, other_index: usize) -> bool {
        let Some((_, member)) = self.locate(index) else {
            return false;
        };
        let Some((_, other_member)) = other.locate(other_index) else {
            return false;
        };
        if member != other_member {
            return false;
        }
        let mut comparison = Equivalence {
            tables: [self, other],
            pending: Vec::new(),
            seen: FxHashSet::default(),
        };
        comparison.enqueue([index - member, other_index - other_member]);
        while let Some(origins) = comparison.pending.pop() {
            let left = self.locate(origins[0]).unwrap().0;
            let right = other.locate(origins[1]).unwrap().0;
            if left.len() != right.len()
                || !left
                    .iter()
                    .zip(right)
                    .all(|(left, right)| comparison.subtype(left, right, origins))
            {
                return false;
            }
        }
        true
    }

    /// Defined-type subtyping follows declared ancestry, never an accidental
    /// structural match between unrelated declarations.
    pub fn is_subtype(&self, mut index: usize, other: &Self, other_index: usize) -> bool {
        let declarations = self.groups.iter().map(Vec::len).sum::<usize>();
        // Validated ancestry is acyclic. The declaration count also bounds
        // malformed embedding input without inventing a runtime type-depth cap.
        for _ in 0..declarations {
            if self.equivalent(index, other, other_index) {
                return true;
            }
            let Some(ty) = self.get(index) else {
                return false;
            };
            let Some(supertype) = ty.supertype_idx else {
                return false;
            };
            let (_, member) = self.locate(index).unwrap();
            match self.edge(supertype.unpack(), index - member) {
                Some(Edge::Local(parent)) => index = index - member + parent,
                Some(Edge::External { origin, member }) => index = origin + member,
                None => return false,
            }
        }
        false
    }

    fn edge(&self, index: UnpackedIndex, origin: usize) -> Option<Edge> {
        let (group, _) = self.locate(origin)?;
        match index {
            UnpackedIndex::RecGroup(member) => {
                let member = member as usize;
                (member < group.len()).then_some(Edge::Local(member))
            }
            UnpackedIndex::Module(index) => {
                let index = index as usize;
                if index >= origin {
                    let member = index - origin;
                    return (member < group.len()).then_some(Edge::Local(member));
                }
                let (_, member) = self.locate(index)?;
                Some(Edge::External {
                    origin: index - member,
                    member,
                })
            }
            // Allocator-specific IDs are not indices in this declaration table.
            UnpackedIndex::Id(_) => None,
        }
    }

    /// Callable ABI facts retain defined reference kinds and exactness in this
    /// graph's source index domain.
    pub fn callable_value_type(&self, ty: ValType) -> Option<super::WasmType> {
        if let Some(ty) = super::WasmType::from_wasm(ty) {
            return Some(ty);
        }
        let ValType::Ref(reference) = ty else {
            return None;
        };
        let (index, exact) = match reference.heap_type() {
            HeapType::Concrete(UnpackedIndex::Module(index)) => (index, false),
            HeapType::Exact(UnpackedIndex::Module(index)) => (index, true),
            _ => return None,
        };
        let declaration = self.get(index as usize)?;
        if declaration.composite_type.shared {
            return None;
        }
        let kind = match declaration.composite_type.inner {
            CompositeInnerType::Func(_) => {
                super::WasmReferenceKind::DeclaredFunction { index, exact }
            }
            CompositeInnerType::Struct(_) | CompositeInnerType::Array(_) => {
                super::WasmReferenceKind::DeclaredGc { index, exact }
            }
            CompositeInnerType::Cont(_) => return None,
        };
        Some(super::WasmType::Reference {
            kind,
            nullable: reference.is_nullable(),
        })
    }

    pub(crate) fn value_subtype(
        &self,
        source: super::WasmType,
        other: &Self,
        target: super::WasmType,
    ) -> bool {
        match (source.reference_type(), target.reference_type()) {
            (Some(source), Some(target)) => self.reference_subtype(source, other, target),
            _ => source == target,
        }
    }

    pub(crate) fn reference_subtype(
        &self,
        source: wasmparser::RefType,
        other: &Self,
        target: wasmparser::RefType,
    ) -> bool {
        if source.is_nullable() && !target.is_nullable() {
            return false;
        }
        match (source.heap_type(), target.heap_type()) {
            (
                HeapType::Concrete(UnpackedIndex::Module(source))
                | HeapType::Exact(UnpackedIndex::Module(source)),
                HeapType::Concrete(UnpackedIndex::Module(target)),
            ) => self.is_subtype(source as usize, other, target as usize),
            (
                HeapType::Concrete(UnpackedIndex::Module(source))
                | HeapType::Exact(UnpackedIndex::Module(source)),
                HeapType::Abstract { shared: false, ty },
            ) => self.get(source as usize).is_some_and(|declaration| {
                !declaration.composite_type.shared
                    && match (ty, &declaration.composite_type.inner) {
                        (wasmparser::AbstractHeapType::Func, CompositeInnerType::Func(_)) => true,
                        (
                            wasmparser::AbstractHeapType::Any | wasmparser::AbstractHeapType::Eq,
                            CompositeInnerType::Struct(_) | CompositeInnerType::Array(_),
                        ) => true,
                        (wasmparser::AbstractHeapType::Struct, CompositeInnerType::Struct(_))
                        | (wasmparser::AbstractHeapType::Array, CompositeInnerType::Array(_)) => {
                            true
                        }
                        _ => false,
                    }
            }),
            (
                HeapType::Abstract { shared: false, ty },
                HeapType::Concrete(UnpackedIndex::Module(target))
                | HeapType::Exact(UnpackedIndex::Module(target)),
            ) => other.get(target as usize).is_some_and(|declaration| {
                !declaration.composite_type.shared
                    && matches!(
                        (ty, &declaration.composite_type.inner),
                        (
                            wasmparser::AbstractHeapType::None,
                            CompositeInnerType::Struct(_) | CompositeInnerType::Array(_)
                        ) | (
                            wasmparser::AbstractHeapType::NoFunc,
                            CompositeInnerType::Func(_)
                        )
                    )
            }),
            (
                HeapType::Abstract {
                    shared: false,
                    ty: source,
                },
                HeapType::Abstract {
                    shared: false,
                    ty: target,
                },
            ) => {
                use wasmparser::AbstractHeapType::*;
                source == target
                    || matches!(
                        (source, target),
                        (Eq | I31 | Struct | Array | None, Any)
                            | (I31 | Struct | Array | None, Eq)
                            | (None, I31 | Struct | Array)
                            | (NoFunc, Func)
                            | (NoExtern, Extern)
                            | (NoExn, Exn)
                    )
            }
            (
                HeapType::Exact(UnpackedIndex::Module(source)),
                HeapType::Exact(UnpackedIndex::Module(target)),
            ) => self.equivalent(source as usize, other, target as usize),
            (HeapType::Concrete(_), HeapType::Exact(_)) => false,
            _ => false,
        }
    }

    /// Whether a declaration is equivalent to an embedding's implicit final
    /// singleton function type, including its exact value types. Machine ABI
    /// projections cannot replace concrete reference identities here.
    pub(crate) fn matches_implicit_function(
        &self,
        index: usize,
        signature: &super::WasmSignature,
    ) -> bool {
        self.locate(index).is_some_and(|(group, member)| {
            let ty = &group[member];
            group.len() == 1
                && ty.is_final
                && ty.supertype_idx.is_none()
                && !ty.composite_type.shared
                && ty.composite_type.descriptor_idx.is_none()
                && ty.composite_type.describes_idx.is_none()
                && match &ty.composite_type.inner {
                    CompositeInnerType::Func(function) => {
                        function.params().iter().copied().eq(signature
                            .params
                            .iter()
                            .copied()
                            .map(super::WasmType::value_type))
                            && function.results().iter().copied().eq(signature
                                .results
                                .iter()
                                .copied()
                                .map(super::WasmType::value_type))
                    }
                    _ => false,
                }
        })
    }

    /// Executable signatures retain declaration identity in the callable pool.
    pub fn function(&self, name: &str, index: usize) -> Result<&FuncType, crate::Diagnostic> {
        let error = |message| crate::Diagnostic::unsupported(name, message);
        let (group, member) = self
            .locate(index)
            .ok_or_else(|| error("Wasm type index out of bounds"))?;
        let ty = &group[member];
        if ty.composite_type.shared
            || ty.composite_type.descriptor_idx.is_some()
            || ty.composite_type.describes_idx.is_some()
        {
            return Err(error("shared Wasm shared or descriptor types"));
        }
        self.function_shape(name, index)
    }

    /// Block types expand to parameter/result shapes without requiring a
    /// callable identity or erasing the declaration's recursive identity.
    pub fn function_shape(&self, name: &str, index: usize) -> Result<&FuncType, crate::Diagnostic> {
        let error = |message| crate::Diagnostic::unsupported(name, message);
        let ty = self
            .get(index)
            .ok_or_else(|| error("Wasm type index out of bounds"))?;
        match &ty.composite_type.inner {
            CompositeInnerType::Func(function) => Ok(function),
            CompositeInnerType::Array(_) | CompositeInnerType::Struct(_) => {
                Err(error("shared Wasm GC types"))
            }
            CompositeInnerType::Cont(_) => Err(error("shared Wasm continuation types")),
        }
    }
}

enum Edge {
    Local(usize),
    External { origin: usize, member: usize },
}

/// Each reachable pair of preceding groups is visited once. Recursive edges
/// stay local, so deep declaration chains never consume the native call stack.
struct Equivalence<'a> {
    tables: [&'a WasmTypes; 2],
    pending: Vec<[usize; 2]>,
    seen: FxHashSet<[usize; 2]>,
}

impl Equivalence<'_> {
    fn enqueue(&mut self, origins: [usize; 2]) {
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

    fn subtype(&mut self, left: &SubType, right: &SubType, origins: [usize; 2]) -> bool {
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
