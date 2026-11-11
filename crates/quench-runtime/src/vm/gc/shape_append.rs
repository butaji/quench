//! The mark callback's visit of one object's shape (`ParallelShapeTrace::append`).

use super::*;

impl ParallelShapeTrace<'_> {
    // Called once per marked object: a shape already seen with no owned roots to add is the
    // common visit, answered by two loads here; the rest is out of line.
    #[inline(always)]
    pub(super) fn append(&self, trace_kind: GcTraceKind, owner: Value, shape: u32, prototype: bool, roots: &mut Vec<Value>) {
        let (word, mask) = (shape as usize / u64::BITS as usize, 1_u64 << (shape % u64::BITS));
        if !prototype && trace_kind != GcTraceKind::VerifyFull && self.owned_roots.is_empty()
            && self.marked_constructor_shapes[word].load(Ordering::Relaxed) & mask != 0 {
            return;
        }
        self.append_visit(trace_kind, owner, shape, prototype, roots);
    }

    #[inline(never)]
    fn append_visit(&self, trace_kind: GcTraceKind, owner: Value, shape: u32, prototype: bool, roots: &mut Vec<Value>) {
        // The constructor bit follows the shape bit for every shape: set, a non-prototype
        // holder has nothing to add, so a shape already seen costs one load.
        let (word, mask) = (shape as usize / u64::BITS as usize, 1_u64 << (shape % u64::BITS));
        if prototype || trace_kind == GcTraceKind::VerifyFull || self.marked_constructor_shapes[word].load(Ordering::Relaxed) & mask == 0 {
            self.append_shape_edges(trace_kind, shape, prototype, roots);
        }
        if !self.owned_roots.is_empty() && self.owned_root_filter.may_hold(owner)
            && let Some(edges) = self.owned_roots.get(&owner)
        {
            roots.extend(edges.iter().copied());
        }
    }
}
