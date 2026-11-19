//! Module type facts retain recursive group boundaries and source index order.
//! Executable signatures are projections, not replacements for declarations.

mod ancestry;
mod equivalence;
mod fingerprint;

use super::gc::ObjectLayout;
use rustc_hash::FxHashSet;
use std::rc::Rc;
use wasmparser::{
    CompositeInnerType, FieldType, FuncType, HeapType, RecGroup, SubType,
    UnpackedIndex, ValType,
};

/// A module's one type table: every holder (module, function, GC object) shares it by one
/// pointer, so allocating a GC object never copies it.
#[derive(Clone, Debug, Default)]
pub struct WasmTypes(Rc<Declarations>);

#[derive(Debug, Default, PartialEq)]
struct Declarations {
    groups: Box<[Vec<SubType>]>,
    /// Each declaration's group and its position in it, so a declaration is found at once.
    members: Box<[(u32, u32)]>,
    /// Each group's shapes, hashed once (`fingerprint`).
    shapes: Box<[u64]>,
    /// Each declaration's canonical index, found once (`ancestry`).
    canonical: Box<[u32]>,
    /// Each declaration's supertype display: where its run of `ancestors` starts and its
    /// depth (`ancestry`).
    displays: Box<[(u32, u32)]>,
    /// The displays' runs, each the canonical indices of a declaration's ancestors from its
    /// root down to itself, so a subtype check reads one entry at the target's depth.
    ancestors: Box<[u32]>,
    /// Each declaration's object layout, built for the module on first use (`layout`).
    layouts: Layouts,
}

/// The module's object layouts, derived from the groups, so they never differ between equal modules.
#[derive(Debug, Default)]
struct Layouts(std::cell::OnceCell<Box<[Option<ObjectLayout>]>>);

impl PartialEq for Layouts {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

#[cfg(test)]
impl WasmTypes {
    /// A module whose declarations are `types`, each a singleton group, for layout tests.
    pub(crate) fn singletons_for_test(types: Vec<SubType>) -> Self {
        Self::of(types.into_iter().map(|ty| vec![ty]).collect())
    }
}

impl PartialEq for WasmTypes {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0) || self.0 == other.0
    }
}

impl WasmTypes {
    pub fn from_groups(groups: impl IntoIterator<Item = RecGroup>) -> Self {
        Self::of(groups.into_iter().map(|group| group.into_types().collect()).collect())
    }

    fn of(groups: Box<[Vec<SubType>]>) -> Self {
        let members = groups.iter().enumerate().flat_map(|(group, types)| (0..types.len()).map(move |member| (group as u32, member as u32))).collect();
        let shapes = groups.iter().map(|group| fingerprint::group_shapes(group)).collect();
        Self(Rc::new(Declarations { shapes, groups, members, ..Declarations::default() })).with_ancestry()
    }

    /// Embedding inputs without a binary type section declare singleton,
    /// final, unshared function groups, just like ordinary function types.
    pub fn from_functions(functions: impl IntoIterator<Item = FuncType>) -> Self {
        Self::of(functions.into_iter().map(|function| vec![SubType::func(function, false)]).collect())
    }

    /// The payload layout of the struct or array declared at `index`, built once for the module.
    pub(crate) fn layout(&self, index: u32) -> Option<&ObjectLayout> {
        let layouts = self.0.layouts.0.get_or_init(|| ObjectLayout::of_module(self));
        layouts.get(index as usize)?.as_ref()
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
        &self.0.groups
    }

    fn locate(&self, index: usize) -> Option<(&[SubType], usize)> {
        let &(group, member) = self.0.members.get(index)?;
        Some((&self.0.groups[group as usize], member as usize))
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
        if Rc::ptr_eq(&self.0, &other.0) && let Some(same) = self.same_class(index, other_index) {
            return same;
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
    #[inline(always)]
    pub fn is_subtype(&self, index: usize, other: &Self, other_index: usize) -> bool {
        if Rc::ptr_eq(&self.0, &other.0) && let Some(subtype) = self.ancestry_subtype(index, other_index) {
            return subtype;
        }
        self.is_subtype_across(index, other, other_index)
    }

    /// `is_subtype` between declarations of two tables, or of types without a display.
    #[inline(never)]
    fn is_subtype_across(&self, mut index: usize, other: &Self, other_index: usize) -> bool {
        let declarations = self.0.members.len();
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

    /// Callable ABI facts retain defined reference kinds and exactness in this
    /// graph's source index domain.
    pub fn callable_value_type(&self, ty: ValType) -> Option<super::WasmType> {
        if let Some(ty) = super::WasmType::from_wasm(ty) {
            return Some(ty);
        }
        let ValType::Ref(reference) = ty else {
            return None;
        };
        let (index, exact) = super::reference::module_type_index(reference.heap_type())?;
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
                    && matches!(
                        (ty, &declaration.composite_type.inner),
                        (wasmparser::AbstractHeapType::Func, CompositeInnerType::Func(_))
                            | (
                                wasmparser::AbstractHeapType::Any | wasmparser::AbstractHeapType::Eq,
                                CompositeInnerType::Struct(_) | CompositeInnerType::Array(_),
                            )
                            | (wasmparser::AbstractHeapType::Struct, CompositeInnerType::Struct(_))
                            | (wasmparser::AbstractHeapType::Array, CompositeInnerType::Array(_))
                    )
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
            (HeapType::Abstract { shared: false, ty: source }, HeapType::Abstract { shared: false, ty: target }) => {
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
            (HeapType::Exact(UnpackedIndex::Module(source)), HeapType::Exact(UnpackedIndex::Module(target))) => self.equivalent(source as usize, other, target as usize),
            _ => false,
        }
    }

    /// Whether a declaration is equivalent to an embedding's implicit final
    /// singleton function type, including its exact value types. Machine ABI
    /// projections cannot replace concrete reference identities here.
    /// The parameter and result types of declaration `index` where it is the type an embedder's
    /// implicit function has (`matches_implicit_function`): a final function type alone in its
    /// group, with no supertype; none for any other declaration.
    pub(crate) fn implicit_function(&self, index: usize) -> Option<(&[ValType], &[ValType])> {
        let (group, member) = self.locate(index)?;
        let ty = &group[member];
        let plain = group.len() == 1
            && ty.is_final
            && ty.supertype_idx.is_none()
            && !ty.composite_type.shared
            && ty.composite_type.descriptor_idx.is_none()
            && ty.composite_type.describes_idx.is_none();
        match &ty.composite_type.inner {
            // A concrete reference names a type of this table, which no embedder's signature
            // can: such a type compares by its declaration.
            CompositeInnerType::Func(function)
                if plain
                    && function.params().iter().chain(function.results()).all(|value| {
                        !matches!(value, ValType::Ref(reference) if !matches!(reference.heap_type(), HeapType::Abstract { .. }))
                    }) =>
            {
                Some((function.params(), function.results()))
            }
            _ => None,
        }
    }

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

