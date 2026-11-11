//! A loop site's compilations (`LoopSite::tier`): its current code, the compile under way on
//! the code generator's thread, the element views it assumes, and the values earlier code
//! found were no `i32`.

use super::*;
use super::super::compiled::{ElementViews, LoopEntry, LoopRegion};

/// A loop site's compilations: the current code, if any, the compile under way, the element
/// views both assume (those the latest entry found), and the values earlier code found were
/// no `i32`, which the next compilation holds as `f64`s.
#[derive(Default)]
pub(super) struct LoopTier {
    views: ElementViews,
    pub(super) code: Option<Rc<crate::code_dependencies::PublishedCode<super::super::compiled::CompiledOsrRegion>>>,
    /// The compile under way, and the entries left before the next look for its code
    /// (`TIERS.lane_body.compile_poll_interval`, as a lane body's).
    pending: Option<Box<super::super::compiled::PendingOsrRegion>>,
    wait: u16,
    pub(super) compilations: u32,
    /// Each operation the code left at, with its leaves so far; and those whose int32
    /// speculation the next compilation drops (`lane_region`'s `generic`), sorted.
    leaves: Vec<(u32, u16)>,
    widened: Vec<usize>,
}

impl LoopTier {
    pub(super) fn is_published(&self) -> bool {
        self.code.as_ref().is_some_and(|code| code.is_published())
    }

    /// Makes the compile under way stale where its code names shape ids, which a shape
    /// compaction renumbers.
    pub(super) fn retire_named_shapes(&self) {
        if let Some(pending) = &self.pending { pending.retire_named_shapes(); }
    }

    /// Counts a run of the code that left at `pc`. An operation or element read left `widen_after_leaves`
    /// times joins the widened ones and the code goes, where a compilation remains: the next
    /// entry compiles the loop holding that operation's result as a double.
    pub(super) fn left_at(&mut self, pc: u32) {
        if self.widened.binary_search(&(pc as usize)).is_ok() || self.compilations >= TIERS.loops.max_compilations { return; }
        let at = self.leaves.iter().position(|(left, _)| *left == pc).unwrap_or_else(|| { self.leaves.push((pc, 0)); self.leaves.len() - 1 });
        self.leaves[at].1 += 1;
        if self.leaves[at].1 < TIERS.loops.widen_after_leaves { return; }
        // Every operation seen leaving widens at once, so one recompile covers them all.
        self.widened.extend(self.leaves.drain(..).map(|(left, _)| left as usize));
        self.widened.sort_unstable();
        self.widened.dedup();
        if let Some(code) = self.code.take() { code.retire(); }
    }

    /// Whether code over the element `views` an entry found may run: code over the other
    /// views goes where a compilation remains, which the entry then makes over these.
    pub(super) fn select(&mut self, views: ElementViews) -> bool {
        if self.views == views { return true; }
        if self.compilations >= TIERS.loops.max_compilations { return false; }
        self.views = views;
        if let Some(code) = self.code.take() { code.retire(); }
        true
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

/// Starts compiling `site`'s region over the tier's element views from the running activation,
/// and looks for its code at once: a code generator without its own thread has it ready.
pub(super) fn lane_loop_compile<H: Host>(
    cx: *mut LaneContext<H>,
    r: Registers,
    site: &LoopSite,
    region: &super::super::compiled::LoopRegion,
    tier: &mut LoopTier,
    number_arrays: &[(crate::bytecode::Register, crate::typed_ir::Type)],
    int_entries: &[crate::bytecode::Register],
    typed_captures: &[u16],
) {
    tier.compilations += 1;
    super::super::compiled::fast_region::count(super::super::compiled::fast_region::Event::Compiled);
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
    let ints = region.registers_where(|register| r.get(register).as_int().is_some());
    let doubles = region.registers_where(|register| {
        let value = r.get(register);
        value.as_number().is_some() && value.as_int().is_none()
    });
    let observation = super::super::compiled::Observation { native_field_loads: &native_field_loads, number_arrays, int_entries, typed_captures,
        int_registers: &ints, double_registers: &doubles };
    let policy = machine.heap.memory_policy();
    let started = program.as_deref().and_then(|program| super::super::compiled::start_osr_region(region, tier.views,
        &observation, policy, program, function, helpers,
        &mut |unit| machine.lane_global_reads(program, function, unit), &tier.widened));
    machine.host.after_native_compilation();
    tier.start(started, &mut machine.heap, site);
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

/// The depth-zero capture slots the running function reads that hold typed arrays now (an observation
/// the compiled loop's entry checks): a loop indexing such a capture views it as a typed array.
pub(super) fn typed_capture_slots<H: Host>(cx: *mut LaneContext<H>) -> Vec<u16> {
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    let Some((_, function, _, _)) = loop_tier::lane_loop_function(cx, code) else { return Vec::new() };
    let mut slots: Vec<u16> = function.instructions()
        .filter(|instruction| instruction.op().is_load_capture() && instruction.capture_depth() == 0)
        .map(|instruction| crate::bytecode::ImmediateLayout::capture_slot(instruction.imm())).collect();
    slots.sort_unstable();
    slots.dedup();
    slots.retain(|slot| super::lane_read_capture_value::<H, true>(cx, 0, *slot).0
        .is_some_and(|value| unsafe { vm(cx) }.lane_typed_array_elements(value, false).is_some()));
    slots
}

/// How many elements of an entry array are sampled for Numbers.
const SAMPLED_ELEMENTS: usize = 64;

/// The registers of `region`'s arrays whose first elements, sampled on entry, are all int32s or all
/// Numbers (int32 or double, no hole), with that type: an observation (a guess the code checks at
/// every read), not a proof.
pub(super) fn number_arrays<H: Host>(vm: &Vm<H>, r: Registers, region: &LoopRegion) -> Vec<(Register, crate::typed_ir::Type)> {
    use crate::typed_ir::Type;
    region.arrays.iter().filter(|array| array.field.is_none()).filter_map(|array| {
        let elements = vm.heap.record_array(r.get(array.register))?.elements().as_slice();
        let sample = &elements[..elements.len().min(SAMPLED_ELEMENTS)];
        if sample.is_empty() { return None; }
        if sample.iter().all(|value| value.as_int().is_some()) { Some((array.register, Type::INT32)) }
        else { sample.iter().all(|value| value.as_number().is_some()).then_some((array.register, Type::NUMBER)) }
    }).collect()
}

/// The registers of the running activation that hold int32s at entry and that the loop writes:
/// an observation the compiled code's entry contract checks, as `number_arrays`'.
pub(super) fn int_entries(r: Registers, region: &LoopRegion) -> Vec<Register> {
    region.entry_registers().filter(|register| r.get(*register).as_int().is_some()).collect()
}
