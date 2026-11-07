//! The published lane bodies' lifecycle: publication under the facts their code relies on,
//! and retirement when one changes (a slot widens, a read site's cache gains an entry, a
//! named callee dies, the realm or heap is replaced).
use super::*;
use crate::vm::compiled::{LaneMethod, PublishedLaneMethod};

#[derive(Default)]
pub(super) struct LanePublications {
    /// Weak native-call proofs; guest functions are never rooted by compiled code.
    native_call_bindings: std::cell::RefCell<Vec<Weak<PublishedLaneMethod>>>,
    /// Per field cache index, the bodies whose reads specialized on its entries.
    field_site_readers: std::cell::RefCell<crate::code_dependencies::CodeReaders>,
}

impl ProgramStore {
    /// Publishes a lane body under its dependencies: the slots it reads (`Heap::subscribe_slot`)
    /// and the field caches its reads specialized on (`field_site_readers`).
    pub(in crate::vm) fn publish_lane_method(&self, method: LaneMethod, heap: &mut crate::heap::Heap) -> Option<Rc<PublishedLaneMethod>> {
        let calls = !method.inlined.is_empty() || !method.direct.is_empty();
        let published = Rc::new(PublishedLaneMethod::new(method));
        let code: Rc<dyn crate::code_dependencies::DependentCode> = published.clone();
        let method = published.lease().expect("new publication");
        let subscribed = method.slot_dependencies.iter().all(|dependency| heap.subscribe_slot(*dependency, &code))
            && method.read_sites.iter().all(|site| self.lane_publications.field_site_readers.borrow_mut().subscribe(*site, &code));
        if !subscribed {
            published.retire();
            return None;
        }
        if calls { self.lane_publications.native_call_bindings.borrow_mut().push(Rc::downgrade(&published)); }
        Some(published)
    }

    /// Retires each published body whose code names a callee that died: an inlined one, or a
    /// direct one, whose body's own retirement retires its direct callers in turn.
    ///
    /// # Safety
    /// Collection has stopped native execution before guest locators can be reused.
    pub(in crate::vm) unsafe fn retire_dead_native_calls(&self, live: impl Fn(Value) -> bool) {
        let mut bindings = self.lane_publications.native_call_bindings.borrow_mut();
        loop {
            let before = bindings.len();
            bindings.retain(|weak| {
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
        for weak in self.lane_publications.native_call_bindings.borrow_mut().drain(..) {
            if let Some(code) = weak.upgrade() {
                code.retire();
            }
        }
    }

    /// Field cache index `site` gained an entry: the bodies whose reads specialized on its
    /// entries predate the receiver and retire, to specialize again from the caches as they
    /// are (`LaneView::settle_retired_body`).
    #[inline(always)]
    pub(in crate::vm) fn retire_field_site_readers(&self, site: usize) {
        self.lane_publications.field_site_readers.borrow_mut().retire(site as u32);
    }
}

#[cfg(test)]
mod tests {
    use crate::{Value, vm::Vm};

    #[test]
    fn published_lane_bodies_retire_when_a_read_site_gains_an_entry() {
        let program = crate::Engine::specialize(r#"
          function read(o) { return o.x + 1; }
          globalThis.first = { x: 1 };
          globalThis.second = { y: 0, x: 2 };
          for (let i = 0; i < 40; i++) read(first);
        "#, "read-site-readers.js").unwrap();
        let mut vm = Vm::new(crate::test_host::TestHost::default());
        vm.execute(&program).unwrap();
        let id = program.functions.iter().position(|body|
            body.header().name.is_some_and(|atom| &program.atoms[atom as usize] == "read")).unwrap();
        let method = vm.compile_lane_method(&program, id as u32, &|_, _| Vec::new(), &|_| (0, 0), Vec::new(), None, &[]).unwrap();
        assert!(!method.read_sites.is_empty(), "the body reads through a field site");
        let published = vm.programs.publish_lane_method(method, &mut vm.heap).unwrap();
        let read = vm.intern_atom("read");
        let read = vm.own_property(vm.realm.globals, read).unwrap();
        let first = vm.intern_atom("first");
        let first = vm.own_property(vm.realm.globals, first).unwrap();
        assert_eq!(vm.call_value(&program, read, Value::UNDEFINED, &[first]).unwrap(), Value::integer(2));
        assert!(published.is_published(), "a receiver the site knows changes nothing");
        let second = vm.intern_atom("second");
        let second = vm.own_property(vm.realm.globals, second).unwrap();
        assert_eq!(vm.call_value(&program, read, Value::UNDEFINED, &[second]).unwrap(), Value::integer(3));
        assert!(!published.is_published(), "a receiver the code predates retires it");
    }
}
