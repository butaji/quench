//! Each declaration's canonical index (the first declaration of its table equivalent to it)
//! and its supertype, found once when the table is made: equivalence and subtyping within
//! one table then compare indices instead of declarations.

use super::{Edge, WasmTypes};
use wasmparser::UnpackedIndex;
use rustc_hash::FxHashMap;
use std::rc::Rc;

/// A declaration's parent when it declares no supertype.
const NO_PARENT: u32 = u32::MAX;

impl WasmTypes {
    /// This table with each declaration's canonical index and parent filled in. Only the
    /// declarations of one fingerprint (`WasmTypes::fingerprint`) are compared.
    pub(super) fn with_ancestry(mut self) -> Self {
        let declarations = self.0.members.len();
        let parents = (0..declarations).map(|index| self.declared_parent(index)).collect();
        let mut classes: FxHashMap<Option<u64>, Vec<u32>> = FxHashMap::default();
        let mut canonical = Vec::with_capacity(declarations);
        for index in 0..declarations {
            let class = classes.entry(self.fingerprint(index)).or_default();
            let first = class.iter().copied().find(|&first| self.equivalent(index, &self, first as usize));
            canonical.push(first.unwrap_or_else(|| {
                class.push(index as u32);
                index as u32
            }));
        }
        let table = Rc::get_mut(&mut self.0).expect("a table being made has one holder");
        table.canonical = canonical.into();
        table.parents = parents;
        self
    }

    fn declared_parent(&self, index: usize) -> u32 {
        let (Some(ty), Some((_, member))) = (self.get(index), self.locate(index)) else {
            return NO_PARENT;
        };
        let parent = ty.supertype_idx.and_then(|supertype| self.edge(supertype.unpack(), index - member));
        match parent {
            Some(Edge::Local(parent)) => (index - member + parent) as u32,
            Some(Edge::External { origin, member }) => (origin + member) as u32,
            None => NO_PARENT,
        }
    }

    /// Whether two declarations of this table are equivalent, once their classes are found.
    pub(super) fn same_class(&self, index: usize, other: usize) -> Option<bool> {
        let canonical = &self.0.canonical;
        Some(*canonical.get(index)? == *canonical.get(other)?)
    }

    /// Whether declaration `index` is `target` or has it as an ancestor, up to equivalence,
    /// once the table's ancestry is found.
    pub(super) fn ancestry_subtype(&self, mut index: usize, target: usize) -> Option<bool> {
        let (canonical, parents) = (&self.0.canonical, &self.0.parents);
        let class = *canonical.get(target)?;
        canonical.get(index)?;
        // Validated ancestry is acyclic; the declaration count bounds malformed embedding input.
        for _ in 0..parents.len() {
            if canonical[index] == class {
                return Some(true);
            }
            match parents[index] {
                NO_PARENT => return Some(false),
                parent => index = parent as usize,
            }
        }
        Some(false)
    }

    /// Where an edge of the group at `origin` leads.
    pub(super) fn edge(&self, index: UnpackedIndex, origin: usize) -> Option<Edge> {
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
}
