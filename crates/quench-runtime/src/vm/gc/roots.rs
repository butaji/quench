//! The roots a collection gathers from the VM, in the order they are gathered.
use super::*;

/// The roots a collection starts from, and the pins its counted edges take under reference
/// counts (`Heap::rc_pin`).
pub(super) struct GatheredRoots {
    pub(super) roots: Vec<Value>,
    pub(super) pins: Vec<Value>,
    pub(super) rc: bool,
    pub(super) repin: bool,
}

impl GatheredRoots {
    /// The list an edge taken once under reference counts goes to: its pins, else its roots.
    pub(super) fn route(&mut self) -> &mut Vec<Value> {
        if self.rc { &mut self.pins } else { &mut self.roots }
    }
}

/// Extends `roots` with the two values of each `(realm, value)` entry.
pub(super) fn extend_pair_roots<'a>(roots: &mut Vec<Value>, pairs: impl Iterator<Item = (&'a Value, &'a Value)>) {
    for (realm, value) in pairs {
        roots.extend([*realm, *value]);
    }
}

/// One zeroed mark word for each `u64::BITS` shapes.
pub(super) fn zeroed_mark_words(shape_count: usize) -> Vec<AtomicU64> {
    (0..shape_count.div_ceil(u64::BITS as usize)).map(|_| AtomicU64::new(0)).collect()
}

/// Roots the edges of every shape, for a cycle's snapshot.
pub(super) fn append_cycle_shape_edges(shape_trace: &ParallelShapeTrace<'_>, shape_count: usize, roots: &mut Vec<Value>) {
    for shape in 0..shape_count as u32 {
        shape_trace.append_shape_edges(GcTraceKind::Collection, shape, false, roots);
    }
}

/// Under counts, a minor collection roots the edges of the owned dictionaries among the shapes
/// it pinned before, and pins the edges of the shapes made since (all, after a repin).
pub(super) fn append_counted_shape_edges(
    shape_trace: &ParallelShapeTrace<'_>,
    shapes: &[Shape],
    rc_pinned_shapes: &mut usize,
    repin: bool,
    roots: &mut Vec<Value>,
    pins: &mut Vec<Value>,
) {
    let shape_count = shapes.len();
    let pinned = if repin { 0 } else { (*rc_pinned_shapes).min(shape_count) };
    for shape in 0..pinned {
        if shape_lookup::is_owned_dictionary(&shapes[shape]) {
            shape_trace.append_shape_edges(GcTraceKind::Collection, shape as u32, false, roots);
        }
    }
    for shape in pinned..shape_count {
        shape_trace.append_shape_edges(GcTraceKind::Collection, shape as u32, false, pins);
    }
    *rc_pinned_shapes = shape_count;
}

impl<H: Host> Vm<H> {
    /// The out-of-cell metadata by owner: descriptors' symbol keys and accessors, and the
    /// promise edges. Out-of-cell metadata belongs to its object, so this index is derived for
    /// each collection and visited only when the owner becomes reachable.
    pub(super) fn owned_edge_index(&self) -> OwnedRoots {
        let mut owned_roots = OwnedRoots::with_capacity_and_hasher(self.gc_owned_root_count, Default::default());
        let mut own = |owner: Value, edge: Value| {
            if edge.heap_index().is_some() {
                owned_roots.entry(owner).or_default().push(edge);
            }
        };
        for ((owner, key), attributes) in &self.descriptors {
            for edge in key.symbol_value().into_iter().chain([attributes.getter, attributes.setter].into_iter().flatten()) {
                own(*owner, edge);
            }
        }
        self.realm.promise.for_each_owned_edge(&mut own);
        owned_roots
    }

    /// The owned edges a minor collection takes as roots: those whose owner is old.
    pub(super) fn external_owned_roots(&self, owned_roots: &OwnedRoots, full: bool) -> Vec<Value> {
        if full {
            return Vec::new();
        }
        owned_roots
            .iter()
            .filter(|(owner, _)| self.heap.is_old(**owner))
            .flat_map(|(_, edges)| edges.iter().copied())
            .collect::<Vec<_>>()
    }

    /// The roots of a collection, gathered source by source: a single iterator chain over every
    /// source costs each root a step through every nesting level.
    pub(super) fn gather_roots(&mut self, full: bool, begin_cycle: bool, external_roots: Vec<Value>) -> GatheredRoots {
        #[cfg(any(feature = "profile-aggregate", feature = "profile-gc-time"))]
        let phase_started = std::time::Instant::now();
        // They go into the previous collection's mark stack, which holds as many already.
        let rc = self.heap.rc_active();
        let repin = rc && self.rc_pin_epoch != self.heap.rc_pin_epoch();
        let all_roots = full || repin;
        let mut gathered = GatheredRoots { roots: self.heap.take_mark_stack(), pins: Vec::new(), rc, repin };
        // Under counts, a minor collection may free any old record nothing counted names. The
        // edges the VM writes once (program constants, kept atom strings, startup roots, shape
        // edges) are taken as a minor would take them, where they may name young records, and
        // their records are pinned (`Heap::rc_pin`) so no epoch frees them; when counts
        // restarted, which drops pins, every one is taken and pinned again.
        self.programs.extend_roots(gathered.route(), all_roots, self.heap.promotion_age());
        self.heap.extend_folded_roots(&mut gathered.roots);
        gathered.roots.extend(self.wasm_js.roots());
        gathered.roots.extend(self.native_roots.live().iter().copied());
        self.extend_prototype_roots(&mut gathered.roots);
        self.extend_realm_roots(&mut gathered.roots);
        self.extend_intl_roots(&mut gathered.roots);
        gathered.roots.extend(self.natives.roots());
        let kept_atoms = self.heap.kept_atom_roots(all_roots);
        gathered.route().extend(kept_atoms);
        self.extend_job_roots(&mut gathered.roots);
        self.extend_vm_state_roots(&mut gathered.roots);
        // Old startup roots stay marked until a full collection, which takes them all. Under
        // counts they are pinned when taken, as a minor collection may free old records too.
        if all_roots || begin_cycle || !self.startup_roots_old() || self.heap.satb_active() {
            let startup = self.startup_intrinsic_roots();
            gathered.route().extend(startup);
        }
        self.extend_frame_roots(&mut gathered.roots);
        gathered.roots.extend(external_roots);
        #[cfg(feature = "profile-memory")]
        if crate::trace::Channel::MemoryPeak.on() {
            let phase = format!("gc_{}_before", self.heap.collection_count() + 1);
            self.report_memory_snapshot(&phase);
        }
        #[cfg(any(feature = "profile-aggregate", feature = "profile-gc-time"))]
        self.heap.record_vm_gc_phase(1, phase_started);
        gathered
    }

    /// The prototypes and constructors of the intrinsics, the first of the realm's roots.
    pub(super) fn extend_prototype_roots(&self, roots: &mut Vec<Value>) {
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
        ]);
        roots.extend([
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
    }

    /// The active realm's top-level lexical bindings, then every inactive realm's, and the
    /// realm-keyed intrinsics.
    pub(super) fn extend_realm_roots(&self, roots: &mut Vec<Value>) {
        roots.extend(self.realm.global_lexical_bindings.values().copied());
        roots.extend(
            self.realm
                .global_lexical_states
                .iter()
                .chain(self.realm.parked.iter().map(|parked| (&parked.global, &parked.state)))
                .flat_map(|(global, state)| std::iter::once(*global).chain(state.bindings.values().copied())),
        );
        roots.extend(self.realm.intrinsics.builtin_prototypes.iter().flat_map(|((realm, _), prototype)| [*realm, *prototype]));
        roots.extend(self.realm.intrinsics.function_constructors.iter().flat_map(|((realm, _), constructor)| [*realm, *constructor]));
        extend_pair_roots(roots, self.realm.intrinsics.promise_constructors.iter());
        roots.extend(self.realm.intrinsics.error_intrinsics.iter().flat_map(|(realm, intrinsics)| {
            [*realm, intrinsics.constructor, intrinsics.stack_getter, intrinsics.stack_setter, intrinsics.call_site_constructor, intrinsics.call_site_prototype]
                .into_iter()
                .chain(intrinsics.host_formatter)
        }));
        roots.extend(self.realm.intrinsics.regexp_intrinsics.iter().flat_map(|(realm, intrinsics)| {
            [*realm, intrinsics.constructor, intrinsics.prototype]
                .into_iter()
                .chain(intrinsics.result_template.map(|template| template.anchor))
                .chain(intrinsics.builtin_exec.get())
        }));
    }

    /// The `Intl` constructors, prototypes and fallback symbols, by realm.
    pub(super) fn extend_intl_roots(&self, roots: &mut Vec<Value>) {
        let intrinsics = &self.realm.intrinsics;
        extend_pair_roots(roots, intrinsics.intl_number_format_constructors.iter());
        extend_pair_roots(roots, intrinsics.intl_number_format_prototypes.iter());
        extend_pair_roots(roots, intrinsics.intl_number_format_fallback_symbols.iter());
        extend_pair_roots(roots, intrinsics.intl_collator_constructors.iter());
        extend_pair_roots(roots, intrinsics.intl_collator_prototypes.iter());
        extend_pair_roots(roots, intrinsics.intl_plural_rules_prototypes.iter());
        extend_pair_roots(roots, intrinsics.intl_datetime_format_constructors.iter());
        extend_pair_roots(roots, intrinsics.intl_datetime_format_prototypes.iter());
        extend_pair_roots(roots, intrinsics.intl_datetime_format_fallback_symbols.iter());
        extend_pair_roots(roots, intrinsics.intl_display_names_constructors.iter());
        extend_pair_roots(roots, intrinsics.intl_display_names_prototypes.iter());
        extend_pair_roots(roots, intrinsics.intl_duration_format_constructors.iter());
        extend_pair_roots(roots, intrinsics.intl_duration_format_prototypes.iter());
        extend_pair_roots(roots, intrinsics.intl_list_format_constructors.iter());
        extend_pair_roots(roots, intrinsics.intl_list_format_prototypes.iter());
        extend_pair_roots(roots, intrinsics.intl_relative_time_format_prototypes.iter());
        extend_pair_roots(roots, intrinsics.intl_segmenter_prototypes.iter());
        extend_pair_roots(roots, intrinsics.intl_segment_iterator_prototypes.iter());
        extend_pair_roots(roots, intrinsics.intl_segments_prototypes.iter());
        extend_pair_roots(roots, intrinsics.intl_locale_prototypes.iter());
    }

    /// The iterator prototypes, the test agent's and the shared async waiters' roots, the
    /// pending promises and modules, and the queued jobs.
    pub(super) fn extend_job_roots(&self, roots: &mut Vec<Value>) {
        roots.extend(self.realm.intrinsics.iterator_prototypes.iter().flat_map(|(realm, prototypes)| {
            [*realm, prototypes.helper, prototypes.wrapper, prototypes.generator, prototypes.async_generator]
        }));
        roots.extend(self.test262_agent.roots());
        roots.extend(self.shared_async_waiters.iter().map(|w| w.promise));
        roots.extend(self.realm.promise.active_native.iter().flat_map(|activation| [activation.callable, activation.receiver]));
        roots.extend(self.realm.promise.rejection_notifications.iter().map(|notification| match notification {
            super::super::promise::RejectionNotification::Unhandled(promise) | super::super::promise::RejectionNotification::Handled(promise) => *promise,
        }));
        roots.extend(
            self.realm
                .promise
                .modules
                .values()
                .chain(self.realm.promise.module_replacements.values())
                .flat_map(ModuleRecord::roots),
        );
        roots.extend(self.realm.promise.module_sources.values().copied());
        roots.extend(self.realm.promise.dynamic_import_jobs.iter().flat_map(super::super::promise::DynamicImportJob::roots));
    }

    /// The queued jobs, the VM's own stacks and holders, and the symbols and names it keeps.
    pub(super) fn extend_vm_state_roots(&self, roots: &mut Vec<Value>) {
        for job in &self.realm.jobs {
            job.push_roots(roots);
        }
        roots.extend(self.realm.template_objects.values().copied());
        roots.extend(self.realm.intrinsics.arguments_objects.iter().flat_map(|((global, _), template)| [*global, template.anchor]));
        roots.extend(self.with_stack.iter().copied());
        self.lane_stack.extend_roots(&self.programs, roots);
        self.lane_roots.iter().for_each(|parked| parked.extend_roots(&self.programs, roots));
        roots.extend(std::iter::once(self.instanceof_has_instance_holder));
        roots.extend(self.active_call_roots.iter().copied());
        roots.extend(self.symbol_registry.values().copied());
        roots.extend(self.well_known_symbols.values());
        roots.extend(self.typeof_names);
    }

    /// The values each active frame holds: its environment, its context, its arguments and its
    /// registers, of which only the live ones where the liveness pass knows the activation.
    pub(super) fn extend_frame_roots(&self, roots: &mut Vec<Value>) {
        for frame in &self.frames {
            // A missing map means the function uses a register form the liveness pass cannot
            // represent; keep the safe conservative scan for that activation.
            roots.extend([frame.env, frame.this]);
            roots.extend(frame.context.callee());
            roots.extend(frame.original_arguments.iter().copied());
            roots.extend(frame.locals.iter().copied());
            roots.extend(frame.dynamic_bindings.iter().map(|(_, value)| *value));
            match self.programs.residual(frame.program).and_then(|program| crate::compile::liveness::RegisterMasks::for_collection(program, frame.function)) {
                Some(masks) => masks.extend_live(frame.pc as usize, &frame.registers, roots),
                None => roots.extend(frame.registers.iter().copied()),
            }
        }
    }

    /// The edges a cycle's snapshot, or a minor collection under counts, takes outside the
    /// shapes: the owned roots, the dynamic strings, the holder hints and the private homes.
    pub(super) fn extend_owned_cycle_roots(&self, owned_roots: &OwnedRoots, roots: &mut Vec<Value>) {
        roots.extend(owned_roots.values().flatten().copied());
        if let Some(strings) = &self.dynamic_strings {
            roots.extend(strings.values().copied());
        }
        roots.extend(self.holder_hints.holder.iter().flat_map(|hint| [hint.holder, hint.value]));
        roots.extend(self.holder_hints.getter.iter().map(|hint| hint.holder));
        roots.extend(self.private_homes.iter().flat_map(|(key, home)| [key.environment, *home]));
        self.private_site_homes.for_each_value(|value| roots.push(value));
    }
}
