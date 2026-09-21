use super::*;

impl<H: Host> Vm<H> {
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
                    self.map_proto,
                    self.set_proto,
                    self.weak_map_proto,
                    self.weak_set_proto,
                    self.weak_ref_proto,
                    self.iterator_proto,
                ])
                .chain(self.natives.iter().map(|(_, value)| *value))
                .chain(self.symbol_registry.values().copied())
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
        self.heap.collect(roots);
        self.descriptors
            .retain(|(object, _), _| self.heap.get(*object).is_some());
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
}
