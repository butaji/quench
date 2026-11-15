mod collection_boundary;
mod roots;
use roots::{append_counted_shape_edges, append_cycle_shape_edges, zeroed_mark_words, GatheredRoots};
use super::{module::ModuleRecord, promise::PromiseState};
use super::*;
use smallvec::SmallVec;
use std::sync::atomic::{AtomicU64, Ordering};

// RayTrace's allocation trace shows about 40–43% of its shape table is
// unreachable after each collection. Compact only once at least half the
// table is dead, amortizing the rebuild and cache invalidation across GCs.
const SHAPE_COMPACTION_MIN_RECLAIM_DENOMINATOR: usize = 2;

/// The closure reads of the VM's programs, which a full collection traces closure
/// environments through (`heap::EnvironmentTrace`).
struct ProgramClosureReads<'a> {
    programs: &'a ProgramStore,
    lexical_this: Atom,
}

impl crate::heap::ClosureReadSource for ProgramClosureReads<'_> {
    fn closure_reads(&self, program: u32) -> Option<&crate::bytecode::ClosureReads> {
        self.programs
            .residual(ProgramId::from_raw(program))?
            .closure_reads
            .as_deref()
    }

    fn lexical_this(&self) -> Atom {
        self.lexical_this
    }

    fn projection_function(&self) -> u32 {
        ROOT_FUNCTION_ID
    }

    fn slot_is_projection(&self, program: u32, function: u32, slot: u16) -> bool {
        // Global lexical slots count as bindings, whatever script is being evaluated.
        self.may_hold_projections(function)
            && !self.programs.root_slot_is_binding(ProgramId::from_raw(program), slot, true)
    }
}

/// Pooled frames a full collection keeps (`Vm::frame_pool`): a depth ordinary call chains
/// reach between collections, each frame bounded by `RETAINED_FRAME_VALUES` per buffer.
const POOLED_FRAMES_KEPT_BY_FULL_COLLECTIONS: usize = 64;

fn shape_reclaim_threshold_met(shape_count: usize, unreachable: usize) -> bool {
    unreachable >= shape_count.div_ceil(SHAPE_COMPACTION_MIN_RECLAIM_DENOMINATOR)
}

pub(super) fn active_shape_attributes(
    shapes: &[Shape],
    shape: u32,
) -> Vec<Option<(property_key::PropertyKey, ShapeAttributes)>> {
    shape_lookup::owned_slots(shapes, shape).unwrap_or_else(|| chain_shape_attributes(shapes, shape))
}

fn chain_shape_attributes(
    shapes: &[Shape],
    shape: u32,
) -> Vec<Option<(property_key::PropertyKey, ShapeAttributes)>> {
    // Both vectors are sized exactly: collections call this for every rooted shape.
    let ancestors = || std::iter::successors(Some(shape), |&id| shapes[id as usize].parent());
    let mut chain = Vec::with_capacity(ancestors().count());
    chain.extend(ancestors());
    chain.reverse();

    let mut entries = Vec::with_capacity(shapes[shape as usize].storage_len());
    for id in chain {
        match shapes[id as usize].transition {
            ShapeTransition::Add {
                key, attributes, ..
            } => {
                entries.push(Some((key, attributes.shape())));
            }
            ShapeTransition::Delete { slot, .. } => {
                if let Some(entry) = entries.get_mut(slot as usize) {
                    *entry = None;
                }
            }
            ShapeTransition::Descriptor { slot, attributes } => {
                if let Some(Some((_, current))) = entries.get_mut(slot as usize) {
                    *current = attributes;
                }
            }
            ShapeTransition::Vacant => entries.push(None),
            ShapeTransition::Root | ShapeTransition::FunctionName { .. } | ShapeTransition::FunctionNameReconfigured | ShapeTransition::FunctionLengthReconfigured | ShapeTransition::Constructed { .. } | ShapeTransition::Dictionary { .. } => {}
        }
    }
    entries
}

fn append_shape_roots(
    shapes: &[Shape],
    accessors: &[ShapeAccessors],
    shape: u32,
    roots: &mut Vec<Value>,
) {
    roots.extend(
        active_shape_attributes(shapes, shape)
            .into_iter()
            .flatten()
            .flat_map(|(key, attributes)| {
                // Slot-held accessors (a getter, a pair cell) are traced with their object's slots.
                let (getter, setter) = attributes.functions(accessors, || (None, None));
                key.gc_value()
                    .into_iter()
                    .chain([getter, setter].into_iter().flatten())
            }),
    );
}

/// Parallel GC readers need only the immutable shape transition metadata.
/// `Shape` also contains `Cell` and `OnceCell` caches, which this view never
/// reads or mutates while workers are active.
/// Edges kept outside their owner's cell (descriptors, promise job and continuation tables),
/// indexed by owner for the collection that derived them. Owners hold one to three such
/// edges, so the first two stay inline.
type OwnedRoots = FxHashMap<Value, SmallVec<[Value; OWNED_ROOTS_INLINE]>>;

/// Filter bits per owned-roots owner: a marked object probes the map only when its bit is
/// set, so the hundreds of owners cost the millions of marked objects one bit test each.
const OWNED_ROOT_FILTER_BITS_PER_OWNER: usize = 16;

/// Which owners `OwnedRoots` may hold: one bit per owner, by a hash of its identity.
struct OwnedRootFilter {
    words: Vec<u64>,
    /// `words.len() * 64 - 1`; the bit count is a power of two.
    mask: usize,
}

impl OwnedRootFilter {
    fn new(roots: &OwnedRoots) -> Self {
        let bits = (roots.len() * OWNED_ROOT_FILTER_BITS_PER_OWNER)
            .next_power_of_two()
            .max(u64::BITS as usize);
        let mut filter = Self {
            words: vec![0; bits / u64::BITS as usize],
            mask: bits - 1,
        };
        for owner in roots.keys() {
            let bit = filter.bit(*owner);
            filter.words[bit / u64::BITS as usize] |= 1 << (bit % u64::BITS as usize);
        }
        filter
    }

    #[inline(always)]
    fn bit(&self, owner: Value) -> usize {
        use std::hash::{Hash, Hasher};
        let mut hasher = rustc_hash::FxHasher::default();
        owner.hash(&mut hasher);
        // The high bits of the multiplicative hash spread consecutive heap indices.
        (hasher.finish() >> 32) as usize & self.mask
    }

    #[inline(always)]
    fn may_hold(&self, owner: Value) -> bool {
        let bit = self.bit(owner);
        self.words[bit / u64::BITS as usize] & (1 << (bit % u64::BITS as usize)) != 0
    }
}
const OWNED_ROOTS_INLINE: usize = 2;

mod shape_append;

struct ParallelShapeTrace<'a> {
    shapes: *const Shape,
    shape_count: usize,
    accessors: &'a [ShapeAccessors],
    marked_shapes: &'a [AtomicU64],
    marked_constructor_shapes: &'a [AtomicU64],
    constructors: &'a [Value],
    owned_roots: &'a OwnedRoots,
    owned_root_filter: &'a OwnedRootFilter,
    _shape_borrow: std::marker::PhantomData<&'a [Shape]>,
}

// SAFETY: the shape vector is immutable for the duration of collection, and
// `append_shape_roots` reads only `parent`, `transition` and an owned dictionary's
// already-initialized lookup index, never initializing or mutating a cache. All shared bitmap updates are atomic; owned roots
// are read-only. The trace object cannot outlive the joined worker scope.
unsafe impl Send for ParallelShapeTrace<'_> {}
// SAFETY: same immutable-metadata contract as the Send implementation.
unsafe impl Sync for ParallelShapeTrace<'_> {}

impl ParallelShapeTrace<'_> {
    /// The mark callback sees every reachable object's shape, including shapes
    /// without symbol or accessor roots. Sweep needs no per-object shape list.
    /// Read the bitmap only after the mark workers have joined.
    fn live_shapes(&self) -> Vec<u32> {
        let mut shapes = Vec::new();
        for (word_index, word) in self.marked_shapes.iter().enumerate() {
            let mut marked = word.load(Ordering::Relaxed);
            while marked != 0 {
                let bit = marked.trailing_zeros() as usize;
                shapes.push((word_index * u64::BITS as usize + bit) as u32);
                marked &= marked - 1;
            }
        }
        shapes
    }

    /// Whether this call first sets `mask` in `bits[word]`; always when verifying.
    fn first_to_set(bits: &[AtomicU64], word: usize, mask: u64, verify: bool) -> bool {
        verify || (bits[word].load(Ordering::Relaxed) & mask == 0 && bits[word].fetch_or(mask, Ordering::Relaxed) & mask == 0)
    }

    /// A shape's first-reached edges: symbol and accessor keys, and its base constructor.
    #[inline(never)]
    fn append_shape_edges(&self, trace_kind: GcTraceKind, shape: u32, prototype: bool, roots: &mut Vec<Value>) {
        let (index, verify, mask) = (shape as usize, trace_kind == GcTraceKind::VerifyFull, 1_u64 << (shape % u64::BITS));
        let word = index / u64::BITS as usize;
        // SAFETY: the shape table is borrowed immutably until all workers join.
        let shapes = unsafe { std::slice::from_raw_parts(self.shapes, self.shape_count) };
        if Self::first_to_set(self.marked_shapes, word, mask, verify) && shapes[index].facts.may_have_gc_roots() {
            append_shape_roots(shapes, self.accessors, shape, roots);
        }
        // A prototype no longer owns its base constructor; its trace bit stays separate.
        let constructor = self.constructors[index];
        if !prototype && Self::first_to_set(self.marked_constructor_shapes, word, mask, verify) && !constructor.is_undefined() {
            roots.push(constructor);
        }
    }
}

fn append_compacted_shape(
    shapes: &mut Vec<Shape>,
    transitions: &mut super::shape_transitions::ShapeTransitionIndex,
    parent: u32,
    transition: ShapeTransition,
    storage_len: usize,
) -> u32 {
    let cache_key = transition.cache_key();
    if let Some(key) = cache_key
        && let Some(shape) = transitions.get(shapes, parent, key)
    {
        return shape;
    }
    let shape = u32::try_from(shapes.len()).expect("object shape table exhausted");
    let dictionary_trigger = match transition {
        ShapeTransition::Dictionary { trigger } => Some(trigger),
        _ => shapes[parent as usize].dictionary_trigger,
    };
    let parent_facts = shapes[parent as usize].facts;
    shapes.push(Shape::child(
        Some(parent),
        transition,
        storage_len,
        dictionary_trigger,
        parent_facts,
    ));
    if let Some(key) = cache_key {
        transitions.insert(shapes, parent, key, shape);
    }
    shape
}

/// Rebuilds `old_shape`'s live chain into `shapes`, recording its accessor functions anew in
/// `accessors`, so compaction drops the dead shapes' accessors too.
fn rebuild_shape(
    old_shapes: &[Shape],
    old_accessors: &[ShapeAccessors],
    old_shape: u32,
    shapes: &mut Vec<Shape>,
    accessors: &mut Vec<ShapeAccessors>,
    transitions: &mut super::shape_transitions::ShapeTransitionIndex,
) -> u32 {
    let mut parent = 0;
    for transition in provenance::metadata_transitions(old_shapes, old_shape) {
        parent = append_compacted_shape(shapes, transitions, parent, transition, 0);
    }
    if let Some(shape) = shape_lookup::rebuild_owned_dictionary(old_shapes, old_accessors, old_shape, shapes, accessors, parent) { return shape; }
    for (slot, entry) in active_shape_attributes(old_shapes, old_shape).into_iter().enumerate() {
        let slot = u32::try_from(slot).expect("object property index exceeds u32");
        let storage_len = slot as usize + 1;
        // Data attributes ride on the add; an accessor's take a descriptor after it.
        let data = entry.and_then(|(_, attributes)| attributes.data());
        let transition = match entry {
            Some((key, _)) => ShapeTransition::Add {
                key,
                slot,
                attributes: data.unwrap_or(DEFAULT_DATA_ATTRIBUTES),
            },
            None => ShapeTransition::Vacant,
        };
        parent = append_compacted_shape(shapes, transitions, parent, transition, storage_len);
        if let Some((_, attributes)) = entry
            && data.is_none()
        {
            let attributes = attributes.rehomed(old_accessors, accessors);
            parent = append_compacted_shape(
                shapes,
                transitions,
                parent,
                ShapeTransition::Descriptor { slot, attributes },
                storage_len,
            );
        }
    }
    if let Some(trigger) = old_shapes[old_shape as usize].dictionary_trigger {
        parent = append_compacted_shape(
            shapes,
            transitions,
            parent,
            ShapeTransition::Dictionary { trigger },
            old_shapes[old_shape as usize].storage_len(),
        );
    }
    parent
}

impl<H: Host> Vm<H> {
    pub(crate) fn set_memory_policy(&mut self, policy: crate::MemoryPolicy) {
        self.heap.set_memory_policy(policy);
    }

    pub(crate) fn memory_policy(&self) -> crate::MemoryPolicy {
        self.heap.memory_policy()
    }

    /// Run a named collection safepoint even when the allocation threshold has
    /// not been reached. Hosts and focused conformance tests use this boundary
    /// to validate root ownership and weak/finalization ordering without
    /// reaching into heap internals.
    pub(crate) fn collect_now(&mut self, program: &ResidualProgram) {
        self.heap.finish_cycle_now();
        self.collect_slow(program, true);
    }

    #[inline(always)]
    pub(crate) fn maybe_collect(&mut self, program: &ResidualProgram) {
        if !self.heap.should_collect() {
            return;
        }
        self.collect_due(program);
    }

    /// An allocation threshold passed: a running concurrent cycle's poll, which remarks once
    /// its marking is done, else a collection, or a cycle's start when its gates pass.
    #[cold]
    #[inline(never)]
    fn collect_due(&mut self, program: &ResidualProgram) {
        if self.heap.cycle_running() {
            if self.heap.cycle_poll() {
                self.collect_slow(program, true);
            }
            return;
        }
        let full = self.heap.should_full_collect();
        let begin_cycle = full && self.heap.concurrent_cycle_refusal().is_none();
        self.collect_inner(program, full, begin_cycle);
    }

    #[cold]
    #[inline(never)]
    /// Drops dynamic programs that no live function, environment, generator
    /// or activation can run any more, with their constants and caches. Runs
    /// after a full collection, when only live cells remain in the heap.
    fn reclaim_unreferenced_programs(&mut self) {
        let mut referenced = vec![false; self.programs.len()];
        let mut reference = |program: ProgramId| {
            if let Some(slot) = referenced.get_mut(program.raw() as usize) {
                *slot = true;
            }
        };
        reference(ProgramId::MAIN);
        reference(self.active_program);
        self.direct_eval_var_program
            .into_iter()
            .for_each(&mut reference);
        self.frames
            .iter()
            .for_each(|frame| reference(frame.program));
        self.heap.for_each_program_reference(&mut reference);
        let reclaimed = self.programs.reclaim_unreferenced(referenced);
        if reclaimed.is_empty() {
            return;
        }
        self.free_cache_layouts_of(reclaimed.iter().copied());
        let reclaimed = reclaimed.into_iter().collect::<FxHashSet<_>>();
        self.function_values
            .retain(|(program, _), _| !reclaimed.contains(program));
        self.realm
            .template_objects
            .retain(|(program, _, _), _| !reclaimed.contains(program));
    }

    /// Drops the unlowered copies of the functions lowered since the last full collection
    /// that no frame runs (`ResidualProgram::release_lowered_tier0`); one a frame runs waits
    /// for the next. Frames match by residual: programs share one.
    fn release_lowered_tier0(&mut self) {
        let mut lowered = std::mem::take(self.lowered_tier0.get_mut());
        if lowered.is_empty() {
            return;
        }
        let key = |code: &crate::bytecode::ResidualProgram| std::ptr::from_ref(code) as usize;
        let live = self.programs.decoded_residuals().map(|code| (key(code), code)).collect::<rustc_hash::FxHashMap<_, _>>();
        let active = self
            .frames
            .iter()
            .filter_map(|frame| Some((key(self.programs.residual(frame.program)?), frame.function)))
            .collect::<FxHashSet<_>>();
        lowered.retain(|&(address, function)| {
            // A residual no program holds any more was dropped with its copies.
            let Some(code) = live.get(&address) else { return false };
            if active.contains(&(address, function)) {
                return true;
            }
            // SAFETY: a full collection, and no frame of any program sharing the residual runs
            // the function; references into a copy are held only while a frame runs it.
            unsafe { code.release_lowered_tier0(function as usize) };
            false
        });
        *self.lowered_tier0.get_mut() = lowered;
    }

    /// Recursion leaves one pooled frame per level it reached; a full collection keeps the few that
    /// ordinary call depth reuses, and later deeper calls allocate their own.
    fn trim_frame_pool(&mut self) {
        self.frame_pool.truncate(POOLED_FRAMES_KEPT_BY_FULL_COLLECTIONS);
        self.frame_pool.shrink_to(POOLED_FRAMES_KEPT_BY_FULL_COLLECTIONS);
    }

    pub(super) fn collect_slow(&mut self, program: &ResidualProgram, full: bool) {
        self.collect_inner(program, full, false);
    }

    /// A collection, or with `begin_cycle` the start of a concurrent cycle: the same roots
    /// plus every edge a collection finds only through a marked owner (shapes' keys and
    /// constructors, owned edges) and the VM's weak caches, all held strongly from the
    /// snapshot, and no sweep until the cycle's remark.
    fn collect_inner(&mut self, _program: &ResidualProgram, full: bool, begin_cycle: bool) {
        let full = self.begin_collection(full, begin_cycle);
        #[cfg(any(feature = "profile-aggregate", feature = "profile-gc-time"))]
        let phase_started = std::time::Instant::now();
        let owned_roots = self.owned_edge_index();
        self.gc_owned_root_count = owned_roots.len();
        let external_roots = self.external_owned_roots(&owned_roots, full);
        #[cfg(any(feature = "profile-aggregate", feature = "profile-gc-time"))]
        self.heap.record_vm_gc_phase(0, phase_started);
        let gathered = self.gather_roots(full, begin_cycle, external_roots);
        let Some((finalization_jobs, live_shapes)) = self.trace_and_sweep(gathered, &owned_roots, full, begin_cycle) else {
            return;
        };
        self.finish_collection(full, finalization_jobs, live_shapes);
    }

    /// The work before a collection's roots are gathered: the boundary hooks, the frame
    /// profile, and the per-collection tables the VM clears. Returns whether it is full.
    fn begin_collection(&mut self, full: bool, begin_cycle: bool) -> bool {
        // Frameless activations' registers are no roots; their frames are.
        self.materialize_deferred_lane();
        // The active for-in names words a collection may move or free.
        self.for_in_forget_active();
        self.clear_call_targets();
        self.forget_outer_environments();
        self.resolved_module_requests.clear();
        let full = self.heap.collection_is_full(full);
        if full && !begin_cycle {
            self.host.at_collection_boundary(self.memory_policy());
        } else {
            self.host.before_collection();
        }
        #[cfg(feature = "profile-memory")]
        if crate::trace::Channel::FunctionUse.on() {
            self.report_function_use();
        }
        self.constructed_shapes.fill(super::provenance::ConstructedShape::EMPTY);
        // Prepared regexp subjects share their strings' text: kept across a collection, they
        // would keep a dropped subject's text alive however large, as no string holds them.
        self.regexp_subjects.clear();
        if full {
            self.trim_frame_pool();
        }
        #[cfg(feature = "profile-aggregate")]
        for (index, frame) in self.frames.iter().enumerate() {
            self.profile.gc_frame(frame.program.raw(), frame.function, frame.pc as u32, index + 1 == self.frames.len());
        }
        full
    }

    /// Traces the heap from the gathered roots and sweeps it, returning the finalization jobs
    /// and the live shapes. A cycle's snapshot returns None: its sweep waits for the remark.
    fn trace_and_sweep(&mut self, gathered: GatheredRoots, owned_roots: &OwnedRoots, full: bool, begin_cycle: bool) -> Option<(Vec<(Value, Value)>, Vec<u32>)> {
        #[cfg(any(feature = "profile-aggregate", feature = "profile-gc-time"))]
        let phase_started = std::time::Instant::now();
        let GatheredRoots { mut roots, mut pins, rc, repin } = gathered;
        let shape_count = self.shapes.len();
        let marked_shape_bits = zeroed_mark_words(shape_count);
        let marked_constructor_shape_bits = zeroed_mark_words(shape_count);
        self.fill_shape_constructors();
        let owned_root_filter = OwnedRootFilter::new(owned_roots);
        let shape_trace = ParallelShapeTrace {
            shapes: self.shapes.as_ptr(),
            shape_count,
            accessors: &self.shape_accessors,
            marked_shapes: &marked_shape_bits,
            marked_constructor_shapes: &marked_constructor_shape_bits,
            constructors: &self.shape_constructors,
            owned_roots,
            owned_root_filter: &owned_root_filter,
            _shape_borrow: std::marker::PhantomData,
        };
        // A cycle's snapshot, and a minor collection under counts (which re-marks the
        // zero-count old records anything uncounted names, `Heap::rc_pre_mark`), take every
        // edge the VM holds outside the heap as a root; under counts the edges of the shapes
        // made since the last minor collection are pinned instead, being written once, but an
        // owned dictionary's change in place, so they are roots every time.
        if begin_cycle {
            append_cycle_shape_edges(&shape_trace, shape_count, &mut roots);
        } else if !full && rc {
            append_counted_shape_edges(&shape_trace, &self.shapes, &mut self.rc_pinned_shapes, repin, &mut roots, &mut pins);
        }
        if begin_cycle || !full && rc {
            self.extend_owned_cycle_roots(owned_roots, &mut roots);
        }
        if rc {
            self.heap.rc_pin(&pins);
            self.rc_pin_epoch = self.heap.rc_pin_epoch();
        }
        roots.append(&mut pins);
        if begin_cycle {
            self.heap.begin_cycle(roots);
            return None;
        }
        let parallel_trace = |trace_kind: GcTraceKind, owner: Value, shape: u32, prototype: bool, roots: &mut Vec<Value>| {
            shape_trace.append(trace_kind, owner, shape, prototype, roots);
        };
        // `ParallelShapeTrace::append`'s own early return, which markers then test inline.
        let parallel_object_roots = crate::heap::ParallelObjectRootTrace {
            trace: &parallel_trace,
            seen_shapes: if owned_roots.is_empty() { &marked_constructor_shape_bits } else { &[] },
        };
        #[cfg(any(feature = "profile-aggregate", feature = "profile-gc-time"))]
        self.heap.record_vm_gc_phase(2, phase_started);
        let closure_reads = ProgramClosureReads { programs: &self.programs, lexical_this: self.runtime_atoms.lexical_this };
        let finalization_jobs = self.heap.collect_with_object_roots(
            roots,
            |trace_kind, owner, shape, prototype, roots| {
                shape_trace.append(trace_kind, owner, shape, prototype, roots);
            },
            Some(&parallel_object_roots),
            Some(&closure_reads),
            full,
        );
        Some((finalization_jobs, shape_trace.live_shapes()))
    }

    /// The shapes' constructors, extended to the shapes made since the last collection.
    fn fill_shape_constructors(&mut self) {
        let constructors = &mut self.shape_constructors;
        for shape in &self.shapes[constructors.len()..] {
            let constructor = match shape.transition {
                ShapeTransition::Constructed { constructor, .. } => constructor,
                _ => shape.parent().map(|parent| constructors[parent as usize]).unwrap_or(Value::UNDEFINED),
            };
            constructors.push(constructor);
        }
    }

    /// The work after a sweep: the program and function caches, the method caches, the dead
    /// entries of the VM's tables, the finalization jobs, and the boundary hooks.
    fn finish_collection(&mut self, full: bool, finalization_jobs: Vec<(Value, Value)>, mut live_shapes: Vec<u32>) {
        // Whether every startup root is old now, so the next minor collection may leave
        // them out (`StartupIntrinsics::roots_old`).
        if !self.startup_roots_old() {
            let heap = &self.heap;
            let old = self.startup_intrinsic_roots().all(|value| value.heap_reference().is_none() || heap.is_old(value));
            self.startup_intrinsics.roots_old = old;
        }
        #[cfg(any(feature = "profile-aggregate", feature = "profile-gc-time"))]
        let after_collection_started = std::time::Instant::now();
        // The startup snapshot reads its holders' sealed shapes, which no object may hold now.
        live_shapes.extend(self.startup_intrinsics.sealed_shapes());
        self.prune_swept_programs(full);
        self.compact_or_retain_method_caches(full, live_shapes);
        self.prune_dead_callees();
        self.enqueue_finalization_jobs(finalization_jobs);
        self.prune_dead_tables();
        self.prune_dead_promise_entries();
        H::after_collection(&mut crate::CollectionContext::new(&mut self.host, &mut self.heap));
        #[cfg(any(feature = "profile-aggregate", feature = "profile-gc-time"))]
        self.heap.record_vm_gc_phase(3, after_collection_started);
        self.collection_boundary(full);
        #[cfg(feature = "profile-memory")]
        if crate::trace::Channel::MemoryPeak.on() {
            let phase = format!("gc_{}_after", self.heap.collection_count());
            self.report_memory_snapshot(&phase);
        }
    }

    /// The program and function state a sweep may have left dead: native calls, script realms,
    /// closures, exported wasm functions, private homes, and programs a full collection frees.
    fn prune_swept_programs(&mut self, full: bool) {
        let heap = &self.heap;
        // SAFETY: native lane bodies cannot collect; sweep has finished and
        // no cleanup allocation can yet reuse a dead function locator.
        unsafe { self.programs.retire_dead_native_calls(|callee| heap.is_live(callee)); }
        self.programs.prune_owned_script_realms(|realm| heap.weak_value(realm).is_some());
        // Under reference counts a minor collection frees old closures too.
        self.prune_function_values(full || self.heap.rc_active());
        self.prune_wasm_exported_functions();
        // A home stays reachable through its environment; entries of swept environments go
        // before their slots can name new cells.
        let heap = &self.heap;
        self.private_homes.retain(|key, _| heap.is_live(key.environment));
        self.private_site_homes.prune_dead(|value| heap.is_live(value));
        if full && self.programs.has_unchecked_reclaimable() {
            self.reclaim_unreferenced_programs();
        }
        if full {
            self.release_lowered_tier0();
        }
        if full {
            self.heap.set_reclaimable_programs_after_full(self.programs.settled_reclaimable_bytes());
        }
    }

    /// Compacts the shapes when a full collection's sweep left enough dead, else retains the
    /// live method caches; then prunes the first chunks.
    fn compact_or_retain_method_caches(&mut self, full: bool, live_shapes: Vec<u32>) {
        // Do shape work immediately after sweep. In particular, dead method
        // cache handles must be pruned before any runtime cleanup can allocate
        // a new heap cell into a freed slot.
        // A remark marks only what its cycle had not: its shape bitmap misses the rest.
        let shapes_complete = !self.heap.last_collection_kept_cycle_marks();
        if full && shapes_complete && self.should_compact_live_shapes(&live_shapes) {
            #[cfg(feature = "profile-aggregate")]
            self.snapshot_method_caches(0);
            self.compact_live_shapes(live_shapes);
            // Compaction renumbers the shapes: pins taken stay, but which shapes they cover is
            // not known by index any more.
            self.rc_pinned_shapes = 0;
        } else {
            self.retain_live_method_caches();
        }
        self.prune_first_chunks();
    }

    /// Forgets the swept callees and holders of the lane stacks and the holder hints.
    fn prune_dead_callees(&mut self) {
        let heap = &self.heap;
        self.lane_stack.forget_dead_callees(|value| heap.is_live(value));
        self.lane_roots.iter_mut().for_each(|parked| parked.stack.forget_dead_callees(|value| heap.is_live(value)));
        self.holder_hints.retire_dead(|holder| heap.is_live(holder));
    }

    /// Queues the finalization callbacks of the swept finalization registries.
    fn enqueue_finalization_jobs(&mut self, finalization_jobs: Vec<(Value, Value)>) {
        self.realm.jobs.extend(finalization_jobs.into_iter().map(|(callback, held)| PendingJob::Call {
            callback,
            this: Value::UNDEFINED,
            args: vec![held],
        }));
    }

    /// Drops the swept owners of descriptors and the abandoned job and promise contexts, and
    /// the dead dynamic strings.
    fn prune_dead_tables(&mut self) {
        self.descriptors.retain(|(object, _), _| self.heap.is_live(*object));
        let abandoned = self.heap.take_abandoned_contexts();
        for context in abandoned.jobs {
            self.release_job_context(context);
        }
        if !abandoned.promises.is_empty() {
            self.host.release_promise_contexts(&abandoned.promises);
        }
        #[cfg(feature = "profile-aggregate")]
        self.retain_live_gc_method_snapshots();
        if let Some(strings) = &mut self.dynamic_strings {
            strings.retain(|_, value| self.heap.is_live(*value));
        }
    }

    /// Drops the swept keys of the promise tables that name a promise, aggregate, job or
    /// callback.
    fn prune_dead_promise_entries(&mut self) {
        self.realm.promise.finally_handler_callbacks.retain(|function, _| self.heap.is_live(*function));
        self.realm.promise.finally_continuation_callbacks.retain(|function, _| self.heap.is_live(*function));
        self.realm.promise.aggregates.retain(|aggregate, _| self.heap.is_live(*aggregate));
        self.realm.promise.aggregate_jobs.retain(|job, _| self.heap.is_live(*job));
        self.realm.promise.reaction_capabilities.retain(|promise, _| self.heap.is_live(*promise));
    }

    fn should_compact_live_shapes(&self, live_shapes: &[u32]) -> bool {
        // Active symbol keys are marked by `append_shape_roots`. A dead symbol
        // can remain in transition history only below its latest Delete;
        // newest-first shape lookup treats that tombstone as absence even if
        // the heap later reuses the symbol's slot for a different symbol.
        let mut reachable = vec![false; self.shapes.len()];
        reachable[0] = true;
        for &object_shape in live_shapes {
            let mut current = Some(object_shape);
            while let Some(id) = current {
                let index = id as usize;
                if reachable[index] {
                    break;
                }
                reachable[index] = true;
                current = self.shapes[index].parent();
            }
        }
        let unreachable = reachable.iter().filter(|live| !**live).count();
        shape_reclaim_threshold_met(self.shapes.len(), unreachable)
    }

    fn compact_live_shapes(&mut self, live_shapes: Vec<u32>) {
        // Compiled code and proofs name shapes by id, which compaction renumbers.
        crate::heap::advance_prototype_layout_epoch();
        self.bound_parts_layout.set(None);
        let old_shapes = std::mem::replace(&mut self.shapes, vec![Shape::root()]);
        #[cfg(feature = "profile-memory")]
        let old_shape_count = old_shapes.len();
        #[cfg(feature = "profile-memory")]
        let live_shape_count = live_shapes.len();
        let mut mapping = vec![u32::MAX; old_shapes.len()];
        let mut shapes = Shape::table();
        let mut transitions = super::shape_transitions::ShapeTransitionIndex::default();
        let old_accessors = std::mem::take(&mut self.shape_accessors);
        let mut accessors = Vec::new();
        for old_shape in live_shapes {
            let mapping_entry = &mut mapping[old_shape as usize];
            if *mapping_entry == u32::MAX {
                *mapping_entry = rebuild_shape(
                    &old_shapes,
                    &old_accessors,
                    old_shape,
                    &mut shapes,
                    &mut accessors,
                    &mut transitions,
                );
            }
        }
        self.shape_accessors = accessors;
        let layouts = super::shape_layouts(&shapes);
        self.heap.remap_live_object_shapes(&mapping, &layouts);
        self.startup_intrinsics.remap_shapes(&mapping);
        self.remap_object_template_shapes(&mapping);
        self.shapes = shapes;
        self.shape_constructors.clear();
        self.transitions = transitions;
        self.shape_slot_memo.clear();
        self.private_site_homes.forget_shapes();
        self.shape_attribute_memo.clear();
        self.plain_copy_shapes.clear();
        self.plain_assign_shapes.clear();
        self.embedding_record_shapes.clear();
        self.function_name_shapes.clear();
        self.object_shapes.reset();
        self.invalidate_field_caches();
        self.holder_hints.clear();
        self.programs.invalidate_shape_bindings();
        self.method_caches.clear_all();
        self.megamorphic_methods.clear();
        self.method_cache_atoms.clear();
        self.method_cache_sites.clear();
        #[cfg(feature = "profile-aggregate")]
        self.remap_invalidated_method_shapes(&mapping);
        #[cfg(feature = "profile-memory")]
        if crate::trace::Channel::Memory.on() {
            eprintln!(
                "{{\"kind\":\"quench-shape-compaction\",\"before\":{},\"after\":{},\"live_shapes\":{}}}",
                old_shape_count,
                self.shapes.len(),
                live_shape_count
            );
        }
    }

    #[cfg(feature = "profile-aggregate")]
    fn remap_invalidated_method_shapes(&mut self, mapping: &[u32]) {
        let mut remapped = FxHashMap::default();
        for (mut key, method) in self.invalidated_methods.drain() {
            let Some(shape) = mapping
                .get(key.shape as usize)
                .copied()
                .filter(|shape| *shape != u32::MAX)
            else {
                continue;
            };
            key.shape = shape;
            remapped.insert(key, method);
        }
        self.invalidated_methods = remapped;
    }

    pub(crate) fn drain_jobs(&mut self, program: &ResidualProgram) -> Result<Value, JsError> {
        self.check_execution_interrupt()?;
        loop{
            self.drain_pending_microtasks(program)?;
            // Async futex completion is an agent task: settle only after the
            // existing microtask queue drains, then run its Promise reactions.
            self.poll_shared_async_waiters(program)?;
            self.advance_static_module_jobs(program)?;
            self.advance_dynamic_import_jobs(program, true)?;
            if self.realm.jobs.is_empty(){break}
        }
        Ok(Value::UNDEFINED)
    }

    /// Preserve existing internal drains, including recursive promise/module
    /// drains. Only NativeContext checkpoint admission checks the running flag.
    fn drain_pending_microtasks(&mut self, program: &ResidualProgram) -> Result<Value, JsError> {
        let previous = std::mem::replace(&mut self.native_microtask_running, true);
        let result = (|| {
            self.check_execution_interrupt()?;
            self.run_job_queue(program, |_| true)?;
            Ok(Value::UNDEFINED)
        })();
        self.native_microtask_running = previous;
        result
    }

    /// The VM-wide admission fact shared with native embedder task checkpoints.
    pub(crate) fn native_microtask_checkpoint_is_active(&self) -> bool {
        self.native_microtask_running
    }

    /// A native microtask-only checkpoint, with no native agent-task polling.
    pub(crate) fn native_microtask_checkpoint(&mut self, program: &ResidualProgram) -> Result<Value, JsError> {
        if self.native_microtask_running { return Ok(Value::UNDEFINED); }
        self.drain_pending_microtasks(program)
    }

    /// Runs queued jobs while `more` holds. A job runs in its realm, and the queue stays
    /// there until work of another realm comes: consecutive jobs of one realm share one
    /// realm entry, and the drain's own realm returns for module advancement, a host job,
    /// and the drain's end, however it ends.
    fn run_job_queue(
        &mut self,
        program: &ResidualProgram,
        mut more: impl FnMut(&Self) -> bool,
    ) -> Result<(), JsError> {
        let home = self.realm.globals;
        // An enclosing queue's entry stays in place below this queue's own.
        let outer = std::mem::replace(&mut self.unobserved_jobs_entered, false);
        let result = (|| {
            while more(self) && let Some(job) = self.realm.jobs.pop_front() {
                self.check_execution_interrupt()?;
                if matches!(job, PendingJob::Call { .. }) {
                    self.leave_unobserved_jobs();
                }
                self.run_pending_job(program, job, home)?;
                // Static continuations may become ready after a reaction. Import
                // loading waits for the agent checkpoint after this queue drains.
                if self.static_module_jobs_pending() {
                    self.leave_unobserved_jobs();
                    self.switch_realm_global(home);
                    self.advance_static_module_jobs(program)?;
                }
            }
            Ok(())
        })();
        self.leave_unobserved_jobs();
        self.unobserved_jobs_entered = outer;
        self.switch_realm_global(home);
        result
    }

    /// Leaves the context unobserved Promise jobs of the running queue share, if entered: a
    /// host job and module advancement run in the queue's own.
    fn leave_unobserved_jobs(&mut self) {
        if std::mem::take(&mut self.unobserved_jobs_entered) {
            for root in self.host.leave_unobserved_job() {
                self.heap.release_root(root);
            }
        }
    }

    /// Runs a job already taken off the queue, leaving its realm current: a finished job
    /// roots nothing, so a drain whose jobs keep enqueueing more holds only the jobs still
    /// pending. A host job runs in the drain's realm `home`.
    fn run_pending_job(
        &mut self,
        program: &ResidualProgram,
        job: PendingJob,
        home: Value,
    ) -> Result<(), JsError> {
        let start = self.active_call_roots.len();
        job.push_roots(&mut self.active_call_roots);
        // A job boundary is a safepoint, as a back edge is: a job may run no loop, so a due
        // collection would otherwise wait while job after job allocates, and the lane, which
        // admits no frameless call while one is due, would push a frame for every call. The
        // job's own values are call roots, and a collection inside its callback, which any
        // back edge makes, is no less possible.
        self.maybe_collect(program);
        let previous = std::mem::replace(&mut self.native_microtask_running, true);
        let result = match job {
            PendingJob::Call {
                callback,
                this,
                args,
            } => {
                self.switch_realm_global(home);
                self.call_value(program, callback, this, &args).map(|_| ())
            }
            PendingJob::Reaction(reaction) => self.run_promise_reaction(program, reaction),
            PendingJob::Thenable(thenable) => self.run_thenable_job(program, thenable),
        };
        self.native_microtask_running = previous;
        self.active_call_roots.truncate(start);
        result
    }


    pub(crate) fn drain_host_jobs(&mut self, program: &ResidualProgram) -> Result<Value, JsError> {
        self.drain_jobs(program)
    }

    pub(crate) fn drain_jobs_until_promise(
        &mut self,
        program: &ResidualProgram,
        promise: Value,
    ) -> Result<(), JsError> {
        loop {
            self.run_job_queue(program, |vm| vm.promise_record(promise)
                .is_some_and(|record| record.state() == PromiseState::Pending))?;
            if self.promise_record(promise).is_none_or(|record| record.state() != PromiseState::Pending)
                || !self.realm.jobs.is_empty() { return Ok(()); }
            self.advance_static_module_jobs(program)?;
            self.advance_dynamic_import_jobs(program, true)?;
            if self.realm.jobs.is_empty() { return Ok(()); }
        }
    }
}

#[cfg(test)]
mod tests;
