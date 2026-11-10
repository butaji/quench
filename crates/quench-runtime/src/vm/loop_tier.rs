//! A loop site's region tier (`LoopSite::tier`): the `Tier` that counts its backward jumps,
//! compiles its region over the element views its entry found, and judges the runs of that code.

use super::*;
use super::super::compiled::{CompiledOsrRegion, ElementViews, LoopEntry, LoopRegion, OsrCompile, PendingOsrRegion};
use super::super::tier_state::{Build, Controller, Event, Past, Polled, Refusal, Tier, Verdict};
use super::super::compiled::tier_policy::Tiers;
use crate::code_dependencies::PublishedCode;

/// The region tier's controller. A speculative body holds the operations it computes as `i32`s
/// and the element views of the entry it compiled for; the generic body holds every operation
/// as a double.
pub(super) struct RegionController;

/// The operations whose `i32` speculation the bodies so far dropped, sorted: the next body
/// holds their results as `f64`s.
#[derive(Clone, Default)]
pub(super) struct Widened(Vec<usize>);

/// How a region body's runs went: the entries not yet paid back, and each operation the code
/// left at with its leaves so far.
#[derive(Default)]
pub(super) struct RegionRuns {
    debt: u32,
    leaves: Vec<(u32, u16)>,
}

/// A region compiling on the code generator's thread, for the element views its entry found.
pub(super) struct RegionBuild {
    region: Box<PendingOsrRegion>,
    views: ElementViews,
}

/// A published region and the element views it assumes. A dropped body is retired, so the
/// facts it subscribed to stop naming it.
pub(super) struct RegionCode {
    publication: Rc<PublishedCode<CompiledOsrRegion>>,
    views: ElementViews,
}

impl Drop for RegionCode {
    fn drop(&mut self) {
        self.publication.retire();
    }
}

/// What a run of a region body did.
#[derive(Clone, Copy)]
pub(super) enum RegionLeave {
    /// The native code ran and took `backedges` backward jumps before it left.
    Ran { backedges: u64 },
    /// The run left at `pc`, a Number operator whose `i32` speculation a body can drop.
    Widen { pc: u32 },
}

/// Why the site's native runs are not paid for by their iterations.
const UNPAID_ENTRIES: &str = "native-runs-do-not-pay";
/// Why the site has no region to compile.
pub(super) const NO_REGION: &str = "no-region";

impl Controller for RegionController {
    type Pending = RegionBuild;
    type Artifact = RegionCode;
    type Feedback = Widened;
    type Stats = RegionRuns;
    type Leave = RegionLeave;

    /// The backward jumps the interpreter takes before the first body compiles; later bodies
    /// compile at the site's next entry.
    fn warmup(policy: &Tiers, past: &Past<Widened>) -> u32 {
        if past.bodies == 0 { policy.loops.threshold } else { 0 }
    }

    /// The warm-up is over once more jumps than it asks for have been taken.
    #[inline(always)]
    fn ready(policy: &Tiers, entries: u32, _: u32, past: &Past<Widened>) -> bool {
        entries > Self::warmup(policy, past)
    }

    /// A run pays for its entry with the iterations it completed, wherever it leaves: a run
    /// that took none counts toward `max_exits`. An operation left `widen_after_leaves` times
    /// joins the widened ones and the body goes.
    fn judge(policy: &Tiers, stats: &mut RegionRuns, past: &Past<Widened>, at: RegionLeave) -> Verdict<Widened> {
        match at {
            RegionLeave::Ran { backedges } => {
                stats.debt = match u32::try_from(backedges) {
                    Ok(0) => stats.debt.saturating_add(1),
                    Ok(paid) => stats.debt.saturating_sub(paid),
                    Err(_) => 0,
                };
                if stats.debt < policy.loops.max_exits { Verdict::Keep } else { Verdict::Interpret(Refusal::Shape(UNPAID_ENTRIES)) }
            }
            RegionLeave::Widen { pc } => stats.left_at(policy, pc, &past.feedback),
        }
    }
}

impl RegionRuns {
    /// Counts a run that left at `pc`: the feedback of the body that drops that operation's
    /// `i32` speculation once it has left there often enough.
    fn left_at(&mut self, policy: &Tiers, pc: u32, widened: &Widened) -> Verdict<Widened> {
        if widened.0.binary_search(&(pc as usize)).is_ok() { return Verdict::Keep; }
        let at = self.leaves.iter().position(|(left, _)| *left == pc).unwrap_or_else(|| { self.leaves.push((pc, 0)); self.leaves.len() - 1 });
        self.leaves[at].1 += 1;
        if self.leaves[at].1 < policy.loops.widen_after_leaves { return Verdict::Keep; }
        self.leaves.swap_remove(at);
        let mut next = widened.0.clone();
        next.push(pc as usize);
        next.sort_unstable();
        Verdict::Respecialize(Widened(next))
    }
}

/// A loop site's region tier.
pub(super) type RegionTier = Tier<RegionController>;

impl RegionTier {
    pub(super) fn is_published(&self) -> bool {
        self.artifact().is_some_and(|code| code.publication.is_published())
    }

    /// The published code's publication, which the tests watch retire.
    #[cfg(all(test, feature = "aot"))]
    pub(super) fn publication(&self) -> Option<Rc<PublishedCode<CompiledOsrRegion>>> {
        self.artifact().map(|code| code.publication.clone())
    }

    /// Whether the site's backward jumps have all been taken in the interpreter that its
    /// warm-up asks for.
    pub(super) fn reached(&self) -> bool {
        match self {
            Self::Cold => false,
            Self::Warming { entries, .. } => *entries >= TIERS.loops.threshold,
            _ => true,
        }
    }

    /// Whether the site has native code or may still compile it. A site whose compiles ran
    /// out or whose entries do not pay, or under a policy that compiles no loop regions, takes
    /// its backward jump in the interpreter without reading an entry: reading one at every
    /// iteration had cost V8-v7 Crypto `--jit` a third of its instructions under memory-first.
    pub(super) fn may_run(&self, policy: crate::MemoryPolicy) -> bool {
        self.is_published() || (!self.is_interpreted() && super::super::compiled::loop_tier_compiles(policy))
    }

    /// Makes the compile under way stale where its code names shape ids, which a shape
    /// compaction renumbers, and retires the published code that names them.
    pub(super) fn retire_named_shapes(&self) {
        if let Self::Compiling { build, .. } = self {
            build.pending().region.retire_named_shapes();
        }
        if let Some(code) = self.artifact()
            && code.publication.lease().is_some_and(|region| !region.facts.shape_dependencies.is_empty()) {
            code.publication.retire();
        }
    }

    /// Whether code over the element `views` an entry found may run: a body over other views
    /// or one a fact retired goes, and the entry compiles the next over these; the generic
    /// body keeps its views, and an entry over others is declined.
    fn admits(&mut self, views: ElementViews) -> bool {
        let Some(code) = self.artifact() else { return true };
        let live = code.publication.is_published();
        if live && code.views == views { return true; }
        if live && matches!(self, Self::Generic { .. }) { return false; }
        self.apply(Event::INVALIDATED, &TIERS);
        true
    }

    /// Whether this entry passes to the interpreter's jump while the code compiles: entries
    /// between looks count down, and a look publishes code that is ready.
    pub(super) fn awaits_code(&mut self, heap: &mut crate::heap::Heap, site: &LoopSite) -> bool {
        if !self.is_compiling() { return false; }
        self.poll_build(TIERS.lane_body.compile_poll_interval, |build| look(heap, *build));
        if self.is_compiling() { return true; }
        if site.diagnostics && self.is_published() {
            site.compile_successes.set(site.compile_successes.get().saturating_add(1));
        }
        false
    }

    /// The operations the next body holds as `f64`s: those the bodies so far widened, and,
    /// where the speculative bodies are spent, every operation of the loop.
    fn widened(&self, region: &LoopRegion) -> Vec<usize> {
        match self.past() {
            Some(past) if past.is_spent() => (region.header as usize..=region.backedge as usize).collect(),
            Some(past) => past.feedback.0.clone(),
            None => Vec::new(),
        }
    }
}

/// A look at the compile `build`: its code, published while the facts it names hold.
fn look(heap: &mut crate::heap::Heap, build: RegionBuild) -> Polled<RegionBuild, RegionCode> {
    let RegionBuild { region, views } = build;
    match region.poll() {
        Err(region) => Polled::Pending(Box::new(RegionBuild { region, views })),
        Ok(OsrCompile::Done(compiled)) => publish(heap, compiled, views).map_or(Polled::Stale, Polled::Ready),
        Ok(OsrCompile::Stale) => Polled::Stale,
        Ok(OsrCompile::Failed) => Polled::Failed,
    }
}

/// Publishes `region` while the facts its code names hold.
fn publish(heap: &mut crate::heap::Heap, region: CompiledOsrRegion, views: ElementViews) -> Option<RegionCode> {
    let publication = Rc::new(PublishedCode::new(region));
    let code = publication.lease().expect("new publication");
    let dependent: Rc<dyn crate::code_dependencies::DependentCode> = publication.clone();
    let held = code.facts.subscribe(&dependent, heap);
    let code = RegionCode { publication, views };
    // A dropped code retires its publication.
    held.then_some(code)
}

/// `site`'s region code for the element `views` its entry found, compiled now if the tier has
/// none and may; or what the lane does instead.
pub(super) fn lane_loop_code<H: Host>(cx: *mut LaneContext<H>, site: &LoopSite, region: &LoopRegion, views: ElementViews)
    -> Result<Rc<CompiledOsrRegion>, LoopContinuation> {
    site.region(|tier| {
        if !tier.admits(views) {
            return Err(LoopContinuation::Jump);
        }
        if tier.artifact().is_none() {
            // SAFETY: the lane's VM is live, and nothing borrows it across the read.
            if !tier.may_run(unsafe { vm(cx) }.heap.memory_policy()) {
                return Err(LoopContinuation::Jump);
            }
            lane_loop_compile(cx, site, region, views, tier);
        }
        tier.artifact().and_then(|code| code.publication.lease()).ok_or(LoopContinuation::Compiling)
    })
}

/// Starts compiling `site`'s region over the element `views` from the running activation,
/// and looks for its code at once: a code generator without its own thread has it ready.
fn lane_loop_compile<H: Host>(
    cx: *mut LaneContext<H>,
    site: &LoopSite,
    region: &LoopRegion,
    views: ElementViews,
    tier: &mut RegionTier,
) {
    if site.diagnostics {
        site.compile_attempts.set(site.compile_attempts.get().saturating_add(1));
    }
    // SAFETY: the lane's VM is live, and nothing borrows it across the call.
    let machine = unsafe { vm(cx) };
    let program = machine.programs.get(machine.active_program);
    let function = site.function();
    let helpers = super::super::compiled::process_lane_helpers::<H>();
    machine.host.before_native_compilation();
    let native_field_loads = native_field_loads(machine, region);
    let observation = super::super::compiled::Observation { native_field_loads: &native_field_loads };
    let policy = machine.heap.memory_policy();
    let widened = tier.widened(region);
    let started = program.as_deref().and_then(|program| super::super::compiled::start_osr_region(region, views,
        &observation, policy, program, function, helpers,
        &mut |unit| machine.lane_global_reads(program, function, unit), &widened));
    machine.host.after_native_compilation();
    // The code names facts that may have changed while it was planned.
    match started.filter(|pending| pending.watch(&mut machine.heap)) {
        Some(pending) => tier.apply(Event::Started(Build::new(Box::new(RegionBuild { region: Box::new(pending), views }), 0)), &TIERS),
        None => tier.apply(Event::Failed(Refusal::Codegen), &TIERS),
    };
    tier.awaits_code(&mut machine.heap, site);
}

/// The descriptors of the dynamic field reads of `region` native code makes itself.
fn native_field_loads<H: Host>(machine: &Vm<H>, region: &super::super::compiled::LoopRegion)
    -> Vec<Vec<super::super::compiled::NativeFieldLoad>> {
    let (properties_base, properties_len) = machine.heap.native_property_values();
    let available = machine.heap.object_record_base().is_some() && !properties_base.is_null() && properties_len > 0;
    region.fields.iter().enumerate()
        .map(|(index, field)| if available && region.inlining.is_none() && region.is_dynamic_field(index) {
            machine.native_field_load_descriptors(field.cache_site)
        } else {
            Default::default()
        })
        .collect()
}

/// The element views of `region`'s arrays on entry (`ElementViews`), each pushed to `entry`:
/// every one a record array, each written one a dense array whose elements a store replaces
/// in place (`Vm::try_replace_record_element`), no prototype claiming an index of it; or every
/// one a typed array a register holds whose elements the loop may address in place
/// (`Vm::lane_typed_array_elements`), which its code views at the loop's header. A mix, or
/// any other value, declines the entry.
pub(super) fn lane_loop_arrays<H: Host>(vm: &mut Vm<H>, r: Registers, region: &LoopRegion, entry: &mut LoopEntry)
    -> Option<ElementViews> {
    let mut views = None;
    for array in &region.arrays {
        let value = match array.field {
            None => r.get(array.register),
            Some(index) => entry.field_value(region, index),
        };
        let found = if vm.heap.record_array(value).is_some() {
            push_dense_array(vm, value, array.written, entry)?;
            ElementViews::Dense
        } else {
            // A typed array read through a field is no loop's hoisted view.
            array.field.is_none().then_some(())?;
            let (elements, length, _) = vm.lane_typed_array_elements(value, array.written)?;
            entry.push_array(elements, length, length)?;
            ElementViews::Typed
        };
        if *views.get_or_insert(found) != found { return None; }
    }
    Some(views.unwrap_or_default())
}

/// Pushes the record array `value`'s elements; a `written` one must be dense, its backing
/// owned alone, and no prototype may claim an index of it: a store below the length or an
/// append at it then defines or replaces an own data element and nothing else
/// (`append_dense_element`, `fill_dense_hole`).
fn push_dense_array<H: Host>(vm: &mut Vm<H>, value: Value, written: bool, entry: &mut LoopEntry) -> Option<()> {
    if !written {
        let elements = vm.heap.record_array(value)?.elements().as_slice();
        return entry.push_array(elements.as_ptr().cast_mut(), elements.len(), elements.len());
    }
    if vm.dense_array_length(value).is_none() || !vm.prototypes_hold_no_index(value) {
        return None;
    }
    // A Number store logs nothing.
    let elements = vm.heap.record_array_store_mut_unlogged(value, Value::integer(0))?.elements_mut();
    if elements.owner_count() != 1 {
        return None;
    }
    let elements = super::super::index::mutable_array_elements(elements);
    entry.push_array(elements.as_mut_ptr(), elements.len(), elements.capacity())
}

/// The running activation's function in `code`, the context's program: its index, body and
/// decoded code, and the program's constants, from which a loop site's region is analyzed.
pub(super) fn lane_loop_function<H: Host>(cx: *mut LaneContext<H>, code: &ResidualProgram)
    -> Option<(u32, &crate::bytecode::Function, Vec<WideInstruction>, Vec<Value>)> {
    let machine = unsafe { vm(cx) };
    let function_index = machine
        .lane_stack
        .activations
        .last()
        .map_or(machine.frames[unsafe { (*cx).frame }].function, |activation| activation.function);
    let function = code.function_get(function_index as usize)?;
    let constants = machine.programs.constants(unsafe { (*cx).program }).to_vec();
    Some((function_index, function, function.instructions().collect(), constants))
}

/// The plain region of `site`'s loop, which leaves at each method call the inliner declined
/// (`MethodCalls::Exit`).
#[inline(never)]
pub(super) fn lane_method_call_region<H: Host>(cx: *mut LaneContext<H>, site: &LoopSite) -> Option<LoopRegion> {
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    let (function_index, function, decoded, constants) = lane_loop_function(cx, code)?;
    super::super::compiled::analyze_osr_region_detailed(function, &decoded, &constants, &code.field_sites,
        code.shared_register_root_masks(function_index), site.header, site.backedge,
        super::super::compiled::MethodCalls::Exit).ok()
}
