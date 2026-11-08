//! A loop site's compilations (`LoopSite::tier`): its current code, the compile under way on
//! the code generator's thread, and the values earlier code found were no `i32`.

use super::*;

/// A loop site's compilations: the current code, if any, the compile under way, and the
/// values earlier code found were no `i32`, which the next compilation holds as `f64`s.
#[derive(Default)]
pub(super) struct LoopTier {
    pub(super) code: Option<Rc<crate::code_dependencies::PublishedCode<super::super::compiled::CompiledOsrRegion>>>,
    /// The compile under way, and the entries left before the next look for its code
    /// (`TIERS.lane_body.compile_poll_interval`, as a lane body's).
    pending: Option<Box<super::super::compiled::PendingOsrRegion>>,
    wait: u16,
    pub(super) widened: std::collections::BTreeSet<super::super::compiled::Widening>,
    pub(super) compilations: u32,
}

impl LoopTier {
    pub(super) fn is_published(&self) -> bool {
        self.code.as_ref().is_some_and(|code| code.is_published())
    }

    /// Whether the site has native code or may still compile it. A site whose compilations
    /// ran out, or under a policy that compiles no loop regions, takes its backward jump in
    /// the interpreter without reading an entry: reading one at every iteration had cost
    /// V8-v7 Crypto `--jit` a third of its instructions under memory-first.
    pub(super) fn may_run(&self, policy: crate::MemoryPolicy) -> bool {
        self.is_published()
            || (self.compilations < TIERS.loops.max_compilations && super::super::compiled::loop_tier_compiles(policy))
    }

    /// Whether this entry passes to the interpreter's jump while the code compiles: entries
    /// between looks count down, and a look publishes code that is ready.
    pub(super) fn awaits_code(&mut self, heap: &mut crate::heap::Heap, site: &LoopSite) -> bool {
        let Some(pending) = self.pending.take() else { return false };
        if self.wait > 0 {
            self.wait -= 1;
            self.pending = Some(pending);
            return true;
        }
        use super::super::compiled::OsrCompile;
        match pending.poll() {
            Err(pending) => {
                self.wait = TIERS.lane_body.compile_poll_interval;
                self.pending = Some(pending);
                return true;
            }
            Ok(OsrCompile::Done(region)) => self.publish(heap, region),
            Ok(OsrCompile::Stale) => {}
            Ok(OsrCompile::Failed) => self.compilations = TIERS.loops.max_compilations,
        }
        if site.diagnostics && self.is_published() {
            site.compile_successes.set(site.compile_successes.get().saturating_add(1));
        }
        false
    }

    /// Holds the compile `started`, while the facts its code names hold, and looks for its
    /// code at once.
    fn start(&mut self, started: Option<super::super::compiled::PendingOsrRegion>, heap: &mut crate::heap::Heap, site: &LoopSite) {
        self.pending = started.filter(|pending| pending.watch(heap)).map(Box::new);
        self.wait = 0;
        if self.pending.is_none() {
            self.compilations = TIERS.loops.max_compilations;
        }
        self.awaits_code(heap, site);
    }

    /// Publishes `region` while the facts its code names hold.
    fn publish(&mut self, heap: &mut crate::heap::Heap, region: super::super::compiled::CompiledOsrRegion) {
        let publication = Rc::new(crate::code_dependencies::PublishedCode::new(region));
        let code = publication.lease().expect("new publication");
        let dependent: Rc<dyn crate::code_dependencies::DependentCode> = publication.clone();
        if !code.facts.subscribe(&dependent, heap) {
            publication.retire();
        }
        self.code = Some(publication);
    }
}

/// Starts compiling `site`'s region from the running activation (`r`, `entry`), and looks
/// for its code at once: a code generator without its own thread has it ready.
pub(super) fn lane_loop_compile<H: Host>(
    cx: *mut LaneContext<H>,
    r: Registers,
    site: &LoopSite,
    region: &super::super::compiled::LoopRegion,
    entry: &mut super::super::compiled::LoopEntry,
    tier: &mut LoopTier,
) {
    tier.compilations += 1;
    if site.diagnostics {
        site.compile_attempts.set(site.compile_attempts.get().saturating_add(1));
    }
    // SAFETY: the lane's VM is live, and nothing borrows it across the call.
    let machine = unsafe { vm(cx) };
    machine.host.before_native_compilation();
    let track_backedges = machine.programs.js_jit_general_dispatch_enabled();
    let started = observe(machine, r, region, entry, &tier.widened, |observation| {
        super::super::compiled::start_osr_region(region, observation, &machine.execution_interrupt, machine.heap.memory_policy(), track_backedges)
    });
    machine.host.after_native_compilation();
    tier.start(started, &mut machine.heap, site);
}

/// Runs `compile` on what the running activation (`r`, `entry`) shows `region`'s values to be.
fn observe<H: Host, T>(
    machine: &Vm<H>,
    r: Registers,
    region: &super::super::compiled::LoopRegion,
    entry: &super::super::compiled::LoopEntry,
    widened: &std::collections::BTreeSet<super::super::compiled::Widening>,
    compile: impl FnOnce(&super::super::compiled::Observation<'_>) -> T,
) -> T {
    let captures = entry.captures(region.captures.len());
    let fields: Vec<Value> = (0..region.fields.len()).map(|index| entry.field_value(region, index)).collect();
    let native_field_loads = native_field_loads(machine, region);
    let heap = &machine.heap;
    // A field's array is the value its entry word holds, not yet the register's.
    let array_value = |register| match region.arrays.iter().find(|array| array.register == register) {
        Some(super::super::compiled::ArrayUse { field: Some(index), .. }) => entry.field_value(region, *index),
        _ => r.get(register),
    };
    let int32_elements = |register| {
        heap.record_array(array_value(register)).is_some_and(|array| {
            array.elements().iter().take(TIERS.loops.element_sample).all(|value| value.as_int().is_some())
        })
    };
    let only_int32 = |register| heap.record_array(array_value(register)).is_some_and(|array| array.elements().holds_only_int32());
    // An inlined callee's register is no register of the running activation: it holds
    // nothing at entry.
    let caller_registers = region.inlining.as_ref().map_or(Register::MAX, |inlining| inlining.caller_registers);
    compile(&super::super::compiled::Observation {
        register: Some(&|register| if register < caller_registers { r.get(register) } else { Value::UNDEFINED }),
        int32_elements: &int32_elements,
        only_int32: &only_int32,
        captures: &captures,
        fields: &fields,
        native_field_loads: &native_field_loads,
        widened,
    })
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
