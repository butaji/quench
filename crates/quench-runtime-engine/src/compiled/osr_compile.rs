//! Loop-region code compiled beside the program: the loop runs in the lane while the code
//! generator's thread compiles, and the next look publishes the code.

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::{
    dump_native_code, DumpSite, native_dump_directory, lane_unit,
    publish_body, CompiledOsrRegion,
    NativeCompileRejection, NativeFieldLoad,
};
use super::lane_compile::LaneDependencies;
use super::lane_region::StableFacts;
use quench_runtime::code_dependencies::Invalidation;
pub(crate) mod calls;
pub(crate) use calls::CallCode;
use quench_runtime::bytecode::ResidualProgram;
use quench_runtime::vm::global_facts::GlobalTable;
use super::DirectCallee;
use std::rc::Rc;

pub(crate) use quench_runtime::vm::engine_types::OsrFacts;



/// A loop region whose code compiles on the code generator's thread, as V8 compiles OSR code
/// concurrently: compiling on the loop's own thread stalled it 2 ms a region (Box2D --jit:
/// 34 ms of a 733 ms run). Its facts are the observation's at the start, to which `watch`
/// subscribes: a shape compaction or a slot widening meanwhile discards the code.
pub(crate) struct PendingOsrRegion {
    /// Its lowering and code on the code generator's thread; a drop waits for the lowering,
    /// which reads `directs` by pointer. First, so it drops before them.
    pub code: quench_runtime::vm::jit_backend::OffThread<Option<LoopLowered>>,
    pub watch: Rc<quench_runtime::code_dependencies::PublishedCode<()>>,
    pub header: u32,
    pub facts: OsrFacts,
    /// What a run of the code builds around it.
    pub crossing: super::Crossing,
    /// Where a run may re-enter the code mid-region.
    pub resumes: super::base_region::ResumeTable,
    /// What its code owns of its calls (`CallCode`): known at the start for a b-jit region, made
    /// from the plan the lowering took (`directs`) for a Cranelift one.
    pub calls: Option<CallCode>,
    /// Per plan the lowering may take, its direct callees, which its code names.
    pub directs: Vec<Box<[DirectCallee]>>,
    /// The caller's registers and invalidation word, which a Cranelift region's `CallCode` takes.
    pub call_frame: (u16, Invalidation),
    /// Per plan, where its lowering folded Stable facts: its unit, global reads and
    /// the VM's answers it took, which must still hold where the code publishes (`poll`).
    pub taken: Vec<Option<TakenPlan>>,
    pub function: u32,
}

/// What a plan whose lowering folded Stable facts took from the VM (`PendingOsrRegion::taken`).
struct TakenPlan {
    unit: lane_unit::Unit,
    fields: Vec<Option<Vec<NativeFieldLoad>>>,
    global_reads: quench_runtime::vm::global_facts::GlobalReads,
    tables: super::lane_compile::FactTables,
}

impl TakenPlan {
    /// Whether the VM answers now what the plan took: its global reads and its fact tables.
    fn holds(&self, vm: &mut quench_runtime::vm::Vm, program: &ResidualProgram, function: u32) -> bool {
        vm.global_table(program, function, &self.unit, GlobalTable::Lane) == self.global_reads
            && super::lane_compile::FactTables::of(vm, function, &self.unit, &self.fields) == self.tables
    }
}

/// What a loop region's lowering and code generation came to: the plan it took (an index of
/// `PendingOsrRegion::directs`), the Stable facts and roots its code folded, its CLIF text where
/// dumps are on, and its code.
pub(crate) struct LoopLowered {
    plan: usize,
    stable: StableFacts,
    roots: quench_runtime::folded_roots::FoldedRoots,
    clif: Option<String>,
    code: Result<Vec<u8>, NativeCompileRejection>,
}

/// What a background loop-region compile came to.
pub(crate) enum OsrCompile {
    Done(CompiledOsrRegion),
    /// A fact it took changed meanwhile: the next entry may compile from fresh facts.
    Stale,
    Failed,
}

#[quench_macros::ext(PendingOsrRegionOps1)]
impl PendingOsrRegion {

    /// Subscribes its watch to the facts its code names; whether they hold.
    pub(crate) fn watch(&self, facts: &mut quench_runtime::vm::tier_queries::FactWatch<'_>,
        programs: &quench_runtime::vm::program_store::ProgramStore) -> bool {
        facts.hold_roots(&self.watch, self.facts.roots.clone());
        let watch: Rc<dyn quench_runtime::code_dependencies::DependentCode> = self.watch.clone();
        self.facts.subscribe(&watch, facts) && programs.subscribe_root_constants(&self.facts.root_programs(), &watch)
    }

    /// The watch a collection that frees any of the callees its code names retires, and those callees
    /// (`ProgramStore::note_pending_native_calls`): while it lowers, those of every plan it may take.
    pub(crate) fn native_calls(&self) -> (Rc<quench_runtime::code_dependencies::PublishedCode<()>>, Box<[quench_runtime::value::Value]>) {
        let identities = match &self.calls {
            Some(calls) => calls.identities(),
            None => self.directs.iter().flat_map(|direct| direct.iter().map(|callee| callee.callee)).collect(),
        };
        (self.watch.clone(), identities)
    }

    /// Makes it stale where its code names shape ids, which a compaction renumbers.
    pub(crate) fn retire_named_shapes(&self) {
        if !self.facts.shape_dependencies.is_empty() { self.watch.retire(); }
    }

    /// What its compile came to, or itself while its code lowers and compiles. A Cranelift
    /// region's Stable facts and roots are its lowering's, which the watch takes here: a fact
    /// that changed while it lowered (a method cache naming an atom it stores in place) fails
    /// the subscription and the code is stale.
    /// A plan that folded Stable facts publishes where the VM still answers what it took (`TakenPlan::holds`).
    pub(crate) fn poll(mut self: Box<Self>, vm: &mut quench_runtime::vm::Vm, program: Option<&ResidualProgram>) -> Result<OsrCompile, Box<Self>> {
        let lowered = match self.code.poll() {
            std::task::Poll::Pending => return Err(self),
            std::task::Poll::Ready(_) if !self.watch.is_published() => return Ok(OsrCompile::Stale),
            std::task::Poll::Ready(Some(Some(lowered))) => lowered,
            std::task::Poll::Ready(_) => return Ok(OsrCompile::Failed),
        };
        let LoopLowered { plan, stable, roots, clif, code } = lowered;
        if self.taken.get(plan).and_then(Option::as_ref).is_some_and(|taken| !program.is_some_and(|program| taken.holds(vm, program, self.function))) {
            return Ok(OsrCompile::Stale);
        }
        let (mut facts, programs) = vm.fact_watch();
        let Ok(code) = code else { return Ok(OsrCompile::Failed) };
        let calls = match self.calls.take() {
            Some(calls) => calls,
            None => {
                let (registers, invalidation) = self.call_frame.clone();
                let direct = self.directs.get_mut(plan).map(std::mem::take).unwrap_or_default();
                let calls = if direct.is_empty() { CallCode::holding(invalidation) } else { CallCode::of((Vec::new(), direct, Vec::new()), registers, invalidation) };
                self.facts.roots = roots;
                self.facts.stable = stable;
                self.facts.calls = LaneDependencies::of(std::iter::empty(), &calls.direct);
                facts.hold_roots(&self.watch, self.facts.roots.clone());
                let watch: Rc<dyn quench_runtime::code_dependencies::DependentCode> = self.watch.clone();
                if !self.facts.calls.subscribe(&self.facts.stable, &watch, &mut facts)
                    || !programs.subscribe_root_constants(&self.facts.root_programs(), &watch) {
                    return Ok(OsrCompile::Stale);
                }
                calls
            }
        };
        let Self { header, facts, crossing, resumes, .. } = *self;
        let Some((memory, entry)) = publish_body(&code) else { return Ok(OsrCompile::Failed) };
        super::entry_cell::reserve_lane_scratch(calls.window());
        if let (Some(directory), Some(clif)) = (native_dump_directory(), clif) {
            dump_native_code(directory, DumpSite::Loop { header }, &memory, &clif);
        }
        Ok(OsrCompile::Done(CompiledOsrRegion { code: memory, entry, facts, crossing, resumes, calls, parked: std::cell::Cell::new(None), made: std::cell::Cell::new(false),
            region_calls: std::cell::OnceCell::new(), dense: std::cell::Cell::new(Vec::new()),
            stamp: std::cell::Cell::new(None) }))
    }
}


#[cfg(test)]
#[path = "osr_compile/tests.rs"]
pub(crate) mod tests;
#[cfg(test)]
#[path = "osr_compile/method_tests.rs"]
pub(crate) mod method_tests;
#[cfg(test)]
#[path = "osr_compile/math_replacement_tests.rs"]
pub(crate) mod math_replacement_tests;
