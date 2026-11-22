//! The published lane bodies' lifecycle: publication under the facts their code relies on,
//! and retirement when one changes (a slot widens, a Stable fact changes, a named callee
//! dies, the realm or heap is replaced).
use super::*;
use crate::code_dependencies::PublishedCode;
use crate::vm::engine_types::{CompiledOsrRegion, LaneMethod, PublishedLaneMethod};
use crate::vm::tier_events::Event;

/// Code that names callees by identity: a published body, or a body's plan whose code still
/// compiles (`PendingLaneMethod`), whose `watch` the poll reads. Both hold their callees
/// alive (`FoldedRoots`), so a collection frees none while the code can compare it; the
/// inventory retires them at the lifecycle edges a callee's body or the realm changes at.
pub(in crate::vm) enum NativeCallBinding {
    Published(Weak<PublishedLaneMethod>),
    Region(Weak<PublishedCode<CompiledOsrRegion>>),
    Pending { watch: Weak<PublishedCode<()>>, callees: Box<[Value]> },
}

/// Whether `code` still stands after a collection: it is published and `dead` finds no callee of it
/// that died; code that names one is retired.
fn stands<T>(code: Option<Rc<PublishedCode<T>>>, dead: impl Fn(&T) -> bool) -> bool {
    let Some(code) = code else { return false };
    let Some(inner) = code.lease() else { return false };
    let died = dead(&inner);
    if died {
        code.retire();
    }
    !died
}

impl NativeCallBinding {
    /// Whether the code still stands after a collection that kept the callees `live` names. Function
    /// bindings depend on the owned program definition, not a particular closure locator; realm/reset
    /// retirement still uses this inventory for every binding kind.
    fn stands(&self, live: &impl Fn(Value) -> bool) -> bool {
        match self {
            Self::Published(method) => stands(method.upgrade(), |method| crate::vm::engine_calls::names_dead_callee(&method.inlined, &method.direct, live) || method.constructors.iter().any(|constructor| !live(*constructor))),
            Self::Region(region) => stands(region.upgrade(), |region| region.calls.names_dead(live)),
            Self::Pending { watch, callees } => stands(watch.upgrade().filter(|watch| watch.is_published()), |_| callees.iter().any(|callee| !live(*callee))),
        }
    }
}

/// What a lane body compiles from: its program and function, the entry it starts at, and its
/// plan (`LanePlan::key`). Equal keys make equal code, so a body published under one serves
/// every later request of it while it holds.
pub type LaneKey = (u32, u32, Option<u32>, u64);

impl ProgramStore {
    /// The loaded ahead-of-time bodies of `id`, which is `program`.
    pub(in crate::vm) fn ahead_images(&self, id: super::ProgramId, program: &crate::ResidualProgram) -> Option<Rc<crate::vm::engine_types::AheadImages>> {
        if self.ahead.borrow().is_empty() { return None; }
        self.ahead.borrow().iter().find(|images| images.is_of(id.raw(), program)).cloned()
    }

    /// Takes `images` as the ahead-of-time bodies of the program they were compiled from.
    pub(in crate::vm) fn add_ahead_images(&self, images: crate::vm::engine_types::AheadImages) {
        self.ahead.borrow_mut().push(Rc::new(images));
    }

    /// Whether the image the system linker placed at `table` is loaded (`Vm::load_linked_image`).
    pub(in crate::vm) fn has_linked_image(&self, table: &crate::vm::engine_types::LinkedTable) -> bool {
        self.ahead.borrow().iter().any(|images| images.is_linked_at(table))
    }

    /// The published body of `key`, if one still holds.
    pub(in crate::vm) fn installed_lane(&self, key: &LaneKey) -> Option<Rc<PublishedLaneMethod>> {
        self.lane_installed.borrow().get(key).and_then(Weak::upgrade).filter(|published| published.is_published())
    }

    /// Counts a compile of `key`'s function from the entry it starts at (the function's own, or
    /// one loop's: each has its own count); whether the bound `max` on them allows it.
    pub(in crate::vm) fn admit_lane_compile(&self, key: &LaneKey, max: u8) -> bool {
        let mut compiles = self.lane_compiles.borrow_mut();
        let count = compiles.entry((key.0, key.1, key.2)).or_insert(0);
        let admitted = *count < max;
        *count = count.saturating_add(u8::from(admitted));
        admitted
    }

    /// Whether a compile of `site`'s function from its entry is still within `max`
    /// (`admit_lane_compile`), without counting one: a plan past the bound is not made.
    pub(in crate::vm) fn compiles_left_in_lane(&self, site: &(u32, u32, Option<u32>), max: u8) -> bool {
        self.lane_compiles.borrow().get(site).is_none_or(|count| *count < max)
    }

    /// The IR size of `site`'s last lane plan, if one was made.
    pub(in crate::vm) fn planned_lane_ir(&self, site: &(u32, u32, Option<u32>)) -> Option<usize> {
        self.lane_plan_ir.borrow().get(site).copied()
    }

    /// Records that `site`'s lane plan made `ir` IR instructions.
    pub(in crate::vm) fn note_planned_lane_ir(&self, site: (u32, u32, Option<u32>), ir: usize) {
        self.lane_plan_ir.borrow_mut().insert(site, ir);
    }

    /// Records `published` as the body of `key`.
    pub(in crate::vm) fn note_installed_lane(&self, key: LaneKey, published: &Rc<PublishedLaneMethod>) {
        self.lane_installed.borrow_mut().insert(key, Rc::downgrade(published));
    }

    /// Publishes a lane body under the dependencies its code took: its slots' types
    /// (`Heap::subscribe_slot`) and its Stable facts (prototype values by holder shape, unnamed
    /// method atoms in `method_atoms`). A change of any retires it and sets its invalidation word.
    pub(in crate::vm) fn publish_lane_method(&self, mut method: LaneMethod, heap: &mut crate::heap::Heap,
        method_atoms: &mut crate::code_dependencies::CodeReaders) -> Option<Rc<PublishedLaneMethod>> {
        let calls = !method.inlined.is_empty() || !method.direct.is_empty();
        method.reserve_scratch();
        method.trim_published();
        let invalidation = method.invalidation.clone();
        let roots = std::mem::take(&mut method.stable.roots);
        let published = Rc::new(PublishedLaneMethod::with_invalidation(method, invalidation));
        published.hold_roots(roots, heap);
        let code: Rc<dyn crate::code_dependencies::DependentCode> = published.clone();
        let method = published.lease().expect("new publication");
        let root_constants = self.subscribe_root_constants(&method.dependencies.root_programs(&method.stable), &code);
        if !root_constants || !method.dependencies.subscribe(&method.stable, &code, heap, method_atoms) {
            published.retire();
            return None;
        }
        if calls { self.native_call_bindings.borrow_mut().push(NativeCallBinding::Published(Rc::downgrade(&published))); }
        self.tier_events.record(Event::Published);
        self.tier_events.add(Event::Inlined, method.inlined.len());
        Some(published)
    }

    /// Publishes a loop region's code under the dependencies its code took (`OsrFacts::subscribe`): a
    /// change of one retires it and sets its invalidation word, and a collection that frees a callee
    /// it names does (`NativeCallBinding::Region`).
    pub(in crate::vm) fn publish_region(&self, mut region: CompiledOsrRegion, heap: &mut crate::heap::Heap,
        method_atoms: &mut crate::code_dependencies::CodeReaders) -> Rc<PublishedCode<CompiledOsrRegion>> {
        region.facts.trim();
        region.calls.trim();
        let invalidation = region.calls.invalidation.clone();
        let published = Rc::new(PublishedCode::with_invalidation(region, invalidation));
        let code: Rc<dyn crate::code_dependencies::DependentCode> = published.clone();
        let region = published.lease().expect("new publication");
        if !region.facts.subscribe(&code, heap, method_atoms) || !self.subscribe_root_constants(&region.facts.root_programs(), &code) {
            published.retire();
        } else if !region.calls.inlined.is_empty() || !region.calls.direct.is_empty() {
            self.native_call_bindings.borrow_mut().push(NativeCallBinding::Region(Rc::downgrade(&published)));
        }
        published
    }

    /// Retires `watch`, a plan's, should a collection free any of `callees` before its code
    /// publishes.
    pub(in crate::vm) fn note_pending_native_calls(&self, watch: &Rc<PublishedCode<()>>, callees: Box<[Value]>) {
        if !callees.is_empty() {
            self.native_call_bindings.borrow_mut().push(NativeCallBinding::Pending { watch: Rc::downgrade(watch), callees });
        }
    }

    /// Retires each published body or region whose code names a callee that died: an inlined one, or
    /// a direct one, whose body's own retirement retires its direct callers in turn.
    ///
    /// # Safety
    /// Collection has stopped native execution before guest locators can be reused.
    pub(in crate::vm) unsafe fn retire_dead_native_calls(&self, live: impl Fn(Value) -> bool) {
        let mut bindings = self.native_call_bindings.borrow_mut();
        loop {
            let before = bindings.len();
            bindings.retain(|binding| binding.stands(&live));
            if bindings.len() == before { break; }
        }
    }

    /// `retire_native_calls` at a switch of the active realm, which keeps the bodies bound to
    /// their own realm (`LaneMethod::realm_bound`): each proves it at entry and names no other.
    ///
    /// # Safety
    /// As `retire_native_calls`.
    pub(in crate::vm) unsafe fn retire_native_calls_across_realms(&self) {
        let mut bindings = self.native_call_bindings.borrow_mut();
        bindings.retain(|binding| {
            if let NativeCallBinding::Published(weak) = binding
                && let Some(code) = weak.upgrade()
            {
                if code.lease().is_some_and(|method| method.realm_bound) { return true; }
                code.retire();
                return false;
            }
            match binding {
                NativeCallBinding::Published(_) => {}
                NativeCallBinding::Region(weak) => if let Some(code) = weak.upgrade() { code.retire() },
                NativeCallBinding::Pending { watch, .. } => if let Some(watch) = watch.upgrade() { watch.retire() },
            }
            false
        });
    }

    /// # Safety
    /// Realm/heap transition excludes execution using the old realm's guest locators.
    pub(in crate::vm) unsafe fn retire_native_calls(&self) {
        for binding in self.native_call_bindings.borrow_mut().drain(..) {
            match binding {
                NativeCallBinding::Published(weak) => if let Some(code) = weak.upgrade() { code.retire() },
                NativeCallBinding::Region(weak) => if let Some(code) = weak.upgrade() { code.retire() },
                NativeCallBinding::Pending { watch, .. } => if let Some(watch) = watch.upgrade() { watch.retire() },
            }
        }
    }

}
