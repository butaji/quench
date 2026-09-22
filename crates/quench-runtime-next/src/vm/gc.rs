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
    pub(super) fn collect_slow(&mut self, program: &ResidualProgram) {
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
            self.constants
                .iter()
                .copied()
                .chain([
                    self.globals,
                    self.object_proto,
                    self.function_proto,
                    self.array_proto,
                    self.array_buffer_proto,
                    self.uint8_array_proto,
                    self.uint8_clamped_array_proto,
                    self.uint16_array_proto,
                    self.uint32_array_proto,
                    self.int8_array_proto,
                    self.int16_array_proto,
                    self.int32_array_proto,
                    self.float32_array_proto,
                    self.float64_array_proto,
                    self.data_view_proto,
                    self.map_proto,
                    self.set_proto,
                    self.weak_map_proto,
                    self.weak_set_proto,
                    self.weak_ref_proto,
                    self.finalization_registry_proto,
                    self.promise.proto,
                    self.iterator_proto,
                    self.regexp_proto,
                ])
                .chain(self.natives.iter().map(|(_, value)| *value))
                .chain(self.promise.active_native.iter().copied())
                .chain(self.promise.records.iter().flat_map(|(promise, record)| {
                    std::iter::once(*promise)
                        .chain(std::iter::once(record.result))
                        .chain(record.reactions.iter().flat_map(|reaction| {
                            [reaction.on_fulfilled, reaction.on_rejected, reaction.next]
                        }))
                }))
                .chain(self.promise.jobs.iter().flat_map(|(job, reaction)| {
                    [*job, reaction.handler, reaction.next, reaction.value]
                }))
                .chain(self.jobs.iter().flat_map(|job| {
                    std::iter::once(job.callback)
                        .chain(std::iter::once(job.this))
                        .chain(job.args.iter().copied())
                }))
                .chain(
                    self.suspended
                        .iter()
                        .filter_map(|entry| entry.continuation.as_ref())
                        .flat_map(Continuation::roots),
                )
                .chain(self.symbol_registry.values().copied())
                .chain(self.well_known_symbols.values().copied())
                .chain(
                    self.symbol_properties
                        .iter()
                        .flat_map(|((object, key), value)| {
                            [Some(*object), key.symbol_value(), Some(*value)]
                                .into_iter()
                                .flatten()
                        }),
                )
                .chain(
                    self.symbol_descriptors
                        .values()
                        .flat_map(|attributes| [attributes.getter, attributes.setter])
                        .flatten(),
                )
                .chain(
                    self.descriptors
                        .values()
                        .flat_map(|attributes| [attributes.getter, attributes.setter])
                        .flatten(),
                )
                .chain(self.frames.iter().flat_map(|frame| {
                    let function = &program.functions[frame.function as usize];
                    let roots = (function.register_root_offset != u32::MAX).then(|| {
                        program.register_roots[function.register_root_offset as usize + frame.pc]
                    });
                    [frame.env, frame.this]
                        .into_iter()
                        .chain(frame.locals.iter().copied())
                        .chain(frame.registers.iter().enumerate().filter_map(
                            move |(index, value)| {
                                roots
                                    .is_none_or(|mask| mask & (1 << index) != 0)
                                    .then_some(*value)
                            },
                        ))
                }));
        self.jobs.extend(
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
        self.symbol_properties.retain(|(object, key), value| {
            self.heap.get(*object).is_some()
                && key
                    .symbol_value()
                    .is_some_and(|key| self.heap.get(key).is_some())
                && self.heap.get(*value).is_some()
        });
        self.symbol_property_order.retain(|object, keys| {
            if self.heap.get(*object).is_none() {
                return false;
            }
            keys.retain(|key| {
                key.symbol_value()
                    .is_some_and(|key| self.heap.get(key).is_some())
            });
            self.promise
                .records
                .retain(|promise, _| self.heap.get(*promise).is_some());
            self.promise
                .jobs
                .retain(|job, _| self.heap.get(*job).is_some());
            !keys.is_empty()
        });
        self.symbol_descriptors.retain(|(object, key), attributes| {
            self.heap.get(*object).is_some()
                && key
                    .symbol_value()
                    .is_some_and(|key| self.heap.get(key).is_some())
                && attributes
                    .getter
                    .into_iter()
                    .chain(attributes.setter)
                    .all(|value| self.heap.get(value).is_some())
        });
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
        while index < self.jobs.len() {
            let job = &self.jobs[index];
            let callback = job.callback;
            let this = job.this;
            let args = job.args.clone();
            self.call_value(program, callback, this, &args)?;
            index += 1;
        }
        self.jobs.drain(..index);
        Ok(Value::UNDEFINED)
    }
}
