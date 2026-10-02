use super::module::ModuleRecord;
use super::promise::PromiseState;
use super::*;

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
    transitions: &mut FxHashMap<(u32, property_key::PropertyKey), u32>,
    parent: u32,
    transition: ShapeTransition,
    storage_len: usize,
) -> u32 {
    if let ShapeTransition::Add {
        key: property_key::PropertyKey::String(_),
        ..
    } = transition
        && let ShapeTransition::Add { key, .. } = transition
        && let Some(shape) = transitions.get(&(parent, key))
    {
        return *shape;
    }
    let shape = u32::try_from(shapes.len()).expect("object shape table exhausted");
    let dictionary_trigger = match transition {
        ShapeTransition::Dictionary { trigger } => Some(trigger),
        _ => shapes[parent as usize].dictionary_trigger,
    };
    shapes.push(Shape::child(
        Some(parent),
        transition,
        storage_len,
        dictionary_trigger,
    ));
    if let ShapeTransition::Add {
        key: key @ property_key::PropertyKey::String(_),
        ..
    } = transition
    {
        transitions.insert((parent, key), shape);
    }
    shape
}

fn rebuild_shape(
    old_shapes: &[Shape],
    old_shape: u32,
    shapes: &mut Vec<Shape>,
    transitions: &mut FxHashMap<(u32, property_key::PropertyKey), u32>,
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
        #[cfg(feature = "profile-aggregate")]
        self.snapshot_method_caches(0);
        let roots =
            self.programs
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
                    self.realm.intrinsics.builtin_prototypes
                        .iter()
                        .flat_map(|((realm, _), prototype)| [*realm, *prototype]),
                )
                .chain(
                    self.realm.intrinsics.regexp_intrinsics
                        .iter()
                        .flat_map(|(realm, intrinsics)| {
                            [*realm, intrinsics.constructor, intrinsics.prototype]
                        }),
                )
                .chain(
                    self.realm.intrinsics.intl_number_format_constructors
                        .iter()
                        .flat_map(|(realm, constructor)| [*realm, *constructor]),
                )
                .chain(
                    self.realm.intrinsics.intl_number_format_prototypes
                        .iter()
                        .flat_map(|(realm, prototype)| [*realm, *prototype]),
                )
                .chain(
                    self.realm.intrinsics.intl_number_format_fallback_symbols
                        .iter()
                        .flat_map(|(realm, symbol)| [*realm, *symbol]),
                )
                .chain(
                    self.realm.intrinsics.intl_collator_constructors
                        .iter()
                        .flat_map(|(realm, constructor)| [*realm, *constructor]),
                )
                .chain(
                    self.realm.intrinsics.intl_collator_prototypes
                        .iter()
                        .flat_map(|(realm, prototype)| [*realm, *prototype]),
                )
                .chain(
                    self.realm.intrinsics.intl_plural_rules_prototypes
                        .iter()
                        .flat_map(|(realm, prototype)| [*realm, *prototype]),
                )
                .chain(
                    self.realm.intrinsics.intl_datetime_format_constructors
                        .iter()
                        .flat_map(|(realm, constructor)| [*realm, *constructor]),
                )
                .chain(
                    self.realm.intrinsics.intl_datetime_format_prototypes
                        .iter()
                        .flat_map(|(realm, prototype)| [*realm, *prototype]),
                )
                .chain(
                    self.realm.intrinsics.intl_datetime_format_fallback_symbols
                        .iter()
                        .flat_map(|(realm, symbol)| [*realm, *symbol]),
                )
                .chain(
                    self.realm.intrinsics.intl_display_names_constructors
                        .iter()
                        .flat_map(|(realm, constructor)| [*realm, *constructor]),
                )
                .chain(
                    self.realm.intrinsics.intl_display_names_prototypes
                        .iter()
                        .flat_map(|(realm, prototype)| [*realm, *prototype]),
                )
                .chain(
                    self.realm.intrinsics.intl_duration_format_constructors
                        .iter()
                        .flat_map(|(realm, constructor)| [*realm, *constructor]),
                )
                .chain(
                    self.realm.intrinsics.intl_duration_format_prototypes
                        .iter()
                        .flat_map(|(realm, prototype)| [*realm, *prototype]),
                )
                .chain(
                    self.realm.intrinsics.intl_list_format_constructors
                        .iter()
                        .flat_map(|(realm, constructor)| [*realm, *constructor]),
                )
                .chain(
                    self.realm.intrinsics.intl_list_format_prototypes
                        .iter()
                        .flat_map(|(realm, prototype)| [*realm, *prototype]),
                )
                .chain(
                    self.realm.intrinsics.intl_relative_time_format_prototypes
                        .iter()
                        .flat_map(|(realm, prototype)| [*realm, *prototype]),
                )
                .chain(
                    self.realm.intrinsics.intl_segmenter_prototypes
                        .iter()
                        .flat_map(|(realm, prototype)| [*realm, *prototype]),
                )
                .chain(
                    self.realm.intrinsics.intl_segments_prototypes
                        .iter()
                        .flat_map(|(realm, prototype)| [*realm, *prototype]),
                )
                .chain(
                    self.realm.intrinsics.intl_locale_prototypes
                        .iter()
                        .flat_map(|(realm, prototype)| [*realm, *prototype]),
                )
                .chain(self.natives.iter().map(|(_, value)| *value))
                .chain(
                    self.realm.intrinsics.iterator_prototypes
                        .iter()
                        .flat_map(|(realm, prototypes)| {
                            [
                                *realm,
                                prototypes.helper,
                                prototypes.wrapper,
                                prototypes.generator,
                                prototypes.async_generator,
                            ]
                        }),
                )
                .chain(self.test262_agent.roots())
                .chain(self.realm.promise.active_native.iter().copied())
                .chain(self.realm.promise.modules.values().flat_map(ModuleRecord::roots))
                .chain(self.realm.promise.module_sources.values().copied())
                .chain(self.realm.promise.records.iter().flat_map(|(promise, record)| {
                    std::iter::once(*promise)
                        .chain(std::iter::once(record.result))
                        .chain(record.reactions.iter().flat_map(|reaction| {
                            [reaction.on_fulfilled, reaction.on_rejected, reaction.next]
                        }))
                        .chain(
                            record
                                .finally_reactions
                                .iter()
                                .flat_map(|reaction| [reaction.handler, reaction.next]),
                        )
                }))
                .chain(self.realm.promise.jobs.iter().flat_map(|(job, reaction)| {
                    [*job, reaction.handler, reaction.next, reaction.value]
                }))
                .chain(
                    self.realm.promise
                        .thenable_jobs
                        .iter()
                        .flat_map(|(job, thenable)| {
                            [*job, thenable.then, thenable.thenable, thenable.promise]
                        }),
                )
                .chain(
                    self.realm.promise
                        .finally_jobs
                        .iter()
                        .flat_map(|(job, finally_job)| {
                            [
                                *job,
                                finally_job.handler,
                                finally_job.next,
                                finally_job.value,
                            ]
                        }),
                )
                .chain(
                    self.realm.promise.finally_continuation_jobs.iter().flat_map(
                        |(job, continuation)| [*job, continuation.next, continuation.value],
                    ),
                )
                .chain(self.realm.promise.finally_handler_callbacks.iter().flat_map(
                    |(function, callback)| [*function, callback.handler, callback.constructor],
                ))
                .chain(
                    self.realm.promise
                        .finally_continuation_callbacks
                        .iter()
                        .flat_map(|(function, callback)| [*function, callback.original]),
                )
                .chain(
                    self.realm.promise
                        .aggregates
                        .iter()
                        .flat_map(|(aggregate, record)| {
                            std::iter::once(*aggregate)
                                .chain(std::iter::once(record.output))
                                .chain(std::iter::once(record.resolve))
                                .chain(std::iter::once(record.reject))
                                .chain(record.values.iter().copied())
                                .chain(record.keys.iter().flatten().copied())
                        }),
                )
                .chain(
                    self.realm.promise
                        .aggregate_jobs
                        .iter()
                        .flat_map(|(job, aggregate_job)| [*job, aggregate_job.aggregate]),
                )
                .chain(
                    self.realm.promise
                        .reaction_capabilities
                        .iter()
                        .flat_map(|(promise, (resolve, reject))| [*promise, *resolve, *reject]),
                )
                .chain(
                    self.realm.promise
                        .async_resume_jobs
                        .iter()
                        .flat_map(|(job, resume)| {
                            [Some(*job), Some(resume.promise), resume.generator]
                                .into_iter()
                                .flatten()
                        }),
                )
                .chain(self.realm.jobs.iter().flat_map(|job| {
                    std::iter::once(job.callback)
                        .chain(std::iter::once(job.this))
                        .chain(job.args.iter().copied())
                }))
                .chain(self.realm.template_objects.values().copied())
                .chain(self.with_stack.iter().copied())
                .chain(self.active_call_roots.iter().copied())
                .chain(
                    self.suspended
                        .iter()
                        .filter_map(|entry| entry.continuation.as_ref())
                        .flat_map(Continuation::roots),
                )
                .chain(self.symbol_registry.values().copied())
                .chain(self.well_known_symbols.values().copied())
                .chain(
                    self.descriptors
                        .values()
                        .flat_map(|attributes| [attributes.getter, attributes.setter])
                        .flatten(),
                )
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
        let shapes = &self.shapes;
        let finalization_jobs = self.heap.collect_with_shape_roots(roots, |shape, roots| {
            append_shape_roots(shapes, shape, roots)
        });
        self.prune_function_values();
        self.compact_live_shapes();
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
        self.realm.promise
            .records
            .retain(|promise, _| self.heap.get(*promise).is_some());
        self.realm.promise
            .jobs
            .retain(|job, _| self.heap.get(*job).is_some());
        self.realm.promise
            .thenable_jobs
            .retain(|job, _| self.heap.get(*job).is_some());
        self.realm.promise
            .finally_jobs
            .retain(|job, _| self.heap.get(*job).is_some());
        self.realm.promise
            .finally_continuation_jobs
            .retain(|job, _| self.heap.get(*job).is_some());
        self.realm.promise
            .finally_handler_callbacks
            .retain(|function, _| self.heap.get(*function).is_some());
        self.realm.promise
            .finally_continuation_callbacks
            .retain(|function, _| self.heap.get(*function).is_some());
        self.realm.promise
            .aggregates
            .retain(|aggregate, _| self.heap.get(*aggregate).is_some());
        self.realm.promise
            .aggregate_jobs
            .retain(|job, _| self.heap.get(*job).is_some());
        self.realm.promise
            .reaction_capabilities
            .retain(|promise, _| self.heap.get(*promise).is_some());
        self.realm.promise
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
    }

    fn compact_live_shapes(&mut self) {
        let live_shapes = self.heap.live_object_shapes();
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
        self.heap.compact_property_arena();
        self.shapes = shapes;
        self.transitions = transitions;
        self.object_shapes.fill(u32::MAX);
        self.invalidate_field_caches();
        self.method_caches.fill([EMPTY_METHOD_CACHE; 2]);
        self.megamorphic_methods.clear();
        #[cfg(feature = "profile-aggregate")]
        self.remap_invalidated_method_shapes(&mapping);
        #[cfg(feature = "profile-memory")]
        if std::env::var_os("RQJ_MEMORY").is_some() {
            eprintln!(
                "{{\"kind\":\"rqj-shape-compaction\",\"before\":{},\"after\":{},\"live_objects\":{}}}",
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
            self.call_value(program, callback, this, &args)?;
            index += 1;
            self.advance_static_module_jobs(program)?;
            self.advance_dynamic_import_jobs(program, true)?;
        }
        self.realm.jobs.drain(..index);
        Ok(Value::UNDEFINED)
    }

    pub(crate) fn drain_jobs_until_promise(
        &mut self,
        program: &ResidualProgram,
        promise: Value,
    ) -> Result<(), JsError> {
        let mut index = 0;
        while self.realm.promise
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
