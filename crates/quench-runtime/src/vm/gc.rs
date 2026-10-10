use super::module::ModuleRecord;
use super::promise::PromiseState;
use super::*;

// RayTrace's allocation trace shows about 40–43% of its shape table is
// unreachable after each collection. Compact only once at least half the
// table is dead, amortizing the rebuild and cache invalidation across GCs.
const SHAPE_COMPACTION_MIN_RECLAIM_DENOMINATOR: usize = 2;

fn shape_reclaim_threshold_met(shape_count: usize, unreachable: usize) -> bool {
    unreachable >= shape_count.div_ceil(SHAPE_COMPACTION_MIN_RECLAIM_DENOMINATOR)
}

fn active_shape_attributes(
    shapes: &[Shape],
    shape: u32,
) -> Vec<Option<(property_key::PropertyKey, PropertyAttributes)>> {
    let mut chain = Vec::new();
    let mut current = Some(shape);
    while let Some(id) = current {
        chain.push(id);
        current = shapes[id as usize].parent;
    }
    chain.reverse();

    let mut entries = Vec::new();
    for id in chain {
        match shapes[id as usize].transition {
            ShapeTransition::Add { key, .. } => {
                entries.push(Some((key, DEFAULT_PROPERTY_ATTRIBUTES)));
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
            ShapeTransition::Root | ShapeTransition::Dictionary { .. } => {}
        }
    }
    entries
}

fn append_shape_roots(shapes: &[Shape], shape: u32, roots: &mut Vec<Value>) {
    roots.extend(
        active_shape_attributes(shapes, shape)
            .into_iter()
            .flatten()
            .flat_map(|(key, attributes)| {
                key.symbol_value()
                    .into_iter()
                    .chain([attributes.getter, attributes.setter].into_iter().flatten())
            }),
    );
}

fn append_compacted_shape(
    shapes: &mut Vec<Shape>,
    transitions: &mut FxHashMap<(u32, ShapeTransitionKey), u32>,
    parent: u32,
    transition: ShapeTransition,
    storage_len: usize,
) -> u32 {
    let cache_key = transition.cache_key();
    if let Some(key) = cache_key
        && let Some(shape) = transitions.get(&(parent, key))
    {
        return *shape;
    }
    let shape = u32::try_from(shapes.len()).expect("object shape table exhausted");
    let dictionary_trigger = match transition {
        ShapeTransition::Dictionary { trigger } => Some(trigger),
        _ => shapes[parent as usize].dictionary_trigger,
    };
    let parent_may_have_gc_roots = shapes[parent as usize].may_have_gc_roots;
    shapes.push(Shape::child(
        Some(parent),
        transition,
        storage_len,
        dictionary_trigger,
        parent_may_have_gc_roots,
    ));
    if let Some(key) = cache_key {
        transitions.insert((parent, key), shape);
    }
    shape
}

fn rebuild_shape(
    old_shapes: &[Shape],
    old_shape: u32,
    shapes: &mut Vec<Shape>,
    transitions: &mut FxHashMap<(u32, ShapeTransitionKey), u32>,
) -> u32 {
    let mut parent = 0;
    for (slot, entry) in active_shape_attributes(old_shapes, old_shape)
        .into_iter()
        .enumerate()
    {
        let slot = u32::try_from(slot).expect("object property index exceeds u32");
        let storage_len = slot as usize + 1;
        let transition = match entry {
            Some((key, _)) => ShapeTransition::Add { key, slot },
            None => ShapeTransition::Vacant,
        };
        parent = append_compacted_shape(shapes, transitions, parent, transition, storage_len);
        if let Some((_, attributes)) = entry
            && attributes != DEFAULT_PROPERTY_ATTRIBUTES
        {
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
            old_shapes[old_shape as usize].storage_len,
        );
    }
    parent
}

impl<H: Host> Vm<H> {
    /// Run a named collection safepoint even when the allocation threshold has
    /// not been reached. Hosts and focused conformance tests use this boundary
    /// to validate root ownership and weak/finalization ordering without
    /// reaching into heap internals.
    pub(crate) fn collect_now(&mut self, program: &ResidualProgram) {
        self.collect_slow(program);
    }

    #[inline(always)]
    pub(super) fn maybe_collect(&mut self, program: &ResidualProgram) {
        if !self.heap.should_collect() {
            return;
        }
        self.collect_slow(program);
    }

    #[cold]
    #[inline(never)]
    pub(super) fn collect_slow(&mut self, _program: &ResidualProgram) {
        #[cfg(feature = "profile-aggregate")]
        for (index, frame) in self.frames.iter().enumerate() {
            self.profile.gc_frame(
                frame.function,
                frame.pc as u32,
                index + 1 == self.frames.len(),
            );
        }
        // Out-of-cell metadata belongs to its object. Derive the edge index for
        // this collection and visit it only when the owner becomes reachable.
        let mut owned_roots: FxHashMap<Value, Vec<Value>> = FxHashMap::default();
        let attached_continuations: FxHashSet<_> = self
            .realm
            .promise
            .async_resume_jobs
            .values()
            .map(|job| job.continuation)
            .collect();
        let continuation_edges =
            self.realm
                .promise
                .async_resume_jobs
                .iter()
                .flat_map(|(owner, job)| {
                    self.suspended_continuation(job.continuation)
                        .into_iter()
                        .flat_map(Continuation::roots)
                        .map(move |edge| (*owner, edge))
                });
        let descriptor_edges = self
            .descriptors
            .iter()
            .flat_map(|((owner, key), attributes)| {
                key.symbol_value()
                    .into_iter()
                    .chain([attributes.getter, attributes.setter].into_iter().flatten())
                    .map(move |edge| (*owner, edge))
            });
        for (owner, edge) in descriptor_edges
            .chain(self.realm.promise.owned_edges())
            .chain(continuation_edges)
            .filter(|(_, edge)| edge.heap_index().is_some())
        {
            owned_roots.entry(owner).or_default().push(edge);
        }
        let roots = self
            .programs
            .roots()
            .chain([
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
            ])
            .chain(
                self.realm
                    .global_lexical_states
                    .iter()
                    .flat_map(|(global, state)| {
                        std::iter::once(*global).chain(state.bindings.values().copied())
                    }),
            )
            .chain(
                self.realm
                    .intrinsics
                    .builtin_prototypes
                    .iter()
                    .flat_map(|((realm, _), prototype)| [*realm, *prototype]),
            )
            .chain(
                self.realm
                    .intrinsics
                    .promise_constructors
                    .iter()
                    .flat_map(|(realm, constructor)| [*realm, *constructor]),
            )
            .chain(self.realm.intrinsics.regexp_intrinsics.iter().flat_map(
                |(realm, intrinsics)| [*realm, intrinsics.constructor, intrinsics.prototype],
            ))
            .chain(
                self.realm
                    .intrinsics
                    .intl_number_format_constructors
                    .iter()
                    .flat_map(|(realm, constructor)| [*realm, *constructor]),
            )
            .chain(
                self.realm
                    .intrinsics
                    .intl_number_format_prototypes
                    .iter()
                    .flat_map(|(realm, prototype)| [*realm, *prototype]),
            )
            .chain(
                self.realm
                    .intrinsics
                    .intl_number_format_fallback_symbols
                    .iter()
                    .flat_map(|(realm, symbol)| [*realm, *symbol]),
            )
            .chain(
                self.realm
                    .intrinsics
                    .intl_collator_constructors
                    .iter()
                    .flat_map(|(realm, constructor)| [*realm, *constructor]),
            )
            .chain(
                self.realm
                    .intrinsics
                    .intl_collator_prototypes
                    .iter()
                    .flat_map(|(realm, prototype)| [*realm, *prototype]),
            )
            .chain(
                self.realm
                    .intrinsics
                    .intl_plural_rules_prototypes
                    .iter()
                    .flat_map(|(realm, prototype)| [*realm, *prototype]),
            )
            .chain(
                self.realm
                    .intrinsics
                    .intl_datetime_format_constructors
                    .iter()
                    .flat_map(|(realm, constructor)| [*realm, *constructor]),
            )
            .chain(
                self.realm
                    .intrinsics
                    .intl_datetime_format_prototypes
                    .iter()
                    .flat_map(|(realm, prototype)| [*realm, *prototype]),
            )
            .chain(
                self.realm
                    .intrinsics
                    .intl_datetime_format_fallback_symbols
                    .iter()
                    .flat_map(|(realm, symbol)| [*realm, *symbol]),
            )
            .chain(
                self.realm
                    .intrinsics
                    .intl_display_names_constructors
                    .iter()
                    .flat_map(|(realm, constructor)| [*realm, *constructor]),
            )
            .chain(
                self.realm
                    .intrinsics
                    .intl_display_names_prototypes
                    .iter()
                    .flat_map(|(realm, prototype)| [*realm, *prototype]),
            )
            .chain(
                self.realm
                    .intrinsics
                    .intl_duration_format_constructors
                    .iter()
                    .flat_map(|(realm, constructor)| [*realm, *constructor]),
            )
            .chain(
                self.realm
                    .intrinsics
                    .intl_duration_format_prototypes
                    .iter()
                    .flat_map(|(realm, prototype)| [*realm, *prototype]),
            )
            .chain(
                self.realm
                    .intrinsics
                    .intl_list_format_constructors
                    .iter()
                    .flat_map(|(realm, constructor)| [*realm, *constructor]),
            )
            .chain(
                self.realm
                    .intrinsics
                    .intl_list_format_prototypes
                    .iter()
                    .flat_map(|(realm, prototype)| [*realm, *prototype]),
            )
            .chain(
                self.realm
                    .intrinsics
                    .intl_relative_time_format_prototypes
                    .iter()
                    .flat_map(|(realm, prototype)| [*realm, *prototype]),
            )
            .chain(
                self.realm
                    .intrinsics
                    .intl_segmenter_prototypes
                    .iter()
                    .flat_map(|(realm, prototype)| [*realm, *prototype]),
            )
            .chain(
                self.realm
                    .intrinsics
                    .intl_segment_iterator_prototypes
                    .iter()
                    .flat_map(|(realm, prototype)| [*realm, *prototype]),
            )
            .chain(
                self.realm
                    .intrinsics
                    .intl_segments_prototypes
                    .iter()
                    .flat_map(|(realm, prototype)| [*realm, *prototype]),
            )
            .chain(
                self.realm
                    .intrinsics
                    .intl_locale_prototypes
                    .iter()
                    .flat_map(|(realm, prototype)| [*realm, *prototype]),
            )
            .chain(self.natives.iter().map(|(_, value)| *value))
            .chain(self.realm.intrinsics.iterator_prototypes.iter().flat_map(
                |(realm, prototypes)| {
                    [
                        *realm,
                        prototypes.helper,
                        prototypes.wrapper,
                        prototypes.generator,
                        prototypes.async_generator,
                    ]
                },
            ))
            .chain(self.test262_agent.roots())
            .chain(
                self.realm
                    .promise
                    .active_native
                    .iter()
                    .map(|activation| activation.callable),
            )
            .chain(
                self.realm
                    .promise
                    .rejection_notifications
                    .iter()
                    .map(|notification| match notification {
                        super::promise::RejectionNotification::Unhandled(promise)
                        | super::promise::RejectionNotification::Handled(promise) => *promise,
                    }),
            )
            .chain(
                self.realm
                    .promise
                    .modules
                    .values()
                    .flat_map(ModuleRecord::roots),
            )
            .chain(self.realm.promise.module_sources.values().copied())
            .chain(
                self.realm
                    .promise
                    .dynamic_import_jobs
                    .iter()
                    .flat_map(|job| job.promises.iter().copied()),
            )
            .chain(self.realm.jobs.iter().flat_map(|job| {
                std::iter::once(job.callback)
                    .chain(std::iter::once(job.this))
                    .chain(job.args.iter().copied())
            }))
            .chain(self.realm.template_objects.values().copied())
            .chain(
                self.realm
                    .intrinsics
                    .arguments_objects
                    .iter()
                    .flat_map(|((global, _), template)| [*global, template.anchor]),
            )
            .chain(self.with_stack.iter().copied())
            .chain(self.active_call_roots.iter().copied())
            .chain(
                self.suspended
                    .iter()
                    .enumerate()
                    .filter_map(|(slot, entry)| {
                        let continuation = entry.continuation.as_ref()?;
                        let id = super::activation::ContinuationId {
                            slot: slot as u32,
                            generation: entry.generation,
                        };
                        (!attached_continuations.contains(&id)).then_some(continuation)
                    })
                    .flat_map(Continuation::roots),
            )
            .chain(self.symbol_registry.values().copied())
            .chain(self.well_known_symbols.values().copied())
            .chain(self.frames.iter().flat_map(|frame| {
                // A missing map means the function uses a register form
                // the liveness pass cannot represent; keep the safe
                // conservative scan for that activation.
                let register_mask = self.programs.get(frame.program).and_then(|program| {
                    let function = program.functions.get(frame.function as usize)?;
                    (function.register_root_offset != crate::bytecode::NO_REGISTER_ROOT_MAP)
                        .then(|| {
                            program
                                .register_roots
                                .get(function.register_root_offset as usize + frame.pc)
                        })
                        .flatten()
                        .copied()
                });
                [frame.env, frame.this]
                    .into_iter()
                    .chain(frame.context.callee())
                    .chain(frame.original_arguments.iter().copied())
                    .chain(frame.locals.iter().copied())
                    .chain(frame.dynamic_bindings.iter().map(|(_, value)| *value))
                    .chain(frame.registers.iter().enumerate().filter_map(
                        move |(register, value)| {
                            register_mask
                                .is_none_or(|mask| {
                                    register < u64::BITS as usize && mask & (1 << register) != 0
                                })
                                .then_some(*value)
                        },
                    ))
            }));
        #[cfg(feature = "profile-memory")]
        if std::env::var_os("QUENCH_MEMORY_PEAK").is_some() {
            let phase = format!("gc_{}_before", self.heap.collection_count() + 1);
            self.report_memory_snapshot(&phase);
        }
        let shape_count = self.shapes.len();
        let mut scanned_root_shapes = None::<Vec<u64>>;
        let mut live_shapes = Vec::new();
        let shapes = &mut self.shapes;
        let finalization_jobs =
            self.heap
                .collect_with_object_roots(roots, |owner, shape, roots| {
                    live_shapes.push(shape);
                    if shapes[shape as usize].may_have_gc_roots {
                        let shape_index = shape as usize;
                        let visited = scanned_root_shapes.get_or_insert_with(|| {
                            vec![0; shape_count.div_ceil(u64::BITS as usize)]
                        });
                        let word = shape_index / u64::BITS as usize;
                        let mask = 1_u64 << (shape_index % u64::BITS as usize);
                        if visited[word] & mask == 0 {
                            visited[word] |= mask;
                            append_shape_roots(shapes, shape, roots);
                        }
                    }
                    if !owned_roots.is_empty()
                        && let Some(edges) = owned_roots.get(&owner)
                    {
                        roots.extend(edges.iter().copied());
                    }
                });
        // Do shape work immediately after sweep. In particular, dead method
        // cache handles must be pruned before any runtime cleanup can allocate
        // a new heap cell into a freed slot.
        if self.should_compact_live_shapes(&live_shapes) {
            #[cfg(feature = "profile-aggregate")]
            self.snapshot_method_caches(0);
            self.compact_live_shapes(live_shapes);
        } else {
            self.retain_live_method_caches();
        }
        let live_continuations: FxHashSet<_> = self
            .realm
            .promise
            .async_resume_jobs
            .iter()
            .filter(|(owner, _)| self.heap.get(**owner).is_some())
            .map(|(_, job)| job.continuation)
            .collect();
        for id in attached_continuations.difference(&live_continuations) {
            self.resume_continuation(*id);
        }
        self.prune_function_values();
        self.heap.compact_property_arena();
        self.realm.jobs.extend(
            finalization_jobs
                .into_iter()
                .map(|(callback, held)| PendingJob {
                    callback,
                    this: Value::UNDEFINED,
                    args: vec![held],
                }),
        );
        self.descriptors
            .retain(|(object, _), _| self.heap.get(*object).is_some());
        let mut abandoned_job_contexts = Vec::new();
        self.realm.promise.records.retain(|promise, record| {
            let live = self.heap.get(*promise).is_some();
            if !live {
                abandoned_job_contexts.extend(
                    record
                        .reactions
                        .iter()
                        .filter_map(|reaction| reaction.execution_context),
                );
            }
            live
        });
        self.realm.promise.jobs.retain(|job, record| {
            let live = self.heap.get(*job).is_some();
            if !live {
                abandoned_job_contexts.extend(record.execution_context);
            }
            live
        });
        self.realm.promise.thenable_jobs.retain(|job, record| {
            let live = self.heap.get(*job).is_some();
            if !live {
                abandoned_job_contexts.extend(record.execution_context);
            }
            live
        });
        for context in abandoned_job_contexts {
            self.release_job_context(context);
        }
        self.realm
            .promise
            .finally_handler_callbacks
            .retain(|function, _| self.heap.get(*function).is_some());
        self.realm
            .promise
            .finally_continuation_callbacks
            .retain(|function, _| self.heap.get(*function).is_some());
        self.realm
            .promise
            .aggregates
            .retain(|aggregate, _| self.heap.get(*aggregate).is_some());
        self.realm
            .promise
            .aggregate_jobs
            .retain(|job, _| self.heap.get(*job).is_some());
        self.realm
            .promise
            .reaction_capabilities
            .retain(|promise, _| self.heap.get(*promise).is_some());
        self.realm
            .promise
            .async_resume_jobs
            .retain(|job, _| self.heap.get(*job).is_some());
        #[cfg(feature = "profile-aggregate")]
        self.retain_live_gc_method_snapshots();
        if let Some(strings) = &mut self.dynamic_strings {
            strings.retain(|_, value| matches!(self.heap.get(*value), Some(Cell::String(_))));
        }
        if let Some(concats) = &mut self.string_concats {
            concats.fill(EMPTY_STRING_CONCAT_CACHE);
        }
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
                current = self.shapes[index].parent;
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
        let live_object_shape_count = live_shapes.len();
        let mut mapping = vec![u32::MAX; old_shapes.len()];
        let mut shapes = vec![Shape::root()];
        let mut transitions = FxHashMap::default();
        for old_shape in live_shapes {
            let mapping_entry = &mut mapping[old_shape as usize];
            if *mapping_entry == u32::MAX {
                *mapping_entry =
                    rebuild_shape(&old_shapes, old_shape, &mut shapes, &mut transitions);
            }
        }
        let lengths = shapes
            .iter()
            .map(|shape| shape.storage_len)
            .collect::<Vec<_>>();
        self.heap.remap_live_object_shapes(&mapping, &lengths);
        self.shapes = shapes;
        self.transitions = transitions;
        self.object_shapes.fill(u32::MAX);
        self.invalidate_field_caches();
        self.method_caches.fill([EMPTY_METHOD_CACHE; 2]);
        self.megamorphic_methods.clear();
        #[cfg(feature = "profile-aggregate")]
        self.remap_invalidated_method_shapes(&mapping);
        #[cfg(feature = "profile-memory")]
        if std::env::var_os("QUENCH_MEMORY").is_some() {
            eprintln!(
                "{{\"kind\":\"quench-shape-compaction\",\"before\":{},\"after\":{},\"live_objects\":{}}}",
                old_shape_count,
                self.shapes.len(),
                live_object_shape_count
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
        let mut index = 0;
        while index < self.realm.jobs.len() {
            let job = &self.realm.jobs[index];
            let callback = job.callback;
            let this = job.this;
            let args = job.args.clone();
            if let Err(error) = self.call_value(program, callback, this, &args) {
                self.realm.jobs.drain(..=index);
                return Err(error);
            }
            index += 1;
            if let Err(error) = self.advance_static_module_jobs(program) {
                self.realm.jobs.drain(..index);
                return Err(error);
            }
            if let Err(error) = self.advance_dynamic_import_jobs(program, true) {
                self.realm.jobs.drain(..index);
                return Err(error);
            }
        }
        self.realm.jobs.drain(..index);
        Ok(Value::UNDEFINED)
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
        let mut index = 0;
        while self
            .realm
            .promise
            .records
            .get(&promise)
            .is_some_and(|record| record.state == PromiseState::Pending)
            && index < self.realm.jobs.len()
        {
            let job = &self.realm.jobs[index];
            let callback = job.callback;
            let this = job.this;
            let args = job.args.clone();
            self.call_value(program, callback, this, &args)?;
            index += 1;
            self.advance_static_module_jobs(program)?;
            self.advance_dynamic_import_jobs(program, true)?;
        }
        self.realm.jobs.drain(..index);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::shape_reclaim_threshold_met;
    use crate::value::Value;
    use crate::vm::{
        DEFAULT_PROPERTY_ATTRIBUTES, DictionaryTrigger, PropertyAttributes, Shape, ShapeTransition,
        property_key::PropertyKey,
    };

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
        assert!(!root.may_have_gc_roots);
        let string_key = Shape::child(
            Some(0),
            ShapeTransition::Add {
                key: PropertyKey::string(0),
                slot: 0,
            },
            1,
            None,
            root.may_have_gc_roots,
        );
        assert!(!string_key.may_have_gc_roots);

        let symbol_key = PropertyKey::symbol(Value::heap(1));
        let symbol_shape = Shape::child(
            Some(0),
            ShapeTransition::Add {
                key: symbol_key,
                slot: 0,
            },
            1,
            None,
            root.may_have_gc_roots,
        );
        assert!(symbol_shape.may_have_gc_roots);
        let deleted_symbol_shape = Shape::child(
            Some(1),
            ShapeTransition::Delete {
                key: symbol_key,
                slot: 0,
            },
            1,
            None,
            symbol_shape.may_have_gc_roots,
        );
        assert!(deleted_symbol_shape.may_have_gc_roots);

        let attributes = PropertyAttributes {
            getter: Some(Value::heap(2)),
            ..DEFAULT_PROPERTY_ATTRIBUTES
        };
        let accessor_shape = Shape::child(
            Some(1),
            ShapeTransition::Descriptor {
                slot: 0,
                attributes,
            },
            1,
            Some(DictionaryTrigger::PropertyCount),
            string_key.may_have_gc_roots,
        );
        assert!(accessor_shape.may_have_gc_roots);
    }
}
