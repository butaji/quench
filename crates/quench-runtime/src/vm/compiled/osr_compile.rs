//! Loop-region code compiled beside the program: the loop runs in the lane while the code
//! generator's thread compiles, and the next look publishes the code.

use super::{
    compile_native_function, dump_native_code, native_dump_directory, osr_loop, poll_address, publish_entry, tier_policy,
    CompiledOsrRegion, LoopRegion, NativeCompileRejection, Observation, Widening,
};
use std::rc::Rc;

/// What a region's code relies on beyond its entry guards: the exits that widen a value,
/// and the facts its own-field descriptors embed, which retire it when they change.
pub(in crate::vm) struct OsrFacts {
    /// By exit PC, in order.
    widenings: Box<[(u32, Widening)]>,
    /// Own-field descriptors embed shape IDs. Shape compaction retires this code
    /// through its lane view before execution can resume with remapped IDs.
    pub(in crate::vm) shape_dependencies: Vec<u32>,
    slot_dependencies: Vec<crate::code_dependencies::SlotDependency>,
}

impl OsrFacts {
    /// The value the code held as an `i32` that its exit at `pc` found none.
    pub(super) fn widening(&self, pc: u32) -> Option<Widening> {
        let found = self.widenings.binary_search_by_key(&pc, |&(exit, _)| exit).ok()?;
        Some(self.widenings[found].1)
    }

    /// Subscribes `code` to the facts; whether all hold.
    pub(in crate::vm) fn subscribe(&self, code: &Rc<dyn crate::code_dependencies::DependentCode>, heap: &mut crate::heap::Heap) -> bool {
        self.shape_dependencies.iter().all(|shape| heap.subscribe_shape(*shape, code))
            && self.slot_dependencies.iter().all(|dependency| heap.subscribe_slot(*dependency, code))
    }
}

/// A loop region whose code compiles on the code generator's thread, as V8 compiles OSR code
/// concurrently: compiling on the loop's own thread stalled it 2 ms a region (Box2D --jit:
/// 34 ms of a 733 ms run). Its facts are the observation's at the start, to which `watch`
/// subscribes: a shape compaction or a slot widening meanwhile discards the code.
pub(in crate::vm) struct PendingOsrRegion {
    code: super::super::jit_backend::Pending<Result<Vec<u8>, NativeCompileRejection>>,
    watch: Rc<crate::code_dependencies::PublishedCode<()>>,
    header: u32,
    clif: Option<String>,
    facts: OsrFacts,
}

/// What a background loop-region compile came to.
pub(in crate::vm) enum OsrCompile {
    Done(CompiledOsrRegion),
    /// A fact it took changed meanwhile: the next entry may compile from fresh facts.
    Stale,
    Failed,
}

/// Starts compiling `region`'s code from `observation`, or `None` where it lowers no code.
pub(in crate::vm) fn start_osr_region(
    region: &LoopRegion,
    observation: &Observation<'_>,
    interrupt: &crate::execution_interrupt::ExecutionInterrupt,
    policy: crate::MemoryPolicy,
    track_backedges: bool,
) -> Option<PendingOsrRegion> {
    let isa = super::super::jit_backend::target_isa().filter(|_| tier_policy::by_policy(policy).loop_tier)?;
    let verified = osr_loop::verify_region(region, observation).ok()?;
    let (function, widenings) =
        osr_loop::lower_verified(verified, observation, &*isa, poll_address(interrupt), track_backedges)?;
    let clif = native_dump_directory().map(|_| function.display().to_string());
    let loads = || observation.native_field_loads.iter().flatten();
    let mut shape_dependencies: Vec<_> = loads().flat_map(|field| std::iter::once(field.shape).chain(field.holder)).collect();
    shape_dependencies.sort_unstable();
    shape_dependencies.dedup();
    let mut slot_dependencies: Vec<_> = loads().filter_map(|field| field.slot_dependency()).collect();
    slot_dependencies.sort_unstable_by_key(|dependency| (dependency.shape, dependency.slot));
    slot_dependencies.dedup();
    let code = super::super::jit_backend::run_in_background(move || {
        compile_native_function(function, &*isa, osr_loop::MAX_CODE_BYTES, |code| Some(code.to_vec()))
    });
    let watch = Rc::new(crate::code_dependencies::PublishedCode::new(()));
    Some(PendingOsrRegion { code, watch, header: region.header, clif, facts: OsrFacts { widenings: widenings.into_iter().collect(), shape_dependencies, slot_dependencies } })
}

impl PendingOsrRegion {
    /// Subscribes its watch to the facts its code names; whether they hold.
    pub(in crate::vm) fn watch(&self, heap: &mut crate::heap::Heap) -> bool {
        let watch: Rc<dyn crate::code_dependencies::DependentCode> = self.watch.clone();
        self.facts.subscribe(&watch, heap)
    }

    /// What its compile came to, or itself while its code compiles.
    pub(in crate::vm) fn poll(self: Box<Self>) -> Result<OsrCompile, Box<Self>> {
        let code = match self.code.poll() {
            std::task::Poll::Pending => return Err(self),
            std::task::Poll::Ready(_) if !self.watch.is_published() => return Ok(OsrCompile::Stale),
            std::task::Poll::Ready(Some(Ok(code))) => code,
            std::task::Poll::Ready(_) => return Ok(OsrCompile::Failed),
        };
        let Self { header, clif, facts, .. } = *self;
        let Some((memory, entry)) = publish_entry(&code) else { return Ok(OsrCompile::Failed) };
        if let (Some(directory), Some(clif)) = (native_dump_directory(), clif) {
            dump_native_code(directory, header, &memory, &clif);
        }
        Ok(OsrCompile::Done(CompiledOsrRegion { _code: memory, entry, facts }))
    }
}
