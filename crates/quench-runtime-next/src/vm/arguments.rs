use super::*;

impl<H: Host> Vm<H> {
    pub(super) fn mapped_argument_load(
        &self,
        p: &ResidualProgram,
        frame: usize,
        slot: usize,
        fallback: Value,
    ) -> Value {
        let function = &p.functions[self.frames[frame].function as usize];
        let Some(argument_slot) = mapped_arguments_slot(function, slot) else {
            return fallback;
        };
        let arguments = if self.frames[frame].captured {
            self.heap
                .environment_slot(self.frames[frame].env, argument_slot)
                .unwrap_or(Value::UNDEFINED)
        } else {
            self.frames[frame]
                .locals
                .get(argument_slot)
                .copied()
                .unwrap_or(Value::UNDEFINED)
        };
        let Some(index) = self.mapped_argument_index(arguments, slot) else {
            return fallback;
        };
        match self.heap.get(arguments) {
            Some(Cell::Array { elements, .. }) => elements
                .get(index)
                .copied()
                .filter(|value| !value.is_deleted())
                .unwrap_or(Value::UNDEFINED),
            _ => fallback,
        }
    }

    pub(super) fn mapped_argument_store(
        &mut self,
        p: &ResidualProgram,
        frame: usize,
        slot: usize,
        value: Value,
    ) {
        let function = &p.functions[self.frames[frame].function as usize];
        let Some(argument_slot) = mapped_arguments_slot(function, slot) else {
            return;
        };
        let arguments = if self.frames[frame].captured {
            self.heap
                .environment_slot(self.frames[frame].env, argument_slot)
                .unwrap_or(Value::UNDEFINED)
        } else {
            self.frames[frame]
                .locals
                .get(argument_slot)
                .copied()
                .unwrap_or(Value::UNDEFINED)
        };
        self.store_mapped_argument(arguments, slot, value);
    }

    pub(super) fn store_environment_mapped_argument(
        &mut self,
        p: &ResidualProgram,
        function: u32,
        environment: Value,
        slot: usize,
        value: Value,
    ) {
        let function = &p.functions[function as usize];
        let Some(argument_slot) = mapped_arguments_slot(function, slot) else {
            return;
        };
        let arguments = self
            .heap
            .environment_slot(environment, argument_slot)
            .unwrap_or(Value::UNDEFINED);
        self.store_mapped_argument(arguments, slot, value);
    }

    fn mapped_argument_index(&self, arguments: Value, slot: usize) -> Option<usize> {
        self.object_data(arguments)
            .and_then(Object::arguments_map)
            .and_then(|mapping| {
                mapping
                    .iter()
                    .position(|mapped| *mapped != u16::MAX && usize::from(*mapped) == slot)
            })
    }

    fn store_mapped_argument(&mut self, arguments: Value, slot: usize, value: Value) {
        let Some(index) = self.mapped_argument_index(arguments, slot) else {
            return;
        };
        let _ = self.set_array_element(arguments, index, value);
    }

    pub(super) fn sync_mapped_argument(&mut self, object: Value, index: usize, value: Value) {
        let mut updates = Vec::new();
        for (frame_index, frame) in self.frames.iter().enumerate() {
            let has_arguments = if frame.captured {
                self.heap.environment_contains(frame.env, object)
            } else {
                frame.locals.contains(&object)
            };
            if has_arguments
                && let Some(slot) = self
                    .object_data(object)
                    .and_then(Object::arguments_map)
                    .and_then(|mapping| mapping.get(index).copied())
                    .filter(|slot| *slot != u16::MAX)
            {
                updates.push((frame_index, usize::from(slot)));
            }
        }
        for (frame_index, slot) in updates {
            if self.frames[frame_index].captured {
                if let Some(target) = self
                    .heap
                    .environment_slot_mut(self.frames[frame_index].env, slot)
                {
                    *target = value;
                }
            } else if let Some(target) = self.frames[frame_index].locals.get_mut(slot) {
                *target = value;
            }
        }
    }

    pub(super) fn unmap_argument_index(&mut self, object: Value, index: usize) {
        if let Some(mapping) = self
            .object_data_mut(object)
            .and_then(Object::arguments_map_mut)
            && let Some(slot) = mapping.get_mut(index)
        {
            *slot = u16::MAX;
        }
    }
}

fn mapped_arguments_slot(function: &crate::bytecode::Function, parameter: usize) -> Option<usize> {
    let slot = function.arguments_slot?;
    (function.arguments_are_mapped() && parameter < usize::from(function.params))
        .then_some(usize::from(slot))
}
