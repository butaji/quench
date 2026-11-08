//! The published lane bodies' lifecycle: publication under the facts their code relies on,
//! and retirement when one changes (a slot widens, a Stable fact changes, a named callee
//! dies, the realm or heap is replaced).
use super::*;
use crate::code_dependencies::PublishedCode;
use crate::vm::compiled::{LaneMethod, PublishedLaneMethod};

/// Code that names callees by identity, which a collection that frees one retires: a
/// published body, or a body's plan whose code still compiles (`PendingLaneMethod`), whose
/// `watch` the poll reads. Without it a collection during the compile would leave the plan
/// naming a freed locator a later allocation may reuse.
pub(in crate::vm) enum NativeCallBinding {
    Published(Weak<PublishedLaneMethod>),
    Pending { watch: Weak<PublishedCode<()>>, callees: Box<[Value]> },
}

impl ProgramStore {
    /// Publishes a lane body under the dependencies its code took: its slots' types
    /// (`Heap::subscribe_slot`) and its Stable facts (prototype values by holder shape, unnamed
    /// method atoms in `method_atoms`). A change of any retires it and sets its invalidation word.
    pub(in crate::vm) fn publish_lane_method(&self, method: LaneMethod, heap: &mut crate::heap::Heap,
        method_atoms: &mut crate::code_dependencies::CodeReaders) -> Option<Rc<PublishedLaneMethod>> {
        let calls = !method.inlined.is_empty() || !method.direct.is_empty();
        let invalidation = method.invalidation.clone();
        let published = Rc::new(PublishedLaneMethod::with_invalidation(method, invalidation));
        let code: Rc<dyn crate::code_dependencies::DependentCode> = published.clone();
        let method = published.lease().expect("new publication");
        if !method.dependencies.subscribe(&method.stable, &code, heap, method_atoms) {
            published.retire();
            return None;
        }
        if calls { self.native_call_bindings.borrow_mut().push(NativeCallBinding::Published(Rc::downgrade(&published))); }
        Some(published)
    }

    /// Retires `watch`, a plan's, should a collection free any of `callees` before its code
    /// publishes.
    pub(in crate::vm) fn note_pending_native_calls(&self, watch: &Rc<PublishedCode<()>>, callees: Box<[Value]>) {
        if !callees.is_empty() {
            self.native_call_bindings.borrow_mut().push(NativeCallBinding::Pending { watch: Rc::downgrade(watch), callees });
        }
    }

    /// Retires each published body whose code names a callee that died: an inlined one, or a
    /// direct one, whose body's own retirement retires its direct callers in turn.
    ///
    /// # Safety
    /// Collection has stopped native execution before guest locators can be reused.
    pub(in crate::vm) unsafe fn retire_dead_native_calls(&self, live: impl Fn(Value) -> bool) {
        let mut bindings = self.native_call_bindings.borrow_mut();
        loop {
            let before = bindings.len();
            bindings.retain(|binding| {
                let weak = match binding {
                    NativeCallBinding::Published(weak) => weak,
                    NativeCallBinding::Pending { watch, callees } => {
                        let Some(watch) = watch.upgrade().filter(|watch| watch.is_published()) else { return false; };
                        let dead = callees.iter().any(|callee| !live(*callee));
                        if dead { watch.retire(); }
                        return !dead;
                    }
                };
                let Some(code) = weak.upgrade() else { return false; };
                let Some(method) = code.lease() else { return false; };
                // Function bindings depend on the owned program definition,
                // not a particular closure locator. Realm/reset retirement
                // still uses this inventory for both binding kinds.
                let dead = method.inlined.iter().any(|call| call.target.identity().is_some_and(|callee| !live(callee)))
                    || method.direct.iter().any(|callee| !live(callee.callee) || !callee.method.is_published());
                if dead {
                    code.retire();
                }
                !dead
            });
            if bindings.len() == before { break; }
        }
    }

    /// # Safety
    /// Realm/heap transition excludes execution using the old realm's guest locators.
    pub(in crate::vm) unsafe fn retire_native_calls(&self) {
        for binding in self.native_call_bindings.borrow_mut().drain(..) {
            match binding {
                NativeCallBinding::Published(weak) => if let Some(code) = weak.upgrade() { code.retire() },
                NativeCallBinding::Pending { watch, .. } => if let Some(watch) = watch.upgrade() { watch.retire() },
            }
        }
    }

}
