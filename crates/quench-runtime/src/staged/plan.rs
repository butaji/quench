//! What a frame knows of a call's callee when it plans the call's entry, as data, and the
//! planner that turns those facts into the entry (`plan_entry`). The facts have binding times
//! (`BindingTime`): a callee the record names or the frame proved Stable is folded under the
//! dependency that retires the caller when it changes, so the call checks nothing; any other is
//! checked once per call. A bound function unwraps at plan time only under a folded identity,
//! because only then are its target, `this` and arguments constants of the call.

// Planning reads the binding times of the frame's facts: runtime only (the stencil rows plan no entry).
#[cfg(not(stencil_build))]
use crate::typed_ir::BindingTime;

/// A native function a call enters directly: its entry, and whether it runs beside the lane's
/// activations (`Native::runs_beside_activations`), which stay frameless around such a call.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct NativeId {
    pub(crate) index: u32,
    pub(crate) beside: bool,
}

/// The kinds of callee a site binds to a direct entry. A bound function is one of these with its
/// `this` and arguments folded in (`Bound`), and any callee the frame has no plan for takes the
/// VM's call (`CallEntry::Helper`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CalleeKind {
    /// A script function called with a receiver.
    Script,
    /// A forwarding class constructor: the construction runs its initializer with the receiver
    /// the constructor's plan allocates, and yields that receiver.
    Class,
    /// A native function: its entry takes the arguments as a slice and has no window of its own.
    #[cfg_attr(not(test), allow(dead_code))]
    Native(NativeId),
}

impl CalleeKind {
    /// Whether the call allocates the callee's receiver (a construction) instead of binding the
    /// `this` of a receiver.
    #[inline(always)]
    pub(crate) const fn allocates_receiver(self) -> bool {
        matches!(self, Self::Class)
    }
}

/// How a direct call knows its callee is the one its plan was made for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Identity {
    /// The identity varies per execution: the call checks it once (`guard_callee`).
    Guarded(u64),
    /// The identity is a constant of the call: a function the record names (Static), or a callee
    /// the frame proved Stable and folded under the dependency that retires the caller when it
    /// changes. The call reads and checks nothing.
    Folded(u64),
}

impl Identity {
    /// The callee's identity word.
    #[inline(always)]
    pub(crate) const fn word(self) -> u64 {
        match self {
            Self::Guarded(word) | Self::Folded(word) => word,
        }
    }
}

/// `count` words of the frame's plan constants from `start` (`CallFrame::call_constants`): a handle,
/// so a plan stays plain data the frame owns the words of.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct WordList {
    pub start: u32,
    pub count: u16,
}

impl WordList {
    /// The words the list names in `constants`; none where the constants do not hold them.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn words(self, constants: &[u64]) -> &[u64] {
        let start = self.start as usize;
        constants.get(start..start + usize::from(self.count)).unwrap_or(&[])
    }
}

/// What a bound function binds, unwrapped to its target: the `this` the target is called with, and
/// the arguments that precede the call's own. Both are constants of the plan.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Bound {
    pub this: u64,
    pub arguments: WordList,
}

/// What a frame knows of a site's callee at the time it is compiled: for a bound function, its
/// target's.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DirectPlan {
    pub kind: CalleeKind,
    /// The identity of the callee the site names, which the site was bound to.
    pub identity: Identity,
    /// The global object a sloppy callee binds for `undefined` and `null`; `None` for a strict one.
    pub sloppy: Option<u64>,
    /// The registers of the callee's window.
    pub registers: u16,
    /// The status word of a callee that returned.
    pub returned: u32,
    /// The `this` and arguments of the bound function the site names, where it names one.
    pub bound: Option<Bound>,
}

/// How a call reaches its callee.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CallEntry {
    Direct(DirectPlan),
    /// The callee's body in the caller, where the frame can inline it (`CallFrame::inline`); the
    /// direct entry where it cannot.
    Inline(DirectPlan),
    Helper,
}

#[cfg(not(stencil_build))]
/// A callee a frame resolved at plan time, with the binding time of the value the site names.
#[cfg(not(stencil_build))]
#[derive(Clone, Copy, Debug)]
pub struct Resolved {
    pub plan: DirectPlan,
    pub binding: BindingTime,
    /// The instructions of the callee's body where a script body is known to the frame.
    pub body: Option<usize>,
}

#[cfg(not(stencil_build))]
impl Resolved {
    /// Whether the callee's identity is a constant of the call: Static, or Stable under a dependency.
    const fn folds(&self) -> bool {
        matches!(self.binding, BindingTime::Static | BindingTime::Stable)
    }
}

#[cfg(not(stencil_build))]
/// The entry of a call to `callee`, by binding time. A folded identity checks nothing; any other is
/// guarded once. A bound function whose identity is not folded is the VM's call. A folded script
/// function whose body fits `inline_budget` instructions inlines, with its direct entry behind it.
pub fn plan_entry(callee: Option<Resolved>, inline_budget: usize) -> CallEntry {
    let Some(callee) = callee else { return CallEntry::Helper };
    if callee.plan.bound.is_some() && !callee.folds() { return CallEntry::Helper; }
    let word = callee.plan.identity.word();
    let identity = if callee.folds() { Identity::Folded(word) } else { Identity::Guarded(word) };
    let plan = DirectPlan { identity, ..callee.plan };
    let inlines = callee.folds() && callee.plan.kind == CalleeKind::Script && callee.plan.bound.is_none()
        && callee.body.is_some_and(|size| size <= inline_budget);
    if inlines { CallEntry::Inline(plan) } else { CallEntry::Direct(plan) }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BUDGET: usize = 96;

    fn plan(kind: CalleeKind, bound: Option<Bound>) -> DirectPlan {
        DirectPlan { kind, identity: Identity::Guarded(7), sloppy: None, registers: 2, returned: 1, bound }
    }

    fn resolved(plan: DirectPlan, binding: BindingTime, body: Option<usize>) -> Option<Resolved> {
        Some(Resolved { plan, binding, body })
    }

    #[test]
    fn a_callee_the_frame_resolved_is_guarded_unless_its_identity_folds() {
        let script = plan(CalleeKind::Script, None);
        assert_eq!(plan_entry(None, BUDGET), CallEntry::Helper);
        for binding in [BindingTime::Location, BindingTime::Site, BindingTime::Dynamic] {
            assert_eq!(plan_entry(resolved(script, binding, None), BUDGET), CallEntry::Direct(script), "{binding:?}");
        }
        let folded = DirectPlan { identity: Identity::Folded(7), ..script };
        for binding in [BindingTime::Static, BindingTime::Stable] {
            assert_eq!(plan_entry(resolved(script, binding, None), BUDGET), CallEntry::Direct(folded), "{binding:?}");
        }
    }

    #[test]
    fn a_stable_script_callee_inlines_within_the_budget_and_nothing_else_does() {
        let script = plan(CalleeKind::Script, None);
        let folded = DirectPlan { identity: Identity::Folded(7), ..script };
        assert_eq!(plan_entry(resolved(script, BindingTime::Stable, Some(BUDGET)), BUDGET), CallEntry::Inline(folded));
        assert_eq!(plan_entry(resolved(script, BindingTime::Stable, Some(BUDGET + 1)), BUDGET), CallEntry::Direct(folded), "too large");
        assert_eq!(plan_entry(resolved(script, BindingTime::Site, Some(1)), BUDGET), CallEntry::Direct(script), "not Stable");
        let class = plan(CalleeKind::Class, None);
        assert!(matches!(plan_entry(resolved(class, BindingTime::Stable, Some(1)), BUDGET), CallEntry::Direct(_)), "a construction");
        let native = plan(CalleeKind::Native(NativeId { index: 3, beside: false }), None);
        assert!(matches!(plan_entry(resolved(native, BindingTime::Stable, Some(1)), BUDGET), CallEntry::Direct(_)), "a native has no body");
    }

    #[test]
    fn a_bound_function_unwraps_only_where_its_identity_folds() {
        let bound = Bound { this: 9, arguments: WordList { start: 1, count: 2 } };
        let target = plan(CalleeKind::Script, Some(bound));
        assert_eq!(plan_entry(resolved(target, BindingTime::Site, Some(1)), BUDGET), CallEntry::Helper, "a guarded bound function is the VM's");
        let folded = DirectPlan { identity: Identity::Folded(7), ..target };
        assert_eq!(plan_entry(resolved(target, BindingTime::Stable, Some(1)), BUDGET), CallEntry::Direct(folded),
            "a Stable bound function takes its target's entry, not an inlined body");
    }

    #[test]
    fn a_word_list_names_its_constants_and_none_past_them() {
        let constants = [10, 20, 30];
        assert_eq!(WordList { start: 1, count: 2 }.words(&constants), [20, 30]);
        assert_eq!(WordList { start: 2, count: 2 }.words(&constants), [] as [u64; 0]);
    }
}
