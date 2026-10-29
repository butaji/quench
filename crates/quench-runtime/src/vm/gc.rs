use super::module::ModuleRecord;
use super::promise::PromiseState;
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
    // Both vectors are sized exactly: collections call this for every rooted shape.
    let mut depth = 0;
    let mut current = Some(shape);
    while let Some(id) = current {
        depth += 1;
        current = shapes[id as usize].parent();
    }
    let mut chain = Vec::with_capacity(depth);
    let mut current = Some(shape);
    while let Some(id) = current {
        chain.push(id);
        current = shapes[id as usize].parent();
    }
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
                // A slot getter is traced with its object's slots.
                let (getter, setter) = attributes.functions(accessors, || None);
                key.symbol_value()
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
// `append_shape_roots` reads only `parent` and `transition`, never the
// interior-mutable caches. All shared bitmap updates are atomic; owned roots
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

    // Called once per marked object, so its frame setup would rival its body.
    #[inline(always)]
    fn append(&self, trace_kind: GcTraceKind, owner: Value, shape: u32, prototype: bool, roots: &mut Vec<Value>) {
        let shape_index = shape as usize;
        debug_assert!(shape_index < self.shape_count);
        // SAFETY: construction holds an immutable borrow of the shape table;
        // collection cannot compact or append shapes until all workers join.
        let shapes = unsafe { std::slice::from_raw_parts(self.shapes, self.shape_count) };
        let word = shape_index / u64::BITS as usize;
        let mask = 1_u64 << (shape_index % u64::BITS as usize);
        if trace_kind == GcTraceKind::VerifyFull {
            if shapes[shape_index].facts.may_have_gc_roots() {
                append_shape_roots(shapes, self.accessors, shape, roots);
            }
        } else if self.marked_shapes[word].load(Ordering::Relaxed) & mask == 0
            && self.marked_shapes[word].fetch_or(mask, Ordering::Relaxed) & mask == 0
            && shapes[shape_index].facts.may_have_gc_roots()
        {
            append_shape_roots(shapes, self.accessors, shape, roots);
        }
        // A prototype no longer owns its original base constructor. Keep this
        // trace bit separate: a prototype may be the first holder of a shape.
        let constructor = self.constructors[shape_index];
        // As for the shape bit: a plain load first, so a shape already counted costs no
        // locked read-modify-write.
        if !prototype && !constructor.is_undefined() &&
            (trace_kind == GcTraceKind::VerifyFull
                || (self.marked_constructor_shapes[word].load(Ordering::Relaxed) & mask == 0
                    && self.marked_constructor_shapes[word].fetch_or(mask, Ordering::Relaxed) & mask == 0)) {
            roots.push(constructor);
        }
        if self.owned_root_filter.may_hold(owner)
            && let Some(edges) = self.owned_roots.get(&owner)
        {
            roots.extend(edges.iter().copied());
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
    let (function_name, constructed_name, name_reconfigured) = provenance::shape_names(old_shapes, old_shape);
    for transition in [
        function_name.map(|name| ShapeTransition::FunctionName { name }),
        name_reconfigured.then_some(ShapeTransition::FunctionNameReconfigured),
        provenance::shape_function_length_reconfigured(old_shapes, old_shape).then_some(ShapeTransition::FunctionLengthReconfigured),
        constructed_name.map(|(constructor, new_target_is_base)| ShapeTransition::Constructed { constructor, new_target_is_base }),
    ].into_iter().flatten() {
        parent = append_compacted_shape(shapes, transitions, parent, transition, 0);
    }
    for (slot, entry) in active_shape_attributes(old_shapes, old_shape)
        .into_iter()
        .enumerate()
    {
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
        self.collect_slow(program, true);
    }

    #[inline(always)]
    pub(crate) fn maybe_collect(&mut self, program: &ResidualProgram) {
        if !self.heap.should_collect() {
            return;
        }
        let full = self.heap.should_full_collect();
        self.collect_slow(program, full);
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
        let reclaimed = reclaimed.into_iter().collect::<FxHashSet<_>>();
        self.function_values
            .retain(|(program, _), _| !reclaimed.contains(program));
        self.realm
            .template_objects
            .retain(|(program, _, _), _| !reclaimed.contains(program));
    }

    pub(super) fn collect_slow(&mut self, _program: &ResidualProgram, full: bool) {
        // Frameless activations' registers are no roots; their frames are.
        self.materialize_deferred_lane();
        self.resolved_module_requests.clear();
        self.invalidate_wasm_reference_caches();
        let full = self.heap.collection_is_full(full);
        if full {
            self.host.at_collection_boundary(self.memory_policy());
        }
        #[cfg(feature = "profile-memory")]
        if std::env::var_os("QUENCH_FUNCTION_USE").is_some() {
            self.report_function_use();
        }
        #[cfg(any(feature = "profile-aggregate", feature = "profile-gc-time"))]
        let mut phase_started = std::time::Instant::now();
        self.constructed_shapes.fill(super::provenance::ConstructedShape::EMPTY);
        self.enumerable_keys.clear();
        // Prepared regexp subjects share their strings' text: kept across a collection, they
        // would keep a dropped subject's text alive however large, as no string holds them.
        self.regexp_subjects.clear();
        if full {
            // Recursion leaves one pooled frame per level it reached; a full collection keeps the
            // few that ordinary call depth reuses, and later deeper calls allocate their own.
            self.frame_pool.truncate(POOLED_FRAMES_KEPT_BY_FULL_COLLECTIONS);
            self.frame_pool.shrink_to(POOLED_FRAMES_KEPT_BY_FULL_COLLECTIONS);
        }
        #[cfg(feature = "profile-aggregate")]
        for (index, frame) in self.frames.iter().enumerate() {
            self.profile.gc_frame(
                frame.program.raw(),
                frame.function,
                frame.pc as u32,
                index + 1 == self.frames.len(),
            );
        }
        // Out-of-cell metadata belongs to its object. Derive the edge index for
        // this collection and visit it only when the owner becomes reachable.
        let mut owned_roots = OwnedRoots::with_capacity_and_hasher(
            self.gc_owned_root_count,
            Default::default(),
        );
        let mut own = |owner: Value, edge: Value| {
            if edge.heap_index().is_some() {
                owned_roots.entry(owner).or_default().push(edge);
            }
        };
        for ((owner, key), attributes) in &self.descriptors {
            for edge in key
                .symbol_value()
                .into_iter()
                .chain([attributes.getter, attributes.setter].into_iter().flatten())
            {
                own(*owner, edge);
            }
        }
        self.realm.promise.for_each_owned_edge(&mut own);
        self.gc_owned_root_count = owned_roots.len();
        let external_roots = if full {
            Vec::new()
        } else {
            owned_roots
                .iter()
                .filter(|(owner, _)| self.heap.is_old(**owner))
                .flat_map(|(_, edges)| edges.iter().copied())
                .collect::<Vec<_>>()
        };
        #[cfg(any(feature = "profile-aggregate", feature = "profile-gc-time"))]
        {
            self.heap.record_vm_gc_phase(0, phase_started);
            phase_started = std::time::Instant::now();
        }
        // Roots are gathered source by source: a single iterator chain over
        // every source costs each root a step through every nesting level. The previous
        // collection's count sizes the vector, which otherwise regrows from empty each time.
        let mut roots = Vec::with_capacity(self.gc_root_count);
        self.programs.extend_roots(&mut roots, full, self.heap.promotion_age());
        roots.extend(self.wasm_js.roots());
        roots.extend([
            self.realm.globals,
            self.object_proto,
            self.function_proto,
            self.array_proto,
            self.array_buffer_proto,
            self.shared_array_buffer_proto,
            self.array_iterator_proto,
            self.typed_array_proto,
            self.uint8_array_proto,
            self.uint8_clamped_array_proto,
            self.uint16_array_proto,
            self.uint32_array_proto,
            self.int8_array_proto,
            self.int16_array_proto,
            self.int32_array_proto,
            self.bigint64_array_proto,
            self.biguint64_array_proto,
            self.float16_array_proto,
            self.float32_array_proto,
            self.float64_array_proto,
            self.data_view_proto,
            self.map_proto,
            self.set_proto,
            self.shadow_realm_proto,
            self.map_iterator_proto,
            self.set_iterator_proto,
            self.weak_map_proto,
            self.weak_set_proto,
            self.weak_ref_proto,
            self.finalization_registry_proto,
            self.iterator_proto,
            self.string_iterator_proto,
            self.regexp_string_iterator_proto,
            self.generator_proto,
            self.iterator_helper_proto,
            self.wrap_for_valid_iterator_proto,
            self.async_iterator_proto,
            self.async_generator_proto,
            self.async_from_sync_iterator_proto,
            self.regexp_proto,
            self.reused_iterator_result,
        ]);
        // The active realm's top-level lexical bindings, then every inactive realm's.
        roots.extend(self.realm.global_lexical_bindings.values().copied());
        roots.extend(
            self.realm
                .global_lexical_states
                .iter()
                .chain(self.realm.parked.iter().map(|parked| (&parked.global, &parked.state)))
                .flat_map(|(global, state)| {
                    std::iter::once(*global).chain(state.bindings.values().copied())
                }),
        );
        roots.extend(
            self.realm
                .intrinsics
                .builtin_prototypes
                .iter()
                .flat_map(|((realm, _), prototype)| [*realm, *prototype]),
        );
        roots.extend(
            self.realm
                .intrinsics
                .function_constructors
                .iter()
                .flat_map(|((realm, _), constructor)| [*realm, *constructor]),
        );
        roots.extend(
            self.realm
                .intrinsics
                .promise_constructors
                .iter()
                .flat_map(|(realm, constructor)| [*realm, *constructor]),
        );
        roots.extend(self.realm.intrinsics.error_intrinsics.iter().flat_map(
            |(realm, intrinsics)| {
                [
                    *realm,
                    intrinsics.constructor,
                    intrinsics.stack_getter,
                    intrinsics.stack_setter,
                    intrinsics.call_site_constructor,
                    intrinsics.call_site_prototype,
                ].into_iter().chain(intrinsics.host_formatter)
            },
        ));
        roots.extend(self.realm.intrinsics.regexp_intrinsics.iter().flat_map(
            |(realm, intrinsics)| {
                [*realm, intrinsics.constructor, intrinsics.prototype]
                    .into_iter()
                    .chain(intrinsics.result_template.map(|template| template.anchor))
            },
        ));
        roots.extend(
            self.realm
                .intrinsics
                .intl_number_format_constructors
                .iter()
                .flat_map(|(realm, constructor)| [*realm, *constructor]),
        );
        roots.extend(
            self.realm
                .intrinsics
                .intl_number_format_prototypes
                .iter()
                .flat_map(|(realm, prototype)| [*realm, *prototype]),
        );
        roots.extend(
            self.realm
                .intrinsics
                .intl_number_format_fallback_symbols
                .iter()
                .flat_map(|(realm, symbol)| [*realm, *symbol]),
        );
        roots.extend(
            self.realm
                .intrinsics
                .intl_collator_constructors
                .iter()
                .flat_map(|(realm, constructor)| [*realm, *constructor]),
        );
        roots.extend(
            self.realm
                .intrinsics
                .intl_collator_prototypes
                .iter()
                .flat_map(|(realm, prototype)| [*realm, *prototype]),
        );
        roots.extend(
            self.realm
                .intrinsics
                .intl_plural_rules_prototypes
                .iter()
                .flat_map(|(realm, prototype)| [*realm, *prototype]),
        );
        roots.extend(
            self.realm
                .intrinsics
                .intl_datetime_format_constructors
                .iter()
                .flat_map(|(realm, constructor)| [*realm, *constructor]),
        );
        roots.extend(
            self.realm
                .intrinsics
                .intl_datetime_format_prototypes
                .iter()
                .flat_map(|(realm, prototype)| [*realm, *prototype]),
        );
        roots.extend(
            self.realm
                .intrinsics
                .intl_datetime_format_fallback_symbols
                .iter()
                .flat_map(|(realm, symbol)| [*realm, *symbol]),
        );
        roots.extend(
            self.realm
                .intrinsics
                .intl_display_names_constructors
                .iter()
                .flat_map(|(realm, constructor)| [*realm, *constructor]),
        );
        roots.extend(
            self.realm
                .intrinsics
                .intl_display_names_prototypes
                .iter()
                .flat_map(|(realm, prototype)| [*realm, *prototype]),
        );
        roots.extend(
            self.realm
                .intrinsics
                .intl_duration_format_constructors
                .iter()
                .flat_map(|(realm, constructor)| [*realm, *constructor]),
        );
        roots.extend(
            self.realm
                .intrinsics
                .intl_duration_format_prototypes
                .iter()
                .flat_map(|(realm, prototype)| [*realm, *prototype]),
        );
        roots.extend(
            self.realm
                .intrinsics
                .intl_list_format_constructors
                .iter()
                .flat_map(|(realm, constructor)| [*realm, *constructor]),
        );
        roots.extend(
            self.realm
                .intrinsics
                .intl_list_format_prototypes
                .iter()
                .flat_map(|(realm, prototype)| [*realm, *prototype]),
        );
        roots.extend(
            self.realm
                .intrinsics
                .intl_relative_time_format_prototypes
                .iter()
                .flat_map(|(realm, prototype)| [*realm, *prototype]),
        );
        roots.extend(
            self.realm
                .intrinsics
                .intl_segmenter_prototypes
                .iter()
                .flat_map(|(realm, prototype)| [*realm, *prototype]),
        );
        roots.extend(
            self.realm
                .intrinsics
                .intl_segment_iterator_prototypes
                .iter()
                .flat_map(|(realm, prototype)| [*realm, *prototype]),
        );
        roots.extend(
            self.realm
                .intrinsics
                .intl_segments_prototypes
                .iter()
                .flat_map(|(realm, prototype)| [*realm, *prototype]),
        );
        roots.extend(
            self.realm
                .intrinsics
                .intl_locale_prototypes
                .iter()
                .flat_map(|(realm, prototype)| [*realm, *prototype]),
        );
        roots.extend(self.natives.roots());
        roots.extend(self.atom_strings.values().copied());
        roots.extend(self.realm.intrinsics.iterator_prototypes.iter().flat_map(
            |(realm, prototypes)| {
                [
                    *realm,
                    prototypes.helper,
                    prototypes.wrapper,
                    prototypes.generator,
                    prototypes.async_generator,
                ]
            },
        ));
        roots.extend(self.test262_agent.roots());
        roots.extend(self.shared_async_waiters.iter().map(|w|w.promise));
        roots.extend(
            self.realm
                .promise
                .active_native
                .iter()
                .map(|activation| activation.callable),
        );
        roots.extend(
            self.realm.promise.rejection_notifications.iter().map(
                |notification| match notification {
                    super::promise::RejectionNotification::Unhandled(promise)
                    | super::promise::RejectionNotification::Handled(promise) => *promise,
                },
            ),
        );
        roots.extend(
            self.realm
                .promise
                .modules
                .values()
                .flat_map(ModuleRecord::roots),
        );
        roots.extend(self.realm.promise.module_sources.values().copied());
        roots.extend(
            self.realm
                .promise
                .dynamic_import_jobs
                .iter()
                .map(|job| job.promise),
        );
        for job in &self.realm.jobs {
            job.push_roots(&mut roots);
        }
        roots.extend(self.realm.template_objects.values().copied());
        roots.extend(
            self.realm
                .intrinsics
                .arguments_objects
                .iter()
                .flat_map(|((global, _), template)| [*global, template.anchor]),
        );
        roots.extend(self.with_stack.iter().copied());
        roots.extend(std::iter::once(self.instanceof_has_instance_holder));
        roots.extend(self.active_call_roots.iter().copied());
        roots.extend(self.symbol_registry.values().copied());
        roots.extend(self.well_known_symbols.values());
        roots.extend(self.typeof_names);
        roots.extend(self.startup_intrinsic_roots());
        roots.extend(self.frames.iter().flat_map(|frame| {
            // A missing map means the function uses a register form
            // the liveness pass cannot represent; keep the safe
            // conservative scan for that activation.
            let register_mask = self
                .programs
                .residual(frame.program)
                .and_then(|program| program.register_root_mask(frame.function, frame.pc));
            [frame.env, frame.this]
                .into_iter()
                .chain(frame.context.callee())
                .chain(frame.original_arguments.iter().copied())
                .chain(frame.locals.iter().copied())
                .chain(frame.dynamic_bindings.iter().map(|(_, value)| *value))
                .chain(
                    frame
                        .registers
                        .iter()
                        .enumerate()
                        .filter_map(move |(register, value)| {
                            register_mask
                                .is_none_or(|mask| crate::compile::liveness::register_in(mask, register))
                                .then_some(*value)
                        }),
                )
        }));
        roots.extend(external_roots);
        self.gc_root_count = roots.len();
        #[cfg(feature = "profile-memory")]
        if std::env::var_os("QUENCH_MEMORY_PEAK").is_some() {
            let phase = format!("gc_{}_before", self.heap.collection_count() + 1);
            self.report_memory_snapshot(&phase);
        }
        #[cfg(any(feature = "profile-aggregate", feature = "profile-gc-time"))]
        {
            self.heap.record_vm_gc_phase(1, phase_started);
            phase_started = std::time::Instant::now();
        }
        let shape_count = self.shapes.len();
        let shape_word_count = shape_count.div_ceil(u64::BITS as usize);
        let marked_shape_bits = (0..shape_word_count)
            .map(|_| AtomicU64::new(0))
            .collect::<Vec<_>>();
        let marked_constructor_shape_bits = (0..shape_word_count).map(|_| AtomicU64::new(0)).collect::<Vec<_>>();
        let constructors = &mut self.shape_constructors;
        for shape in &self.shapes[constructors.len()..] {
            let constructor = match shape.transition {
                ShapeTransition::Constructed { constructor, .. } => constructor,
                _ => shape.parent().map(|parent| constructors[parent as usize]).unwrap_or(Value::UNDEFINED),
            };
            constructors.push(constructor);
        }
        let constructors = &self.shape_constructors;
        let owned_root_filter = OwnedRootFilter::new(&owned_roots);
        let shape_trace = ParallelShapeTrace {
            shapes: self.shapes.as_ptr(),
            shape_count,
            accessors: &self.shape_accessors,
            marked_shapes: &marked_shape_bits,
            marked_constructor_shapes: &marked_constructor_shape_bits,
            constructors,
            owned_roots: &owned_roots,
            owned_root_filter: &owned_root_filter,
            _shape_borrow: std::marker::PhantomData,
        };
        let parallel_object_roots =
            |trace_kind: GcTraceKind, owner: Value, shape: u32, prototype: bool, roots: &mut Vec<Value>| {
                shape_trace.append(trace_kind, owner, shape, prototype, roots);
            };
        #[cfg(any(feature = "profile-aggregate", feature = "profile-gc-time"))]
        self.heap.record_vm_gc_phase(2, phase_started);
        let closure_reads = ProgramClosureReads {
            programs: &self.programs,
            lexical_this: self.runtime_atoms.lexical_this,
        };
        let finalization_jobs = self.heap.collect_with_object_roots(
            roots,
            |trace_kind, owner, shape, prototype, roots| {
                shape_trace.append(trace_kind, owner, shape, prototype, roots);
            },
            Some(&parallel_object_roots),
            Some(&closure_reads),
            full,
        );
        #[cfg(any(feature = "profile-aggregate", feature = "profile-gc-time"))]
        let after_collection_started = std::time::Instant::now();
        let mut live_shapes = shape_trace.live_shapes();
        // The startup snapshot reads its holders' sealed shapes, which no object may hold now.
        live_shapes.extend(self.startup_intrinsics.sealed_shapes());
        // Heap weak fields are pruned during collection. Prune the VM-owned
        // weak function cache before any post-collection cleanup can allocate
        // into newly freed slots.
        let heap = &self.heap;
        self.programs.prune_owned_script_realms(|realm| heap.weak_value(realm).is_some());
        self.programs.clear_wasm_reference_caches();
        self.prune_function_values();
        self.prune_wasm_exported_functions();
        // A home stays reachable through its environment; entries of swept environments go
        // before their slots can name new cells.
        let heap = &self.heap;
        self.private_homes
            .retain(|key, _| heap.is_live(key.environment));
        if full && self.programs.has_unchecked_reclaimable() {
            self.reclaim_unreferenced_programs();
        }
        if full {
            self.heap.set_reclaimable_programs_after_full(self.programs.settled_reclaimable_bytes());
        }
        // Do shape work immediately after sweep. In particular, dead method
        // cache handles must be pruned before any runtime cleanup can allocate
        // a new heap cell into a freed slot.
        if full && self.should_compact_live_shapes(&live_shapes) {
            #[cfg(feature = "profile-aggregate")]
            self.snapshot_method_caches(0);
            self.compact_live_shapes(live_shapes);
        } else {
            self.retain_live_method_caches();
        }
        self.realm.jobs.extend(
            finalization_jobs
                .into_iter()
                .map(|(callback, held)| PendingJob::Call {
                    callback,
                    this: Value::UNDEFINED,
                    args: vec![held],
                }),
        );
        self.descriptors
            .retain(|(object, _), _| self.heap.is_live(*object));
        let abandoned = self.heap.take_abandoned_contexts();
        for context in abandoned.jobs {
            self.release_job_context(context);
        }
        if !abandoned.promises.is_empty() {
            self.host.release_promise_contexts(&abandoned.promises);
        }
        self.realm
            .promise
            .finally_handler_callbacks
            .retain(|function, _| self.heap.is_live(*function));
        self.realm
            .promise
            .finally_continuation_callbacks
            .retain(|function, _| self.heap.is_live(*function));
        self.realm
            .promise
            .aggregates
            .retain(|aggregate, _| self.heap.is_live(*aggregate));
        self.realm
            .promise
            .aggregate_jobs
            .retain(|job, _| self.heap.is_live(*job));
        self.realm
            .promise
            .reaction_capabilities
            .retain(|promise, _| self.heap.is_live(*promise));
        #[cfg(feature = "profile-aggregate")]
        self.retain_live_gc_method_snapshots();
        if let Some(strings) = &mut self.dynamic_strings {
            strings.retain(|_, value| self.heap.is_live(*value));
        }
        if let Some(concats) = &mut self.string_concats {
            concats.fill(EMPTY_STRING_CONCAT_CACHE);
        }
        if let Some(atoms) = &mut self.string_atoms {
            atoms.fill((Value::UNDEFINED, 0));
        }
        if let Some(strings) = &mut self.recent_atom_strings {
            strings.fill((super::NO_RECENT_ATOM, Value::UNDEFINED));
        }
        #[cfg(any(feature = "profile-aggregate", feature = "profile-gc-time"))]
        self.heap.record_vm_gc_phase(3, after_collection_started);
        self.host.at_collection_boundary(self.memory_policy());
        #[cfg(feature = "profile-memory")]
        if std::env::var_os("QUENCH_MEMORY_PEAK").is_some() {
            let phase = format!("gc_{}_after", self.heap.collection_count());
            self.report_memory_snapshot(&phase);
        }
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
        let lengths = shapes
            .iter()
            .map(|shape| shape.storage_len())
            .collect::<Vec<_>>();
        self.heap.remap_live_object_shapes(&mapping, &lengths);
        self.startup_intrinsics.remap_shapes(&mapping);
        self.shapes = shapes;
        self.shape_constructors.clear();
        self.transitions = transitions;
        self.shape_slot_memo.clear();
        self.plain_copy_shapes.clear();
        self.plain_assign_shapes.clear();
        self.embedding_record_shapes.clear();
        self.function_name_shapes.clear();
        self.shape_attribute_memo.clear();
        self.object_shapes.reset();
        self.invalidate_field_caches();
        self.programs.clear_field_hints();
        self.method_caches.fill([EMPTY_METHOD_CACHE; 2]);
        self.megamorphic_methods.clear();
        self.method_cache_atoms.clear();
        #[cfg(feature = "profile-aggregate")]
        self.remap_invalidated_method_shapes(&mapping);
        #[cfg(feature = "profile-memory")]
        if std::env::var_os("QUENCH_MEMORY").is_some() {
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
        let result = (|| {
            while more(self) && let Some(job) = self.realm.jobs.pop_front() {
                self.check_execution_interrupt()?;
                self.run_pending_job(program, job, home)?;
                // Module advancement does nothing in a realm with no module work due
                // (`module_jobs_pending`), the common case after a job.
                if self.module_jobs_pending() {
                    self.switch_realm_global(home);
                    self.advance_static_module_jobs(program)?;
                    self.advance_dynamic_import_jobs(program, true)?;
                }
            }
            Ok(())
        })();
        self.switch_realm_global(home);
        result
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
        self.advance_static_module_jobs(program)?;
        self.advance_dynamic_import_jobs(program, true)?;
        self.drain_jobs(program)
    }

    pub(crate) fn drain_jobs_until_promise(
        &mut self,
        program: &ResidualProgram,
        promise: Value,
    ) -> Result<(), JsError> {
        self.run_job_queue(program, |vm| {
            vm.promise_record(promise)
                .is_some_and(|record| record.state == PromiseState::Pending)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::shape_reclaim_threshold_met;
    use crate::value::Value;
    use crate::vm::{
        DEFAULT_DATA_ATTRIBUTES, DEFAULT_PROPERTY_ATTRIBUTES, DictionaryTrigger,
        PropertyAttributes, Shape, ShapeAttributes, ShapeTransition, property_key::PropertyKey,
    };

    #[test]
    fn shape_compaction_preserves_creation_metadata_without_property_slots() {
        use super::{append_compacted_shape, rebuild_shape, provenance};
        let mut shapes = vec![Shape::root()];
        let mut transitions = crate::vm::shape_transitions::ShapeTransitionIndex::default();
        let named = append_compacted_shape(&mut shapes, &mut transitions, 0,
            ShapeTransition::FunctionName { name: 7 }, 0);
        let reconfigured = append_compacted_shape(&mut shapes, &mut transitions, named,
            ShapeTransition::FunctionNameReconfigured, 0);
        let length_reconfigured = append_compacted_shape(&mut shapes, &mut transitions, reconfigured,
            ShapeTransition::FunctionLengthReconfigured, 0);
        let created = append_compacted_shape(&mut shapes, &mut transitions, length_reconfigured,
            ShapeTransition::Constructed { constructor: Value::heap(17), new_target_is_base: false }, 0);
        let with_field = append_compacted_shape(&mut shapes, &mut transitions, created,
            ShapeTransition::Add { key: PropertyKey::string(9), slot: 0, attributes: DEFAULT_DATA_ATTRIBUTES }, 1);
        let mut rebuilt = vec![Shape::root()];
        let shape = rebuild_shape(&shapes, &[], with_field, &mut rebuilt,
            &mut Vec::new(), &mut crate::vm::shape_transitions::ShapeTransitionIndex::default());
        assert_eq!(provenance::shape_names(&rebuilt, shape), (Some(7), Some((Value::heap(17), false)), true));
        assert!(provenance::shape_function_length_reconfigured(&rebuilt, shape));
        assert_eq!(rebuilt[shape as usize].storage_len(), 1);
        assert_eq!(super::active_shape_attributes(&rebuilt, shape).len(), 1);
    }

    #[test]
    fn shape_compaction_waits_until_reclaim_is_material() {
        assert!(!shape_reclaim_threshold_met(1_000, 25));
        assert!(!shape_reclaim_threshold_met(1_000, 200));
        assert!(!shape_reclaim_threshold_met(1_000, 499));
        assert!(shape_reclaim_threshold_met(1_000, 500));
    }

    #[test]
    fn shape_gc_root_fact_is_inherited_from_symbol_and_accessor_transitions() {
        let root = Shape::root();
        assert!(!root.facts.may_have_gc_roots());
        let string_key = Shape::child(
            Some(0),
            ShapeTransition::Add {
                key: PropertyKey::string(0),
                slot: 0,
                attributes: DEFAULT_DATA_ATTRIBUTES,
            },
            1,
            None,
            root.facts,
        );
        assert!(!string_key.facts.may_have_gc_roots());

        let symbol_key = PropertyKey::symbol(Value::heap(1));
        let symbol_shape = Shape::child(
            Some(0),
            ShapeTransition::Add {
                key: symbol_key,
                slot: 0,
                attributes: DEFAULT_DATA_ATTRIBUTES,
            },
            1,
            None,
            root.facts,
        );
        assert!(symbol_shape.facts.may_have_gc_roots());
        let deleted_symbol_shape = Shape::child(
            Some(1),
            ShapeTransition::Delete {
                key: symbol_key,
                slot: 0,
            },
            1,
            None,
            symbol_shape.facts,
        );
        assert!(deleted_symbol_shape.facts.may_have_gc_roots());

        let accessor_shape = |getter, setter| {
            let attributes = ShapeAttributes::record(
                PropertyAttributes {
                    accessor: true,
                    getter,
                    setter,
                    ..DEFAULT_PROPERTY_ATTRIBUTES
                },
                &mut Vec::new(),
            );
            Shape::child(
                Some(1),
                ShapeTransition::Descriptor {
                    slot: 0,
                    attributes,
                },
                1,
                Some(DictionaryTrigger::PropertyCount),
                string_key.facts,
            )
        };
        // A getter and a setter live in the shape's accessor table, which the shape roots.
        let table_accessor = accessor_shape(Some(Value::heap(2)), Some(Value::heap(3)));
        assert!(table_accessor.facts.may_have_gc_roots());
        assert!(!table_accessor.facts.slot_getters());
        // A lone getter lives in its object's slot, traced with the object.
        let slot_getter = accessor_shape(Some(Value::heap(2)), None);
        assert!(!slot_getter.facts.may_have_gc_roots());
        assert!(slot_getter.facts.slot_getters());
    }
}
