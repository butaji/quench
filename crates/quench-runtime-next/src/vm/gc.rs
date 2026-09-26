use super::module::ModuleRecord;
use super::promise::PromiseState;
use super::*;

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
                    self.uint8_array_proto,
                    self.uint8_clamped_array_proto,
                    self.uint16_array_proto,
                    self.uint32_array_proto,
                    self.int8_array_proto,
                    self.int16_array_proto,
                    self.int32_array_proto,
                    self.bigint64_array_proto,
                    self.biguint64_array_proto,
                    self.float32_array_proto,
                    self.float64_array_proto,
                    self.data_view_proto,
                    self.map_proto,
                    self.set_proto,
                    self.map_iterator_proto,
                    self.set_iterator_proto,
                    self.weak_map_proto,
                    self.weak_set_proto,
                    self.weak_ref_proto,
                    self.finalization_registry_proto,
                    self.promise.proto,
                    self.iterator_proto,
                    self.string_iterator_proto,
                    self.generator_proto,
                    self.iterator_helper_proto,
                    self.wrap_for_valid_iterator_proto,
                    self.async_iterator_proto,
                    self.async_generator_proto,
                    self.async_from_sync_iterator_proto,
                    self.regexp_proto,
                ])
                .chain(self.natives.iter().map(|(_, value)| *value))
                .chain(
                    self.iterator_realm_prototypes
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
                .chain(self.promise.active_native.iter().copied())
                .chain(self.promise.modules.values().flat_map(ModuleRecord::roots))
                .chain(self.promise.module_sources.values().copied())
                .chain(self.promise.records.iter().flat_map(|(promise, record)| {
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
                .chain(self.promise.jobs.iter().flat_map(|(job, reaction)| {
                    [*job, reaction.handler, reaction.next, reaction.value]
                }))
                .chain(
                    self.promise
                        .thenable_jobs
                        .iter()
                        .flat_map(|(job, thenable)| {
                            [*job, thenable.then, thenable.thenable, thenable.promise]
                        }),
                )
                .chain(
                    self.promise
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
                    self.promise.finally_continuation_jobs.iter().flat_map(
                        |(job, continuation)| [*job, continuation.next, continuation.value],
                    ),
                )
                .chain(self.promise.finally_handler_callbacks.iter().flat_map(
                    |(function, callback)| [*function, callback.handler, callback.constructor],
                ))
                .chain(
                    self.promise
                        .finally_continuation_callbacks
                        .iter()
                        .flat_map(|(function, callback)| [*function, callback.original]),
                )
                .chain(
                    self.promise
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
                    self.promise
                        .aggregate_jobs
                        .iter()
                        .flat_map(|(job, aggregate_job)| [*job, aggregate_job.aggregate]),
                )
                .chain(
                    self.promise
                        .reaction_capabilities
                        .iter()
                        .flat_map(|(promise, (resolve, reject))| [*promise, *resolve, *reject]),
                )
                .chain(
                    self.promise
                        .resolving_functions
                        .iter()
                        .flat_map(|(state, resolving)| [*state, resolving.promise]),
                )
                .chain(
                    self.promise
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
                .chain(
                    self.suspended
                        .iter()
                        .filter_map(|entry| entry.continuation.as_ref())
                        .flat_map(Continuation::roots),
                )
                .chain(self.symbol_registry.values().copied())
                .chain(self.well_known_symbols.values().copied())
                .chain(
                    self.shapes
                        .iter()
                        .flat_map(|shape| shape.keys.iter().filter_map(|key| key.symbol_value())),
                )
                .chain(
                    self.descriptors
                        .values()
                        .flat_map(|attributes| [attributes.getter, attributes.setter])
                        .flatten(),
                )
                .chain(
                    self.shapes
                        .iter()
                        .flat_map(|shape| shape.descriptors.iter())
                        .flat_map(|attributes| [attributes.getter, attributes.setter])
                        .flatten(),
                )
                .chain(self.frames.iter().flat_map(|frame| {
                    [frame.env, frame.this]
                        .into_iter()
                        .chain(frame.locals.iter().copied())
                        .chain(frame.dynamic_bindings.iter().map(|(_, value)| *value))
                        // Register-root masks are an optimization over the
                        // canonical activation state. Keep every live-frame
                        // register rooted until the mask proof is complete;
                        // dropping a closure still referenced by a call-site
                        // argument is a semantic use-after-collection.
                        .chain(frame.registers.iter().copied())
                }));
        self.realm.jobs.extend(
            self.heap
                .collect(roots)
                .into_iter()
                .map(|(callback, held)| PendingJob {
                    callback,
                    this: Value::UNDEFINED,
                    args: vec![held],
                }),
        );
        self.invalidate_field_caches();
        self.descriptors
            .retain(|(object, _), _| self.heap.get(*object).is_some());
        self.promise
            .records
            .retain(|promise, _| self.heap.get(*promise).is_some());
        self.promise
            .jobs
            .retain(|job, _| self.heap.get(*job).is_some());
        self.promise
            .thenable_jobs
            .retain(|job, _| self.heap.get(*job).is_some());
        self.promise
            .finally_jobs
            .retain(|job, _| self.heap.get(*job).is_some());
        self.promise
            .finally_continuation_jobs
            .retain(|job, _| self.heap.get(*job).is_some());
        self.promise
            .finally_handler_callbacks
            .retain(|function, _| self.heap.get(*function).is_some());
        self.promise
            .finally_continuation_callbacks
            .retain(|function, _| self.heap.get(*function).is_some());
        self.promise
            .aggregates
            .retain(|aggregate, _| self.heap.get(*aggregate).is_some());
        self.promise
            .aggregate_jobs
            .retain(|job, _| self.heap.get(*job).is_some());
        self.promise
            .reaction_capabilities
            .retain(|promise, _| self.heap.get(*promise).is_some());
        self.promise
            .resolving_functions
            .retain(|state, _| self.heap.get(*state).is_some());
        self.promise
            .async_resume_jobs
            .retain(|job, _| self.heap.get(*job).is_some());
        self.retain_live_method_caches();
        #[cfg(feature = "profile-aggregate")]
        self.retain_live_gc_method_snapshots();
        if let Some(strings) = &mut self.dynamic_strings {
            strings.retain(|_, value| matches!(self.heap.get(*value), Some(Cell::String(_))));
        }
        if let Some(concats) = &mut self.string_concats {
            concats.fill(EMPTY_STRING_CONCAT_CACHE);
        }
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
        while self
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
