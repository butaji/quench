//! Typed context records for native control-flow stencils.
//!
//! These records are physical ABI data only. Canonical residual instructions
//! remain the source of JavaScript semantics and admission facts.

#[repr(C)]
pub(crate) struct NativeCompareBranchContext {
    pub lhs: f64,
    pub rhs: f64,
    pub true_pc: usize,
    pub false_pc: usize,
    result: u64,
    next_pc: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct NativeCompareBranchOutcome {
    pub result: bool,
    pub next_pc: usize,
}

impl NativeCompareBranchContext {
    pub(crate) const fn new(lhs: f64, rhs: f64, true_pc: usize, false_pc: usize) -> Self {
        Self {
            lhs,
            rhs,
            true_pc,
            false_pc,
            result: 0,
            next_pc: false_pc,
        }
    }

    pub(crate) fn finish(&self, status: u32) -> Option<NativeCompareBranchOutcome> {
        if status != 1 || self.result > 1 {
            return None;
        }
        let expected = if self.result == 0 {
            self.false_pc
        } else {
            self.true_pc
        };
        if self.next_pc != expected {
            return None;
        }
        Some(NativeCompareBranchOutcome {
            result: self.result != 0,
            next_pc: self.next_pc,
        })
    }
}

const _: () = {
    assert!(std::mem::size_of::<NativeCompareBranchContext>() == 48);
    assert!(std::mem::align_of::<NativeCompareBranchContext>() == 8);
    assert!(std::mem::offset_of!(NativeCompareBranchContext, lhs) == 0);
    assert!(std::mem::offset_of!(NativeCompareBranchContext, rhs) == 8);
    assert!(std::mem::offset_of!(NativeCompareBranchContext, true_pc) == 16);
    assert!(std::mem::offset_of!(NativeCompareBranchContext, false_pc) == 24);
    assert!(std::mem::offset_of!(NativeCompareBranchContext, result) == 32);
    assert!(std::mem::offset_of!(NativeCompareBranchContext, next_pc) == 40);
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outcome_requires_matching_declared_successor() {
        let mut context = NativeCompareBranchContext::new(1.0, 2.0, 7, 11);
        context.result = 1;
        context.next_pc = 7;
        assert_eq!(context.finish(1).unwrap().next_pc, 7);
        context.next_pc = 11;
        assert!(context.finish(1).is_none());
        assert!(context.finish(0).is_none());
    }
}
